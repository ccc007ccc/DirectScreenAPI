package org.directscreenapi.adapter;

import java.util.Locale;

public final class AndroidAdapterMain {
    private static int parseInt(String s, int fallback) {
        try {
            return Integer.parseInt(s);
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private static float parseFloat(String s, float fallback) {
        try {
            return Float.parseFloat(s);
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private static void usage() {
        System.out.println("usage:");
        System.out.println("  AndroidAdapterMain display-kv");
        System.out.println("  AndroidAdapterMain display-line");
        System.out.println("  AndroidAdapterMain blur-probe");
        System.out.println("  AndroidAdapterMain present-loop [control_socket_path] [wait_timeout_ms] [z_layer] [layer_name] [blur_radius] [blur_sigma] [filter_chain] [frame_rate]");
        System.out.println("  AndroidAdapterMain screen-stream [control_socket_path] [target_fps]");
        System.out.println("  AndroidAdapterMain cap-ui [dsapi_service_ctl_path] [refresh_ms]");
        System.out.println("  AndroidAdapterMain bridge-server [dsapi_service_ctl_path] [service_name] [ready_file]");
        System.out.println("  AndroidAdapterMain manager-host [dsapi_service_ctl_path] [manager_component] [manager_package] [bridge_service] [refresh_ms] [ready_file]");
        System.out.println("  AndroidAdapterMain zygote-agent [zygote_service_name] [daemon_service_name] [ready_file] [scope_file]");
    }

    private static void printDisplayKv(DisplayAdapter.DisplaySnapshot s) {
        System.out.println("width=" + s.width);
        System.out.println("height=" + s.height);
        System.out.println(String.format(Locale.US, "refresh_hz=%.2f", s.refreshHz));
        System.out.println(String.format(Locale.US, "max_refresh_hz=%.2f", s.maxRefreshHz));
        System.out.println("density_dpi=" + s.densityDpi);
        System.out.println("rotation=" + s.rotation);
    }

    private static void printDisplayLine(DisplayAdapter.DisplaySnapshot s) {
        System.out.println(String.format(
                Locale.US,
                "display_snapshot width=%d height=%d refresh_hz=%.2f max_refresh_hz=%.2f density_dpi=%d rotation=%d",
                s.width,
                s.height,
                s.refreshHz,
                s.maxRefreshHz,
                s.densityDpi,
                s.rotation
        ));
    }

    private static boolean hasMethodByArity(Class<?> clazz, String name, int arity) {
        try {
            ReflectBridge.findMethodByArity(clazz, name, arity);
            return true;
        } catch (Throwable ignored) {
            return false;
        }
    }

    private static boolean readSystemPropertyBoolean(String key, boolean fallback) {
        try {
            Class<?> sp = Class.forName("android.os.SystemProperties");
            Object v = ReflectBridge.invokeStatic(sp, "getBoolean", key, Boolean.valueOf(fallback));
            if (v instanceof Boolean) {
                return ((Boolean) v).booleanValue();
            }
        } catch (Throwable ignored) {
        }
        return fallback;
    }

    private static String readSystemPropertyString(String key, String fallback) {
        try {
            Class<?> sp = Class.forName("android.os.SystemProperties");
            Object v = ReflectBridge.invokeStatic(sp, "get", key, fallback);
            if (v instanceof String) {
                return (String) v;
            }
        } catch (Throwable ignored) {
        }
        return fallback;
    }

    private static int readDisableWindowBlursSetting() {
        try {
            Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
            Object app = ReflectBridge.invokeStatic(activityThreadClass, "currentApplication");
            if (app == null) return -1;

            Object resolver = ReflectBridge.invoke(app, "getContentResolver");
            Class<?> settingsGlobalClass = Class.forName("android.provider.Settings$Global");
            Object value = ReflectBridge.invokeStatic(
                    settingsGlobalClass,
                    "getInt",
                    resolver,
                    "disable_window_blurs",
                    Integer.valueOf(0)
            );
            if (value instanceof Integer) {
                return ((Integer) value).intValue();
            }
        } catch (Throwable ignored) {
        }
        return -1;
    }

    private static int readBlurRadiusLimit() {
        try {
            Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
            Object app = ReflectBridge.invokeStatic(activityThreadClass, "currentApplication");
            if (app == null) return -1;

            Object resolver = ReflectBridge.invoke(app, "getContentResolver");
            Class<?> settingsGlobalClass = Class.forName("android.provider.Settings$Global");
            Object value = ReflectBridge.invokeStatic(
                    settingsGlobalClass,
                    "getInt",
                    resolver,
                    "window_blur_radius",
                    Integer.valueOf(-1)
            );
            if (value instanceof Integer) {
                return ((Integer) value).intValue();
            }
        } catch (Throwable ignored) {
        }
        return -1;
    }

    private static int readCrossWindowBlurEnabled() {
        try {
            Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
            Object app = ReflectBridge.invokeStatic(activityThreadClass, "currentApplication");
            if (app == null) return -1;

            Object wm = ReflectBridge.invoke(app, "getSystemService", "window");
            if (wm == null) return -1;
            Object enabled = ReflectBridge.invoke(wm, "isCrossWindowBlurEnabled");
            if (enabled instanceof Boolean) {
                return ((Boolean) enabled).booleanValue() ? 1 : 0;
            }
        } catch (Throwable ignored) {
        }
        return -1;
    }

    private static void printBlurProbe() {
        boolean txHasBackgroundBlur = false;
        boolean txHasBlurRegion = false;
        boolean builderHasEffectLayer = false;
        boolean builderHasColorLayer = false;
        try {
            Class<?> txClass = Class.forName("android.view.SurfaceControl$Transaction");
            txHasBackgroundBlur = hasMethodByArity(txClass, "setBackgroundBlurRadius", 2);
            txHasBlurRegion = hasMethodByArity(txClass, "setBlurRegions", 2);
        } catch (Throwable ignored) {
        }
        try {
            Class<?> builderClass = Class.forName("android.view.SurfaceControl$Builder");
            builderHasEffectLayer = hasMethodByArity(builderClass, "setEffectLayer", 0);
            builderHasColorLayer = hasMethodByArity(builderClass, "setColorLayer", 0);
        } catch (Throwable ignored) {
        }

        boolean sfSupportsBackgroundBlur = readSystemPropertyBoolean(
                "ro.surface_flinger.supports_background_blur",
                false
        );
        String sfDisableBlurs = readSystemPropertyString("persist.sys.sf.disable_blurs", "");
        int disableWindowBlurs = readDisableWindowBlursSetting();
        int crossWindowBlurEnabled = readCrossWindowBlurEnabled();
        int windowBlurRadius = readBlurRadiusLimit();

        System.out.println(
                "blur_probe "
                        + "tx_background_blur=" + (txHasBackgroundBlur ? 1 : 0)
                        + " tx_blur_regions=" + (txHasBlurRegion ? 1 : 0)
                        + " builder_effect_layer=" + (builderHasEffectLayer ? 1 : 0)
                        + " builder_color_layer=" + (builderHasColorLayer ? 1 : 0)
                        + " sf_supports_background_blur=" + (sfSupportsBackgroundBlur ? 1 : 0)
                        + " sf_disable_blurs=" + (sfDisableBlurs.isEmpty() ? "<empty>" : sfDisableBlurs)
                        + " setting_disable_window_blurs=" + disableWindowBlurs
                        + " wm_cross_window_blur_enabled=" + crossWindowBlurEnabled
                        + " setting_window_blur_radius=" + windowBlurRadius
        );
    }

    public static void main(String[] args) {
        if (args.length < 1) {
            usage();
            System.exit(1);
        }

        String cmd = args[0];
        if ("display-kv".equals(cmd)) {
            try {
                DisplayAdapter adapter = new AndroidDisplayAdapter();
                DisplayAdapter.DisplaySnapshot snapshot = adapter.queryDisplaySnapshot();
                printDisplayKv(snapshot);
                return;
            } catch (Throwable t) {
                System.out.println("display_status=failed");
                System.out.println("display_error=" + t.getClass().getName() + ":" + t.getMessage());
                t.printStackTrace(System.out);
                System.exit(2);
            }
        }
        if ("display-line".equals(cmd)) {
            try {
                DisplayAdapter adapter = new AndroidDisplayAdapter();
                DisplayAdapter.DisplaySnapshot snapshot = adapter.queryDisplaySnapshot();
                printDisplayLine(snapshot);
                return;
            } catch (Throwable t) {
                System.out.println("display_status=failed");
                System.out.println("display_error=" + t.getClass().getName() + ":" + t.getMessage());
                t.printStackTrace(System.out);
                System.exit(2);
            }
        }
        if ("blur-probe".equals(cmd)) {
            printBlurProbe();
            return;
        }
        if ("present-loop".equals(cmd)) {
            String controlSocketPath = args.length > 1 ? args[1] : "artifacts/run/dsapi.sock";
            int baseIdx = 2;
            int waitTimeoutMs = args.length > baseIdx ? parseInt(args[baseIdx], 0) : 0;
            int zLayer = args.length > (baseIdx + 1) ? parseInt(args[baseIdx + 1], 1_000_000) : 1_000_000;
            String layerName = args.length > (baseIdx + 2) ? args[baseIdx + 2] : "DirectScreenAPI";
            int blurRadius = args.length > (baseIdx + 3) ? parseInt(args[baseIdx + 3], 0) : 0;
            float blurSigma = args.length > (baseIdx + 4) ? parseFloat(args[baseIdx + 4], 0.0f) : 0.0f;
            String filterChain = args.length > (baseIdx + 5) ? args[baseIdx + 5] : "";
            String frameRateSpec = args.length > (baseIdx + 6) ? args[baseIdx + 6] : "auto";
            try {
                RgbaFramePresenter presenter = new RgbaFramePresenter(
                        controlSocketPath,
                        waitTimeoutMs,
                        zLayer,
                        layerName,
                        blurRadius,
                        blurSigma,
                        filterChain,
                        frameRateSpec
                );
                presenter.runLoop();
                return;
            } catch (Throwable t) {
                System.out.println("presenter_status=failed");
                System.out.println("presenter_error=" + t.getClass().getName() + ":" + t.getMessage());
                t.printStackTrace(System.out);
                System.exit(2);
            }
        }
        if ("screen-stream".equals(cmd)) {
            String controlSocketPath = args.length > 1 ? args[1] : "artifacts/run/dsapi.sock";
            int baseIdx = 2;
            int targetFps = args.length > baseIdx ? parseInt(args[baseIdx], 60) : 60;
            try {
                ScreenCaptureStreamer streamer = new ScreenCaptureStreamer(
                        controlSocketPath,
                        targetFps
                );
                streamer.runLoop();
                return;
            } catch (Throwable t) {
                System.out.println("screen_stream_status=failed");
                System.out.println("screen_stream_error=" + t.getClass().getName() + ":" + t.getMessage());
                t.printStackTrace(System.out);
                System.exit(2);
            }
        }
        if ("cap-ui".equals(cmd)) {
            String ctlPath = args.length > 1 ? args[1] : "/data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh";
            int refreshMs = args.length > 2 ? parseInt(args[2], 1200) : 1200;
            try {
                CapabilityManagerUi ui = new CapabilityManagerUi(ctlPath, refreshMs);
                ui.runLoop();
                return;
            } catch (Throwable t) {
                System.out.println("cap_ui_status=failed");
                System.out.println("cap_ui_error=" + t.getClass().getName() + ":" + t.getMessage());
                t.printStackTrace(System.out);
                System.exit(2);
            }
        }
        if ("bridge-server".equals(cmd)) {
            String ctlPath = args.length > 1 ? args[1] : "/data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh";
            String serviceName = args.length > 2 ? args[2] : "dsapi.directscreenapi.bridge";
            String readyFile = args.length > 3 ? args[3] : "";
            if (serviceName == null || serviceName.trim().isEmpty()) {
                System.out.println("bridge_server_error=invalid_service");
                System.exit(2);
            }
            try {
                BridgeControlServer bridge = new BridgeControlServer(ctlPath, serviceName, readyFile);
                bridge.runLoop();
                return;
            } catch (Throwable t) {
                System.out.println("bridge_server_status=failed");
                System.out.println("bridge_server_error=" + t.getClass().getName() + ":" + t.getMessage());
                t.printStackTrace(System.out);
                System.exit(2);
            }
        }
        if ("manager-host".equals(cmd)) {
            String ctlPath = args.length > 1 ? args[1] : "/data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh";
            String managerComponent = args.length > 2 ? args[2] : "org.directscreenapi.manager/.MainActivity";
            String managerPackage = args.length > 3 ? args[3] : "org.directscreenapi.manager";
            String bridgeService = args.length > 4 ? args[4] : "assetatlas";
            int refreshMs = args.length > 5 ? parseInt(args[5], 1200) : 1200;
            String readyFile = args.length > 6 ? args[6] : "";
            try {
                ParasiticManagerHost host = new ParasiticManagerHost(
                        ctlPath,
                        managerComponent,
                        managerPackage,
                        bridgeService,
                        refreshMs,
                        readyFile
                );
                host.runLoop();
                return;
            } catch (Throwable t) {
                System.out.println("manager_host_status=failed");
                System.out.println("manager_host_error=" + t.getClass().getName() + ":" + t.getMessage());
                t.printStackTrace(System.out);
                System.exit(2);
            }
        }
        if ("zygote-agent".equals(cmd)) {
            String zygoteService = args.length > 1 ? args[1] : "dsapi.zygote.injector";
            String daemonService = args.length > 2 ? args[2] : "assetatlas";
            String readyFile = args.length > 3 ? args[3] : "";
            String scopeFile = args.length > 4 ? args[4] : "/data/adb/dsapi/state/zygote_scope.db";
            try {
                ZygoteAgentServer server = new ZygoteAgentServer(zygoteService, daemonService, readyFile, scopeFile);
                server.runLoop();
                return;
            } catch (Throwable t) {
                System.out.println("zygote_agent_status=failed");
                System.out.println("zygote_agent_error=" + t.getClass().getName() + ":" + t.getMessage());
                t.printStackTrace(System.out);
                System.exit(2);
            }
        }
        usage();
        System.exit(1);
    }
}
