package org.directscreenapi.adapter;

import java.io.IOException;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.Locale;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * 监听系统 Display 变化并将 DISPLAY_SET 同步到 dsapid。
 *
 * 目的：
 * - 让 touch_router / demo / 模块能在旋转、分辨率变化时自动更新坐标映射。
 * - 事件驱动（DisplayListener），不使用轮询。
 */
final class DisplayStateWatcher {
    private static final long STARTUP_SYNC_BACKOFF_MS = 120L;

    private final String controlSocketPath;
    private final DisplayAdapter displayAdapter = new AndroidDisplayAdapter();

    private final AtomicBoolean stopping = new AtomicBoolean(false);
    private final AtomicBoolean displayDirty = new AtomicBoolean(true);
    private final Object waitLock = new Object();

    private volatile DaemonSession session;
    private volatile Object displayManager;
    private volatile Object displayListener;
    private volatile Object displayListenerThread;

    private int lastWidth = -1;
    private int lastHeight = -1;
    private int lastDensityDpi = -1;
    private int lastRotation = -1;
    private float lastRefreshHz = -1.0f;

    DisplayStateWatcher(String controlSocketPath) {
        this.controlSocketPath = controlSocketPath == null ? "" : controlSocketPath.trim();
    }

    void runLoop() throws Exception {
        if (controlSocketPath.isEmpty()) {
            throw new IllegalArgumentException("display_watch_control_socket_empty");
        }
        Runtime.getRuntime().addShutdownHook(new Thread(this::shutdown, "dsapi-display-watch-shutdown"));

        ensureLooperPrepared();
        initDisplayListener();

        // 首次启动后立刻同步一次，避免 daemon 使用默认 display 参数。
        // 有些 ROM 在 listener 刚注册时会触发一次 onDisplayChanged，我们允许合并。
        syncDisplayToDaemon("startup");
        displayDirty.set(false);

        log("display_watch_status=started socket=" + sanitizeToken(controlSocketPath)
                + " listener=" + (displayListener != null ? 1 : 0));

        while (!stopping.get()) {
            synchronized (waitLock) {
                while (!stopping.get() && !displayDirty.get()) {
                    try {
                        waitLock.wait();
                    } catch (InterruptedException ignored) {
                    }
                }
            }
            if (stopping.get()) {
                break;
            }
            if (!displayDirty.getAndSet(false)) {
                continue;
            }
            syncDisplayToDaemon("display_changed");
        }
    }

    private void shutdown() {
        if (!stopping.compareAndSet(false, true)) {
            return;
        }
        synchronized (waitLock) {
            waitLock.notifyAll();
        }

        Object dm = displayManager;
        Object listener = displayListener;
        if (dm != null && listener != null) {
            try {
                ReflectBridge.invoke(dm, "unregisterDisplayListener", listener);
            } catch (Throwable ignored) {
            }
        }
        displayListener = null;
        displayManager = null;

        Object ht = displayListenerThread;
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
        displayListenerThread = null;

        DaemonSession s = session;
        if (s != null) {
            s.closeQuietly();
        }
        session = null;

        log("display_watch_status=stopped");
    }

    private void markDirty(String reason) {
        if (stopping.get()) {
            return;
        }
        displayDirty.set(true);
        synchronized (waitLock) {
            waitLock.notifyAll();
        }
        if (reason != null && !reason.isEmpty()) {
            log("display_watch_status=dirty reason=" + sanitizeToken(reason));
        }
    }

    private void syncDisplayToDaemon(String reason) {
        DisplayAdapter.DisplaySnapshot snapshot;
        try {
            snapshot = displayAdapter.queryDisplaySnapshot();
        } catch (Throwable t) {
            log("display_watch_warn=query_display_failed err=" + describeThrowable(t));
            return;
        }

        int width = Math.max(1, snapshot.width);
        int height = Math.max(1, snapshot.height);
        int dpi = Math.max(1, snapshot.densityDpi);
        int rotation = Math.max(0, snapshot.rotation);
        float hz = snapshot.refreshHz > 0f ? snapshot.refreshHz : 60f;

        boolean changed = width != lastWidth
                || height != lastHeight
                || dpi != lastDensityDpi
                || rotation != lastRotation
                || (!Float.isFinite(lastRefreshHz) || Math.abs(hz - lastRefreshHz) >= 0.01f);
        if (!changed) {
            return;
        }

        String cmd = "DISPLAY_SET "
                + width + " "
                + height + " "
                + String.format(Locale.US, "%.2f", hz) + " "
                + dpi + " "
                + rotation;

        for (int attempt = 0; attempt < 2; attempt++) {
            DaemonSession s = session;
            if (s == null) {
                try {
                    s = new DaemonSession(controlSocketPath, false);
                    session = s;
                } catch (Throwable t) {
                    log("display_watch_warn=session_connect_failed attempt=" + attempt
                            + " err=" + describeThrowable(t));
                    sleepQuietly(STARTUP_SYNC_BACKOFF_MS);
                    continue;
                }
            }
            try {
                s.command(cmd);
                lastWidth = width;
                lastHeight = height;
                lastDensityDpi = dpi;
                lastRotation = rotation;
                lastRefreshHz = hz;
                log("display_watch_status=display_set reason=" + sanitizeToken(reason)
                        + " width=" + width
                        + " height=" + height
                        + " refresh_hz=" + String.format(Locale.US, "%.2f", hz)
                        + " density_dpi=" + dpi
                        + " rotation=" + rotation);
                return;
            } catch (Throwable t) {
                log("display_watch_warn=display_set_failed attempt=" + attempt
                        + " err=" + describeThrowable(t));
                // daemon 可能重启，丢掉 session 触发重连。
                try {
                    s.closeQuietly();
                } catch (Throwable ignored) {
                }
                session = null;
                sleepQuietly(STARTUP_SYNC_BACKOFF_MS);
            }
        }
    }

    private static void sleepQuietly(long ms) {
        if (ms <= 0) return;
        try {
            Thread.sleep(ms);
        } catch (InterruptedException ignored) {
        }
    }

    private void initDisplayListener() {
        Object dm = null;
        Object listener = null;
        Object ht = null;
        boolean registered = false;
        try {
            dm = resolveDisplayManager();
            if (dm == null) {
                log("display_watch_warn=no_display_manager");
                return;
            }

            Class<?> listenerIface = Class.forName("android.hardware.display.DisplayManager$DisplayListener");
            InvocationHandler handler = new DisplayListenerInvocationHandler(new Runnable() {
                @Override
                public void run() {
                    markDirty("display_listener");
                }
            });
            listener = Proxy.newProxyInstance(
                    listenerIface.getClassLoader(),
                    new Class<?>[]{listenerIface},
                    handler
            );

            Class<?> handlerThreadClass = Class.forName("android.os.HandlerThread");
            ht = handlerThreadClass
                    .getDeclaredConstructor(String.class)
                    .newInstance("dsapi-display-watch-listener");
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
            log("display_watch_warn=display_listener_init_failed err=" + describeThrowable(t));
        }
    }

    private Object resolveDisplayManager() {
        try {
            Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
            Object app = ReflectBridge.invokeStatic(activityThreadClass, "currentApplication");
            if (app != null) {
                Object dm = ReflectBridge.invoke(app, "getSystemService", "display");
                if (dm != null) {
                    return dm;
                }
            }
        } catch (Throwable ignored) {
        }

        try {
            Class<?> globalClass = Class.forName("android.hardware.display.DisplayManagerGlobal");
            Object global = ReflectBridge.invokeStatic(globalClass, "getInstance");
            if (global != null) {
                return global;
            }
        } catch (Throwable ignored) {
        }
        return null;
    }

    private static void ensureLooperPrepared() {
        try {
            Class<?> looperClass = Class.forName("android.os.Looper");
            Object myLooper = ReflectBridge.invokeStatic(looperClass, "myLooper");
            if (myLooper == null) {
                ReflectBridge.invokeStatic(looperClass, "prepare");
            }
        } catch (Throwable ignored) {
        }
    }

    private static final class DisplayListenerInvocationHandler implements InvocationHandler {
        private final Runnable onDirty;

        DisplayListenerInvocationHandler(Runnable onDirty) {
            this.onDirty = onDirty;
        }

        @Override
        public Object invoke(Object proxy, Method method, Object[] args) {
            if (method == null) return null;
            String name = method.getName();
            if ("onDisplayAdded".equals(name)
                    || "onDisplayRemoved".equals(name)
                    || "onDisplayChanged".equals(name)) {
                try {
                    onDirty.run();
                } catch (Throwable ignored) {
                }
                return null;
            }
            if ("toString".equals(name)) {
                return "DsapiDisplayWatchListener";
            }
            return null;
        }
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

    private static String sanitizeToken(String raw) {
        if (raw == null || raw.trim().isEmpty()) {
            return "-";
        }
        return raw.trim()
                .replace('\n', '_')
                .replace('\r', '_')
                .replace('\t', '_')
                .replace(' ', '_');
    }

    private static String describeThrowable(Throwable t) {
        if (t == null) {
            return "unknown";
        }
        Throwable root = t;
        while (root.getCause() != null && root.getCause() != root) {
            root = root.getCause();
        }
        String msg = root.getMessage();
        if (msg == null || msg.trim().isEmpty()) {
            return root.getClass().getName();
        }
        return root.getClass().getName() + ":" + msg.replace('\n', ' ').replace('\r', ' ');
    }

    private static void log(String line) {
        System.out.println(line);
    }
}

