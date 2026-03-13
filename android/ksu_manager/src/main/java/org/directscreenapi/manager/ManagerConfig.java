package org.directscreenapi.manager;

import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;

final class ManagerConfig {
    static final String PREFS_NAME = "dsapi_manager_config";
    static final String KEY_CTL_PATH = "ctl_path";
    static final String KEY_BRIDGE_SERVICE = "bridge_service";
    static final String KEY_REFRESH_MS = "refresh_ms";
    static final String KEY_TRANSPORT = "transport";

    static final String DEFAULT_CTL_PATH = "/data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh";
    static final String DEFAULT_BRIDGE_SERVICE = "dsapi.core";
    static final int DEFAULT_REFRESH_MS = 1000;
    static final String DEFAULT_TRANSPORT = "zygote";

    String ctlPath = DEFAULT_CTL_PATH;
    String bridgeService = DEFAULT_BRIDGE_SERVICE;
    int refreshMs = DEFAULT_REFRESH_MS;
    String transport = DEFAULT_TRANSPORT;

    static ManagerConfig load(Context context, Intent intent) {
        ManagerConfig cfg = new ManagerConfig();
        SharedPreferences sp = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE);

        cfg.ctlPath = normalizeString(sp.getString(KEY_CTL_PATH, DEFAULT_CTL_PATH), DEFAULT_CTL_PATH);
        cfg.bridgeService = normalizeServiceName(
                sp.getString(KEY_BRIDGE_SERVICE, DEFAULT_BRIDGE_SERVICE),
                DEFAULT_BRIDGE_SERVICE
        );
        cfg.refreshMs = clampInt(sp.getInt(KEY_REFRESH_MS, DEFAULT_REFRESH_MS), 250, 60000, DEFAULT_REFRESH_MS);
        cfg.transport = normalizeTransport(sp.getString(KEY_TRANSPORT, DEFAULT_TRANSPORT), DEFAULT_TRANSPORT);

        if (intent != null) {
            String ctl = intent.getStringExtra(KEY_CTL_PATH);
            if (ctl != null && !ctl.trim().isEmpty()) {
                cfg.ctlPath = ctl.trim();
            }
            String service = intent.getStringExtra(KEY_BRIDGE_SERVICE);
            if (service != null && !service.trim().isEmpty()) {
                cfg.bridgeService = normalizeServiceName(service, cfg.bridgeService);
            }
            String refreshRaw = intent.getStringExtra(KEY_REFRESH_MS);
            if (refreshRaw != null) {
                try {
                    cfg.refreshMs = clampInt(Integer.parseInt(refreshRaw.trim()), 250, 60000, cfg.refreshMs);
                } catch (Throwable ignored) {
                }
            }
            String transport = intent.getStringExtra(KEY_TRANSPORT);
            if (transport != null && !transport.trim().isEmpty()) {
                cfg.transport = normalizeTransport(transport, cfg.transport);
            }
        }

        return cfg;
    }

    void save(Context context) {
        SharedPreferences sp = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE);
        SharedPreferences.Editor editor = sp.edit();
        editor.putString(KEY_CTL_PATH, normalizeString(ctlPath, DEFAULT_CTL_PATH));
        editor.putString(KEY_BRIDGE_SERVICE, normalizeServiceName(bridgeService, DEFAULT_BRIDGE_SERVICE));
        editor.putInt(KEY_REFRESH_MS, clampInt(refreshMs, 250, 60000, DEFAULT_REFRESH_MS));
        editor.putString(KEY_TRANSPORT, normalizeTransport(transport, DEFAULT_TRANSPORT));
        editor.apply();
    }

    Intent applyToIntent(Intent intent) {
        if (intent == null) {
            return null;
        }
        intent.putExtra(KEY_CTL_PATH, ctlPath);
        intent.putExtra(KEY_BRIDGE_SERVICE, bridgeService);
        intent.putExtra(KEY_REFRESH_MS, String.valueOf(refreshMs));
        intent.putExtra(KEY_TRANSPORT, normalizeTransport(transport, DEFAULT_TRANSPORT));
        return intent;
    }

    private static int clampInt(int value, int min, int max, int fallback) {
        if (value < min || value > max) {
            return fallback;
        }
        return value;
    }

    private static String normalizeString(String raw, String fallback) {
        if (raw == null) {
            return fallback;
        }
        String v = raw.trim();
        if (v.isEmpty()) {
            return fallback;
        }
        return v;
    }

    private static String normalizeServiceName(String raw, String fallback) {
        String v = normalizeString(raw, fallback);
        if (v.indexOf(' ') >= 0 || v.indexOf('\t') >= 0 || v.indexOf('\r') >= 0 || v.indexOf('\n') >= 0) {
            return fallback;
        }
        // 仅允许 dsapi.* 命名空间，避免误配置系统 service（例如 assetatlas）导致桥接失效。
        if (!v.startsWith("dsapi.")) {
            return fallback;
        }
        return v;
    }

    private static String normalizeTransport(String raw, String fallback) {
        String v = normalizeString(raw, fallback).toLowerCase();
        if ("zygote".equals(v) || "binder".equals(v)) {
            return v;
        }
        return fallback;
    }

}
