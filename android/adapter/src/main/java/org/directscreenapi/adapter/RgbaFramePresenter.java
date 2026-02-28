package org.directscreenapi.adapter;

import java.nio.ByteBuffer;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.Locale;
import java.util.concurrent.atomic.AtomicBoolean;

final class RgbaFramePresenter {
    private static final int DISPLAY_SYNC_FALLBACK_INTERVAL_MS = 5000;

    private static final class DisplayListenerInvocationHandler implements InvocationHandler {
        private final AtomicBoolean dirtyFlag;

        DisplayListenerInvocationHandler(AtomicBoolean dirtyFlag) {
            this.dirtyFlag = dirtyFlag;
        }

        @Override
        public Object invoke(Object proxy, Method method, Object[] args) {
            String name = method.getName();
            if ("onDisplayAdded".equals(name)
                    || "onDisplayChanged".equals(name)
                    || "onDisplayRemoved".equals(name)) {
                dirtyFlag.set(true);
            }
            return null;
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
    private final AtomicBoolean displayDirty = new AtomicBoolean(true);

    private SurfaceLayerSession surfaceSession;
    private int surfaceWidth = 0;
    private int surfaceHeight = 0;
    private int pendingSurfaceWidth = 0;
    private int pendingSurfaceHeight = 0;
    private int pendingSurfaceRotation = 0;
    private SurfaceLayerSession pendingSurfaceSession;
    private Object pendingDestRect;
    private Object displayManager;
    private Object displayListener;
    private Object displayListenerThread;

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

    RgbaFramePresenter(String controlSocketPath, String dataSocketPath, int pollMs, int zLayer, String layerName) throws Exception {
        this.daemon = new DaemonSession(controlSocketPath, dataSocketPath);
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

        initDisplayListener();
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
            boolean shouldSync = displayDirty.getAndSet(false);
            if (!shouldSync && (now - lastDisplaySyncMs >= DISPLAY_SYNC_FALLBACK_INTERVAL_MS)) {
                shouldSync = true;
            }
            if (shouldSync) {
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
                maybeSwitchSurfaceForFrame(mappedFrame.width, mappedFrame.height);
                if (mappedFrame.width == surfaceWidth && mappedFrame.height == surfaceHeight) {
                    drawFrame(mappedFrame.width, mappedFrame.height, mappedFrame.rgba8);
                    lastFrameSeq = mappedFrame.frameSeq;
                    markFramePresented(mappedFrame.byteLen);
                    continue;
                }

                if (pendingSurfaceSession != null
                        && mappedFrame.width == pendingSurfaceWidth
                        && mappedFrame.height == pendingSurfaceHeight) {
                    drawFrameToTarget(
                            pendingSurfaceSession,
                            pendingDestRect,
                            mappedFrame.rgba8,
                            mappedFrame.width,
                            mappedFrame.height
                    );
                    activatePendingSurface();
                    lastFrameSeq = mappedFrame.frameSeq;
                    markFramePresented(mappedFrame.byteLen);
                    continue;
                }

                // 仅丢弃既不匹配当前 surface、也不匹配待切换尺寸的异常帧。
                log("presenter_warn=drop_mismatched_frame frame="
                        + mappedFrame.width + "x" + mappedFrame.height
                        + " surface=" + surfaceWidth + "x" + surfaceHeight
                        + " pending=" + pendingSurfaceWidth + "x" + pendingSurfaceHeight
                        + " seq=" + mappedFrame.frameSeq);
                lastFrameSeq = mappedFrame.frameSeq;
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
        if (pendingSurfaceSession != null) {
            pendingSurfaceSession.closeQuietly();
            pendingSurfaceSession = null;
        }
        if (displayManager != null && displayListener != null) {
            try {
                ReflectBridge.invoke(displayManager, "unregisterDisplayListener", displayListener);
            } catch (Throwable ignored) {
            }
            displayListener = null;
            displayManager = null;
        }
        if (displayListenerThread != null) {
            try {
                ReflectBridge.invoke(displayListenerThread, "quitSafely");
            } catch (Throwable ignored) {
                try {
                    ReflectBridge.invoke(displayListenerThread, "quit");
                } catch (Throwable ignored2) {
                }
            }
            displayListenerThread = null;
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

        if (surfaceSession == null) {
            recreateSurface(width, height, snapshot.rotation);
            pendingSurfaceWidth = 0;
            pendingSurfaceHeight = 0;
            pendingSurfaceRotation = 0;
            pendingDestRect = null;
            if (pendingSurfaceSession != null) {
                pendingSurfaceSession.closeQuietly();
                pendingSurfaceSession = null;
            }
            return;
        }

        if (surfaceWidth != width || surfaceHeight != height) {
            if (pendingSurfaceWidth != width || pendingSurfaceHeight != height) {
                if (pendingSurfaceSession != null) {
                    pendingSurfaceSession.closeQuietly();
                    pendingSurfaceSession = null;
                }
                pendingDestRect = null;
                pendingSurfaceWidth = width;
                pendingSurfaceHeight = height;
                pendingSurfaceRotation = snapshot.rotation;
                log("presenter_surface=pending_resize from="
                        + surfaceWidth + "x" + surfaceHeight
                        + " to=" + pendingSurfaceWidth + "x" + pendingSurfaceHeight
                        + " rotation=" + pendingSurfaceRotation);
            }
        }
    }

    private void maybeSwitchSurfaceForFrame(int frameWidth, int frameHeight) throws Exception {
        if (pendingSurfaceWidth <= 0 || pendingSurfaceHeight <= 0) {
            return;
        }
        if (frameWidth != pendingSurfaceWidth || frameHeight != pendingSurfaceHeight) {
            return;
        }
        if (pendingSurfaceSession == null) {
            pendingSurfaceSession = SurfaceLayerSession.create(
                    pendingSurfaceWidth,
                    pendingSurfaceHeight,
                    zLayer,
                    layerName,
                    false
            );
            pendingDestRect = rectClass
                    .getDeclaredConstructor(int.class, int.class, int.class, int.class)
                    .newInstance(0, 0, pendingSurfaceWidth, pendingSurfaceHeight);
        }
    }

    private void recreateSurface(int width, int height, int rotation) throws Exception {
        SurfaceLayerSession newSession = SurfaceLayerSession.create(width, height, zLayer, layerName);
        Object newRect = rectClass
                .getDeclaredConstructor(int.class, int.class, int.class, int.class)
                .newInstance(0, 0, width, height);

        SurfaceLayerSession oldSession = surfaceSession;
        surfaceSession = newSession;
        surfaceWidth = width;
        surfaceHeight = height;
        destRect = newRect;

        if (oldSession != null) {
            oldSession.closeQuietly();
        }
        log("presenter_surface=recreated size=" + surfaceWidth + "x" + surfaceHeight + " rotation=" + rotation);
    }

    private void activatePendingSurface() throws Exception {
        if (pendingSurfaceSession == null || pendingDestRect == null) {
            return;
        }
        pendingSurfaceSession.show();

        SurfaceLayerSession oldSession = surfaceSession;
        surfaceSession = pendingSurfaceSession;
        surfaceWidth = pendingSurfaceWidth;
        surfaceHeight = pendingSurfaceHeight;
        destRect = pendingDestRect;

        pendingSurfaceSession = null;
        pendingDestRect = null;
        pendingSurfaceWidth = 0;
        pendingSurfaceHeight = 0;
        pendingSurfaceRotation = 0;

        if (oldSession != null) {
            oldSession.closeQuietly();
        }
    }

    private void drawFrameToTarget(SurfaceLayerSession targetSession, Object targetRect, ByteBuffer rgba, int frameWidth, int frameHeight) throws Exception {
        ensureBitmap(frameWidth, frameHeight);
        rgba.position(0);
        ReflectBridge.invoke(bitmap, "copyPixelsFromBuffer", rgba);

        Object canvas = targetSession.lockFrame();
        try {
            ReflectBridge.invoke(canvas, "drawBitmap", bitmap, null, targetRect, paint);
        } finally {
            targetSession.unlockFrame(canvas);
        }
    }

    private void drawFrame(int frameWidth, int frameHeight, ByteBuffer rgba) throws Exception {
        drawFrameToTarget(surfaceSession, destRect, rgba, frameWidth, frameHeight);
    }

    private void initDisplayListener() {
        try {
            Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
            Method currentApplication = activityThreadClass.getMethod("currentApplication");
            Object app = currentApplication.invoke(null);
            if (app == null) {
                log("presenter_warn=display_listener_no_application fallback=poll");
                return;
            }

            Object dm = ReflectBridge.invoke(app, "getSystemService", "display");
            if (dm == null) {
                log("presenter_warn=display_listener_no_display_manager fallback=poll");
                return;
            }

            Class<?> listenerIface = Class.forName("android.hardware.display.DisplayManager$DisplayListener");
            InvocationHandler handler = new DisplayListenerInvocationHandler(displayDirty);
            Object listener = Proxy.newProxyInstance(
                    listenerIface.getClassLoader(),
                    new Class<?>[]{listenerIface},
                    handler
            );

            Class<?> handlerThreadClass = Class.forName("android.os.HandlerThread");
            Object ht = handlerThreadClass
                    .getDeclaredConstructor(String.class)
                    .newInstance("dsapi-display-listener");
            ReflectBridge.invoke(ht, "start");
            Object looper = ReflectBridge.invoke(ht, "getLooper");

            Class<?> looperClass = Class.forName("android.os.Looper");
            Class<?> handlerClass = Class.forName("android.os.Handler");
            Object h = handlerClass
                    .getDeclaredConstructor(looperClass)
                    .newInstance(looper);

            ReflectBridge.invoke(dm, "registerDisplayListener", listener, h);

            displayManager = dm;
            displayListener = listener;
            displayListenerThread = ht;
            log("presenter_status=display_listener_enabled");
        } catch (Throwable t) {
            log("presenter_warn=display_listener_init_failed err=" + t.getClass().getSimpleName() + " fallback=poll");
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
