package org.directscreenapi.adapter;

import android.os.IBinder;

import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * 轻量服务注册表：
 *
 * - Provider 通过 Core(Binder) 注册 serviceId -> IBinder。
 * - Consumer 通过 Core(Binder) 获取 IBinder 并直连 Provider。
 *
 * 约束：
 * - 当前仅内存态（后续可将元数据落盘到统一状态源）。
 * - 通过 linkToDeath 自动清理失效 binder，避免“僵尸服务”。
 */
final class DsapiServiceRegistry {
    static final class Record {
        final String serviceId;
        final int version;
        final IBinder binder;
        final String meta;
        final int callingUid;
        final int callingPid;
        final long registeredAtMs;
        final IBinder.DeathRecipient deathRecipient;

        Record(
                String serviceId,
                int version,
                IBinder binder,
                String meta,
                int callingUid,
                int callingPid,
                long registeredAtMs,
                IBinder.DeathRecipient deathRecipient
        ) {
            this.serviceId = serviceId;
            this.version = version;
            this.binder = binder;
            this.meta = meta == null ? "" : meta;
            this.callingUid = callingUid;
            this.callingPid = callingPid;
            this.registeredAtMs = registeredAtMs;
            this.deathRecipient = deathRecipient;
        }
    }

    private final Object lock = new Object();
    private final Map<String, Record> services = new HashMap<String, Record>();

    static boolean serviceIdValid(String id) {
        if (id == null) return false;
        String s = id.trim();
        if (s.isEmpty()) return false;
        if (s.length() > 80) return false;
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            boolean ok = (c >= 'a' && c <= 'z')
                    || (c >= 'A' && c <= 'Z')
                    || (c >= '0' && c <= '9')
                    || c == '.'
                    || c == '_'
                    || c == '-';
            if (!ok) return false;
        }
        return true;
    }

    Record register(
            final String serviceId,
            final int version,
            final IBinder binder,
            final String meta,
            final int callingUid,
            final int callingPid
    ) throws IllegalArgumentException {
        if (!serviceIdValid(serviceId)) {
            throw new IllegalArgumentException("service_id_invalid");
        }
        if (binder == null) {
            throw new IllegalArgumentException("binder_null");
        }
        final long now = System.currentTimeMillis();
        final IBinder.DeathRecipient dr = new IBinder.DeathRecipient() {
            @Override
            public void binderDied() {
                unregisterIfSameBinder(serviceId, binder);
            }
        };

        // 先尝试绑定死亡监听（可能抛异常；失败不影响注册）。
        try {
            binder.linkToDeath(dr, 0);
        } catch (Throwable ignored) {
        }

        Record replaced = null;
        Record rec = new Record(serviceId, version, binder, meta, callingUid, callingPid, now, dr);
        synchronized (lock) {
            replaced = services.put(serviceId, rec);
        }

        if (replaced != null && replaced.binder != null && replaced.deathRecipient != null) {
            try {
                replaced.binder.unlinkToDeath(replaced.deathRecipient, 0);
            } catch (Throwable ignored) {
            }
        }
        return rec;
    }

    Record get(String serviceId) {
        if (serviceId == null) return null;
        synchronized (lock) {
            return services.get(serviceId);
        }
    }

    boolean unregister(String serviceId) {
        if (serviceId == null) return false;
        Record removed = null;
        synchronized (lock) {
            removed = services.remove(serviceId);
        }
        if (removed != null && removed.binder != null && removed.deathRecipient != null) {
            try {
                removed.binder.unlinkToDeath(removed.deathRecipient, 0);
            } catch (Throwable ignored) {
            }
            return true;
        }
        return removed != null;
    }

    private void unregisterIfSameBinder(String serviceId, IBinder binder) {
        if (serviceId == null || binder == null) return;
        Record removed = null;
        synchronized (lock) {
            Record cur = services.get(serviceId);
            if (cur != null && cur.binder == binder) {
                removed = services.remove(serviceId);
            }
        }
        if (removed != null && removed.binder != null && removed.deathRecipient != null) {
            try {
                removed.binder.unlinkToDeath(removed.deathRecipient, 0);
            } catch (Throwable ignored) {
            }
        }
    }

    List<String> listRows() {
        List<Record> snapshot = new ArrayList<Record>();
        synchronized (lock) {
            snapshot.addAll(services.values());
        }
        if (snapshot.isEmpty()) {
            return Collections.emptyList();
        }
        Collections.sort(snapshot, (a, b) -> a.serviceId.compareTo(b.serviceId));
        List<String> rows = new ArrayList<String>(snapshot.size());
        for (Record r : snapshot) {
            rows.add(String.format(
                    Locale.US,
                    "%s|%d|uid=%d|pid=%d|ts=%d|meta=%s",
                    r.serviceId,
                    r.version,
                    r.callingUid,
                    r.callingPid,
                    r.registeredAtMs,
                    sanitizeMeta(r.meta)
            ));
        }
        return rows;
    }

    private static String sanitizeMeta(String meta) {
        if (meta == null || meta.isEmpty()) return "";
        // 避免换行污染行协议（后续会换成结构化返回）。
        String s = meta.replace('\n', ' ').replace('\r', ' ').trim();
        if (s.length() > 200) {
            return s.substring(0, 200);
        }
        return s;
    }
}

