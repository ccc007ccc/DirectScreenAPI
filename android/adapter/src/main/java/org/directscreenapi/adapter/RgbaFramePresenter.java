package org.directscreenapi.adapter;

import java.nio.ByteBuffer;
import java.util.Locale;

final class RgbaFramePresenter {
    private static final int DISPLAY_SYNC_INTERVAL_MS = 1000;

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

            DaemonSession.MappedFrame mappedFrame;
            try {
                mappedFrame = daemon.frameWaitBoundPresent(lastFrameSeq, pollMs);
            } catch (Throwable t) {
                logConnectWarn("frame_wait_bound_present_failed", t);
                continue;
            }
            if (mappedFrame == null || mappedFrame.frameSeq == lastFrameSeq) {
                if (mappedFrame != null) {
                    mappedFrame.closeQuietly();
                }
                continue;
            }

            try {
                drawFrame(mappedFrame.width, mappedFrame.height, mappedFrame.rgba8);
                lastFrameSeq = mappedFrame.frameSeq;
                markFramePresented(mappedFrame.byteLen);
            } catch (Throwable t) {
                log("presenter_warn=draw_failed seq=" + mappedFrame.frameSeq + " err=" + t.getClass().getSimpleName());
            } finally {
                mappedFrame.closeQuietly();
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

    private void drawFrame(int frameWidth, int frameHeight, ByteBuffer rgba) throws Exception {
        ensureBitmap(frameWidth, frameHeight);
        rgba.position(0);
        ReflectBridge.invoke(bitmap, "copyPixelsFromBuffer", rgba);

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
