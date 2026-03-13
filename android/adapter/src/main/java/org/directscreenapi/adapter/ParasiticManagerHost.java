package org.directscreenapi.adapter;

import android.content.ComponentName;
import android.content.Context;
import android.content.ServiceConnection;
import android.content.Intent;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.os.IBinder;
import android.os.Parcel;
import android.os.Looper;
import android.os.SystemClock;

import java.io.File;
import java.io.FileOutputStream;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

final class ParasiticManagerHost {
    private static final String ATMS_CALLING_PACKAGE = "com.android.shell";
    private static final String HANDSHAKE_DESCRIPTOR = "org.directscreenapi.manager.IHostHandshake";
    private static final int HANDSHAKE_SET_CORE = 1;

    private final String ctlPath;
    private final String managerComponent;
    private final String managerPackage;
    private final String bridgeService;
    private final int refreshMs;
    private final String readyFilePath;

    ParasiticManagerHost(
            String ctlPath,
            String managerComponent,
            String managerPackage,
            String bridgeService,
            int refreshMs,
            String readyFilePath
    ) {
        this.ctlPath = ctlPath == null ? "" : ctlPath.trim();
        this.managerComponent = managerComponent == null ? "" : managerComponent.trim();
        this.managerPackage = managerPackage == null ? "" : managerPackage.trim();
        this.bridgeService = bridgeService == null ? "" : bridgeService.trim();
        this.refreshMs = refreshMs <= 0 ? 1200 : refreshMs;
        this.readyFilePath = readyFilePath == null ? "" : readyFilePath.trim();
    }

    void runLoop() throws Exception {
        if (managerPackage.isEmpty()) {
            throw new IllegalArgumentException("manager_package_missing");
        }
        if (managerComponent.isEmpty()) {
            throw new IllegalArgumentException("manager_component_missing");
        }
        ensureLooperPrepared();

        Context context = resolveContext();
        ensurePackageInstalled(context, managerPackage);
        launchManagerActivity(context);
        injectCoreBinderToManager(context);
        writeReadyState("ready", "-");

        while (true) {
            try {
                Thread.sleep(60_000L);
            } catch (InterruptedException ignored) {
            }
        }
    }

    private static void ensureLooperPrepared() {
        if (Looper.getMainLooper() == null) {
            Looper.prepareMainLooper();
            return;
        }
        if (Looper.myLooper() == null) {
            Looper.prepare();
        }
    }

    private Context resolveContext() throws Exception {
        try {
            Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
            Object app = ReflectBridge.invokeStatic(activityThreadClass, "currentApplication");
            if (app instanceof Context) {
                return (Context) app;
            }
        } catch (Throwable ignored) {
        }

        Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
        Object thread = ReflectBridge.invokeStatic(activityThreadClass, "systemMain");
        Object systemContext = ReflectBridge.invoke(thread, "getSystemContext");
        if (systemContext instanceof Context) {
            return (Context) systemContext;
        }
        throw new IllegalStateException("host_context_unavailable");
    }

    private static void ensurePackageInstalled(Context context, String packageName) throws Exception {
        if (context == null) {
            throw new IllegalStateException("context_null");
        }
        PackageManager pm = context.getPackageManager();
        if (pm == null) {
            throw new IllegalStateException("package_manager_unavailable");
        }
        try {
            PackageInfo ignored = pm.getPackageInfo(packageName, 0);
        } catch (Throwable t) {
            throw new IllegalStateException("manager_not_installed:" + packageName, t);
        }
    }

    private void launchManagerActivity(Context context) throws Exception {
        ComponentName component = ComponentName.unflattenFromString(managerComponent);
        if (component == null) {
            component = new ComponentName(managerPackage, managerComponent);
        }
        Intent intent = new Intent("org.directscreenapi.manager.OPEN");
        intent.setComponent(component);
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        intent.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
        intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP);
        if (!ctlPath.isEmpty()) {
            intent.putExtra("ctl_path", ctlPath);
        }
        if (!bridgeService.isEmpty()) {
            intent.putExtra("bridge_service", bridgeService);
        }
        intent.putExtra("refresh_ms", String.valueOf(refreshMs));
        // LSP 风格：由寄生 host 注入 binder，Manager 不依赖 ServiceManager.find，不需要 root。
        intent.putExtra("transport", "zygote");
        int result = startActivityViaAtm(intent);
        if (result < 0) {
            throw new IllegalStateException("manager_activity_launch_failed result=" + result);
        }
    }

    private void injectCoreBinderToManager(Context context) {
        if (context == null) {
            return;
        }
        if (bridgeService.isEmpty()) {
            return;
        }
        try {
            IBinder core = queryServiceBinder(bridgeService);
            if (core == null) {
                System.out.println("manager_host_warn=core_binder_missing service=" + sanitizeToken(bridgeService));
                return;
            }
            boolean ok = bindHandshakeAndInject(context, core);
            System.out.println("manager_host_info=inject_core_binder ok=" + (ok ? "1" : "0")
                    + " service=" + sanitizeToken(bridgeService));
        } catch (Throwable t) {
            System.out.println("manager_host_warn=inject_core_binder_failed error=" + sanitizeToken(t.getClass().getName() + ":" + t.getMessage()));
        }
    }

    private static IBinder queryServiceBinder(String serviceName) {
        if (serviceName == null || serviceName.trim().isEmpty()) {
            return null;
        }
        try {
            Class<?> serviceManager = Class.forName("android.os.ServiceManager");
            Method getService = serviceManager.getMethod("getService", String.class);
            long deadline = SystemClock.uptimeMillis() + 1500L;
            while (true) {
                Object binder = getService.invoke(null, serviceName);
                if (binder instanceof IBinder) {
                    return (IBinder) binder;
                }
                if (SystemClock.uptimeMillis() >= deadline) {
                    break;
                }
                try {
                    Thread.sleep(50L);
                } catch (InterruptedException ignored) {
                    break;
                }
            }
        } catch (Throwable ignored) {
        }
        return null;
    }

    private boolean bindHandshakeAndInject(Context context, final IBinder coreBinder) throws Exception {
        ComponentName component = new ComponentName(managerPackage, managerPackage + ".HostHandshakeService");
        Intent intent = new Intent();
        intent.setComponent(component);

        final CountDownLatch latch = new CountDownLatch(1);
        final IBinder[] remote = new IBinder[1];

        ServiceConnection conn = new ServiceConnection() {
            @Override
            public void onServiceConnected(ComponentName name, IBinder service) {
                remote[0] = service;
                latch.countDown();
            }

            @Override
            public void onServiceDisconnected(ComponentName name) {
            }
        };

        boolean bound = false;
        try {
            bound = context.bindService(intent, conn, Context.BIND_AUTO_CREATE);
            if (!bound) {
                return false;
            }
            if (!latch.await(2500L, TimeUnit.MILLISECONDS)) {
                return false;
            }
            IBinder handshake = remote[0];
            if (handshake == null) {
                return false;
            }

            Parcel data = null;
            Parcel reply = null;
            try {
                data = Parcel.obtain();
                reply = Parcel.obtain();
                data.writeInterfaceToken(HANDSHAKE_DESCRIPTOR);
                data.writeStrongBinder(coreBinder);
                data.writeString(bridgeService);
                boolean ok = handshake.transact(HANDSHAKE_SET_CORE, data, reply, 0);
                if (!ok) {
                    return false;
                }
                reply.readException();
                int rc = reply.readInt();
                return rc == 0;
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
        } finally {
            if (bound) {
                try {
                    context.unbindService(conn);
                } catch (Throwable ignored) {
                }
            }
        }
    }

    private int startActivityViaAtm(Intent intent) throws Exception {
        Class<?> activityTaskManagerClass = Class.forName("android.app.ActivityTaskManager");
        Object taskManager = ReflectBridge.invokeStatic(activityTaskManagerClass, "getService");
        if (taskManager == null) {
            throw new IllegalStateException("activity_task_manager_unavailable");
        }

        List<String> tried = new ArrayList<String>();
        Throwable lastError = null;
        Method[] methods = taskManager.getClass().getMethods();
        for (Method method : methods) {
            String name = method.getName();
            if (!"startActivityAsUser".equals(name) && !"startActivity".equals(name)) {
                continue;
            }
            Object[] args = buildAtmStartArgs(method, intent);
            if (args == null) {
                continue;
            }
            String sig = buildMethodSignature(method);
            tried.add(sig);
            try {
                Object ret = method.invoke(taskManager, args);
                if (ret instanceof Integer) {
                    int code = ((Integer) ret).intValue();
                    System.out.println("manager_host_info=atm_start method=" + sanitizeToken(sig)
                            + " result=" + code);
                    return code;
                }
                System.out.println("manager_host_info=atm_start method=" + sanitizeToken(sig)
                        + " result=void");
                return 0;
            } catch (Throwable t) {
                lastError = t;
                System.out.println("manager_host_warn=atm_start_failed method=" + sanitizeToken(sig)
                        + " error=" + sanitizeToken(t.getClass().getName() + ":" + t.getMessage()));
            }
        }

        String triedSig = tried.isEmpty() ? "-" : sanitizeToken(joinTokens(tried));
        if (lastError != null) {
            throw new IllegalStateException(
                    "atm_start_no_success tried=" + triedSig
                            + " last=" + lastError.getClass().getName() + ":" + lastError.getMessage(),
                    lastError
            );
        }
        throw new IllegalStateException("atm_start_method_missing tried=" + triedSig);
    }

    private Object[] buildAtmStartArgs(Method method, Intent intent) {
        Class<?>[] pt = method.getParameterTypes();
        Object[] args = new Object[pt.length];
        boolean callingPackageAssigned = false;
        boolean requestCodeAssigned = false;
        boolean asUserMethod = method.getName().contains("AsUser");
        for (int i = 0; i < pt.length; i++) {
            Class<?> type = pt[i];
            String typeName = type.getName();
            if ("android.content.Intent".equals(typeName)) {
                args[i] = intent;
                continue;
            }
            if ("android.app.IApplicationThread".equals(typeName)) {
                args[i] = null;
                continue;
            }
            if ("android.os.IBinder".equals(typeName)) {
                args[i] = null;
                continue;
            }
            if ("android.app.ProfilerInfo".equals(typeName)) {
                args[i] = null;
                continue;
            }
            if ("android.os.Bundle".equals(typeName)) {
                args[i] = null;
                continue;
            }
            if (type == String.class) {
                if (!callingPackageAssigned) {
                    args[i] = ATMS_CALLING_PACKAGE;
                    callingPackageAssigned = true;
                } else {
                    args[i] = null;
                }
                continue;
            }
            if (type == int.class || type == Integer.TYPE) {
                int remainingInt = countRemainingIntParams(pt, i);
                if (asUserMethod && remainingInt == 1) {
                    args[i] = Integer.valueOf(0);
                } else if (!requestCodeAssigned) {
                    args[i] = Integer.valueOf(-1);
                    requestCodeAssigned = true;
                } else {
                    args[i] = Integer.valueOf(0);
                }
                continue;
            }
            if (type == boolean.class || type == Boolean.TYPE) {
                args[i] = Boolean.FALSE;
                continue;
            }
            if (type == long.class || type == Long.TYPE) {
                args[i] = Long.valueOf(0L);
                continue;
            }
            if (type == float.class || type == Float.TYPE) {
                args[i] = Float.valueOf(0f);
                continue;
            }
            if (type == double.class || type == Double.TYPE) {
                args[i] = Double.valueOf(0d);
                continue;
            }
            if (type.isArray()) {
                args[i] = null;
                continue;
            }
            if (!type.isPrimitive()) {
                args[i] = null;
                continue;
            }
            return null;
        }
        return args;
    }

    private static int countRemainingIntParams(Class<?>[] types, int startIndex) {
        int out = 0;
        for (int i = startIndex; i < types.length; i++) {
            if (types[i] == int.class || types[i] == Integer.TYPE) {
                out += 1;
            }
        }
        return out;
    }

    private static String buildMethodSignature(Method method) {
        StringBuilder out = new StringBuilder();
        out.append(method.getName()).append("(");
        Class<?>[] pt = method.getParameterTypes();
        for (int i = 0; i < pt.length; i++) {
            if (i > 0) {
                out.append(",");
            }
            out.append(pt[i].getSimpleName());
        }
        out.append(")");
        return out.toString();
    }

    private static String joinTokens(List<String> values) {
        StringBuilder out = new StringBuilder();
        for (String value : values) {
            if (value == null || value.trim().isEmpty()) {
                continue;
            }
            if (out.length() > 0) {
                out.append(';');
            }
            out.append(value.trim());
        }
        return out.toString();
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
                    + " bridge_service=" + sanitizeToken(bridgeService)
                    + " package=" + sanitizeToken(managerPackage)
                    + " component=" + sanitizeToken(managerComponent)
                    + " reason=" + sanitizeToken(reason)
                    + "\n";
            fos = new FileOutputStream(file, false);
            fos.write(line.getBytes(StandardCharsets.UTF_8));
            fos.flush();
        } catch (Throwable t) {
            System.out.println("manager_host_warn=ready_write_failed error="
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
}
