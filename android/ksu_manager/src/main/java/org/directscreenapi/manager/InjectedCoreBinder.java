package org.directscreenapi.manager;

import android.os.IBinder;
import android.os.SystemClock;

/**
 * 由寄生 host 注入的 core binder（避免 Manager 进程直接 ServiceManager.find 被 SELinux 拦截）。
 */
final class InjectedCoreBinder {
    private static volatile IBinder binder;
    private static volatile String source = "";
    private static volatile long injectedUptimeMs = 0L;

    private InjectedCoreBinder() {
    }

    static void set(IBinder core, String src) {
        binder = core;
        source = src == null ? "" : src;
        injectedUptimeMs = SystemClock.uptimeMillis();
    }

    /**
     * 由 Zygisk loader 在 postAppSpecialize 后注入。
     *
     * 说明：
     * - 不能依赖 Manager 进程直接 ServiceManager.find（会被 SELinux 拦截）。
     * - Zygote 阶段可拿到 binder 句柄，但必须在 app sandbox 生效后再把句柄落进 Java 层。
     */
    public static void setFromZygisk(IBinder core, String src) {
        set(core, (src == null || src.trim().isEmpty()) ? "zygisk" : src.trim());
    }

    static IBinder get() {
        return binder;
    }

    static String debugLine() {
        return "injected=" + (binder == null ? "0" : "1")
                + " source=" + (source.isEmpty() ? "-" : source)
                + " uptime_ms=" + injectedUptimeMs;
    }
}
