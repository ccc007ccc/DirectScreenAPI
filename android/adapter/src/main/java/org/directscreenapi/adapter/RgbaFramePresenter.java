package org.directscreenapi.adapter;

import java.nio.ByteBuffer;
import java.util.Locale;

final class RgbaFramePresenter {
    private static final int DISPLAY_SYNC_INTERVAL_MS = 1000;

    private static final class FrameInfo {
        final long frameSeq;
        final int width;
        final int height;
        final int byteLen;

        FrameInfo(long frameSeq, int width, int height, int byteLen) {
            this.frameSeq = frameSeq;
            this.width = width;
            this.height = height;
            this.byteLen = byteLen;
        }
    }

    private final DaemonSession daemon;
    private final AndroidDisplayAdapter displayAdapter;
    private final int pollMs;
    private final int zLayer;
    private final String layerName;

    private volatile boolean running = true;
    private long lastFrameSeq = -1L;
    private long lastDisplaySyncMs = 0L;
    private long lastConnectWarnMs = 0L;
    private long perfWindowStartMs = 0L;
    private int perfFrames = 0;
    private long perfBytes = 0L;

    private SurfaceLayerSession surfaceSession;
    private int surfaceWidth = 0;
    private int surfaceHeight = 0;

    private Object bitmap;
    private int bitmapWidth = 0;
    private int bitmapHeight = 0;
    private Object destRect;

    private final Class<?> bitmapClass;
    private final Class<?> bitmapConfigClass;
    private final Object bitmapArgb8888;
    private final Class<?> rectClass;
    private final Class<?> paintClass;
    private final Class<?> porterDuffModeClass;
    private final Class<?> porterDuffXfermodeClass;
    private final Object modeSrc;
    private final Object xferSrc;
    private final Object paint;

    RgbaFramePresenter(String socketPath, int pollMs, int zLayer, String layerName) throws Exception {
        this.daemon = new DaemonSession(socketPath);
        this.displayAdapter = new AndroidDisplayAdapter();
        this.pollMs = Math.max(1, pollMs);
        this.zLayer = zLayer;
        this.layerName = layerName;

        this.bitmapClass = Class.forName("android.graphics.Bitmap");
        this.bitmapConfigClass = Class.forName("android.graphics.Bitmap$Config");
        this.bitmapArgb8888 = bitmapConfigClass.getField("ARGB_8888").get(null);
        this.rectClass = Class.forName("android.graphics.Rect");
        this.paintClass = Class.forName("android.graphics.Paint");
        this.porterDuffModeClass = Class.forName("android.graphics.PorterDuff$Mode");
        this.porterDuffXfermodeClass = Class.forName("android.graphics.PorterDuffXfermode");
        this.modeSrc = porterDuffModeClass.getField("SRC").get(null);
        this.xferSrc = porterDuffXfermodeClass
                .getDeclaredConstructor(porterDuffModeClass)
                .newInstance(modeSrc);
        this.paint = paintClass.getDeclaredConstructor().newInstance();
        ReflectBridge.invoke(this.paint, "setXfermode", xferSrc);
    }

    void runLoop() throws Exception {
        Runtime.getRuntime().addShutdownHook(new Thread(this::shutdown, "dsapi-presenter-shutdown"));
        try {
            syncDisplayAndEnsureSurface();
        } catch (Throwable t) {
            log("presenter_warn=display_sync_failed err=" + t.getClass().getSimpleName());
        }
        log("presenter_status=started poll_ms=" + pollMs + " z_layer=" + zLayer + " layer=" + layerName);

        while (running && !Thread.currentThread().isInterrupted()) {
            long now = System.currentTimeMillis();
            if (now - lastDisplaySyncMs >= DISPLAY_SYNC_INTERVAL_MS) {
                try {
                    syncDisplayAndEnsureSurface();
                } catch (Throwable t) {
                    logConnectWarn("display_sync_failed", t);
                }
                lastDisplaySyncMs = now;
            }

            FrameInfo info;
            try {
                info = readFrameInfo();
            } catch (Throwable t) {
                logConnectWarn("frame_info_failed", t);
                sleepQuietly(pollMs);
                continue;
            }

            if (info == null || info.frameSeq == lastFrameSeq) {
                sleepQuietly(pollMs);
                continue;
            }

            DaemonSession.RawFrame rawFrame;
            try {
                rawFrame = daemon.frameGetRaw();
            } catch (Throwable t) {
                log(
                        "presenter_warn=frame_read_failed seq="
                                + info.frameSeq
                                + " err="
                                + t.getClass().getSimpleName()
                                + " msg="
                                + String.valueOf(t.getMessage())
                );
                sleepQuietly(pollMs);
                continue;
            }
            if (rawFrame == null || rawFrame.frameSeq == lastFrameSeq) {
                sleepQuietly(pollMs);
                continue;
            }

            try {
                drawFrame(rawFrame.width, rawFrame.height, rawFrame.rgba8);
                daemon.command("RENDER_PRESENT");
                lastFrameSeq = rawFrame.frameSeq;
                markFramePresented(rawFrame.byteLen);
            } catch (Throwable t) {
                log("presenter_warn=draw_failed seq=" + rawFrame.frameSeq + " err=" + t.getClass().getSimpleName());
            }
        }

        shutdown();
    }

    private void shutdown() {
        running = false;
        if (bitmap != null) {
            try {
                ReflectBridge.invoke(bitmap, "recycle");
            } catch (Throwable ignored) {
            }
            bitmap = null;
        }
        if (surfaceSession != null) {
            surfaceSession.closeQuietly();
            surfaceSession = null;
        }
        daemon.closeQuietly();
        log("presenter_status=stopped");
    }

    private void syncDisplayAndEnsureSurface() throws Exception {
        DisplayAdapter.DisplaySnapshot snapshot = displayAdapter.queryDisplaySnapshot();
        int width = Math.max(1, snapshot.width);
        int height = Math.max(1, snapshot.height);
        daemon.command(
                "DISPLAY_SET "
                        + width + " "
                        + height + " "
                        + String.format(java.util.Locale.US, "%.2f", snapshot.refreshHz) + " "
                        + Math.max(1, snapshot.densityDpi) + " "
                        + Math.max(0, snapshot.rotation)
        );

        if (surfaceSession == null || surfaceWidth != width || surfaceHeight != height) {
            if (surfaceSession != null) {
                surfaceSession.closeQuietly();
            }
            surfaceSession = SurfaceLayerSession.create(width, height, zLayer, layerName);
            surfaceWidth = width;
            surfaceHeight = height;
            destRect = rectClass
                    .getDeclaredConstructor(int.class, int.class, int.class, int.class)
                    .newInstance(0, 0, surfaceWidth, surfaceHeight);
            log("presenter_surface=recreated size=" + surfaceWidth + "x" + surfaceHeight + " rotation=" + snapshot.rotation);
        }
    }

    private FrameInfo readFrameInfo() throws Exception {
        String line = daemon.command("RENDER_FRAME_GET");
        if (!line.startsWith("OK ")) {
            return null;
        }
        String[] tokens = line.split("\\s+");
        if (tokens.length < 7) {
            throw new IllegalStateException("frame_info_tokens_invalid");
        }
        long frameSeq = parseLong(tokens[1], -1L);
        int width = parseInt(tokens[2], -1);
        int height = parseInt(tokens[3], -1);
        int byteLen = parseInt(tokens[5], -1);
        if (frameSeq < 0 || width <= 0 || height <= 0 || byteLen <= 0) {
            throw new IllegalStateException("frame_info_invalid_values");
        }
        return new FrameInfo(frameSeq, width, height, byteLen);
    }

    private void drawFrame(int frameWidth, int frameHeight, byte[] rgba) throws Exception {
        ensureBitmap(frameWidth, frameHeight);
        ByteBuffer buffer = ByteBuffer.wrap(rgba);
        ReflectBridge.invoke(bitmap, "copyPixelsFromBuffer", buffer);

        Object canvas = surfaceSession.lockFrame();
        try {
            ReflectBridge.invoke(canvas, "drawBitmap", bitmap, null, destRect, paint);
        } finally {
            surfaceSession.unlockFrame(canvas);
        }
    }

    private void ensureBitmap(int width, int height) throws Exception {
        if (bitmap != null && bitmapWidth == width && bitmapHeight == height) {
            return;
        }

        if (bitmap != null) {
            try {
                ReflectBridge.invoke(bitmap, "recycle");
            } catch (Throwable ignored) {
            }
        }

        bitmap = ReflectBridge.invokeStatic(
                bitmapClass,
                "createBitmap",
                Integer.valueOf(width),
                Integer.valueOf(height),
                bitmapArgb8888
        );
        bitmapWidth = width;
        bitmapHeight = height;
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

    private static void sleepQuietly(int ms) {
        try {
            Thread.sleep(Math.max(1, ms));
        } catch (InterruptedException ie) {
            Thread.currentThread().interrupt();
        }
    }

    private void markFramePresented(int byteLen) {
        long now = System.currentTimeMillis();
        if (perfWindowStartMs <= 0L) {
            perfWindowStartMs = now;
        }
        perfFrames += 1;
        perfBytes += Math.max(0, byteLen);

        long elapsedMs = now - perfWindowStartMs;
        if (elapsedMs < 1000L) {
            return;
        }

        double fps = (perfFrames * 1000.0) / Math.max(1L, elapsedMs);
        double mbPerSec = ((perfBytes / 1024.0) / 1024.0) * (1000.0 / Math.max(1L, elapsedMs));
        log(String.format(
                Locale.US,
                "presenter_perf fps=%.1f throughput_mib_s=%.1f frame_seq=%d",
                fps,
                mbPerSec,
                lastFrameSeq
        ));
        perfWindowStartMs = now;
        perfFrames = 0;
        perfBytes = 0L;
    }

    private void logConnectWarn(String action, Throwable t) {
        long now = System.currentTimeMillis();
        if (now - lastConnectWarnMs < 1000L) {
            return;
        }
        lastConnectWarnMs = now;
        log("presenter_warn=" + action + " err=" + t.getClass().getSimpleName());
    }

    private static void log(String msg) {
        System.out.println(msg);
    }
}
