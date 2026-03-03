package org.directscreenapi.adapter;

import java.nio.ByteBuffer;
import java.nio.Buffer;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.Locale;
import java.util.concurrent.atomic.AtomicBoolean;

final class RgbaFramePresenter {
    private static final int DEFAULT_DISPLAY_SYNC_FALLBACK_INTERVAL_MS = 250;
    private static final String DISPLAY_SYNC_FALLBACK_PROPERTY = "dsapi.display_sync_fallback_ms";

    private static final class FrameRatePolicy {
        final String modeLabel;
        final float forcedHz;
        final boolean autoMax;

        FrameRatePolicy(String modeLabel, float forcedHz, boolean autoMax) {
            this.modeLabel = modeLabel;
            this.forcedHz = forcedHz;
            this.autoMax = autoMax;
        }
    }

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
    private final int layerBlurRadius;
    private final String frameRateModeLabel;
    private final float forcedFrameRateHz;
    private final boolean autoMaxFrameRate;
    private final String layerName;
    private final String startupFilterCommand;
    private final AtomicBoolean shutdownOnce = new AtomicBoolean(false);

    private volatile boolean running = true;
    private long lastFrameSeq = -1L;
    private long lastDisplaySyncMs = 0L;
    private long perfWindowStartMs = 0L;
    private int perfFrames = 0;
    private long perfBytes = 0L;
    private float latestDisplayRefreshHz = 60.0f;
    private final AtomicBoolean displayDirty = new AtomicBoolean(true);
    private int surfacePosX = Integer.MIN_VALUE;
    private int surfacePosY = Integer.MIN_VALUE;
    private int pendingSurfacePosX = Integer.MIN_VALUE;
    private int pendingSurfacePosY = Integer.MIN_VALUE;

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
    private final Method bitmapCreateBitmapMethod;
    private final Method bitmapCopyPixelsFromBufferMethod;
    private final Method bitmapRecycleMethod;
    private final Method canvasDrawBitmapMethod;
    private final Object modeSrc;
    private final Object xferSrc;
    private final Object paint;

    RgbaFramePresenter(
            String controlSocketPath,
            String dataSocketPath,
            int pollMs,
            int zLayer,
            String layerName,
            int blurRadius,
            float blurSigma,
            String filterChainSpec,
            String frameRateSpec
    ) throws Exception {
        this.daemon = new DaemonSession(controlSocketPath, dataSocketPath);
        this.displayAdapter = new AndroidDisplayAdapter();
        this.pollMs = Math.max(1, pollMs);
        this.displaySyncFallbackIntervalMs = resolveDisplaySyncFallbackIntervalMs();
        this.zLayer = zLayer;
        this.layerName = layerName;
        this.layerBlurRadius = 0;
        FrameRatePolicy frameRatePolicy = parseFrameRatePolicy(frameRateSpec);
        this.frameRateModeLabel = frameRatePolicy.modeLabel;
        this.forcedFrameRateHz = frameRatePolicy.forcedHz;
        this.autoMaxFrameRate = frameRatePolicy.autoMax;
        String safeFilterChainSpec = filterChainSpec == null ? "" : filterChainSpec.trim();
        this.startupFilterCommand = resolveStartupFilterCommand(safeFilterChainSpec);

        this.bitmapClass = Class.forName("android.graphics.Bitmap");
        this.bitmapConfigClass = Class.forName("android.graphics.Bitmap$Config");
        this.bitmapArgb8888 = bitmapConfigClass.getField("ARGB_8888").get(null);
        this.rectClass = Class.forName("android.graphics.Rect");
        this.paintClass = Class.forName("android.graphics.Paint");
        Class<?> canvasClass = Class.forName("android.graphics.Canvas");
        Class<?> xfermodeClass = Class.forName("android.graphics.Xfermode");
        this.porterDuffModeClass = Class.forName("android.graphics.PorterDuff$Mode");
        this.porterDuffXfermodeClass = Class.forName("android.graphics.PorterDuffXfermode");
        this.bitmapCreateBitmapMethod = bitmapClass.getMethod(
                "createBitmap",
                int.class,
                int.class,
                bitmapConfigClass
        );
        this.bitmapCopyPixelsFromBufferMethod = bitmapClass.getMethod("copyPixelsFromBuffer", Buffer.class);
        this.bitmapRecycleMethod = bitmapClass.getMethod("recycle");
        this.canvasDrawBitmapMethod = canvasClass.getMethod(
                "drawBitmap",
                bitmapClass,
                rectClass,
                rectClass,
                paintClass
        );
        this.modeSrc = porterDuffModeClass.getField("SRC").get(null);
        this.xferSrc = porterDuffXfermodeClass
                .getDeclaredConstructor(porterDuffModeClass)
                .newInstance(modeSrc);
        this.paint = paintClass.getDeclaredConstructor().newInstance();
        Method paintSetXfermodeMethod = paintClass.getMethod("setXfermode", xfermodeClass);
        paintSetXfermodeMethod.invoke(this.paint, xferSrc);

        initDisplayListener();
        applyStartupFilter("constructor");
    }

    void runLoop() throws Exception {
        Runtime.getRuntime().addShutdownHook(new Thread(this::shutdown, "dsapi-presenter-shutdown"));
        try {
            applyStartupFilter("run_loop_start");
            syncDisplayAndEnsureSurface();
            log("presenter_status=started poll_ms="
                    + pollMs
                    + " z_layer="
                    + zLayer
                    + " layer="
                    + layerName
                    + " frame_rate_mode="
                    + frameRateModeLabel
                    + " compositor_blur_radius="
                    + layerBlurRadius);

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
                    if (pendingSurfaceSession != null
                            && mappedFrame.width == pendingSurfaceWidth
                            && mappedFrame.height == pendingSurfaceHeight) {
                        drawFrameToTarget(
                                pendingSurfaceSession,
                                pendingDestRect,
                                mappedFrame.rgba8,
                                mappedFrame.width,
                                mappedFrame.height,
                                mappedFrame.originX,
                                mappedFrame.originY,
                                true
                        );
                        activatePendingSurface();
                        lastFrameSeq = mappedFrame.frameSeq;
                        markFramePresented(mappedFrame.byteLen);
                        continue;
                    }

                    if (surfaceSession != null
                            && destRect != null
                            && mappedFrame.width == surfaceWidth
                            && mappedFrame.height == surfaceHeight) {
                        drawFrame(
                                mappedFrame.width,
                                mappedFrame.height,
                                mappedFrame.rgba8,
                                mappedFrame.originX,
                                mappedFrame.originY
                        );
                        lastFrameSeq = mappedFrame.frameSeq;
                        markFramePresented(mappedFrame.byteLen);
                        continue;
                    }

                    log("presenter_warn=drop_frame_no_surface seq=" + mappedFrame.frameSeq);
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
                bitmapRecycleMethod.invoke(bitmap);
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

    private void applyStartupFilter(String stage) {
        if (startupFilterCommand == null || startupFilterCommand.isEmpty()) {
            return;
        }
        try {
            String reply = daemon.command(startupFilterCommand);
            log("presenter_filter=applied stage=" + stage + " cmd=" + startupFilterCommand + " reply=" + reply);
        } catch (Throwable t) {
            log("presenter_warn=filter_apply_failed stage="
                    + stage
                    + " cmd="
                    + startupFilterCommand
                    + " err="
                    + describeThrowable(t));
        }
    }

    private void syncDisplayAndEnsureSurface() throws Exception {
        DisplayAdapter.DisplaySnapshot snapshot = displayAdapter.queryDisplaySnapshot();
        int width = Math.max(1, snapshot.width);
        int height = Math.max(1, snapshot.height);
        latestDisplayRefreshHz = resolveTargetFrameRateHz(snapshot);
        daemon.command(
                "DISPLAY_SET "
                        + width + " "
                        + height + " "
                        + String.format(java.util.Locale.US, "%.2f", latestDisplayRefreshHz) + " "
                        + Math.max(1, snapshot.densityDpi) + " "
                        + Math.max(0, snapshot.rotation)
        );
        if (surfaceSession != null) {
            try {
                surfaceSession.setFrameRate(latestDisplayRefreshHz);
            } catch (Throwable ignored) {
            }
        }
        if (pendingSurfaceSession != null) {
            try {
                pendingSurfaceSession.setFrameRate(latestDisplayRefreshHz);
            } catch (Throwable ignored) {
            }
        }
    }

    private void maybeSwitchSurfaceForFrame(int frameWidth, int frameHeight) throws Exception {
        if (frameWidth <= 0 || frameHeight <= 0) {
            return;
        }
        if (surfaceSession != null && frameWidth == surfaceWidth && frameHeight == surfaceHeight) {
            return;
        }
        if (pendingSurfaceSession != null
                && frameWidth == pendingSurfaceWidth
                && frameHeight == pendingSurfaceHeight) {
            return;
        }
        if (pendingSurfaceSession != null) {
            pendingSurfaceSession.closeQuietly();
            pendingSurfaceSession = null;
        }
        pendingDestRect = null;
        pendingSurfaceWidth = frameWidth;
        pendingSurfaceHeight = frameHeight;
        pendingSurfaceRotation = 0;
        pendingSurfacePosX = Integer.MIN_VALUE;
        pendingSurfacePosY = Integer.MIN_VALUE;
        pendingSurfaceSession = SurfaceLayerSession.create(
                pendingSurfaceWidth,
                pendingSurfaceHeight,
                zLayer,
                layerName,
                false,
                layerBlurRadius,
                latestDisplayRefreshHz
        );
        pendingDestRect = rectClass
                .getDeclaredConstructor(int.class, int.class, int.class, int.class)
                .newInstance(0, 0, pendingSurfaceWidth, pendingSurfaceHeight);
        log("presenter_surface=pending_resize from="
                + surfaceWidth + "x" + surfaceHeight
                + " to=" + pendingSurfaceWidth + "x" + pendingSurfaceHeight
                + " rotation=0");
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
        surfacePosX = pendingSurfacePosX;
        surfacePosY = pendingSurfacePosY;

        pendingSurfaceSession = null;
        pendingDestRect = null;
        pendingSurfaceWidth = 0;
        pendingSurfaceHeight = 0;
        pendingSurfaceRotation = 0;
        pendingSurfacePosX = Integer.MIN_VALUE;
        pendingSurfacePosY = Integer.MIN_VALUE;

        if (oldSession != null) {
            oldSession.closeQuietly();
        }
    }

    private void drawFrameToTarget(
            SurfaceLayerSession targetSession,
            Object targetRect,
            ByteBuffer rgba,
            int frameWidth,
            int frameHeight,
            int originX,
            int originY,
            boolean pendingTarget
    ) throws Exception {
        updateSurfacePosition(targetSession, originX, originY, pendingTarget);
        ensureBitmap(frameWidth, frameHeight);
        rgba.position(0);
        bitmapCopyPixelsFromBufferMethod.invoke(bitmap, rgba);

        Object canvas = targetSession.lockFrame();
        try {
            canvasDrawBitmapMethod.invoke(canvas, bitmap, null, targetRect, paint);
        } finally {
            targetSession.unlockFrame(canvas);
        }
    }

    private void drawFrame(int frameWidth, int frameHeight, ByteBuffer rgba, int originX, int originY) throws Exception {
        drawFrameToTarget(surfaceSession, destRect, rgba, frameWidth, frameHeight, originX, originY, false);
    }

    private void updateSurfacePosition(
            SurfaceLayerSession targetSession,
            int originX,
            int originY,
            boolean pendingTarget
    ) throws Exception {
        if (targetSession == null) {
            return;
        }
        if (pendingTarget) {
            if (pendingSurfacePosX == originX && pendingSurfacePosY == originY) {
                return;
            }
            targetSession.setPosition((float) originX, (float) originY);
            pendingSurfacePosX = originX;
            pendingSurfacePosY = originY;
            return;
        }
        if (surfacePosX == originX && surfacePosY == originY) {
            return;
        }
        targetSession.setPosition((float) originX, (float) originY);
        surfacePosX = originX;
        surfacePosY = originY;
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

    private static FrameRatePolicy parseFrameRatePolicy(String specRaw) {
        String spec = specRaw == null ? "" : specRaw.trim();
        if (spec.isEmpty()) {
            return new FrameRatePolicy("auto_max", 0.0f, true);
        }

        String normalized = spec.toLowerCase(Locale.US);
        if ("auto".equals(normalized)
                || "max".equals(normalized)
                || "upper".equals(normalized)
                || "auto_max".equals(normalized)
                || "auto-max".equals(normalized)) {
            return new FrameRatePolicy("auto_max", 0.0f, true);
        }
        if ("current".equals(normalized)
                || "display".equals(normalized)
                || "display_current".equals(normalized)
                || "display-current".equals(normalized)) {
            return new FrameRatePolicy("display_current", 0.0f, false);
        }

        try {
            float hz = Float.parseFloat(spec);
            if (!Float.isFinite(hz) || hz <= 0.0f) {
                throw new IllegalArgumentException("invalid_hz");
            }
            return new FrameRatePolicy(
                    String.format(Locale.US, "forced_%.2f", hz),
                    hz,
                    false
            );
        } catch (Throwable t) {
            log("presenter_warn=frame_rate_spec_invalid spec=" + spec + " fallback=auto_max");
            return new FrameRatePolicy("auto_max", 0.0f, true);
        }
    }

    private float resolveTargetFrameRateHz(DisplayAdapter.DisplaySnapshot snapshot) {
        float currentHz = snapshot != null ? snapshot.refreshHz : 60.0f;
        float maxHz = snapshot != null ? snapshot.maxRefreshHz : currentHz;

        if (!Float.isFinite(currentHz) || currentHz <= 0.0f) {
            currentHz = 60.0f;
        }
        if (!Float.isFinite(maxHz) || maxHz <= 0.0f) {
            maxHz = currentHz;
        }
        if (maxHz < currentHz) {
            maxHz = currentHz;
        }

        float targetHz;
        if (forcedFrameRateHz > 0.0f && Float.isFinite(forcedFrameRateHz)) {
            targetHz = forcedFrameRateHz;
        } else if (autoMaxFrameRate) {
            targetHz = Math.max(currentHz, maxHz);
        } else {
            targetHz = currentHz;
        }
        if (!Float.isFinite(targetHz) || targetHz <= 0.0f) {
            targetHz = currentHz;
        }
        return targetHz;
    }

    private static String resolveStartupFilterCommand(String filterChainSpec) {
        String filterChainCmd = buildFilterChainSetCommand(filterChainSpec);
        if (filterChainCmd != null) {
            return filterChainCmd;
        }
        return "FILTER_CLEAR";
    }

    private static String buildFilterChainSetCommand(String filterChainSpec) {
        if (filterChainSpec == null) {
            return null;
        }
        String trimmed = filterChainSpec.trim();
        if (trimmed.isEmpty()) {
            return null;
        }

        String[] parts = trimmed.split(",");
        if (parts.length < 1) {
            log("presenter_warn=filter_chain_invalid reason=empty");
            return null;
        }

        int passCount;
        try {
            passCount = Integer.parseInt(parts[0].trim());
        } catch (Throwable t) {
            log("presenter_warn=filter_chain_invalid reason=pass_count_parse spec=" + trimmed);
            return null;
        }
        if (passCount < 0) {
            log("presenter_warn=filter_chain_invalid reason=pass_count_negative spec=" + trimmed);
            return null;
        }

        long expectedParts = 1L + (2L * (long) passCount);
        if (parts.length != expectedParts) {
            log("presenter_warn=filter_chain_invalid reason=parts_mismatch spec=" + trimmed);
            return null;
        }

        StringBuilder cmd = new StringBuilder(32 + parts.length * 8);
        cmd.append("FILTER_CHAIN_SET ").append(passCount);

        for (int i = 0; i < passCount; i++) {
            String radiusToken = parts[1 + (i * 2)].trim();
            String sigmaToken = parts[2 + (i * 2)].trim();

            long radius;
            float sigma;
            try {
                radius = Long.parseLong(radiusToken);
                sigma = Float.parseFloat(sigmaToken);
            } catch (Throwable t) {
                log("presenter_warn=filter_chain_invalid reason=pass_parse spec=" + trimmed);
                return null;
            }
            if (radius < 0L || radius > 0xffff_ffffL || !Float.isFinite(sigma)) {
                log("presenter_warn=filter_chain_invalid reason=pass_value spec=" + trimmed);
                return null;
            }

            cmd.append(' ')
                    .append(radius)
                    .append(' ')
                    .append(String.format(Locale.US, "%.3f", sigma));
        }

        return cmd.toString();
    }

    private static String describeThrowable(Throwable t) {
        if (t == null) {
            return "Unknown";
        }
        Throwable root = t;
        for (int i = 0; i < 16; i++) {
            Throwable cause = root.getCause();
            if (cause == null || cause == root) {
                break;
            }
            root = cause;
        }
        String msg = root.getMessage();
        if (msg == null || msg.isEmpty()) {
            return root.getClass().getSimpleName();
        }
        return root.getClass().getSimpleName() + ":" + msg.replace('\n', ' ').replace('\r', ' ');
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
                bitmapRecycleMethod.invoke(bitmap);
            } catch (Throwable ignored) {
            }
        }

        bitmap = bitmapCreateBitmapMethod.invoke(
                null,
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
