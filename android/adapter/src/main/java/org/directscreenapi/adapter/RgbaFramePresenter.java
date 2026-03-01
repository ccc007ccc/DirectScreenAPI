package org.directscreenapi.adapter;

import java.nio.ByteBuffer;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.Locale;
import java.util.concurrent.atomic.AtomicBoolean;

final class RgbaFramePresenter {
    private static final int DEFAULT_DISPLAY_SYNC_FALLBACK_INTERVAL_MS = 250;
    private static final String DISPLAY_SYNC_FALLBACK_PROPERTY = "dsapi.display_sync_fallback_ms";

    private static final class DisplayListenerInvocationHandler implements InvocationHandler {
        private final AtomicBoolean dirtyFlag;

        DisplayListenerInvocationHandler(AtomicBoolean dirtyFlag) {
            this.dirtyFlag = dirtyFlag;
        }

        @Override
        public Object invoke(Object proxy, Method method, Object[] args) {
            String name = method.getName();
            if ("equals".equals(name) && method.getParameterTypes().length == 1) {
                Object other = (args != null && args.length > 0) ? args[0] : null;
                return Boolean.valueOf(proxy == other);
            }
            if ("hashCode".equals(name) && method.getParameterTypes().length == 0) {
                return Integer.valueOf(System.identityHashCode(proxy));
            }
            if ("toString".equals(name) && method.getParameterTypes().length == 0) {
                return "DisplayListenerProxy";
            }
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
    private final int displaySyncFallbackIntervalMs;
    private final int zLayer;
    private final String layerName;
    private final AtomicBoolean shutdownOnce = new AtomicBoolean(false);

    private volatile boolean running = true;
    private long lastFrameSeq = -1L;
    private long lastDisplaySyncMs = 0L;
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
        this.displaySyncFallbackIntervalMs = resolveDisplaySyncFallbackIntervalMs();
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
            log("presenter_status=started poll_ms=" + pollMs + " z_layer=" + zLayer + " layer=" + layerName);

            while (running && !Thread.currentThread().isInterrupted()) {
                long now = System.currentTimeMillis();
                boolean shouldSync = displayDirty.getAndSet(false);
                if (!shouldSync && (now - lastDisplaySyncMs >= displaySyncFallbackIntervalMs)) {
                    shouldSync = true;
                }
                if (shouldSync) {
                    syncDisplayAndEnsureSurface();
                    lastDisplaySyncMs = now;
                }

                DaemonSession.MappedFrame mappedFrame = daemon.frameWaitBoundPresent(lastFrameSeq, pollMs);
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
                } finally {
                    mappedFrame.closeQuietly();
                }
            }
        } finally {
            shutdown();
        }
    }

    private void shutdown() {
        if (!shutdownOnce.compareAndSet(false, true)) {
            return;
        }
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
        Object dm = null;
        Object listener = null;
        Object ht = null;
        boolean registered = false;
        try {
            dm = resolveDisplayManager();
            if (dm == null) {
                log("presenter_warn=display_listener_no_display_manager fallback=poll");
                return;
            }

            Class<?> listenerIface = Class.forName("android.hardware.display.DisplayManager$DisplayListener");
            InvocationHandler handler = new DisplayListenerInvocationHandler(displayDirty);
            listener = Proxy.newProxyInstance(
                    listenerIface.getClassLoader(),
                    new Class<?>[]{listenerIface},
                    handler
            );

            Class<?> handlerThreadClass = Class.forName("android.os.HandlerThread");
            ht = handlerThreadClass
                    .getDeclaredConstructor(String.class)
                    .newInstance("dsapi-display-listener");
            ReflectBridge.invoke(ht, "start");
            Object looper = ReflectBridge.invoke(ht, "getLooper");

            Class<?> looperClass = Class.forName("android.os.Looper");
            Class<?> handlerClass = Class.forName("android.os.Handler");
            Object h = handlerClass
                    .getDeclaredConstructor(looperClass)
                    .newInstance(looper);

            registerDisplayListenerCompat(dm, listener, h);
            registered = true;

            displayManager = dm;
            displayListener = listener;
            displayListenerThread = ht;
            log("presenter_status=display_listener_enabled backend=" + dm.getClass().getName());
        } catch (Throwable t) {
            if (registered && dm != null && listener != null) {
                try {
                    ReflectBridge.invoke(dm, "unregisterDisplayListener", listener);
                } catch (Throwable ignored) {
                }
            }
            if (ht != null) {
                try {
                    ReflectBridge.invoke(ht, "quitSafely");
                } catch (Throwable ignored) {
                    try {
                        ReflectBridge.invoke(ht, "quit");
                    } catch (Throwable ignored2) {
                    }
                }
            }
            log("presenter_warn=display_listener_init_failed err=" + t.getClass().getSimpleName() + " fallback=poll");
        }
    }

    private static int resolveDisplaySyncFallbackIntervalMs() {
        int value = DEFAULT_DISPLAY_SYNC_FALLBACK_INTERVAL_MS;
        try {
            String raw = System.getProperty(DISPLAY_SYNC_FALLBACK_PROPERTY);
            if (raw == null || raw.trim().isEmpty()) {
                raw = System.getenv("DSAPI_DISPLAY_SYNC_FALLBACK_MS");
            }
            if (raw != null) {
                value = Integer.parseInt(raw.trim());
            }
        } catch (Throwable ignored) {
            value = DEFAULT_DISPLAY_SYNC_FALLBACK_INTERVAL_MS;
        }
        if (value < 50) {
            return 50;
        }
        if (value > 5000) {
            return 5000;
        }
        return value;
    }

    private Object resolveDisplayManager() {
        try {
            Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
            Method currentApplication = activityThreadClass.getMethod("currentApplication");
            Object app = currentApplication.invoke(null);
            if (app != null) {
                Object dm = ReflectBridge.invoke(app, "getSystemService", "display");
                if (dm != null) {
                    return dm;
                }
            } else {
                log("presenter_warn=display_listener_no_application try_global");
            }
        } catch (Throwable t) {
            log("presenter_warn=display_manager_from_app_failed err=" + t.getClass().getSimpleName());
        }

        try {
            Class<?> globalClass = Class.forName("android.hardware.display.DisplayManagerGlobal");
            Object global = ReflectBridge.invokeStatic(globalClass, "getInstance");
            if (global != null) {
                log("presenter_status=display_listener_using_global");
                return global;
            }
        } catch (Throwable t) {
            log("presenter_warn=display_manager_global_failed err=" + t.getClass().getSimpleName());
        }
        return null;
    }

    private static boolean isArgAssignable(Class<?> paramType, Object arg) {
        if (arg == null) {
            return !paramType.isPrimitive();
        }
        Class<?> argType = arg.getClass();
        if (paramType.isAssignableFrom(argType)) {
            return true;
        }
        if (!paramType.isPrimitive()) {
            return false;
        }
        if (paramType == int.class && argType == Integer.class) return true;
        if (paramType == long.class && argType == Long.class) return true;
        if (paramType == boolean.class && argType == Boolean.class) return true;
        return false;
    }

    private static Object defaultExtraArg(Class<?> type) {
        if (type == int.class || type == Integer.class) {
            return Integer.valueOf(Integer.MAX_VALUE);
        }
        if (type == long.class || type == Long.class) {
            return Long.valueOf(Long.MAX_VALUE);
        }
        if (type == String.class) {
            return "directscreenapi";
        }
        if (type == boolean.class || type == Boolean.class) {
            return Boolean.FALSE;
        }
        if (!type.isPrimitive()) {
            return null;
        }
        return null;
    }

    private static void registerDisplayListenerCompat(Object dm, Object listener, Object handler) throws Exception {
        Method[] methods = dm.getClass().getMethods();
        for (Method m : methods) {
            if (!"registerDisplayListener".equals(m.getName())) {
                continue;
            }
            Class<?>[] params = m.getParameterTypes();
            if (params.length < 2 || params.length > 4) {
                continue;
            }
            if (!isArgAssignable(params[0], listener) || !isArgAssignable(params[1], handler)) {
                continue;
            }

            Object[] args = new Object[params.length];
            args[0] = listener;
            args[1] = handler;
            boolean ok = true;
            for (int i = 2; i < params.length; i++) {
                Object extra = defaultExtraArg(params[i]);
                if (extra == null && params[i].isPrimitive()) {
                    ok = false;
                    break;
                }
                args[i] = extra;
            }
            if (!ok) {
                continue;
            }

            try {
                m.invoke(dm, args);
                return;
            } catch (Throwable ignored) {
            }
        }

        ReflectBridge.invoke(dm, "registerDisplayListener", listener, handler);
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

    private static void log(String msg) {
        System.out.println(msg);
    }
}
