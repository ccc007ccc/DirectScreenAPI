package org.directscreenapi.adapter;

import java.io.FileDescriptor;

final class HardwareBufferBridge {
    static final class NativeFrame {
        final FileDescriptor fd;
        final long frameSeq;
        final int width;
        final int height;
        final int stride;
        final int format;
        final long usage;
        final int byteOffset;
        final int byteLen;

        NativeFrame(
                FileDescriptor fd,
                long frameSeq,
                int width,
                int height,
                int stride,
                int format,
                long usage,
                int byteOffset,
                int byteLen
        ) {
            this.fd = fd;
            this.frameSeq = frameSeq;
            this.width = width;
            this.height = height;
            this.stride = stride;
            this.format = format;
            this.usage = usage;
            this.byteOffset = byteOffset;
            this.byteLen = byteLen;
        }
    }

    private static final String BRIDGE_LIB_ENV = "DSAPI_HWBUFFER_BRIDGE_LIB";
    private static final Object LOAD_LOCK = new Object();
    private static volatile boolean loadAttempted;
    private static volatile boolean loaded;

    private HardwareBufferBridge() {
    }

    static boolean isAvailable() {
        ensureLoaded();
        return loaded;
    }

    static NativeFrame exportFrame(Object hardwareBuffer, long frameSeq) {
        if (hardwareBuffer == null) {
            return null;
        }
        if (!isAvailable()) {
            return null;
        }
        try {
            return nativeExportFrame(hardwareBuffer, frameSeq);
        } catch (Throwable ignored) {
            return null;
        }
    }

    static void closeQuietly(FileDescriptor fd) {
        if (fd == null) {
            return;
        }
        if (!isAvailable()) {
            return;
        }
        try {
            nativeCloseFd(fd);
        } catch (Throwable ignored) {
        }
    }

    private static void ensureLoaded() {
        if (loadAttempted) {
            return;
        }
        synchronized (LOAD_LOCK) {
            if (loadAttempted) {
                return;
            }
            loadAttempted = true;
            String path = System.getenv(BRIDGE_LIB_ENV);
            if (path == null || path.isEmpty()) {
                loaded = false;
                return;
            }
            try {
                System.load(path);
                loaded = true;
            } catch (Throwable ignored) {
                loaded = false;
            }
        }
    }

    private static native NativeFrame nativeExportFrame(Object hardwareBuffer, long frameSeq);

    private static native void nativeCloseFd(FileDescriptor fd);
}
