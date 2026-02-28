package org.directscreenapi.adapter;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.io.OutputStreamWriter;
import java.nio.ByteBuffer;
import java.nio.MappedByteBuffer;
import java.nio.channels.FileChannel;
import java.nio.charset.StandardCharsets;

final class DaemonSession {
    static final class MappedFrame {
        final long frameSeq;
        final int width;
        final int height;
        final int byteLen;
        final ByteBuffer rgba8;

        MappedFrame(
                long frameSeq,
                int width,
                int height,
                int byteLen,
                ByteBuffer rgba8
        ) {
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

    private final String socketPath;
    private Object localSocket;
    private BufferedReader reader;
    private BufferedWriter writer;
    private Object rawSocket;
    private InputStream rawInput;
    private OutputStream rawOutput;
    private FileInputStream rawFrameFdStream;
    private FileChannel rawFrameFdChannel;
    private MappedByteBuffer rawFrameMapped;
    private int rawFrameCapacity;

    DaemonSession(String socketPath) {
        this.socketPath = socketPath;
    }

    synchronized String command(String cmd) throws Exception {
        Exception last = null;
        for (int attempt = 0; attempt < 2; attempt++) {
            try {
                ensureConnected();
                writer.write(cmd);
                writer.write('\n');
                writer.flush();
                String line = reader.readLine();
                if (line == null) {
                    throw new IOException("daemon_eof");
                }
                return line.trim();
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
                writeAsciiLine(rawOutput, "RENDER_FRAME_WAIT_BOUND_PRESENT " + safeSeq + " " + safeTimeout);
                String line = readAsciiLine(rawInput);
                if (line == null) {
                    throw new IOException("daemon_eof");
                }
                String trimmed = line.trim();
                if ("OK TIMEOUT".equals(trimmed)) {
                    return null;
                }
                if (trimmed.startsWith("ERR ")) {
                    throw new IOException("daemon_wait_bound_present_err=" + trimmed);
                }
                if (!trimmed.startsWith("OK ")) {
                    throw new IOException("daemon_wait_bound_present_bad_line");
                }
                String[] tokens = trimmed.split("\\s+");
                if (tokens.length < 7) {
                    throw new IOException("daemon_wait_bound_present_tokens_invalid");
                }

                long frameSeq = parseLong(tokens[1], -1L);
                int width = parseInt(tokens[2], -1);
                int height = parseInt(tokens[3], -1);
                int byteLen = parseInt(tokens[5], -1);
                if (frameSeq < 0 || width <= 0 || height <= 0 || byteLen <= 0) {
                    throw new IOException("daemon_wait_bound_present_header_invalid");
                }
                if (rawFrameMapped == null || rawFrameCapacity <= 0) {
                    throw new IOException("daemon_wait_bound_present_uninitialized");
                }
                if (byteLen > rawFrameCapacity) {
                    throw new IOException("daemon_wait_bound_present_len_over_capacity");
                }

                ByteBuffer view = rawFrameMapped.duplicate();
                view.position(0);
                view.limit(byteLen);
                ByteBuffer rgba = view.slice();
                return new MappedFrame(frameSeq, width, height, byteLen, rgba);
            } catch (Exception e) {
                last = e;
                closeRawQuietly();
            }
        }
        throw last != null ? last : new IOException("daemon_wait_bound_present_failed");
    }

    synchronized void closeQuietly() {
        if (reader != null) {
            try {
                reader.close();
            } catch (Throwable ignored) {
            }
            reader = null;
        }
        if (writer != null) {
            try {
                writer.close();
            } catch (Throwable ignored) {
            }
            writer = null;
        }
        if (localSocket != null) {
            try {
                ReflectBridge.invoke(localSocket, "close");
            } catch (Throwable ignored) {
            }
            localSocket = null;
        }
        closeRawQuietly();
    }

    private void ensureConnected() throws Exception {
        if (localSocket != null && reader != null && writer != null) {
            return;
        }

        SocketIo io = openSocket();
        this.localSocket = io.socket;
        this.writer = new BufferedWriter(new OutputStreamWriter(io.output, StandardCharsets.UTF_8));
        this.reader = new BufferedReader(new InputStreamReader(io.input, StandardCharsets.UTF_8));
    }

    private SocketIo openSocket() throws Exception {
        Class<?> localSocketClass = Class.forName("android.net.LocalSocket");
        Class<?> addressClass = Class.forName("android.net.LocalSocketAddress");
        Class<?> namespaceClass = Class.forName("android.net.LocalSocketAddress$Namespace");
        Object namespaceFilesystem = resolveNamespaceFilesystem(namespaceClass);

        Object socket = localSocketClass.getDeclaredConstructor().newInstance();
        Object address = addressClass
                .getDeclaredConstructor(String.class, namespaceClass)
                .newInstance(socketPath, namespaceFilesystem);
        ReflectBridge.invoke(socket, "connect", address);

        OutputStream os = (OutputStream) ReflectBridge.invoke(socket, "getOutputStream");
        InputStream is = (InputStream) ReflectBridge.invoke(socket, "getInputStream");
        return new SocketIo(socket, is, os);
    }

    private void ensureRawConnected() throws Exception {
        if (rawSocket != null && rawInput != null && rawOutput != null) {
            return;
        }
        SocketIo io = openSocket();
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
        rawFrameCapacity = 0;

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

        writeAsciiLine(rawOutput, "RENDER_FRAME_BIND_FD");
        String line = readAsciiLine(rawInput);
        if (line == null) {
            throw new IOException("daemon_eof");
        }
        String trimmed = line.trim();
        if (!trimmed.startsWith("OK ")) {
            throw new IOException("daemon_bind_fd_bad_line");
        }
        String[] tokens = trimmed.split("\\s+");
        if (tokens.length < 3 || !"BOUND".equals(tokens[1])) {
            throw new IOException("daemon_bind_fd_tokens_invalid");
        }
        int capacity = parseInt(tokens[2], -1);
        if (capacity <= 0) {
            throw new IOException("daemon_bind_fd_capacity_invalid");
        }

        FileDescriptor fd = pollSingleAncillaryFd(rawSocket);
        FileInputStream stream = new FileInputStream(fd);
        FileChannel channel = stream.getChannel();
        try {
            MappedByteBuffer mapped = channel.map(FileChannel.MapMode.READ_ONLY, 0L, capacity);
            rawFrameFdStream = stream;
            rawFrameFdChannel = channel;
            rawFrameMapped = mapped;
            rawFrameCapacity = capacity;
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
}
