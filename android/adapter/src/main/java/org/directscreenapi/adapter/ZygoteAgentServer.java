package org.directscreenapi.adapter;

import android.os.Binder;
import android.os.IBinder;
import android.os.Parcel;
import android.os.RemoteException;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStreamReader;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;

final class ZygoteAgentServer {
    private static final String MANAGER_PACKAGE = "org.directscreenapi.manager";
    private final String zygoteServiceName;
    private final String daemonServiceName;
    private final String readyFilePath;
    private final String scopeFilePath;
    private Object serviceNotificationCallback;

    ZygoteAgentServer(String zygoteServiceName, String daemonServiceName, String readyFilePath, String scopeFilePath) {
        this.zygoteServiceName = zygoteServiceName == null ? "" : zygoteServiceName.trim();
        this.daemonServiceName = daemonServiceName == null ? "" : daemonServiceName.trim();
        this.readyFilePath = readyFilePath == null ? "" : readyFilePath.trim();
        this.scopeFilePath = scopeFilePath == null ? "" : scopeFilePath.trim();
    }

    void runLoop() {
        if (zygoteServiceName.isEmpty()) {
            throw new IllegalArgumentException("zygote_service_missing");
        }
        if (containsWhitespace(zygoteServiceName)) {
            throw new IllegalArgumentException("zygote_service_invalid");
        }
        if (daemonServiceName.isEmpty()) {
            throw new IllegalArgumentException("daemon_service_missing");
        }

        AgentBinder agentBinder = new AgentBinder();
        if (!registerService(zygoteServiceName, agentBinder)) {
            throw new IllegalStateException("zygote_service_register_failed");
        }
        enableServiceSelfHeal(zygoteServiceName, ZygoteAgentContract.DESCRIPTOR, agentBinder);

        writeReadyState("ready", "-");
        System.out.println("zygote_agent_status=started zygote_service=" + zygoteServiceName
                + " daemon_service=" + daemonServiceName + " mode=aidl_binder_proxy");

        while (true) {
            try {
                Thread.sleep(60_000L);
            } catch (InterruptedException ignored) {
            }
        }
    }

    private boolean registerService(String name, IBinder binder) {
        try {
            Class<?> serviceManager = Class.forName("android.os.ServiceManager");
            java.lang.reflect.Method[] methods = serviceManager.getMethods();
            for (java.lang.reflect.Method method : methods) {
                if (!"addService".equals(method.getName())) {
                    continue;
                }
                Class<?>[] pt = method.getParameterTypes();
                if (pt.length < 2 || pt[0] != String.class || !IBinder.class.isAssignableFrom(pt[1])) {
                    continue;
                }
                Object[] args = new Object[pt.length];
                args[0] = name;
                args[1] = binder;
                if (pt.length >= 3) {
                    if (pt[2] == boolean.class || pt[2] == Boolean.class) {
                        args[2] = Boolean.TRUE;
                    } else if (pt[2] == int.class || pt[2] == Integer.class) {
                        args[2] = Integer.valueOf(0);
                    } else {
                        continue;
                    }
                }
                if (pt.length >= 4) {
                    if (pt[3] == int.class || pt[3] == Integer.class) {
                        args[3] = Integer.valueOf(0);
                    } else if (pt[3] == boolean.class || pt[3] == Boolean.class) {
                        args[3] = Boolean.TRUE;
                    } else {
                        continue;
                    }
                }
                try {
                    method.invoke(null, args);
                    return true;
                } catch (Throwable ignored) {
                }
            }
        } catch (Throwable t) {
            System.out.println("zygote_agent_warn=service_register_reflect_failed error="
                    + t.getClass().getName() + ":" + t.getMessage());
        }
        return false;
    }

    private void enableServiceSelfHeal(final String watchService, final String expectedDescriptor, final IBinder localBinder) {
        if (watchService == null || watchService.isEmpty() || localBinder == null) {
            return;
        }
        try {
            Class<?> serviceManagerClass = Class.forName("android.os.ServiceManager");
            final Class<?> callbackClass = Class.forName("android.os.IServiceCallback");
            Method registerMethod = serviceManagerClass.getMethod("registerForNotifications", String.class, callbackClass);
            InvocationHandler handler = new InvocationHandler() {
                @Override
                public Object invoke(Object proxy, Method method, Object[] args) {
                    String methodName = method == null ? "" : method.getName();
                    if ("onRegistration".equals(methodName)) {
                        IBinder remote = null;
                        if (args != null && args.length >= 2 && args[1] instanceof IBinder) {
                            remote = (IBinder) args[1];
                        }
                        if (remote == null) {
                            remote = queryServiceBinder(watchService);
                        }
                        String descriptor = readBinderDescriptor(remote);
                        if (!expectedDescriptor.equals(descriptor)) {
                            if (registerService(watchService, localBinder)) {
                                System.out.println("zygote_agent_watch=re_registered service=" + sanitizeToken(watchService)
                                        + " old_descriptor=" + sanitizeToken(descriptor));
                            } else {
                                System.out.println("zygote_agent_warn=re_register_failed service=" + sanitizeToken(watchService)
                                        + " old_descriptor=" + sanitizeToken(descriptor));
                            }
                        }
                        return null;
                    }
                    if ("asBinder".equals(methodName)) {
                        return null;
                    }
                    if ("toString".equals(methodName)) {
                        return "ZygoteAgentCallbackProxy(" + watchService + ")";
                    }
                    if ("hashCode".equals(methodName)) {
                        return Integer.valueOf(System.identityHashCode(proxy));
                    }
                    if ("equals".equals(methodName)) {
                        return Boolean.valueOf(args != null && args.length == 1 && proxy == args[0]);
                    }
                    return null;
                }
            };
            Object callback = Proxy.newProxyInstance(
                    callbackClass.getClassLoader(),
                    new Class<?>[]{callbackClass},
                    handler
            );
            registerMethod.invoke(null, watchService, callback);
            serviceNotificationCallback = callback;
            System.out.println("zygote_agent_watch=enabled service=" + sanitizeToken(watchService));
        } catch (Throwable t) {
            System.out.println("zygote_agent_warn=watch_not_available service=" + sanitizeToken(watchService)
                    + " error=" + t.getClass().getName() + ":" + t.getMessage());
        }
    }

    private static IBinder queryServiceBinder(String serviceName) {
        if (serviceName == null || serviceName.trim().isEmpty()) {
            return null;
        }
        try {
            Class<?> serviceManager = Class.forName("android.os.ServiceManager");
            Method getService = serviceManager.getMethod("getService", String.class);
            Object binder = getService.invoke(null, serviceName);
            if (binder instanceof IBinder) {
                return (IBinder) binder;
            }
        } catch (Throwable ignored) {
        }
        return null;
    }

    private static String readBinderDescriptor(IBinder binder) {
        if (binder == null) {
            return "";
        }
        try {
            String d = binder.getInterfaceDescriptor();
            if (d != null && !d.trim().isEmpty()) {
                return d.trim();
            }
        } catch (Throwable ignored) {
        }
        Parcel data = null;
        Parcel reply = null;
        try {
            data = Parcel.obtain();
            reply = Parcel.obtain();
            if (binder.transact(IBinder.INTERFACE_TRANSACTION, data, reply, 0)) {
                String d = reply.readString();
                return d == null ? "" : d.trim();
            }
        } catch (Throwable ignored) {
        } finally {
            if (reply != null) {
                try {
                    reply.recycle();
                } catch (Throwable ignored) {
                }
            }
            if (data != null) {
                try {
                    data.recycle();
                } catch (Throwable ignored) {
                }
            }
        }
        return "";
    }

    private static boolean containsWhitespace(String value) {
        return value.indexOf(' ') >= 0
                || value.indexOf('\t') >= 0
                || value.indexOf('\r') >= 0
                || value.indexOf('\n') >= 0;
    }

    private IBinder queryDaemonServiceBinder() {
        try {
            Class<?> serviceManager = Class.forName("android.os.ServiceManager");
            java.lang.reflect.Method getService = serviceManager.getMethod("getService", String.class);
            Object binder = getService.invoke(null, daemonServiceName);
            if (binder instanceof IBinder) {
                return (IBinder) binder;
            }
        } catch (Throwable ignored) {
        }
        return null;
    }

    private Decision shouldInject(String packageName, int userId, boolean isolated, boolean childZygote, boolean hasDataDir) {
        if (isolated) {
            return new Decision(false, "skip_isolated");
        }
        if (childZygote) {
            return new Decision(false, "skip_child_zygote");
        }
        if (!hasDataDir) {
            return new Decision(false, "skip_no_data_dir");
        }

        String pkg = packageName == null ? "" : packageName.trim();
        if (pkg.isEmpty()) {
            pkg = "*";
        }
        // 默认 fail-closed：仅允许白名单 scope，避免注入所有进程带来稳定性/性能风险。
        // Manager 例外：必须可注入，否则 UI 无法拿到核心 Binder 做控制面。
        if (MANAGER_PACKAGE.equals(pkg)) {
            return new Decision(true, "allow_manager");
        }

        ScopeRule best = null;
        for (ScopeRule rule : loadScopeRules()) {
            if (!rule.matches(pkg, userId)) {
                continue;
            }
            if (best == null || rule.priority > best.priority) {
                best = rule;
            }
        }
        if (best == null) {
            return new Decision(false, "deny_default");
        }
        if (best.allow) {
            return new Decision(true, "allow_scope");
        }
        return new Decision(false, "deny_scope");
    }

    private List<ScopeRule> loadScopeRules() {
        List<ScopeRule> out = new ArrayList<ScopeRule>();
        if (scopeFilePath.isEmpty()) {
            return out;
        }
        File file = new File(scopeFilePath);
        if (!file.isFile()) {
            return out;
        }
        for (String line : readLines(file)) {
            if (line == null) {
                continue;
            }
            String row = line.trim();
            if (row.isEmpty() || row.startsWith("#")) {
                continue;
            }
            String[] cols = row.split("\\|");
            if (cols.length < 4) {
                continue;
            }
            String kind = cols[0].trim();
            String pkg = cols[1].trim();
            String userText = cols[2].trim();
            String policy = cols[3].trim().toLowerCase();
            if (!"scope".equals(kind)) {
                continue;
            }
            if (pkg.isEmpty()) {
                continue;
            }
            Integer uid = parseIntSafe(userText);
            if (uid == null) {
                continue;
            }
            boolean allow;
            if ("allow".equals(policy)) {
                allow = true;
            } else if ("deny".equals(policy)) {
                allow = false;
            } else {
                continue;
            }
            out.add(new ScopeRule(pkg, uid.intValue(), allow));
        }
        return out;
    }

    private static List<String> readLines(File file) {
        ArrayList<String> out = new ArrayList<String>();
        if (file == null || !file.isFile()) {
            return out;
        }
        BufferedReader br = null;
        try {
            br = new BufferedReader(new InputStreamReader(new FileInputStream(file), StandardCharsets.UTF_8));
            String line;
            while ((line = br.readLine()) != null) {
                out.add(line);
            }
        } catch (Throwable ignored) {
        } finally {
            if (br != null) {
                try {
                    br.close();
                } catch (Throwable ignored) {
                }
            }
        }
        return out;
    }

    private static Integer parseIntSafe(String raw) {
        if (raw == null || raw.trim().isEmpty()) {
            return null;
        }
        try {
            return Integer.valueOf(Integer.parseInt(raw.trim()));
        } catch (Throwable ignored) {
            return null;
        }
    }

    private void writeReadyState(String state, String reason) {
        if (readyFilePath.isEmpty()) {
            return;
        }
        FileOutputStream fos = null;
        try {
            File file = new File(readyFilePath);
            File parent = file.getParentFile();
            if (parent != null && !parent.exists()) {
                parent.mkdirs();
            }
            String line = "state=" + sanitizeToken(state)
                    + " pid=" + android.os.Process.myPid()
                    + " zygote_service=" + sanitizeToken(zygoteServiceName)
                    + " daemon_service=" + sanitizeToken(daemonServiceName)
                    + " reason=" + sanitizeToken(reason)
                    + "\n";
            fos = new FileOutputStream(file, false);
            fos.write(line.getBytes(StandardCharsets.UTF_8));
            fos.flush();
        } catch (Throwable t) {
            System.out.println("zygote_agent_warn=ready_write_failed error="
                    + t.getClass().getName() + ":" + t.getMessage());
        } finally {
            if (fos != null) {
                try {
                    fos.close();
                } catch (Throwable ignored) {
                }
            }
        }
    }

    private static String sanitizeToken(String raw) {
        if (raw == null) {
            return "-";
        }
        String out = raw.trim();
        if (out.isEmpty()) {
            return "-";
        }
        out = out.replace('\n', '_').replace('\r', '_').replace('\t', '_').replace(' ', '_');
        if (out.isEmpty()) {
            return "-";
        }
        return out;
    }

    private static boolean enforceInterface(Parcel data) {
        if (data == null) {
            return false;
        }
        try {
            data.enforceInterface(ZygoteAgentContract.DESCRIPTOR);
            return true;
        } catch (Throwable ignored) {
        }
        return false;
    }

    private final class AgentBinder extends Binder {
        @Override
        protected boolean onTransact(int code, Parcel data, Parcel reply, int flags) throws RemoteException {
            if (code == INTERFACE_TRANSACTION) {
                if (reply != null) {
                    reply.writeString(ZygoteAgentContract.DESCRIPTOR);
                }
                return true;
            }
            if (code == ZygoteAgentContract.TRANSACTION_GET_INFO) {
                if (data == null || reply == null || !enforceInterface(data)) {
                    return false;
                }
                reply.writeNoException();
                reply.writeInt(ZygoteAgentContract.INTERFACE_VERSION);
                reply.writeString(ZygoteAgentContract.INTERFACE_NAME);
                reply.writeStringArray(new String[]{
                        ZygoteAgentContract.FEATURE_DAEMON_BINDER,
                        ZygoteAgentContract.FEATURE_SCOPE_DECIDER
                });
                return true;
            }
            if (code == ZygoteAgentContract.TRANSACTION_GET_DAEMON_BINDER) {
                if (data == null || reply == null || !enforceInterface(data)) {
                    return false;
                }
                IBinder daemonBinder = queryDaemonServiceBinder();
                reply.writeNoException();
                reply.writeStrongBinder(daemonBinder);
                return true;
            }
            if (code == ZygoteAgentContract.TRANSACTION_SHOULD_INJECT) {
                if (data == null || reply == null || !enforceInterface(data)) {
                    return false;
                }
                String packageName = data.readString();
                data.readString(); // processName，当前版本仅用于协议占位
                int userId = data.readInt();
                boolean isolated = data.readInt() != 0;
                boolean childZygote = data.readInt() != 0;
                boolean hasDataDir = data.readInt() != 0;
                Decision decision = shouldInject(packageName, userId, isolated, childZygote, hasDataDir);
                reply.writeNoException();
                reply.writeInt(decision.allow ? 1 : 0);
                reply.writeString(decision.reason);
                return true;
            }
            return super.onTransact(code, data, reply, flags);
        }
    }

    private static final class Decision {
        final boolean allow;
        final String reason;

        Decision(boolean allow, String reason) {
            this.allow = allow;
            this.reason = reason == null ? "-" : reason;
        }
    }

    private static final class ScopeRule {
        final String packageName;
        final int userId;
        final boolean allow;
        final int priority;

        ScopeRule(String packageName, int userId, boolean allow) {
            this.packageName = packageName;
            this.userId = userId;
            this.allow = allow;
            this.priority = computePriority(packageName, userId);
        }

        boolean matches(String pkg, int uid) {
            boolean pkgMatch = "*".equals(packageName) || packageName.equals(pkg);
            boolean userMatch = userId == -1 || userId == uid;
            return pkgMatch && userMatch;
        }

        private static int computePriority(String pkg, int uid) {
            int score = 0;
            score += "*".equals(pkg) ? 1 : 4;
            score += uid == -1 ? 1 : 4;
            return score;
        }
    }
}
