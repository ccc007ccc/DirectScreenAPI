package org.directscreenapi.adapter;

import java.io.FileDescriptor;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.ByteBuffer;
import java.nio.MappedByteBuffer;
import java.nio.channels.FileChannel;
import java.nio.charset.StandardCharsets;
import java.util.Locale;

final class DaemonSession {
    private static final int DEFAULT_SOCKET_TIMEOUT_MS = 5000;
    private static final int TRANSIENT_READ_BACKOFF_MS = 2;
    private static final String SOCKET_TIMEOUT_PROPERTY = "dsapi.socket_timeout_ms";
    private static final String PIXEL_FORMAT_RGBA8888 = "RGBA8888";

    private static final int BIN_MAGIC = 0x50415344; // "DSAP"
    private static final int BIN_VERSION = 1;
    private static final int BIN_HEADER_BYTES = 20;
    private static final int BIN_RESPONSE_VALUES = 8;
    private static final int BIN_RESPONSE_PAYLOAD_BYTES = 4 + (BIN_RESPONSE_VALUES * 8);

    private static final int BIN_OP_PING = 1;
    private static final int BIN_OP_DISPLAY_GET = 4;
    private static final int BIN_OP_DISPLAY_SET = 5;
    private static final int BIN_OP_FILTER_CHAIN_SET = 12;
    private static final int BIN_OP_FILTER_CLEAR = 13;
    private static final int BIN_OP_FILTER_GET = 14;
    private static final int FILTER_PASS_KIND_GAUSSIAN = 1;

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
            // Mapped buffer is session-scoped.
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
    private long controlSeq;

    private Object rawSocket;
    private InputStream rawInput;
    private OutputStream rawOutput;

    private FileInputStream rawFrameFdStream;
    private FileOutputStream rawFrameFdWriteStream;
    private FileChannel rawFrameFdChannel;
    private FileChannel rawFrameFdWriteChannel;
    private MappedByteBuffer rawFrameMapped;
    private int rawFrameMappedLen;
    private int rawFrameCapacity;
    private int rawFrameDataOffset = -1;

    private boolean closed;

    DaemonSession(String controlSocketPath, String dataSocketPath) throws Exception {
        this(controlSocketPath, dataSocketPath, true);
    }

    DaemonSession(String controlSocketPath, String dataSocketPath, boolean autoBindFrameShm) throws Exception {
        if (controlSocketPath == null || controlSocketPath.trim().isEmpty()) {
            throw new IOException("control_socket_path_invalid");
        }
        if (dataSocketPath == null || dataSocketPath.trim().isEmpty()) {
            throw new IOException("data_socket_path_invalid");
        }
        this.controlSocketPath = controlSocketPath;
        this.dataSocketPath = dataSocketPath;

        SocketIo control = openSocket(this.controlSocketPath);
        SocketIo data = openSocket(this.dataSocketPath);

        boolean ok = false;
        try {
            this.controlSocket = control.socket;
            this.controlInput = control.input;
            this.controlOutput = control.output;
            this.controlSeq = 1L;

            this.rawSocket = data.socket;
            this.rawInput = data.input;
            this.rawOutput = data.output;

            if (autoBindFrameShm) {
                bindFrameShm();
            }
            ok = true;
        } finally {
            if (!ok) {
                closeQuietly();
            }
        }
    }

    synchronized void ensureFrameShmBound() throws Exception {
        ensureOpen();
        if (rawFrameMapped != null && rawFrameCapacity > 0 && rawFrameMappedLen > 0) {
            return;
        }
        bindFrameShm();
    }

    synchronized String command(String cmd) throws Exception {
        ensureOpen();
        String[] tokens = splitTokens(cmd);
        if (tokens.length == 0) {
            throw new IOException("daemon_command_empty");
        }
        BinaryReply reply = executeControlCommand(tokens);
        if (reply.status != 0) {
            throw new IOException(
                    "daemon_command_status="
                            + reply.status
                            + " opcode="
                            + reply.opcode
                            + " seq="
                            + reply.seq
            );
        }
        return formatControlReply(tokens, reply);
    }

    synchronized MappedFrame frameWaitBoundPresent(long lastFrameSeq, int timeoutMs) throws Exception {
        ensureOpen();
        ensureRawFrameBound();

        long safeSeq = Math.max(0L, lastFrameSeq);
        int safeTimeout = Math.max(1, timeoutMs);
        writeAsciiLine(rawOutput, CMD_WAIT_SHM + " " + safeSeq + " " + safeTimeout);

        String line = requireOkLine(readAsciiLine(rawInput), "daemon_wait_shm_present");
        String[] tokens = line.split("\\s+");
        if (tokens.length == 2 && "TIMEOUT".equals(tokens[1])) {
            return null;
        }
        if (tokens.length != 8) {
            throw new IOException("daemon_wait_shm_present_tokens_invalid");
        }
        if (!PIXEL_FORMAT_RGBA8888.equals(tokens[4])) {
            throw new IOException("daemon_wait_shm_present_pixel_format_invalid");
        }

        long frameSeq = parseLongStrict(tokens[1], "daemon_wait_shm_present_frame_seq_invalid");
        int width = parseIntStrict(tokens[2], "daemon_wait_shm_present_width_invalid");
        int height = parseIntStrict(tokens[3], "daemon_wait_shm_present_height_invalid");
        int byteLen = parseIntStrict(tokens[5], "daemon_wait_shm_present_len_invalid");
        int offset = parseIntStrict(tokens[7], "daemon_wait_shm_present_offset_invalid");

        if (frameSeq < 0L || width <= 0 || height <= 0 || byteLen <= 0 || offset < 0) {
            throw new IOException("daemon_wait_shm_present_header_invalid");
        }
        long expectedByteLen = (long) width * (long) height * 4L;
        if (expectedByteLen <= 0L || expectedByteLen > Integer.MAX_VALUE || byteLen != (int) expectedByteLen) {
            throw new IOException("daemon_wait_shm_present_len_mismatch");
        }
        if (byteLen > rawFrameCapacity) {
            throw new IOException("daemon_wait_shm_present_len_over_capacity");
        }
        int mapEnd = safeAdd(offset, byteLen, "daemon_wait_shm_present_offset_overflow");
        if (mapEnd > rawFrameMappedLen) {
            throw new IOException("daemon_wait_shm_present_offset_overflow");
        }

        ByteBuffer view = rawFrameMapped.duplicate();
        view.position(offset);
        view.limit(mapEnd);
        ByteBuffer rgba = view.slice();
        return new MappedFrame(frameSeq, width, height, byteLen, rgba);
    }

    synchronized long frameSubmitBoundShm(int width, int height, ByteBuffer rgba8) throws Exception {
        ensureOpen();
        ensureRawFrameBound();
        if (width <= 0 || height <= 0 || rgba8 == null) {
            throw new IOException("daemon_submit_shm_args_invalid");
        }

        long expectedLong = (long) width * (long) height * 4L;
        if (expectedLong <= 0L || expectedLong > Integer.MAX_VALUE) {
            throw new IOException("daemon_submit_shm_len_invalid");
        }
        int expectedLen = (int) expectedLong;
        if (rgba8.remaining() != expectedLen) {
            throw new IOException("daemon_submit_shm_len_mismatch");
        }
        if (expectedLen > rawFrameCapacity) {
            throw new IOException("daemon_submit_shm_over_capacity");
        }

        int mapEnd = safeAdd(rawFrameDataOffset, expectedLen, "daemon_submit_shm_offset_overflow");
        if (mapEnd > rawFrameMappedLen || rawFrameFdWriteChannel == null) {
            throw new IOException("daemon_submit_shm_offset_overflow");
        }

        ByteBuffer src = rgba8.duplicate();
        long writePos = rawFrameDataOffset;
        while (src.hasRemaining()) {
            int n = rawFrameFdWriteChannel.write(src, writePos);
            if (n <= 0) {
                throw new IOException("daemon_submit_shm_write_failed");
            }
            writePos += n;
        }

        writeAsciiLine(
                rawOutput,
                "RENDER_FRAME_SUBMIT_SHM "
                        + width
                        + " "
                        + height
                        + " "
                        + expectedLen
                        + " "
                        + rawFrameDataOffset
        );
        String line = requireOkLine(readAsciiLine(rawInput), "daemon_submit_shm");
        String[] tokens = line.split("\\s+");
        if (tokens.length != 7) {
            throw new IOException("daemon_submit_shm_tokens_invalid");
        }
        if (!PIXEL_FORMAT_RGBA8888.equals(tokens[4])) {
            throw new IOException("daemon_submit_shm_pixel_format_invalid");
        }
        long frameSeq = parseLongStrict(tokens[1], "daemon_submit_shm_frame_seq_invalid");
        int outWidth = parseIntStrict(tokens[2], "daemon_submit_shm_width_invalid");
        int outHeight = parseIntStrict(tokens[3], "daemon_submit_shm_height_invalid");
        int outByteLen = parseIntStrict(tokens[5], "daemon_submit_shm_len_invalid");
        if (outWidth != width || outHeight != height || outByteLen != expectedLen) {
            throw new IOException("daemon_submit_shm_reply_mismatch");
        }
        return frameSeq;
    }

    synchronized void closeQuietly() {
        if (closed) {
            return;
        }
        closed = true;

        closeRawFrameBinding();

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
    }

    private void ensureOpen() throws IOException {
        if (closed) {
            throw new IOException("daemon_session_closed");
        }
        if (controlSocket == null || controlInput == null || controlOutput == null) {
            throw new IOException("daemon_control_socket_not_ready");
        }
        if (rawSocket == null || rawInput == null || rawOutput == null) {
            throw new IOException("daemon_data_socket_not_ready");
        }
    }

    private void ensureRawFrameBound() throws IOException {
        if (rawFrameMapped == null || rawFrameCapacity <= 0 || rawFrameMappedLen <= 0 || rawFrameDataOffset < 0) {
            throw new IOException("daemon_frame_shm_unbound");
        }
    }

    private void closeRawFrameBinding() {
        if (rawFrameFdWriteChannel != null) {
            try {
                rawFrameFdWriteChannel.close();
            } catch (Throwable ignored) {
            }
            rawFrameFdWriteChannel = null;
        }
        if (rawFrameFdChannel != null) {
            try {
                rawFrameFdChannel.close();
            } catch (Throwable ignored) {
            }
            rawFrameFdChannel = null;
        }
        if (rawFrameFdWriteStream != null) {
            try {
                rawFrameFdWriteStream.close();
            } catch (Throwable ignored) {
            }
            rawFrameFdWriteStream = null;
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
        rawFrameDataOffset = -1;
    }

    private SocketIo openSocket(String path) throws Exception {
        Class<?> localSocketClass = Class.forName("android.net.LocalSocket");
        Class<?> addressClass = Class.forName("android.net.LocalSocketAddress");
        Class<?> namespaceClass = Class.forName("android.net.LocalSocketAddress$Namespace");
        Object namespaceFilesystem = resolveNamespaceFilesystem(namespaceClass);

        Object socket = null;
        try {
            socket = localSocketClass.getDeclaredConstructor().newInstance();
            Object address = addressClass
                    .getDeclaredConstructor(String.class, namespaceClass)
                    .newInstance(path, namespaceFilesystem);
            ReflectBridge.invoke(socket, "connect", address);
            configureSocketTimeout(socket);
            OutputStream os = (OutputStream) ReflectBridge.invoke(socket, "getOutputStream");
            InputStream is = (InputStream) ReflectBridge.invoke(socket, "getInputStream");
            return new SocketIo(socket, is, os);
        } catch (Throwable t) {
            closeSocketQuietly(socket);
            throw asIOException("daemon_open_socket_failed path=" + path, t);
        }
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

    private void bindFrameShm() throws Exception {
        closeRawFrameBinding();
        writeAsciiLine(rawOutput, CMD_BIND_SHM);
        String line = requireOkLine(readAsciiLine(rawInput), "daemon_bind_shm");
        String[] tokens = line.split("\\s+");
        if (tokens.length != 4 || !"SHM_BOUND".equals(tokens[1])) {
            throw new IOException("daemon_bind_shm_tokens_invalid");
        }

        int capacity = parseIntStrict(tokens[2], "daemon_bind_shm_capacity_invalid");
        int offset = parseIntStrict(tokens[3], "daemon_bind_shm_offset_invalid");
        if (capacity <= 0 || offset < 0) {
            throw new IOException("daemon_bind_shm_layout_invalid");
        }
        int mapLen = safeAdd(capacity, offset, "daemon_bind_shm_map_len_overflow");

        FileDescriptor fd = readSingleAncillaryFd(rawSocket);
        FileInputStream readStream = new FileInputStream(fd);
        FileOutputStream writeStream = new FileOutputStream(fd);
        FileChannel readChannel = readStream.getChannel();
        FileChannel writeChannel = writeStream.getChannel();
        try {
            MappedByteBuffer mapped = readChannel.map(FileChannel.MapMode.READ_ONLY, 0L, mapLen);
            rawFrameFdStream = readStream;
            rawFrameFdWriteStream = writeStream;
            rawFrameFdChannel = readChannel;
            rawFrameFdWriteChannel = writeChannel;
            rawFrameMapped = mapped;
            rawFrameMappedLen = mapLen;
            rawFrameCapacity = capacity;
            rawFrameDataOffset = offset;
        } catch (Throwable t) {
            try {
                writeChannel.close();
            } catch (Throwable ignored) {
            }
            try {
                readChannel.close();
            } catch (Throwable ignored) {
            }
            try {
                writeStream.close();
            } catch (Throwable ignored) {
            }
            try {
                readStream.close();
            } catch (Throwable ignored) {
            }
            throw t;
        }
    }

    private BinaryReply executeControlCommand(String[] tokens) throws Exception {
        String cmd = tokens[0].toUpperCase(Locale.US);
        if ("DISPLAY_SET".equals(cmd)) {
            if (tokens.length != 6) {
                throw new IOException("display_set_args_invalid");
            }
            int width = parseIntStrict(tokens[1], "display_set_width_invalid");
            int height = parseIntStrict(tokens[2], "display_set_height_invalid");
            float refresh = parseFloatStrict(tokens[3], "display_set_refresh_invalid");
            int dpi = parseIntStrict(tokens[4], "display_set_dpi_invalid");
            int rotation = parseIntStrict(tokens[5], "display_set_rotation_invalid");
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
        if ("FILTER_SET_GAUSSIAN".equals(cmd)) {
            if (tokens.length != 3) {
                throw new IOException("filter_set_gaussian_args_invalid");
            }
            int radius = parseU32BitsStrict(tokens[1], "filter_set_gaussian_radius_invalid");
            float sigma = parseFloatStrict(tokens[2], "filter_set_gaussian_sigma_invalid");
            return sendBinaryControl(
                    BIN_OP_FILTER_CHAIN_SET,
                    buildFilterChainPayload(
                            new int[]{radius},
                            new float[]{sigma}
                    )
            );
        }
        if ("FILTER_CHAIN_SET".equals(cmd)) {
            if (tokens.length < 2) {
                throw new IOException("filter_chain_set_args_invalid");
            }
            int passCount = parsePassCountStrict(tokens[1], "filter_chain_set_args_invalid");
            long expectedTokens = 2L + ((long) passCount * 2L);
            if (tokens.length != expectedTokens) {
                throw new IOException("filter_chain_set_args_invalid");
            }

            int[] radii = new int[passCount];
            float[] sigmas = new float[passCount];
            for (int i = 0; i < passCount; i++) {
                int tokenIdx = 2 + (i * 2);
                radii[i] = parseU32BitsStrict(tokens[tokenIdx], "filter_chain_set_radius_invalid");
                sigmas[i] = parseFloatStrict(tokens[tokenIdx + 1], "filter_chain_set_sigma_invalid");
            }
            return sendBinaryControl(BIN_OP_FILTER_CHAIN_SET, buildFilterChainPayload(radii, sigmas));
        }
        if ("FILTER_CLEAR".equals(cmd) || "FILTER_CHAIN_CLEAR".equals(cmd)) {
            if (tokens.length != 1) {
                throw new IOException("filter_clear_args_invalid");
            }
            return sendBinaryControl(BIN_OP_FILTER_CLEAR, new byte[0]);
        }
        if ("FILTER_GET".equals(cmd)) {
            if (tokens.length != 1) {
                throw new IOException("filter_get_args_invalid");
            }
            return sendBinaryControl(BIN_OP_FILTER_GET, new byte[0]);
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
        String cmd = tokens[0].toUpperCase(Locale.US);
        if ("DISPLAY_SET".equals(cmd)) {
            return "OK";
        }
        if ("DISPLAY_GET".equals(cmd)) {
            float refresh = Float.intBitsToFloat((int) reply.values[2]);
            return "OK "
                    + reply.values[0] + " "
                    + reply.values[1] + " "
                    + String.format(Locale.US, "%.2f", refresh) + " "
                    + reply.values[3] + " "
                    + reply.values[4];
        }
        if ("PING".equals(cmd)) {
            return "OK PONG";
        }
        if ("FILTER_SET_GAUSSIAN".equals(cmd)
                || "FILTER_CHAIN_SET".equals(cmd)
                || "FILTER_CLEAR".equals(cmd)
                || "FILTER_CHAIN_CLEAR".equals(cmd)
                || "FILTER_GET".equals(cmd)) {
            return formatFilterInfo(reply.values);
        }
        throw new IOException("control_reply_unsupported:" + cmd);
    }

    private static String formatFilterInfo(long[] values) {
        String backend = values[0] == 1L ? "vulkan" : "cpu";
        long gpuActive = values[1];
        long passCount = values[2];
        long firstRadius = values[3];
        float firstSigma = Float.intBitsToFloat((int) values[4]);
        long secondRadius = values[5];
        float secondSigma = Float.intBitsToFloat((int) values[6]);
        return String.format(
                Locale.US,
                "OK backend=%s gpu_active=%d pass_count=%d first_gaussian=%d:%.3f second_gaussian=%d:%.3f",
                backend,
                gpuActive,
                passCount,
                firstRadius,
                firstSigma,
                secondRadius,
                secondSigma
        );
    }

    private static byte[] buildFilterChainPayload(int[] radii, float[] sigmas) throws IOException {
        if (radii == null || sigmas == null || radii.length != sigmas.length) {
            throw new IOException("filter_chain_payload_invalid");
        }
        int passCount = radii.length;
        long payloadLen = 4L + (12L * (long) passCount);
        if (payloadLen <= 0L || payloadLen > Integer.MAX_VALUE) {
            throw new IOException("filter_chain_payload_len_invalid");
        }

        byte[] payload = new byte[(int) payloadLen];
        writeLe32(payload, 0, passCount);
        int cursor = 4;
        for (int i = 0; i < passCount; i++) {
            writeLe32(payload, cursor, FILTER_PASS_KIND_GAUSSIAN);
            writeLe32(payload, cursor + 4, radii[i]);
            writeLe32(payload, cursor + 8, Float.floatToIntBits(sigmas[i]));
            cursor += 12;
        }
        return payload;
    }

    private BinaryReply sendBinaryControl(int opcode, byte[] payload) throws Exception {
        byte[] body = payload == null ? new byte[0] : payload;

        byte[] header = new byte[BIN_HEADER_BYTES];
        writeLe32(header, 0, BIN_MAGIC);
        writeLe16(header, 4, BIN_VERSION);
        writeLe16(header, 6, opcode);
        writeLe32(header, 8, body.length);
        writeLe64(header, 12, controlSeq);

        controlOutput.write(header);
        if (body.length > 0) {
            controlOutput.write(body);
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
        if (respOpcode != opcode) {
            throw new IOException("daemon_control_opcode_mismatch");
        }
        if (seq != controlSeq) {
            throw new IOException("daemon_control_seq_mismatch");
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

        BinaryReply reply = new BinaryReply(seq, respOpcode, status, values);
        controlSeq += 1L;
        return reply;
    }

    private static void writeAsciiLine(OutputStream os, String line) throws IOException {
        if (line == null) {
            throw new IOException("daemon_line_null");
        }
        os.write(line.getBytes(StandardCharsets.UTF_8));
        os.write('\n');
        os.flush();
    }

    private static String readAsciiLine(InputStream is) throws IOException {
        StringBuilder sb = new StringBuilder(128);
        byte[] chunk = new byte[256];
        long deadlineMs = computeReadDeadlineMs();
        while (true) {
            int n = readWithRetry(is, chunk, 0, chunk.length, deadlineMs, "daemon_line_read_timeout");
            if (n < 0) {
                if (sb.length() == 0) {
                    return null;
                }
                return sb.toString();
            }

            for (int i = 0; i < n; i++) {
                int b = chunk[i] & 0xff;
                if (b == '\n') {
                    return sb.toString();
                }
                if (b != '\r') {
                    sb.append((char) b);
                }
                if (sb.length() > 4096) {
                    throw new IOException("daemon_line_too_long");
                }
            }
        }
    }

    private static String requireOkLine(String line, String context) throws IOException {
        if (line == null) {
            throw new IOException(context + "_eof");
        }
        String trimmed = line.trim();
        if ("OK".equals(trimmed) || trimmed.startsWith("OK ")) {
            return trimmed;
        }
        if ("ERR".equals(trimmed) || trimmed.startsWith("ERR ")) {
            throw new IOException(context + "_err=" + trimmed);
        }
        throw new IOException(context + "_bad_line=" + trimmed);
    }

    private static void configureSocketTimeout(Object socket) throws IOException {
        int timeoutMs = resolveSocketTimeoutMs();
        try {
            ReflectBridge.invoke(socket, "setSoTimeout", Integer.valueOf(timeoutMs));
        } catch (Throwable t) {
            throw asIOException("daemon_socket_timeout_config_failed", t);
        }
    }

    private static int resolveSocketTimeoutMs() {
        int timeoutMs;
        try {
            timeoutMs = Integer.parseInt(System.getProperty(SOCKET_TIMEOUT_PROPERTY));
        } catch (Throwable ignored) {
            timeoutMs = DEFAULT_SOCKET_TIMEOUT_MS;
        }
        if (timeoutMs <= 0) {
            return DEFAULT_SOCKET_TIMEOUT_MS;
        }
        return timeoutMs;
    }

    private static Object resolveNamespaceFilesystem(Class<?> namespaceClass) throws Exception {
        Object[] constants = namespaceClass.getEnumConstants();
        if (constants == null || constants.length == 0) {
            throw new IOException("localsocket_namespace_missing");
        }
        for (Object constant : constants) {
            if (constant instanceof Enum && "FILESYSTEM".equals(((Enum<?>) constant).name())) {
                return constant;
            }
        }
        return constants[0];
    }

    private static FileDescriptor readSingleAncillaryFd(Object socket) throws Exception {
        Object out = ReflectBridge.invoke(socket, "getAncillaryFileDescriptors");
        if (!(out instanceof FileDescriptor[])) {
            throw new IOException("daemon_frame_fd_missing_ancillary");
        }
        FileDescriptor[] fds = (FileDescriptor[]) out;
        for (FileDescriptor fd : fds) {
            if (fd != null) {
                return fd;
            }
        }
        throw new IOException("daemon_frame_fd_missing_ancillary");
    }

    private static IOException asIOException(String prefix, Throwable t) {
        Throwable root = rootCause(t);
        String detail = describeThrowable(root);
        String message = prefix == null || prefix.isEmpty() ? detail : prefix + ":" + detail;
        return new IOException(message, root);
    }

    private static Throwable rootCause(Throwable t) {
        if (t == null) {
            return new IOException("unknown_error");
        }
        Throwable cur = t;
        for (int i = 0; i < 16; i++) {
            Throwable cause = cur.getCause();
            if (cause == null || cause == cur) {
                return cur;
            }
            cur = cause;
        }
        return cur;
    }

    private static String describeThrowable(Throwable t) {
        if (t == null) {
            return "Unknown";
        }
        String name = t.getClass().getSimpleName();
        String msg = t.getMessage();
        if (msg == null || msg.isEmpty()) {
            return name;
        }
        return name + ":" + msg.replace('\n', ' ').replace('\r', ' ');
    }

    private static int parseIntStrict(String s, String err) throws IOException {
        try {
            return Integer.parseInt(s);
        } catch (Throwable t) {
            throw new IOException(err, t);
        }
    }

    private static int parseU32BitsStrict(String s, String err) throws IOException {
        try {
            long parsed = Long.parseLong(s);
            if (parsed < 0L || parsed > 0xffff_ffffL) {
                throw new NumberFormatException("u32_out_of_range");
            }
            return (int) parsed;
        } catch (Throwable t) {
            throw new IOException(err, t);
        }
    }

    private static int parsePassCountStrict(String s, String err) throws IOException {
        try {
            long parsed = Long.parseLong(s);
            if (parsed < 0L || parsed > Integer.MAX_VALUE) {
                throw new NumberFormatException("pass_count_out_of_range");
            }
            return (int) parsed;
        } catch (Throwable t) {
            throw new IOException(err, t);
        }
    }

    private static long parseLongStrict(String s, String err) throws IOException {
        try {
            return Long.parseLong(s);
        } catch (Throwable t) {
            throw new IOException(err, t);
        }
    }

    private static float parseFloatStrict(String s, String err) throws IOException {
        try {
            return Float.parseFloat(s);
        } catch (Throwable t) {
            throw new IOException(err, t);
        }
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
        long deadlineMs = computeReadDeadlineMs();
        while (offset < out.length) {
            int n = readWithRetry(
                    is,
                    out,
                    offset,
                    out.length - offset,
                    deadlineMs,
                    "daemon_binary_read_timeout"
            );
            if (n < 0) {
                throw new IOException("daemon_binary_eof");
            }
            offset += n;
        }
    }

    private static int readWithRetry(
            InputStream is,
            byte[] dst,
            int off,
            int len,
            long deadlineMs,
            String timeoutMsg
    ) throws IOException {
        while (true) {
            try {
                return is.read(dst, off, len);
            } catch (IOException e) {
                if (isTransientReadError(e)) {
                    if (isDeadlineReached(deadlineMs)) {
                        throw new IOException(timeoutMsg, e);
                    }
                    sleepBackoff();
                    continue;
                }
                throw e;
            }
        }
    }

    private static long computeReadDeadlineMs() {
        int timeoutMs = resolveSocketTimeoutMs();
        if (timeoutMs <= 0) {
            return 0L;
        }
        return System.currentTimeMillis() + timeoutMs;
    }

    private static boolean isDeadlineReached(long deadlineMs) {
        return deadlineMs > 0L && System.currentTimeMillis() >= deadlineMs;
    }

    private static void sleepBackoff() throws IOException {
        try {
            Thread.sleep(TRANSIENT_READ_BACKOFF_MS);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new IOException("daemon_read_interrupted", e);
        }
    }

    private static boolean isTransientReadError(IOException e) {
        if (e == null) {
            return false;
        }
        String msg = e.getMessage();
        if (msg == null || msg.isEmpty()) {
            return false;
        }
        String lower = msg.toLowerCase(Locale.US);
        return lower.contains("try again")
                || lower.contains("temporarily unavailable")
                || lower.contains("would block")
                || lower.contains("resource temporarily unavailable");
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
