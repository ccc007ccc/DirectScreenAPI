package org.directscreenapi.adapter;

import java.io.FileDescriptor;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.ByteBuffer;
import java.nio.MappedByteBuffer;
import java.nio.channels.FileChannel;
import java.nio.charset.StandardCharsets;

final class DaemonSession {
    private static final int DEFAULT_SOCKET_TIMEOUT_MS = 5000;
    private static final String SOCKET_TIMEOUT_PROPERTY = "dsapi.socket_timeout_ms";
    private static final String PIXEL_FORMAT_RGBA8888 = "RGBA8888";

    private static final int BIN_MAGIC = 0x50415344; // "DSAP"
    private static final int BIN_VERSION = 1;
    private static final int BIN_HEADER_BYTES = 20;
    private static final int BIN_RESPONSE_VALUES = 8;
    private static final int BIN_RESPONSE_PAYLOAD_BYTES = 4 + (BIN_RESPONSE_VALUES * 8);

    private static final int BIN_OP_PING = 1;
    private static final int BIN_OP_VERSION = 2;
    private static final int BIN_OP_SHUTDOWN = 3;
    private static final int BIN_OP_DISPLAY_GET = 4;
    private static final int BIN_OP_DISPLAY_SET = 5;
    private static final int BIN_OP_TOUCH_CLEAR = 6;
    private static final int BIN_OP_TOUCH_COUNT = 7;
    private static final int BIN_OP_TOUCH_MOVE = 8;
    private static final int BIN_OP_RENDER_SUBMIT = 9;
    private static final int BIN_OP_RENDER_GET = 10;

    private static final String CMD_BIND_SHM = "RENDER_FRAME_BIND_SHM";
    private static final String CMD_WAIT_SHM = "RENDER_FRAME_WAIT_SHM_PRESENT";

    static final class MappedFrame {
        final long frameSeq;
        final int width;
        final int height;
        final int byteLen;
        final ByteBuffer rgba8;

        MappedFrame(long frameSeq, int width, int height, int byteLen, ByteBuffer rgba8) {
            this.frameSeq = frameSeq;
            this.width = width;
            this.height = height;
            this.byteLen = byteLen;
            this.rgba8 = rgba8;
        }

        void closeQuietly() {
            // no-op: mapped buffer lifecycle is session-scoped.
        }
    }

    private static final class SocketIo {
        final Object socket;
        final InputStream input;
        final OutputStream output;

        SocketIo(Object socket, InputStream input, OutputStream output) {
            this.socket = socket;
            this.input = input;
            this.output = output;
        }
    }

    private static final class BinaryReply {
        final long seq;
        final int opcode;
        final int status;
        final long[] values;

        BinaryReply(long seq, int opcode, int status, long[] values) {
            this.seq = seq;
            this.opcode = opcode;
            this.status = status;
            this.values = values;
        }
    }

    private final String controlSocketPath;
    private final String dataSocketPath;

    private Object controlSocket;
    private InputStream controlInput;
    private OutputStream controlOutput;
    private long controlSeq = 1L;

    private Object rawSocket;
    private InputStream rawInput;
    private OutputStream rawOutput;
    private FileInputStream rawFrameFdStream;
    private FileChannel rawFrameFdChannel;
    private MappedByteBuffer rawFrameMapped;
    private int rawFrameMappedLen;
    private int rawFrameCapacity;
    private int rawFrameOffset;

    DaemonSession(String controlSocketPath, String dataSocketPath) {
        this.controlSocketPath = controlSocketPath;
        this.dataSocketPath = dataSocketPath;
    }

    synchronized String command(String cmd) throws Exception {
        Exception last = null;
        for (int attempt = 0; attempt < 2; attempt++) {
            try {
                ensureControlConnected();
                String[] tokens = splitTokens(cmd);
                BinaryReply reply = executeControlCommand(tokens);
                if (reply.status != 0) {
                    throw new IOException("daemon_command_status=" + reply.status);
                }
                return formatControlReply(tokens, reply);
            } catch (Exception e) {
                last = e;
                closeQuietly();
            }
        }
        throw last != null ? last : new IOException("daemon_command_failed");
    }

    synchronized MappedFrame frameWaitBoundPresent(long lastFrameSeq, int timeoutMs) throws Exception {
        Exception last = null;
        for (int attempt = 0; attempt < 2; attempt++) {
            try {
                ensureRawConnected();
                bindFrameFdIfNeeded();
                long safeSeq = Math.max(0L, lastFrameSeq);
                int safeTimeout = Math.max(1, timeoutMs);
                writeAsciiLine(rawOutput, CMD_WAIT_SHM + " " + safeSeq + " " + safeTimeout);
                String trimmed = requireOkLine(readAsciiLine(rawInput), "daemon_wait_shm_present");
                String[] tokens = trimmed.split("\\s+");
                if (tokens.length == 2 && "TIMEOUT".equals(tokens[1])) {
                    return null;
                }
                if (tokens.length != 8) {
                    throw new IOException("daemon_wait_shm_present_tokens_invalid");
                }
                if (!PIXEL_FORMAT_RGBA8888.equals(tokens[4])) {
                    throw new IOException("daemon_wait_shm_present_pixel_format_invalid");
                }

                long frameSeq = parseLong(tokens[1], -1L);
                int width = parseInt(tokens[2], -1);
                int height = parseInt(tokens[3], -1);
                int byteLen = parseInt(tokens[5], -1);
                int offset = parseInt(tokens[7], -1);
                if (frameSeq < 0 || width <= 0 || height <= 0 || byteLen <= 0 || offset < 0) {
                    throw new IOException("daemon_wait_shm_present_header_invalid");
                }
                long expectedByteLen = (long) width * (long) height * 4L;
                if (expectedByteLen <= 0L || expectedByteLen > Integer.MAX_VALUE || byteLen != (int) expectedByteLen) {
                    throw new IOException("daemon_wait_shm_present_len_mismatch");
                }
                if (rawFrameMapped == null || rawFrameCapacity <= 0 || rawFrameMappedLen <= 0) {
                    throw new IOException("daemon_wait_shm_present_uninitialized");
                }
                if (byteLen > rawFrameCapacity) {
                    throw new IOException("daemon_wait_shm_present_len_over_capacity");
                }
                if (offset > rawFrameMappedLen || byteLen > (rawFrameMappedLen - offset)) {
                    throw new IOException("daemon_wait_shm_present_offset_overflow");
                }

                ByteBuffer view = rawFrameMapped.duplicate();
                view.position(offset);
                view.limit(offset + byteLen);
                ByteBuffer rgba = view.slice();
                return new MappedFrame(frameSeq, width, height, byteLen, rgba);
            } catch (Exception e) {
                last = e;
                closeRawQuietly();
            }
        }
        throw last != null ? last : new IOException("daemon_wait_shm_present_failed");
    }

    synchronized void closeQuietly() {
        if (controlInput != null) {
            try {
                controlInput.close();
            } catch (Throwable ignored) {
            }
            controlInput = null;
        }
        if (controlOutput != null) {
            try {
                controlOutput.close();
            } catch (Throwable ignored) {
            }
            controlOutput = null;
        }
        if (controlSocket != null) {
            closeSocketQuietly(controlSocket);
            controlSocket = null;
        }
        controlSeq = 1L;
        closeRawQuietly();
    }

    private void ensureControlConnected() throws Exception {
        if (controlSocket != null && controlInput != null && controlOutput != null) {
            return;
        }
        SocketIo io = openSocket(controlSocketPath);
        controlSocket = io.socket;
        controlInput = io.input;
        controlOutput = io.output;
        controlSeq = 1L;
    }

    private SocketIo openSocket(String path) throws Exception {
        Class<?> localSocketClass = Class.forName("android.net.LocalSocket");
        Class<?> addressClass = Class.forName("android.net.LocalSocketAddress");
        Class<?> namespaceClass = Class.forName("android.net.LocalSocketAddress$Namespace");
        Object namespaceFilesystem = resolveNamespaceFilesystem(namespaceClass);

        Object socket = localSocketClass.getDeclaredConstructor().newInstance();
        Object address = addressClass
                .getDeclaredConstructor(String.class, namespaceClass)
                .newInstance(path, namespaceFilesystem);
        ReflectBridge.invoke(socket, "connect", address);
        configureSocketTimeout(socket);

        OutputStream os = (OutputStream) ReflectBridge.invoke(socket, "getOutputStream");
        InputStream is = (InputStream) ReflectBridge.invoke(socket, "getInputStream");
        return new SocketIo(socket, is, os);
    }

    private void ensureRawConnected() throws Exception {
        if (rawSocket != null && rawInput != null && rawOutput != null) {
            return;
        }
        SocketIo io = openSocket(dataSocketPath);
        rawSocket = io.socket;
        rawInput = io.input;
        rawOutput = io.output;
    }

    private static void closeSocketQuietly(Object socket) {
        if (socket == null) {
            return;
        }
        try {
            ReflectBridge.invoke(socket, "close");
        } catch (Throwable ignored) {
        }
    }

    private void closeRawQuietly() {
        if (rawFrameFdChannel != null) {
            try {
                rawFrameFdChannel.close();
            } catch (Throwable ignored) {
            }
            rawFrameFdChannel = null;
        }
        if (rawFrameFdStream != null) {
            try {
                rawFrameFdStream.close();
            } catch (Throwable ignored) {
            }
            rawFrameFdStream = null;
        }
        rawFrameMapped = null;
        rawFrameMappedLen = 0;
        rawFrameCapacity = 0;
        rawFrameOffset = 0;

        if (rawInput != null) {
            try {
                rawInput.close();
            } catch (Throwable ignored) {
            }
            rawInput = null;
        }
        if (rawOutput != null) {
            try {
                rawOutput.close();
            } catch (Throwable ignored) {
            }
            rawOutput = null;
        }
        if (rawSocket != null) {
            closeSocketQuietly(rawSocket);
            rawSocket = null;
        }
    }

    private void bindFrameFdIfNeeded() throws Exception {
        if (rawFrameMapped != null && rawFrameCapacity > 0) {
            return;
        }
        bindFrameShm();
    }

    private void bindFrameShm() throws Exception {
        writeAsciiLine(rawOutput, CMD_BIND_SHM);
        String trimmed = requireOkLine(readAsciiLine(rawInput), "daemon_bind_shm");
        String[] tokens = trimmed.split("\\s+");
        if (tokens.length != 4 || !"SHM_BOUND".equals(tokens[1])) {
            throw new IOException("daemon_bind_shm_tokens_invalid");
        }
        int capacity = parseInt(tokens[2], -1);
        int offset = parseInt(tokens[3], -1);
        if (capacity <= 0 || offset < 0) {
            throw new IOException("daemon_bind_shm_layout_invalid");
        }
        int mapLen = safeAdd(capacity, offset, "daemon_bind_shm_map_len_overflow");

        FileDescriptor fd = pollSingleAncillaryFd(rawSocket);
        FileInputStream stream = new FileInputStream(fd);
        FileChannel channel = stream.getChannel();
        try {
            MappedByteBuffer mapped = channel.map(FileChannel.MapMode.READ_ONLY, 0L, mapLen);
            rawFrameFdStream = stream;
            rawFrameFdChannel = channel;
            rawFrameMapped = mapped;
            rawFrameMappedLen = mapLen;
            rawFrameCapacity = capacity;
            rawFrameOffset = offset;
        } catch (Throwable t) {
            try {
                channel.close();
            } catch (Throwable ignored) {
            }
            try {
                stream.close();
            } catch (Throwable ignored) {
            }
            throw t;
        }
    }

    private BinaryReply executeControlCommand(String[] tokens) throws Exception {
        if (tokens.length == 0) {
            throw new IOException("control_command_empty");
        }

        String cmd = tokens[0].toUpperCase();
        if ("DISPLAY_SET".equals(cmd)) {
            if (tokens.length != 6) {
                throw new IOException("display_set_args_invalid");
            }
            int width = parseInt(tokens[1], -1);
            int height = parseInt(tokens[2], -1);
            float refresh = parseFloat(tokens[3], Float.NaN);
            int dpi = parseInt(tokens[4], -1);
            int rotation = parseInt(tokens[5], -1);
            if (width <= 0 || height <= 0 || !Float.isFinite(refresh) || refresh <= 0f || dpi <= 0 || rotation < 0) {
                throw new IOException("display_set_args_invalid");
            }
            byte[] payload = new byte[20];
            writeLe32(payload, 0, width);
            writeLe32(payload, 4, height);
            writeLe32(payload, 8, Float.floatToIntBits(refresh));
            writeLe32(payload, 12, dpi);
            writeLe32(payload, 16, rotation);
            return sendBinaryControl(BIN_OP_DISPLAY_SET, payload);
        }

        if ("DISPLAY_GET".equals(cmd)) {
            if (tokens.length != 1) {
                throw new IOException("display_get_args_invalid");
            }
            return sendBinaryControl(BIN_OP_DISPLAY_GET, new byte[0]);
        }

        if ("PING".equals(cmd)) {
            if (tokens.length != 1) {
                throw new IOException("ping_args_invalid");
            }
            return sendBinaryControl(BIN_OP_PING, new byte[0]);
        }

        throw new IOException("control_command_unsupported:" + cmd);
    }

    private String formatControlReply(String[] tokens, BinaryReply reply) throws IOException {
        if (reply.status != 0) {
            throw new IOException("daemon_control_status=" + reply.status);
        }
        String cmd = tokens[0].toUpperCase();
        if ("DISPLAY_SET".equals(cmd)) {
            return "OK";
        }
        if ("DISPLAY_GET".equals(cmd)) {
            float refresh = Float.intBitsToFloat((int) reply.values[2]);
            return "OK "
                    + reply.values[0] + " "
                    + reply.values[1] + " "
                    + String.format(java.util.Locale.US, "%.2f", refresh) + " "
                    + reply.values[3] + " "
                    + reply.values[4];
        }
        if ("PING".equals(cmd)) {
            return "OK PONG";
        }
        return "OK";
    }

    private BinaryReply sendBinaryControl(int opcode, byte[] payload) throws Exception {
        byte[] header = new byte[BIN_HEADER_BYTES];
        writeLe32(header, 0, BIN_MAGIC);
        writeLe16(header, 4, BIN_VERSION);
        writeLe16(header, 6, opcode);
        writeLe32(header, 8, payload.length);
        writeLe64(header, 12, controlSeq);

        controlOutput.write(header);
        if (payload.length > 0) {
            controlOutput.write(payload);
        }
        controlOutput.flush();

        byte[] respHeader = new byte[BIN_HEADER_BYTES];
        readFully(controlInput, respHeader);
        int magic = readLe32(respHeader, 0);
        int version = readLe16(respHeader, 4);
        int respOpcode = readLe16(respHeader, 6);
        int payloadLen = readLe32(respHeader, 8);
        long seq = readLe64(respHeader, 12);

        if (magic != BIN_MAGIC || version != BIN_VERSION) {
            throw new IOException("daemon_control_header_invalid");
        }
        if (payloadLen != BIN_RESPONSE_PAYLOAD_BYTES) {
            throw new IOException("daemon_control_payload_len_invalid");
        }

        byte[] respPayload = new byte[BIN_RESPONSE_PAYLOAD_BYTES];
        readFully(controlInput, respPayload);

        int status = readLe32(respPayload, 0);
        long[] values = new long[BIN_RESPONSE_VALUES];
        int cursor = 4;
        for (int i = 0; i < BIN_RESPONSE_VALUES; i++) {
            values[i] = readLe64(respPayload, cursor);
            cursor += 8;
        }

        controlSeq = seq + 1L;
        return new BinaryReply(seq, respOpcode, status, values);
    }

    private static void writeAsciiLine(OutputStream os, String line) throws IOException {
        os.write(line.getBytes(StandardCharsets.UTF_8));
        os.write('\n');
        os.flush();
    }

    private static String readAsciiLine(InputStream is) throws IOException {
        StringBuilder sb = new StringBuilder(128);
        while (true) {
            int b = is.read();
            if (b < 0) {
                if (sb.length() == 0) {
                    return null;
                }
                break;
            }
            if (b == '\n') {
                break;
            }
            if (b != '\r') {
                sb.append((char) b);
            }
            if (sb.length() > 4096) {
                throw new IOException("daemon_line_too_long");
            }
        }
        return sb.toString();
    }

    private static String requireOkLine(String line, String context) throws IOException {
        if (line == null) {
            throw new IOException("daemon_eof");
        }
        String trimmed = line.trim();
        if ("OK".equals(trimmed) || trimmed.startsWith("OK ")) {
            return trimmed;
        }
        if ("ERR".equals(trimmed) || trimmed.startsWith("ERR ")) {
            throw new IOException(context + "_err=" + trimmed);
        }
        throw new IOException(context + "_bad_line");
    }

    private static void configureSocketTimeout(Object socket) {
        int timeoutMs = resolveSocketTimeoutMs();
        try {
            ReflectBridge.invoke(socket, "setSoTimeout", Integer.valueOf(timeoutMs));
        } catch (Throwable ignored) {
        }
    }

    private static int resolveSocketTimeoutMs() {
        int timeoutMs = parseInt(System.getProperty(SOCKET_TIMEOUT_PROPERTY), DEFAULT_SOCKET_TIMEOUT_MS);
        if (timeoutMs <= 0) {
            return DEFAULT_SOCKET_TIMEOUT_MS;
        }
        return timeoutMs;
    }

    private static int parseInt(String s, int fallback) {
        try {
            return Integer.parseInt(s);
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private static long parseLong(String s, long fallback) {
        try {
            return Long.parseLong(s);
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private static float parseFloat(String s, float fallback) {
        try {
            return Float.parseFloat(s);
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private static Object resolveNamespaceFilesystem(Class<?> namespaceClass) throws Exception {
        Object[] constants = namespaceClass.getEnumConstants();
        if (constants == null || constants.length == 0) {
            throw new IllegalStateException("localsocket_namespace_missing");
        }
        for (Object c : constants) {
            if (c instanceof Enum && "FILESYSTEM".equals(((Enum<?>) c).name())) {
                return c;
            }
        }
        return constants[0];
    }

    private static FileDescriptor pollSingleAncillaryFd(Object socket) throws Exception {
        Object out = ReflectBridge.invoke(socket, "getAncillaryFileDescriptors");
        if (!(out instanceof FileDescriptor[])) {
            throw new IOException("daemon_frame_fd_missing_ancillary");
        }
        FileDescriptor[] fds = (FileDescriptor[]) out;
        if (fds.length < 1 || fds[0] == null) {
            throw new IOException("daemon_frame_fd_empty_ancillary");
        }
        return fds[0];
    }

    private static int safeAdd(int a, int b, String errMsg) throws IOException {
        long out = (long) a + (long) b;
        if (out <= 0L || out > Integer.MAX_VALUE) {
            throw new IOException(errMsg);
        }
        return (int) out;
    }

    private static String[] splitTokens(String cmd) {
        if (cmd == null) {
            return new String[0];
        }
        String trimmed = cmd.trim();
        if (trimmed.isEmpty()) {
            return new String[0];
        }
        return trimmed.split("\\s+");
    }

    private static void readFully(InputStream is, byte[] out) throws IOException {
        int offset = 0;
        while (offset < out.length) {
            int n = is.read(out, offset, out.length - offset);
            if (n < 0) {
                throw new IOException("daemon_binary_eof");
            }
            offset += n;
        }
    }

    private static int readLe16(byte[] buf, int off) {
        return (buf[off] & 0xff) | ((buf[off + 1] & 0xff) << 8);
    }

    private static int readLe32(byte[] buf, int off) {
        return (buf[off] & 0xff)
                | ((buf[off + 1] & 0xff) << 8)
                | ((buf[off + 2] & 0xff) << 16)
                | ((buf[off + 3] & 0xff) << 24);
    }

    private static long readLe64(byte[] buf, int off) {
        return ((long) buf[off] & 0xffL)
                | (((long) buf[off + 1] & 0xffL) << 8)
                | (((long) buf[off + 2] & 0xffL) << 16)
                | (((long) buf[off + 3] & 0xffL) << 24)
                | (((long) buf[off + 4] & 0xffL) << 32)
                | (((long) buf[off + 5] & 0xffL) << 40)
                | (((long) buf[off + 6] & 0xffL) << 48)
                | (((long) buf[off + 7] & 0xffL) << 56);
    }

    private static void writeLe16(byte[] buf, int off, int value) {
        buf[off] = (byte) (value & 0xff);
        buf[off + 1] = (byte) ((value >>> 8) & 0xff);
    }

    private static void writeLe32(byte[] buf, int off, int value) {
        buf[off] = (byte) (value & 0xff);
        buf[off + 1] = (byte) ((value >>> 8) & 0xff);
        buf[off + 2] = (byte) ((value >>> 16) & 0xff);
        buf[off + 3] = (byte) ((value >>> 24) & 0xff);
    }

    private static void writeLe64(byte[] buf, int off, long value) {
        buf[off] = (byte) (value & 0xffL);
        buf[off + 1] = (byte) ((value >>> 8) & 0xffL);
        buf[off + 2] = (byte) ((value >>> 16) & 0xffL);
        buf[off + 3] = (byte) ((value >>> 24) & 0xffL);
        buf[off + 4] = (byte) ((value >>> 32) & 0xffL);
        buf[off + 5] = (byte) ((value >>> 40) & 0xffL);
        buf[off + 6] = (byte) ((value >>> 48) & 0xffL);
        buf[off + 7] = (byte) ((value >>> 56) & 0xffL);
    }
}
