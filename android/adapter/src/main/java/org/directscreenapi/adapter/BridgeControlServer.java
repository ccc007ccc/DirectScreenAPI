package org.directscreenapi.adapter;

import android.content.Context;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.os.Binder;
import android.os.IBinder;
import android.os.Parcel;
import android.os.ParcelFileDescriptor;
import android.os.RemoteException;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.BufferedReader;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.concurrent.TimeUnit;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

final class BridgeControlServer {
    private static final String BASE_DIR = "/data/adb/dsapi";
    private static final String RUN_DIR = BASE_DIR + "/run";
    private static final String LOG_DIR = BASE_DIR + "/log";
    private static final String STATE_DIR = BASE_DIR + "/state";
    private static final String RUNTIME_DIR = BASE_DIR + "/runtime";
    private static final String RELEASES_DIR = RUNTIME_DIR + "/releases";
    private static final String CURRENT_DIR = RUNTIME_DIR + "/current";

    private static final String DAEMON_PID_FILE = RUN_DIR + "/dsapid.pid";
    private static final String DAEMON_SOCKET = RUN_DIR + "/dsapi.sock";
    private static final String DAEMON_LOG_FILE = LOG_DIR + "/dsapid.log";

    private static final String UI_PID_FILE = RUN_DIR + "/cap_ui.pid";
    private static final String MANAGER_HOST_PID_FILE = RUN_DIR + "/manager_host.pid";
    private static final String MANAGER_HOST_READY_FILE = RUN_DIR + "/manager_host.ready";
    private static final String MANAGER_HOST_LOG_FILE = LOG_DIR + "/manager_host.log";
    private static final String BRIDGE_PID_FILE = RUN_DIR + "/manager_bridge.pid";
    private static final String BRIDGE_SERVICE_FILE = RUN_DIR + "/manager_bridge.service";
    private static final String ZYGOTE_AGENT_PID_FILE = RUN_DIR + "/zygote_agent.pid";
    private static final String ZYGOTE_AGENT_READY_FILE = RUN_DIR + "/zygote_agent.ready";
    private static final String ZYGOTE_AGENT_SERVICE_FILE = RUN_DIR + "/zygote_agent.service";
    private static final String ZYGOTE_AGENT_DAEMON_FILE = RUN_DIR + "/zygote_agent.daemon_service";
    private static final String ZYGOTE_AGENT_LOG_FILE = LOG_DIR + "/zygote_agent.log";
    private static final String ZYGOTE_AGENT_DEFAULT_SERVICE = "dsapi.zygote.injector";
    private static final String ENABLED_FILE = STATE_DIR + "/enabled";
    private static final String LAST_ERROR_FILE = STATE_DIR + "/last_error.kv";
    private static final String ACTIVE_RELEASE_FILE = STATE_DIR + "/active_release";
    private static final String ZYGOTE_SCOPE_FILE = STATE_DIR + "/zygote_scope.db";

    private static final String MODULE_ROOT = BASE_DIR + "/modules";
    private static final String MODULE_STATE_ROOT = STATE_DIR + "/modules";
    private static final String MODULE_DISABLED_DIR = STATE_DIR + "/modules_disabled";
    private static final String MODULE_REGISTRY_FILE = STATE_DIR + "/module_registry.db";
    private static final String MODULE_SCOPE_FILE = STATE_DIR + "/module_scope.db";

    private static final String MANAGER_PACKAGE = "org.directscreenapi.manager";
    private static final String MANAGER_MAIN_COMPONENT = "org.directscreenapi.manager/.MainActivity";

    private final String ctlPath;
    private final String serviceName;
    private final String readyFilePath;
    private Context bridgeContext;
    private Object serviceNotificationCallback;

    BridgeControlServer(String ctlPath, String serviceName, String readyFilePath) {
        this.ctlPath = ctlPath;
        this.serviceName = serviceName == null ? "" : serviceName.trim();
        this.readyFilePath = readyFilePath == null ? "" : readyFilePath.trim();
    }

    void runLoop() throws Exception {
        if (serviceName.isEmpty()) {
            throw new IllegalArgumentException("bridge_service_missing");
        }
        if (containsWhitespace(serviceName)) {
            throw new IllegalArgumentException("bridge_service_invalid");
        }
        BridgeBinder bridgeBinder = new BridgeBinder();
        if (!registerService(serviceName, bridgeBinder)) {
            throw new IllegalStateException("bridge_service_register_failed");
        }
        enableServiceSelfHeal(serviceName, BridgeContract.DESCRIPTOR_MANAGER, bridgeBinder);

        ensureBridgePidFile();
        writeReadyState("ready", "-");
        System.out.println("daemon_service_status=started transport=binder service=" + serviceName + " mode=aidl_direct");
        while (true) {
            try {
                Thread.sleep(60_000L);
            } catch (InterruptedException ignored) {
            }
        }
    }

    private void ensureBridgePidFile() {
        writeText(new File(BRIDGE_PID_FILE), String.valueOf(android.os.Process.myPid()) + "\n");
        writeText(new File(BRIDGE_SERVICE_FILE), serviceName + "\n");
    }

    private static boolean containsWhitespace(String value) {
        return value.indexOf(' ') >= 0
                || value.indexOf('\t') >= 0
                || value.indexOf('\r') >= 0
                || value.indexOf('\n') >= 0;
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
            System.out.println("daemon_service_warn=service_register_reflect_failed error=" + t.getClass().getName() + ":" + t.getMessage());
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
                                System.out.println("daemon_service_watch=re_registered service=" + sanitizeToken(watchService)
                                        + " old_descriptor=" + sanitizeToken(descriptor));
                            } else {
                                System.out.println("daemon_service_warn=re_register_failed service=" + sanitizeToken(watchService)
                                        + " old_descriptor=" + sanitizeToken(descriptor));
                            }
                        }
                        return null;
                    }
                    if ("asBinder".equals(methodName)) {
                        return null;
                    }
                    if ("toString".equals(methodName)) {
                        return "DaemonServiceCallbackProxy(" + watchService + ")";
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
            System.out.println("daemon_service_watch=enabled service=" + sanitizeToken(watchService));
        } catch (Throwable t) {
            System.out.println("daemon_service_warn=watch_not_available service=" + sanitizeToken(watchService)
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

    private CtlV2Envelope execCtlV2(String[] args) {
        if (args == null || args.length == 0) {
            return error("ctl.exec", 2, "ksu_dsapi_error=bridge_empty_request");
        }
        for (String arg : args) {
            if (arg == null || arg.indexOf('\n') >= 0 || arg.indexOf('\r') >= 0 || arg.indexOf('\t') >= 0) {
                return error("ctl.exec", 2, "ksu_dsapi_error=bridge_bad_arg");
            }
        }

        String cmd = args[0];
        try {
            switch (cmd) {
                case "status":
                    return ok("ctl.status", buildStatusBody());
                case "start":
                    return daemonStart();
                case "stop":
                    return daemonStop();
                case "module":
                    return handleModule(args);
                case "errors":
                    return handleErrors(args);
                case "runtime":
                    return handleRuntime(args);
                case "bridge":
                    return handleBridge(args);
                case "ui":
                    return handleUi(args);
                case "zygote":
                    return handleZygote(args);
                case "capability":
                    return handleCapability(args);
                case "cmd":
                    return handleDaemonCmd(args);
                default:
                    return error("ctl." + sanitizeType(cmd), 2, "ksu_dsapi_error=unsupported_command");
            }
        } catch (Throwable t) {
            return error("ctl." + sanitizeType(cmd), 255,
                    "ksu_dsapi_error=daemon_exec_failed error=" + t.getClass().getName() + ":" + t.getMessage());
        }
    }

    private CtlV2Envelope daemonStart() {
        if (isDaemonRunning()) {
            return ok("ctl.start", "ksu_dsapi_status=running pid=" + readPid(DAEMON_PID_FILE) + " reason=-");
        }

        String dsapidBin = resolveDsapidBin();
        if (dsapidBin.isEmpty()) {
            return error("ctl.start", 2, "ksu_dsapi_error=dsapid_missing");
        }

        new File(DAEMON_SOCKET).delete();
        new File(RUN_DIR + "/dsapi.data.sock").delete();

        List<String> cmd = new ArrayList<String>();
        cmd.add(dsapidBin);
        cmd.add("--control-socket");
        cmd.add(DAEMON_SOCKET);
        cmd.add("--data-socket");
        cmd.add(DAEMON_SOCKET);
        cmd.add("--unified-socket");
        cmd.add("1");
        cmd.add("--render-output-dir");
        cmd.add(BASE_DIR + "/render");
        cmd.add("--module-root-dir");
        cmd.add(MODULE_ROOT);
        cmd.add("--module-state-root-dir");
        cmd.add(MODULE_STATE_ROOT);
        cmd.add("--module-disabled-dir");
        cmd.add(MODULE_DISABLED_DIR);
        cmd.add("--module-registry-file");
        cmd.add(MODULE_REGISTRY_FILE);
        cmd.add("--module-scope-file");
        cmd.add(MODULE_SCOPE_FILE);
        cmd.add("--module-action-timeout-sec");
        cmd.add("60");

        File logFile = new File(DAEMON_LOG_FILE);
        File parent = logFile.getParentFile();
        if (parent != null && !parent.exists()) {
            parent.mkdirs();
        }

        Process p;
        try {
            ProcessBuilder pb = new ProcessBuilder(cmd);
            pb.redirectErrorStream(true);
            pb.redirectOutput(ProcessBuilder.Redirect.appendTo(logFile));
            p = pb.start();
        } catch (Throwable t) {
            return error("ctl.start", 2,
                    "ksu_dsapi_error=daemon_start_failed error=" + t.getClass().getName() + ":" + t.getMessage());
        }

        int pid = getProcessPid(p);
        if (pid > 0) {
            writeText(new File(DAEMON_PID_FILE), String.valueOf(pid) + "\n");
        }

        sleepSilently(120L);
        if (!isDaemonRunning()) {
            new File(DAEMON_PID_FILE).delete();
            return error("ctl.start", 2, "ksu_dsapi_error=daemon_start_failed");
        }

        CmdResult sync = execDsapictl("MODULE_SYNC");
        String body = "ksu_dsapi_status=running pid=" + readPid(DAEMON_PID_FILE) + " reason=-";
        if (sync.exitCode != 0) {
            body += "\nksu_dsapi_error=module_sync_failed";
        }
        String zygoteService = readZygoteAgentService();
        if (zygoteService.isEmpty()) {
            zygoteService = ZYGOTE_AGENT_DEFAULT_SERVICE;
        }
        CtlV2Envelope zygote = startZygoteAgent(zygoteService, serviceName);
        if (zygote.resultCode != 0) {
            body += "\nksu_dsapi_error=zygote_agent_start_failed";
        }
        return ok("ctl.start", body);
    }

    private CtlV2Envelope daemonStop() {
        String pid = readPid(DAEMON_PID_FILE);
        if (!pid.isEmpty() && isPidAlive(pid)) {
            runProcess(Arrays.asList("/system/bin/kill", pid), 2_000L);
            sleepSilently(220L);
            if (isPidAlive(pid)) {
                runProcess(Arrays.asList("/system/bin/kill", "-9", pid), 2_000L);
            }
        }
        new File(DAEMON_PID_FILE).delete();
        new File(DAEMON_SOCKET).delete();
        new File(RUN_DIR + "/dsapi.data.sock").delete();
        stopZygoteAgent();
        return ok("ctl.stop", "ksu_dsapi_status=stopped pid=- reason=manual_stop");
    }

    private CtlV2Envelope handleDaemonCmd(String[] args) {
        if (args.length < 2) {
            return error("ctl.cmd", 2, "ksu_dsapi_error=missing_command");
        }
        String[] daemonArgs = Arrays.copyOfRange(args, 1, args.length);
        return fromDsapictl("ctl.cmd", daemonArgs);
    }

    private CtlV2Envelope handleModule(String[] args) {
        if (args.length < 2) {
            return error("ctl.module", 1, "usage: module <subcommand>");
        }
        String sub = args[1];
        switch (sub) {
            case "list":
                return fromDsapictl("ctl.module.list", new String[]{"MODULE_LIST"});
            case "status":
                if (args.length < 3) {
                    return error("ctl.module.status", 2, "ksu_dsapi_error=module_id_missing");
                }
                return fromDsapictl("ctl.module.status", new String[]{"MODULE_STATUS", args[2]});
            case "detail":
                if (args.length < 3) {
                    return error("ctl.module.detail", 2, "ksu_dsapi_error=module_id_missing");
                }
                return fromDsapictl("ctl.module.detail", new String[]{"MODULE_DETAIL", args[2]});
            case "start":
                if (args.length < 3) {
                    return error("ctl.module.start", 2, "ksu_dsapi_error=module_id_missing");
                }
                return fromDsapictl("ctl.module.start", new String[]{"MODULE_START", args[2], "*", "-1"});
            case "stop":
                if (args.length < 3) {
                    return error("ctl.module.stop", 2, "ksu_dsapi_error=module_id_missing");
                }
                return fromDsapictl("ctl.module.stop", new String[]{"MODULE_STOP", args[2]});
            case "reload":
                if (args.length < 3) {
                    return error("ctl.module.reload", 2, "ksu_dsapi_error=module_id_missing");
                }
                return fromDsapictl("ctl.module.reload", new String[]{"MODULE_RELOAD", args[2], "*", "-1"});
            case "reload-all":
                return fromDsapictl("ctl.module.reload_all", new String[]{"MODULE_RELOAD_ALL", "*", "-1"});
            case "disable":
                if (args.length < 3) {
                    return error("ctl.module.disable", 2, "ksu_dsapi_error=module_id_missing");
                }
                return fromDsapictl("ctl.module.disable", new String[]{"MODULE_DISABLE", args[2]});
            case "enable":
                if (args.length < 3) {
                    return error("ctl.module.enable", 2, "ksu_dsapi_error=module_id_missing");
                }
                return fromDsapictl("ctl.module.enable", new String[]{"MODULE_ENABLE", args[2]});
            case "remove":
                if (args.length < 3) {
                    return error("ctl.module.remove", 2, "ksu_dsapi_error=module_id_missing");
                }
                return fromDsapictl("ctl.module.remove", new String[]{"MODULE_REMOVE", args[2]});
            case "action-list":
                if (args.length < 3) {
                    return error("ctl.module.action_list", 2, "ksu_dsapi_error=module_id_missing");
                }
                return fromDsapictl("ctl.module.action_list", new String[]{"MODULE_ACTION_LIST", args[2]});
            case "action-run":
                if (args.length < 4) {
                    return error("ctl.module.action_run", 2, "ksu_dsapi_error=module_action_missing");
                }
                return fromDsapictl("ctl.module.action_run", new String[]{"MODULE_ACTION_RUN", args[2], args[3], "*", "-1"});
            case "env-list":
                if (args.length < 3) {
                    return error("ctl.module.env_list", 2, "ksu_dsapi_error=module_id_missing");
                }
                return fromDsapictl("ctl.module.env_list", new String[]{"MODULE_ENV_LIST", args[2]});
            case "env-set":
                if (args.length < 5) {
                    return error("ctl.module.env_set", 2, "ksu_dsapi_error=module_env_key_missing");
                }
                return fromDsapictl("ctl.module.env_set", new String[]{"MODULE_ENV_SET", args[2], args[3], args[4]});
            case "env-unset":
                if (args.length < 4) {
                    return error("ctl.module.env_unset", 2, "ksu_dsapi_error=module_env_key_missing");
                }
                return fromDsapictl("ctl.module.env_unset", new String[]{"MODULE_ENV_UNSET", args[2], args[3]});
            case "scope-list":
                if (args.length >= 3) {
                    return fromDsapictl("ctl.module.scope_list", new String[]{"MODULE_SCOPE_LIST", args[2]});
                }
                return fromDsapictl("ctl.module.scope_list", new String[]{"MODULE_SCOPE_LIST"});
            case "scope-set":
                if (args.length < 6) {
                    return error("ctl.module.scope_set", 2, "ksu_dsapi_error=scope_policy_missing");
                }
                return fromDsapictl("ctl.module.scope_set",
                        new String[]{"MODULE_SCOPE_SET", args[2], args[3], args[4], args[5]});
            case "scope-clear":
                if (args.length < 3) {
                    return error("ctl.module.scope_clear", 2, "ksu_dsapi_error=module_id_missing");
                }
                return fromDsapictl("ctl.module.scope_clear", new String[]{"MODULE_SCOPE_CLEAR", args[2]});
            case "zip-list":
                return moduleZipList();
            case "install":
            case "install-zip":
                if (args.length < 3) {
                    return error("ctl.module.install", 2, "ksu_dsapi_error=module_zip_missing");
                }
                return moduleInstallZip(args[2]);
            case "install-builtin":
                if (args.length < 3) {
                    return error("ctl.module.install_builtin", 2, "ksu_dsapi_error=module_zip_name_missing");
                }
                return moduleInstallBuiltin(args[2]);
            default:
                return error("ctl.module", 1, "ksu_dsapi_error=module_subcommand_invalid");
        }
    }

    private CtlV2Envelope handleErrors(String[] args) {
        String sub = args.length >= 2 ? args[1] : "last";
        if ("clear".equals(sub)) {
            new File(LAST_ERROR_FILE).delete();
            return ok("ctl.errors.clear", "last_error_state=cleared");
        }
        if (!"last".equals(sub)) {
            return error("ctl.errors", 1, "usage: errors last|clear");
        }
        File f = new File(LAST_ERROR_FILE);
        if (!f.isFile()) {
            return ok("ctl.errors.last", "last_error_state=none");
        }
        List<String> lines = readLines(f);
        StringBuilder out = new StringBuilder();
        for (String line : lines) {
            if (line == null || line.trim().isEmpty()) {
                continue;
            }
            if (out.length() > 0) {
                out.append('\n');
            }
            out.append("last_error_").append(line.trim());
        }
        if (out.length() == 0) {
            out.append("last_error_state=none");
        }
        return ok("ctl.errors.last", out.toString());
    }

    private CtlV2Envelope handleRuntime(String[] args) {
        String sub = args.length >= 2 ? args[1] : "active";
        if ("active".equals(sub)) {
            String active = resolveActiveReleaseId();
            if (active.isEmpty()) {
                active = "<none>";
            }
            return ok("ctl.runtime.active", "runtime_active=" + active);
        }
        if ("list".equals(sub)) {
            String active = resolveActiveReleaseId();
            File root = new File(RELEASES_DIR);
            File[] entries = root.listFiles();
            if (entries == null || entries.length == 0) {
                return ok("ctl.runtime.list", "");
            }
            List<String> ids = new ArrayList<String>();
            for (File f : entries) {
                if (f.isDirectory()) {
                    ids.add(f.getName());
                }
            }
            Collections.sort(ids);
            StringBuilder out = new StringBuilder();
            for (String id : ids) {
                if (out.length() > 0) {
                    out.append('\n');
                }
                out.append("runtime_release=").append(id)
                        .append(" active=").append(id.equals(active) ? "1" : "0");
            }
            return ok("ctl.runtime.list", out.toString());
        }
        if ("activate".equals(sub)) {
            if (args.length < 3) {
                return error("ctl.runtime.activate", 2, "ksu_dsapi_error=runtime_id_missing");
            }
            return runtimeActivate(args[2]);
        }
        if ("install".equals(sub)) {
            if (args.length < 4) {
                return error("ctl.runtime.install", 2, "ksu_dsapi_error=runtime_src_missing");
            }
            return runtimeInstall(args[2], args[3]);
        }
        if ("remove".equals(sub)) {
            if (args.length < 3) {
                return error("ctl.runtime.remove", 2, "ksu_dsapi_error=runtime_id_missing");
            }
            return runtimeRemove(args[2]);
        }
        return error("ctl.runtime", 1, "ksu_dsapi_error=runtime_subcommand_invalid");
    }

    private CtlV2Envelope handleBridge(String[] args) {
        String sub = args.length >= 2 ? args[1] : "status";
        String body = "ksu_dsapi_bridge state=running pid=" + android.os.Process.myPid() + " service=" + sanitizeToken(serviceName);
        if ("status".equals(sub) || "start".equals(sub) || "restart".equals(sub) || "stop".equals(sub)) {
            return ok("ctl.bridge." + sanitizeType(sub), body);
        }
        return error("ctl.bridge", 1, "ksu_dsapi_error=bridge_subcommand_invalid");
    }

    private CtlV2Envelope handleUi(String[] args) {
        String sub = args.length >= 2 ? args[1] : "status";
        if ("status".equals(sub)) {
            return ok("ctl.ui.status", "ksu_dsapi_ui " + uiStatusLine());
        }
        if ("stop".equals(sub)) {
            stopManagerHost();
            forceStopManagerPackage();
            new File(UI_PID_FILE).delete();
            return ok("ctl.ui.stop", "ksu_dsapi_ui=stopped mode=parasitic_host package=" + MANAGER_PACKAGE);
        }
        if ("start".equals(sub) || "run".equals(sub)) {
            String refresh = args.length >= 3 ? args[2] : "1000";
            if (!isUint(refresh)) {
                return error("ctl.ui.start", 2, "ksu_dsapi_error=invalid_refresh_ms");
            }
            CtlV2Envelope hostStart = startManagerHost(refresh);
            if (hostStart.resultCode != 0) {
                return hostStart;
            }
            return ok("ctl.ui.start", "ksu_dsapi_ui=started " + uiStatusLine() + " refresh_ms=" + refresh);
        }
        return error("ctl.ui", 1, "ksu_dsapi_error=ui_subcommand_invalid");
    }

    private CtlV2Envelope handleZygote(String[] args) {
        String sub = args.length >= 2 ? args[1] : "status";
        if ("status".equals(sub)) {
            return ok("ctl.zygote.status", "ksu_dsapi_zygote " + zygoteAgentStatusLine());
        }
        if ("policy-list".equals(sub)) {
            return ok("ctl.zygote.policy_list", buildZygoteScopeListBody());
        }
        if ("policy-set".equals(sub)) {
            if (args.length < 5) {
                return error("ctl.zygote.policy_set", 2, "ksu_dsapi_error=zygote_policy_missing");
            }
            String pkg = args[2];
            String user = args[3];
            String policy = args[4];
            if (!isValidScopePackage(pkg)) {
                return error("ctl.zygote.policy_set", 2, "ksu_dsapi_error=scope_package_invalid");
            }
            if (!isSignedInteger(user)) {
                return error("ctl.zygote.policy_set", 2, "ksu_dsapi_error=scope_user_invalid");
            }
            if (!("allow".equals(policy) || "deny".equals(policy))) {
                return error("ctl.zygote.policy_set", 2, "ksu_dsapi_error=scope_policy_invalid");
            }
            if (!upsertZygoteScopeRule(pkg, Integer.parseInt(user), policy)) {
                return error("ctl.zygote.policy_set", 2, "ksu_dsapi_error=zygote_scope_write_failed");
            }
            return ok("ctl.zygote.policy_set", "zygote_scope_action=set package=" + pkg + " user=" + user + " policy=" + policy);
        }
        if ("policy-clear".equals(sub)) {
            if (args.length >= 4) {
                String pkg = args[2];
                String user = args[3];
                if (!isValidScopePackage(pkg) || !isSignedInteger(user)) {
                    return error("ctl.zygote.policy_clear", 2, "ksu_dsapi_error=zygote_scope_filter_invalid");
                }
                if (!clearZygoteScopeRule(pkg, Integer.parseInt(user))) {
                    return error("ctl.zygote.policy_clear", 2, "ksu_dsapi_error=zygote_scope_write_failed");
                }
                return ok("ctl.zygote.policy_clear", "zygote_scope_action=cleared package=" + pkg + " user=" + user);
            }
            new File(ZYGOTE_SCOPE_FILE).delete();
            return ok("ctl.zygote.policy_clear", "zygote_scope_action=cleared_all");
        }
        if ("stop".equals(sub)) {
            stopZygoteAgent();
            return ok("ctl.zygote.stop", "ksu_dsapi_zygote " + zygoteAgentStatusLine());
        }
        if ("start".equals(sub) || "restart".equals(sub)) {
            String zygoteService = args.length >= 3 ? args[2] : readZygoteAgentService();
            if (zygoteService.isEmpty()) {
                zygoteService = ZYGOTE_AGENT_DEFAULT_SERVICE;
            }
            String daemonService = args.length >= 4 ? args[3] : readZygoteDaemonService();
            if (daemonService.isEmpty()) {
                daemonService = serviceName;
            }
            if ("restart".equals(sub)) {
                stopZygoteAgent();
            }
            CtlV2Envelope started = startZygoteAgent(zygoteService, daemonService);
            if (started.resultCode != 0) {
                return started;
            }
            return ok("ctl.zygote." + sanitizeType(sub), "ksu_dsapi_zygote " + zygoteAgentStatusLine());
        }
        return error("ctl.zygote", 1, "ksu_dsapi_error=zygote_subcommand_invalid");
    }

    private CtlV2Envelope handleCapability(String[] args) {
        if (args.length < 2) {
            return error("ctl.capability", 1, "ksu_dsapi_error=capability_subcommand_missing");
        }
        String sub = args[1];
        if ("list".equals(sub)) {
            String line = "capability_row=core.daemon|DSAPI Core Daemon|core|builtin|"
                    + daemonStateToken() + "|" + daemonPidToken() + "|" + daemonReasonToken();
            return ok("ctl.capability.list", line);
        }
        if ("status".equals(sub) && args.length >= 3 && "core.daemon".equals(args[2])) {
            String body = "capability_status id=core.daemon state=" + daemonStateToken()
                    + " pid=" + daemonPidToken() + " reason=" + daemonReasonToken();
            return ok("ctl.capability.status", body);
        }
        if ("start".equals(sub) && args.length >= 3 && "core.daemon".equals(args[2])) {
            CtlV2Envelope start = daemonStart();
            return new CtlV2Envelope(start.resultVersion, "ctl.capability.start", start.resultCode, start.resultMessage,
                    "capability_action=started id=core.daemon\n" + start.resultBody);
        }
        if ("stop".equals(sub) && args.length >= 3 && "core.daemon".equals(args[2])) {
            CtlV2Envelope stop = daemonStop();
            return new CtlV2Envelope(stop.resultVersion, "ctl.capability.stop", stop.resultCode, stop.resultMessage,
                    "capability_action=stopped id=core.daemon\n" + stop.resultBody);
        }
        if ("detail".equals(sub) && args.length >= 3 && "core.daemon".equals(args[2])) {
            String body = "id=core.daemon\n"
                    + "name=DSAPI Core Daemon\n"
                    + "kind=core\n"
                    + "source=builtin\n"
                    + "desc=DSAPI core daemon\n"
                    + "status=state=" + daemonStateToken() + " pid=" + daemonPidToken() + " reason=" + daemonReasonToken();
            return ok("ctl.capability.detail", body);
        }
        return error("ctl.capability", 1, "ksu_dsapi_error=capability_subcommand_invalid");
    }

    private CtlV2Envelope moduleZipList() {
        File zipDir = new File(moduleRootDir(), "module_zips");
        File[] entries = zipDir.listFiles();
        if (entries == null || entries.length == 0) {
            return ok("ctl.module.zip_list", "");
        }
        List<String> rows = new ArrayList<String>();
        for (File file : entries) {
            if (file == null || !file.isFile()) {
                continue;
            }
            String name = file.getName();
            if (!name.endsWith(".zip")) {
                continue;
            }
            String shortName = name.substring(0, name.length() - 4);
            rows.add("module_zip_row=" + shortName + "|" + file.getAbsolutePath());
        }
        Collections.sort(rows);
        StringBuilder body = new StringBuilder();
        for (String row : rows) {
            if (body.length() > 0) {
                body.append('\n');
            }
            body.append(row);
        }
        return ok("ctl.module.zip_list", body.toString());
    }

    private CtlV2Envelope moduleInstallBuiltin(String zipName) {
        String base = zipName == null ? "" : zipName.trim();
        if (base.isEmpty()) {
            return error("ctl.module.install_builtin", 2, "ksu_dsapi_error=module_zip_name_missing");
        }
        String fileName = base.endsWith(".zip") ? base : base + ".zip";
        File zip = new File(new File(moduleRootDir(), "module_zips"), fileName);
        if (!zip.isFile()) {
            return error("ctl.module.install_builtin", 2,
                    "ksu_dsapi_error=module_zip_not_found path=" + zip.getAbsolutePath());
        }
        return moduleInstallZipInternal(zip, "ctl.module.install_builtin");
    }

    private CtlV2Envelope moduleInstallZip(String zipPath) {
        if (zipPath == null || zipPath.trim().isEmpty()) {
            return error("ctl.module.install", 2, "ksu_dsapi_error=module_zip_missing");
        }
        return moduleInstallZipInternal(new File(zipPath), "ctl.module.install");
    }

    private CtlV2Envelope moduleInstallZipInternal(File zipFile, String resultType) {
        if (zipFile == null || !zipFile.isFile()) {
            return error(resultType, 2,
                    "ksu_dsapi_error=module_zip_not_found path=" + (zipFile == null ? "-" : zipFile.getAbsolutePath()));
        }

        File unpackDir = new File(RUN_DIR, "module.unpack." + System.nanoTime());
        if (!unpackDir.mkdirs()) {
            return error(resultType, 2, "ksu_dsapi_error=module_unpack_dir_create_failed");
        }

        String moduleId = "";
        try {
            if (!unzipArchive(zipFile, unpackDir)) {
                return error(resultType, 2, "ksu_dsapi_error=module_install_failed path=" + zipFile.getAbsolutePath());
            }
            File meta = findFirstFileByName(unpackDir, "dsapi.module", 4);
            if (meta == null || !meta.isFile()) {
                return error(resultType, 2, "ksu_dsapi_error=module_meta_missing path=" + zipFile.getAbsolutePath());
            }
            File srcRoot = meta.getParentFile();
            if (srcRoot == null || !srcRoot.isDirectory()) {
                return error(resultType, 2, "ksu_dsapi_error=module_root_invalid path=" + zipFile.getAbsolutePath());
            }

            moduleId = metaGet(meta, "MODULE_ID");
            if (moduleId.isEmpty()) {
                moduleId = metaGet(meta, "DSAPI_MODULE_ID");
            }
            if (moduleId.isEmpty()) {
                moduleId = srcRoot.getName();
            }
            if (!isValidId(moduleId)) {
                return error(resultType, 2, "ksu_dsapi_error=module_id_invalid id=" + sanitizeToken(moduleId));
            }

            File dstDir = new File(MODULE_ROOT, moduleId);
            if (!deleteTree(dstDir)) {
                return error(resultType, 2, "ksu_dsapi_error=module_replace_failed id=" + moduleId);
            }
            if (!dstDir.mkdirs()) {
                return error(resultType, 2, "ksu_dsapi_error=module_dir_create_failed id=" + moduleId);
            }
            if (!copyTree(srcRoot, dstDir)) {
                deleteTree(dstDir);
                return error(resultType, 2, "ksu_dsapi_error=module_copy_failed id=" + moduleId);
            }

            File dstMeta = new File(dstDir, "dsapi.module");
            if (!dstMeta.isFile() && !copyFile(meta, dstMeta)) {
                deleteTree(dstDir);
                return error(resultType, 2, "ksu_dsapi_error=module_meta_copy_failed id=" + moduleId);
            }

            File stateDir = new File(MODULE_STATE_ROOT, moduleId);
            if (!stateDir.exists() && !stateDir.mkdirs()) {
                return error(resultType, 2, "ksu_dsapi_error=module_state_dir_create_failed id=" + moduleId);
            }

            setExecutableScripts(new File(dstDir, "capabilities"));
            setExecutableScripts(new File(dstDir, "actions"));

            File envValues = new File(dstDir, "env.values");
            if (!envValues.isFile()) {
                writeText(envValues, "");
            }
            new File(MODULE_DISABLED_DIR, moduleId + ".disabled").delete();

            CmdResult sync = execDsapictl("MODULE_SYNC");
            if (sync.exitCode != 0) {
                return error(resultType, 2, "ksu_dsapi_error=module_sync_failed id=" + moduleId);
            }

            String autoStart = metaGet(dstMeta, "MODULE_AUTO_START");
            if (autoStart.isEmpty()) {
                autoStart = metaGet(dstMeta, "DSAPI_MODULE_AUTO_START");
            }
            if (isTrueToken(autoStart)) {
                execDsapictl("MODULE_START", moduleId, scopePackageArg(), scopeUserArg());
            }
            return ok(resultType,
                    "module_action=installed id=" + moduleId + " path=" + zipFile.getAbsolutePath());
        } finally {
            deleteTree(unpackDir);
        }
    }

    private CtlV2Envelope runtimeActivate(String releaseId) {
        String id = releaseId == null ? "" : releaseId.trim();
        if (!isValidId(id)) {
            return error("ctl.runtime.activate", 2, "ksu_dsapi_error=runtime_id_invalid");
        }
        File target = new File(RELEASES_DIR, id);
        if (!target.isDirectory()) {
            return error("ctl.runtime.activate", 2, "ksu_dsapi_error=runtime_not_found release=" + id);
        }
        if (!switchCurrentRelease(target)) {
            return error("ctl.runtime.activate", 2, "ksu_dsapi_error=runtime_activate_failed release=" + id);
        }
        writeText(new File(ACTIVE_RELEASE_FILE), id + "\n");
        if ("1".equals(readEnabledToken())) {
            daemonStop();
            CtlV2Envelope started = daemonStart();
            if (started.resultCode != 0) {
                return error("ctl.runtime.activate", started.resultCode,
                        "ksu_dsapi_error=runtime_restart_failed release=" + id);
            }
        }
        return ok("ctl.runtime.activate", "runtime_action=activated release=" + id);
    }

    private CtlV2Envelope runtimeInstall(String releaseId, String sourceDir) {
        String id = releaseId == null ? "" : releaseId.trim();
        if (!isValidId(id)) {
            return error("ctl.runtime.install", 2, "ksu_dsapi_error=runtime_id_invalid");
        }
        File src = new File(sourceDir == null ? "" : sourceDir);
        if (!src.isDirectory()) {
            return error("ctl.runtime.install", 2, "ksu_dsapi_error=runtime_src_missing");
        }
        File dst = new File(RELEASES_DIR, id);
        if (dst.exists()) {
            return error("ctl.runtime.install", 2, "ksu_dsapi_error=runtime_exists release=" + id);
        }
        File srcDsapid = new File(src, "bin/dsapid");
        File srcDsapictl = new File(src, "bin/dsapictl");
        if (!srcDsapid.isFile() || !srcDsapictl.isFile()) {
            return error("ctl.runtime.install", 2, "ksu_dsapi_error=runtime_bins_missing");
        }
        if (!dst.mkdirs()) {
            return error("ctl.runtime.install", 2, "ksu_dsapi_error=runtime_dir_create_failed");
        }
        if (!copyTree(src, dst)) {
            deleteTree(dst);
            return error("ctl.runtime.install", 2, "ksu_dsapi_error=runtime_copy_failed");
        }
        new File(dst, "bin/dsapid").setExecutable(true, false);
        new File(dst, "bin/dsapictl").setExecutable(true, false);
        setExecutableScripts(new File(dst, "capabilities"));
        return ok("ctl.runtime.install", "runtime_action=installed release=" + id);
    }

    private CtlV2Envelope runtimeRemove(String releaseId) {
        String id = releaseId == null ? "" : releaseId.trim();
        if (!isValidId(id)) {
            return error("ctl.runtime.remove", 2, "ksu_dsapi_error=runtime_id_invalid");
        }
        String active = resolveActiveReleaseId();
        if (id.equals(active)) {
            return error("ctl.runtime.remove", 2, "ksu_dsapi_error=runtime_remove_active_forbidden");
        }
        File target = new File(RELEASES_DIR, id);
        if (!target.isDirectory()) {
            return error("ctl.runtime.remove", 2, "ksu_dsapi_error=runtime_not_found release=" + id);
        }
        if (!deleteTree(target)) {
            return error("ctl.runtime.remove", 2, "ksu_dsapi_error=runtime_remove_failed release=" + id);
        }
        return ok("ctl.runtime.remove", "runtime_action=removed release=" + id);
    }

    private boolean switchCurrentRelease(File targetRelease) {
        File current = new File(CURRENT_DIR);
        File parent = current.getParentFile();
        if (parent != null && !parent.exists() && !parent.mkdirs()) {
            return false;
        }
        Path currentPath = current.toPath();
        try {
            if (Files.exists(currentPath, LinkOption.NOFOLLOW_LINKS) && !deleteTree(current)) {
                return false;
            }
            Files.createSymbolicLink(currentPath, targetRelease.toPath());
            return true;
        } catch (Throwable ignored) {
            return false;
        }
    }

    private static String scopePackageArg() {
        String value = System.getenv("DSAPI_SCOPE_PACKAGE");
        if (value == null) {
            return "*";
        }
        String out = value.trim();
        if (out.isEmpty() || containsWhitespace(out)) {
            return "*";
        }
        return out;
    }

    private static String scopeUserArg() {
        String value = System.getenv("DSAPI_SCOPE_USER");
        if (value == null) {
            return "-1";
        }
        String out = value.trim();
        if (out.isEmpty()) {
            return "-1";
        }
        if (!isSignedInteger(out)) {
            return "-1";
        }
        return out;
    }

    private static boolean isSignedInteger(String text) {
        if (text == null || text.isEmpty()) {
            return false;
        }
        int start = text.charAt(0) == '-' ? 1 : 0;
        if (start >= text.length()) {
            return false;
        }
        for (int i = start; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c < '0' || c > '9') {
                return false;
            }
        }
        return true;
    }

    private static boolean isValidId(String text) {
        if (text == null || text.isEmpty()) {
            return false;
        }
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            boolean ok = (c >= 'a' && c <= 'z')
                    || (c >= 'A' && c <= 'Z')
                    || (c >= '0' && c <= '9')
                    || c == '.'
                    || c == '_'
                    || c == '-';
            if (!ok) {
                return false;
            }
        }
        return true;
    }

    private static String metaGet(File metaFile, String key) {
        if (metaFile == null || !metaFile.isFile() || key == null || key.isEmpty()) {
            return "";
        }
        String prefix = key + "=";
        for (String line : readLines(metaFile)) {
            if (line == null) {
                continue;
            }
            if (!line.startsWith(prefix)) {
                continue;
            }
            String value = line.substring(prefix.length()).replace("\r", "").trim();
            return value;
        }
        return "";
    }

    private static boolean isTrueToken(String raw) {
        if (raw == null) {
            return false;
        }
        String v = raw.trim().toLowerCase();
        return "1".equals(v) || "true".equals(v) || "yes".equals(v) || "on".equals(v);
    }

    private static void setExecutableScripts(File dir) {
        if (dir == null || !dir.isDirectory()) {
            return;
        }
        File[] files = dir.listFiles();
        if (files == null) {
            return;
        }
        for (File file : files) {
            if (file == null || !file.isFile()) {
                continue;
            }
            if (!file.getName().endsWith(".sh")) {
                continue;
            }
            file.setExecutable(true, false);
        }
    }

    private static boolean unzipArchive(File zipFile, File outDir) {
        if (zipFile == null || !zipFile.isFile() || outDir == null) {
            return false;
        }
        byte[] buffer = new byte[16 * 1024];
        String outCanonical;
        try {
            outCanonical = outDir.getCanonicalPath() + File.separator;
        } catch (Throwable t) {
            return false;
        }
        ZipInputStream zis = null;
        try {
            zis = new ZipInputStream(new BufferedInputStream(new FileInputStream(zipFile)));
            ZipEntry entry;
            while ((entry = zis.getNextEntry()) != null) {
                String name = entry.getName();
                if (name == null || name.isEmpty()) {
                    continue;
                }
                File target = new File(outDir, name);
                String canonical = target.getCanonicalPath();
                if (!canonical.startsWith(outCanonical)) {
                    return false;
                }
                if (entry.isDirectory()) {
                    if (!target.exists() && !target.mkdirs()) {
                        return false;
                    }
                    continue;
                }
                File parent = target.getParentFile();
                if (parent != null && !parent.exists() && !parent.mkdirs()) {
                    return false;
                }
                FileOutputStream fos = null;
                BufferedOutputStream bos = null;
                try {
                    fos = new FileOutputStream(target, false);
                    bos = new BufferedOutputStream(fos, buffer.length);
                    int n;
                    while ((n = zis.read(buffer)) > 0) {
                        bos.write(buffer, 0, n);
                    }
                    bos.flush();
                } finally {
                    if (bos != null) {
                        try {
                            bos.close();
                        } catch (Throwable ignored) {
                        }
                    } else if (fos != null) {
                        try {
                            fos.close();
                        } catch (Throwable ignored) {
                        }
                    }
                }
            }
            return true;
        } catch (Throwable ignored) {
            return false;
        } finally {
            if (zis != null) {
                try {
                    zis.close();
                } catch (Throwable ignored) {
                }
            }
        }
    }

    private static File findFirstFileByName(File root, String fileName, int maxDepth) {
        if (root == null || !root.isDirectory() || fileName == null || fileName.isEmpty() || maxDepth < 0) {
            return null;
        }
        File[] entries = root.listFiles();
        if (entries == null) {
            return null;
        }
        for (File entry : entries) {
            if (entry != null && entry.isFile() && fileName.equals(entry.getName())) {
                return entry;
            }
        }
        if (maxDepth == 0) {
            return null;
        }
        for (File entry : entries) {
            if (entry == null || !entry.isDirectory()) {
                continue;
            }
            try {
                if (Files.isSymbolicLink(entry.toPath())) {
                    continue;
                }
            } catch (Throwable ignored) {
            }
            File found = findFirstFileByName(entry, fileName, maxDepth - 1);
            if (found != null) {
                return found;
            }
        }
        return null;
    }

    private static boolean copyTree(File src, File dst) {
        if (src == null || dst == null || !src.exists()) {
            return false;
        }
        try {
            if (Files.isSymbolicLink(src.toPath())) {
                return false;
            }
        } catch (Throwable ignored) {
        }
        if (src.isDirectory()) {
            if (!dst.exists() && !dst.mkdirs()) {
                return false;
            }
            File[] children = src.listFiles();
            if (children == null) {
                return true;
            }
            for (File child : children) {
                if (child == null) {
                    continue;
                }
                if (!copyTree(child, new File(dst, child.getName()))) {
                    return false;
                }
            }
            return true;
        }
        return copyFile(src, dst);
    }

    private static boolean copyFile(File src, File dst) {
        if (src == null || dst == null || !src.isFile()) {
            return false;
        }
        File parent = dst.getParentFile();
        if (parent != null && !parent.exists() && !parent.mkdirs()) {
            return false;
        }
        FileInputStream in = null;
        FileOutputStream out = null;
        byte[] buffer = new byte[16 * 1024];
        try {
            in = new FileInputStream(src);
            out = new FileOutputStream(dst, false);
            int n;
            while ((n = in.read(buffer)) > 0) {
                out.write(buffer, 0, n);
            }
            out.flush();
            return true;
        } catch (Throwable ignored) {
            return false;
        } finally {
            if (in != null) {
                try {
                    in.close();
                } catch (Throwable ignored) {
                }
            }
            if (out != null) {
                try {
                    out.close();
                } catch (Throwable ignored) {
                }
            }
        }
    }

    private static boolean deleteTree(File target) {
        if (target == null) {
            return true;
        }
        Path path;
        try {
            path = target.toPath();
            if (!Files.exists(path, LinkOption.NOFOLLOW_LINKS)) {
                return true;
            }
            if (Files.isSymbolicLink(path)) {
                Files.deleteIfExists(path);
                return true;
            }
        } catch (Throwable ignored) {
            path = null;
        }

        if (target.isDirectory()) {
            File[] children = target.listFiles();
            if (children != null) {
                for (File child : children) {
                    if (!deleteTree(child)) {
                        return false;
                    }
                }
            }
        }

        try {
            if (path != null) {
                Files.deleteIfExists(path);
            } else {
                target.delete();
            }
        } catch (Throwable ignored) {
            return false;
        }
        return !target.exists();
    }

    private CtlV2Envelope fromDsapictl(String type, String[] daemonArgs) {
        CmdResult res = execDsapictl(daemonArgs);
        if (res.exitCode == 0) {
            return ok(type, normalizeDaemonOutput(res.output));
        }
        return error(type, res.exitCode == 0 ? 255 : res.exitCode, normalizeDaemonOutput(res.output));
    }

    private CmdResult execDsapictl(String... daemonArgs) {
        String bin = resolveDsapictlBin();
        if (bin.isEmpty()) {
            return new CmdResult(127, "ksu_dsapi_error=dsapictl_missing");
        }
        if (!new File(DAEMON_SOCKET).exists()) {
            return new CmdResult(2, "ksu_dsapi_error=daemon_channel_unavailable");
        }
        List<String> cmd = new ArrayList<String>();
        cmd.add(bin);
        cmd.add("--socket");
        cmd.add(DAEMON_SOCKET);
        cmd.addAll(Arrays.asList(daemonArgs));
        return runProcess(cmd, 15_000L);
    }

    private String buildStatusBody() {
        StringBuilder out = new StringBuilder();
        out.append("ksu_dsapi_status=").append(daemonStateToken())
                .append(" pid=").append(daemonPidToken())
                .append(" reason=").append(daemonReasonToken());

        out.append("\nksu_dsapi_ui ").append(uiStatusLine());
        out.append("\nksu_dsapi_zygote ").append(zygoteAgentStatusLine());
        out.append("\nzygote_scope_count=").append(String.valueOf(countZygoteScopeRules()));

        out.append("\nksu_dsapi_bridge state=running pid=")
                .append(android.os.Process.myPid())
                .append(" service=").append(sanitizeToken(serviceName));

        CmdResult ready = execDsapictl("READY");
        String readyBody = normalizeDaemonOutput(ready.output);
        String readyLine = firstNonEmptyLine(readyBody);
        if (ready.exitCode == 0 && readyLine.startsWith("OK ")) {
            String kvLine = readyLine.substring(3);
            String readyState = kvGet(kvLine, "state", "-");
            String modCount = kvGet(kvLine, "module_count", "0");
            String modSeq = kvGet(kvLine, "module_event_seq", "0");
            String modErr = kvGet(kvLine, "module_error", "0");
            out.append("\ndaemon_ready_state=").append(readyState);
            out.append("\ndaemon_module_registry_count=").append(modCount);
            out.append("\ndaemon_module_registry_event_seq=").append(modSeq);
            out.append("\ndaemon_module_registry_error=").append(modErr);
            out.append("\nmodule_count=").append(modCount);
        } else {
            out.append("\ndaemon_ready_state=-");
            out.append("\ndaemon_module_registry_count=0");
            out.append("\ndaemon_module_registry_event_seq=0");
            out.append("\ndaemon_module_registry_error=1");
            out.append("\nmodule_count=0");
        }

        out.append("\nksu_dsapi_last_error ").append(lastErrorStatusLine());

        String active = resolveActiveReleaseId();
        out.append("\nruntime_active=").append(active.isEmpty() ? "<none>" : active);
        out.append("\nenabled=").append(readEnabledToken());
        return out.toString();
    }

    private int countZygoteScopeRules() {
        int count = 0;
        File file = new File(ZYGOTE_SCOPE_FILE);
        if (!file.isFile()) {
            return 0;
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
            if (cols.length < 4 || !"scope".equals(cols[0])) {
                continue;
            }
            count += 1;
        }
        return count;
    }

    private String daemonStateToken() {
        String pid = readPid(DAEMON_PID_FILE);
        if (!pid.isEmpty()) {
            if (isPidAlive(pid)) {
                return "running";
            }
            return "error";
        }
        if (new File(DAEMON_SOCKET).exists()) {
            return "error";
        }
        return "stopped";
    }

    private String daemonPidToken() {
        String pid = readPid(DAEMON_PID_FILE);
        return pid.isEmpty() ? "-" : pid;
    }

    private String daemonReasonToken() {
        String pid = readPid(DAEMON_PID_FILE);
        if (!pid.isEmpty()) {
            if (isPidAlive(pid)) {
                return "-";
            }
            return "stale_pid";
        }
        if (new File(DAEMON_SOCKET).exists()) {
            return "pid_missing";
        }
        return "-";
    }

    private boolean isDaemonRunning() {
        String pid = readPid(DAEMON_PID_FILE);
        return !pid.isEmpty() && isPidAlive(pid);
    }

    private String lastErrorStatusLine() {
        File f = new File(LAST_ERROR_FILE);
        if (!f.isFile()) {
            return "state=none";
        }
        String scope = "unknown";
        String code = "unknown";
        String message = "-";
        String detail = "-";
        String ts = "-";
        for (String line : readLines(f)) {
            int idx = line.indexOf('=');
            if (idx <= 0) {
                continue;
            }
            String k = line.substring(0, idx).trim();
            String v = sanitizeToken(line.substring(idx + 1).trim());
            if ("scope".equals(k)) scope = v;
            else if ("code".equals(k)) code = v;
            else if ("message".equals(k)) message = v;
            else if ("detail".equals(k)) detail = v;
            else if ("ts".equals(k)) ts = v;
        }
        return "state=present scope=" + scope + " code=" + code + " message=" + message + " detail=" + detail + " ts=" + ts;
    }

    private String uiStatusLine() {
        String host = managerHostStatusLine();
        String hostState = kvGet(host, "state", "stopped");
        if ("running".equals(hostState) || "starting".equals(hostState)) {
            return host + " package=" + MANAGER_PACKAGE + " component=" + MANAGER_MAIN_COMPONENT;
        }
        if (isPackageInstalled(MANAGER_PACKAGE)) {
            return "state=stopped pid=- mode=parasitic_host visible=0 package=" + MANAGER_PACKAGE + " component=" + MANAGER_MAIN_COMPONENT;
        }
        return "state=error pid=- mode=parasitic_host reason=manager_not_installed visible=0 package=" + MANAGER_PACKAGE
                + " component=" + MANAGER_MAIN_COMPONENT;
    }

    private String managerHostStatusLine() {
        String visible = isManagerForeground() ? "1" : "0";
        String pid = readPid(MANAGER_HOST_PID_FILE);
        if (!pid.isEmpty()) {
            if (isPidAlive(pid)) {
                String ready = readFirstLine(new File(MANAGER_HOST_READY_FILE));
                String readyState = kvGet(ready, "state", "");
                String readyPid = kvGet(ready, "pid", "");
                String bridge = kvGet(ready, "bridge_service", serviceName);
                if ("ready".equals(readyState) && pid.equals(readyPid)) {
                    return "state=running pid=" + pid + " mode=parasitic_host bridge_service=" + sanitizeToken(bridge)
                            + " visible=" + visible;
                }
                return "state=starting pid=" + pid + " mode=parasitic_host bridge_service=" + sanitizeToken(bridge)
                        + " visible=" + visible;
            }
            new File(MANAGER_HOST_READY_FILE).delete();
            new File(MANAGER_HOST_PID_FILE).delete();
            return "state=error pid=" + pid + " mode=parasitic_host reason=stale_pid visible=" + visible;
        }
        return "state=stopped pid=- mode=parasitic_host visible=" + visible;
    }

    private boolean isManagerForeground() {
        String focusLine = readCurrentFocusLine();
        return !focusLine.isEmpty() && focusLine.contains(MANAGER_PACKAGE + "/");
    }

    private String readCurrentFocusLine() {
        CmdResult res = runProcess(Arrays.asList("/system/bin/dumpsys", "window"), 6_000L);
        if (res.exitCode != 0 || res.output == null || res.output.isEmpty()) {
            return "";
        }
        String[] lines = res.output.split("\\r?\\n");
        for (String line : lines) {
            if (line == null) {
                continue;
            }
            String text = line.trim();
            if (text.startsWith("mCurrentFocus=")) {
                return text;
            }
        }
        return "";
    }

    private void forceManagerForeground() {
        List<String> cmdTool = Arrays.asList(
                "/system/bin/cmd", "activity", "start-activity",
                "--user", "0",
                "--windowingMode", "1",
                "-W",
                "-a", "org.directscreenapi.manager.OPEN",
                "-n", MANAGER_MAIN_COMPONENT,
                "-f", "0x14000000"
        );
        CmdResult result = runProcess(cmdTool, 8_000L);
        if (result.exitCode == 0) {
            return;
        }
        List<String> amTool = Arrays.asList(
                "/system/bin/am", "start",
                "--user", "0",
                "--windowingMode", "1",
                "-W",
                "-a", "org.directscreenapi.manager.OPEN",
                "-n", MANAGER_MAIN_COMPONENT,
                "-f", "0x14000000"
        );
        runProcess(amTool, 8_000L);
    }

    private CtlV2Envelope startManagerHost(String refreshText) {
        if (!isUint(refreshText)) {
            return error("ctl.ui.start", 2, "ksu_dsapi_error=invalid_refresh_ms");
        }
        if (!ensureManagerInstalled()) {
            return error("ctl.ui.start", 2, "ksu_dsapi_error=manager_auto_install_failed");
        }

        String dexPath = resolveAdapterDexPath();
        if (dexPath.isEmpty()) {
            return error("ctl.ui.start", 2, "ksu_dsapi_error=adapter_dex_missing");
        }
        String appProcessBin = findAppProcessBin();
        if (appProcessBin.isEmpty()) {
            return error("ctl.ui.start", 2, "ksu_dsapi_error=app_process_missing");
        }

        stopManagerHost();

        List<String> cmd = new ArrayList<String>();
        cmd.add(appProcessBin);
        cmd.add("/system/bin");
        cmd.add("--nice-name=shell");
        cmd.add("org.directscreenapi.adapter.AndroidAdapterMain");
        cmd.add("manager-host");
        cmd.add(ctlPath);
        cmd.add(MANAGER_MAIN_COMPONENT);
        cmd.add(MANAGER_PACKAGE);
        cmd.add(serviceName);
        cmd.add(refreshText);
        cmd.add(MANAGER_HOST_READY_FILE);

        File logFile = new File(MANAGER_HOST_LOG_FILE);
        File parent = logFile.getParentFile();
        if (parent != null && !parent.exists()) {
            parent.mkdirs();
        }

        Process process;
        try {
            ProcessBuilder pb = new ProcessBuilder(cmd);
            pb.environment().put("CLASSPATH", dexPath);
            pb.redirectErrorStream(true);
            pb.redirectOutput(ProcessBuilder.Redirect.appendTo(logFile));
            process = pb.start();
        } catch (Throwable t) {
            return error("ctl.ui.start", 2, "ksu_dsapi_error=manager_host_spawn_failed");
        }

        int pidInt = getProcessPid(process);
        if (pidInt <= 0) {
            return error("ctl.ui.start", 2, "ksu_dsapi_error=manager_host_pid_missing");
        }
        String pid = String.valueOf(pidInt);
        writeText(new File(MANAGER_HOST_PID_FILE), pid + "\n");
        new File(UI_PID_FILE).delete();
        writeText(new File(UI_PID_FILE), pid + "\n");

        int waitRounds = 60;
        while (waitRounds > 0) {
            if (!isPidAlive(pid)) {
                stopManagerHost();
                return error("ctl.ui.start", 2, "ksu_dsapi_error=manager_host_start_failed");
            }
            String ready = readFirstLine(new File(MANAGER_HOST_READY_FILE));
            String readyState = kvGet(ready, "state", "");
            String readyPid = kvGet(ready, "pid", "");
            if ("ready".equals(readyState) && pid.equals(readyPid)) {
                if (!isManagerForeground()) {
                    forceManagerForeground();
                    sleepSilently(220L);
                }
                if (!isManagerForeground()) {
                    return error("ctl.ui.start", 2,
                            "ksu_dsapi_error=ui_not_foreground package=" + MANAGER_PACKAGE
                                    + " component=" + MANAGER_MAIN_COMPONENT);
                }
                return ok("ctl.ui.start", "ksu_dsapi_ui=started " + uiStatusLine() + " refresh_ms=" + refreshText);
            }
            sleepSilently(50L);
            waitRounds -= 1;
        }
        stopManagerHost();
        return error("ctl.ui.start", 2, "ksu_dsapi_error=manager_host_ready_timeout");
    }

    private void stopManagerHost() {
        String pid = readPid(MANAGER_HOST_PID_FILE);
        if (!pid.isEmpty() && isPidAlive(pid)) {
            runProcess(Arrays.asList("/system/bin/kill", pid), 2_000L);
            sleepSilently(120L);
            if (isPidAlive(pid)) {
                runProcess(Arrays.asList("/system/bin/kill", "-9", pid), 2_000L);
            }
        }
        new File(MANAGER_HOST_PID_FILE).delete();
        new File(MANAGER_HOST_READY_FILE).delete();
        new File(UI_PID_FILE).delete();
    }

    private void forceStopManagerPackage() {
        try {
            Class<?> activityManager = Class.forName("android.app.ActivityManager");
            java.lang.reflect.Method getService = activityManager.getDeclaredMethod("getService");
            Object am = getService.invoke(null);
            if (am != null) {
                java.lang.reflect.Method[] methods = am.getClass().getMethods();
                for (java.lang.reflect.Method method : methods) {
                    if (!"forceStopPackage".equals(method.getName())) {
                        continue;
                    }
                    Class<?>[] pt = method.getParameterTypes();
                    try {
                        if (pt.length == 1 && pt[0] == String.class) {
                            method.invoke(am, MANAGER_PACKAGE);
                            return;
                        }
                        if (pt.length == 2 && pt[0] == String.class && (pt[1] == int.class || pt[1] == Integer.class)) {
                            method.invoke(am, MANAGER_PACKAGE, Integer.valueOf(0));
                            return;
                        }
                    } catch (Throwable ignored) {
                    }
                }
            }
        } catch (Throwable ignored) {
        }
    }

    private String buildZygoteScopeListBody() {
        File file = new File(ZYGOTE_SCOPE_FILE);
        if (!file.isFile()) {
            return "";
        }
        List<String> rows = new ArrayList<String>();
        for (String line : readLines(file)) {
            if (line == null) {
                continue;
            }
            String row = line.trim();
            if (row.isEmpty() || row.startsWith("#")) {
                continue;
            }
            String[] cols = row.split("\\|");
            if (cols.length < 4 || !"scope".equals(cols[0])) {
                continue;
            }
            String pkg = cols[1];
            String user = cols[2];
            String policy = cols[3];
            rows.add("zygote_scope_row=" + pkg + "|" + user + "|" + policy);
        }
        Collections.sort(rows);
        StringBuilder out = new StringBuilder();
        for (String row : rows) {
            if (out.length() > 0) {
                out.append('\n');
            }
            out.append(row);
        }
        return out.toString();
    }

    private boolean upsertZygoteScopeRule(String pkg, int userId, String policy) {
        List<String> out = new ArrayList<String>();
        File file = new File(ZYGOTE_SCOPE_FILE);
        boolean replaced = false;
        if (file.isFile()) {
            for (String line : readLines(file)) {
                if (line == null) {
                    continue;
                }
                String row = line.trim();
                if (row.isEmpty() || row.startsWith("#")) {
                    continue;
                }
                String[] cols = row.split("\\|");
                if (cols.length < 4 || !"scope".equals(cols[0])) {
                    continue;
                }
                if (pkg.equals(cols[1]) && String.valueOf(userId).equals(cols[2])) {
                    out.add("scope|" + pkg + "|" + userId + "|" + policy);
                    replaced = true;
                } else {
                    out.add("scope|" + cols[1] + "|" + cols[2] + "|" + cols[3]);
                }
            }
        }
        if (!replaced) {
            out.add("scope|" + pkg + "|" + userId + "|" + policy);
        }
        Collections.sort(out);
        StringBuilder body = new StringBuilder();
        for (String line : out) {
            if (body.length() > 0) {
                body.append('\n');
            }
            body.append(line);
        }
        if (body.length() > 0) {
            body.append('\n');
        }
        return writeText(file, body.toString());
    }

    private boolean clearZygoteScopeRule(String pkg, int userId) {
        File file = new File(ZYGOTE_SCOPE_FILE);
        if (!file.isFile()) {
            return true;
        }
        List<String> out = new ArrayList<String>();
        for (String line : readLines(file)) {
            if (line == null) {
                continue;
            }
            String row = line.trim();
            if (row.isEmpty() || row.startsWith("#")) {
                continue;
            }
            String[] cols = row.split("\\|");
            if (cols.length < 4 || !"scope".equals(cols[0])) {
                continue;
            }
            if (pkg.equals(cols[1]) && String.valueOf(userId).equals(cols[2])) {
                continue;
            }
            out.add("scope|" + cols[1] + "|" + cols[2] + "|" + cols[3]);
        }
        if (out.isEmpty()) {
            file.delete();
            return true;
        }
        Collections.sort(out);
        StringBuilder body = new StringBuilder();
        for (String line : out) {
            if (body.length() > 0) {
                body.append('\n');
            }
            body.append(line);
        }
        body.append('\n');
        return writeText(file, body.toString());
    }

    private static boolean isValidScopePackage(String pkg) {
        if (pkg == null || pkg.trim().isEmpty()) {
            return false;
        }
        String text = pkg.trim();
        if ("*".equals(text)) {
            return true;
        }
        if (containsWhitespace(text)) {
            return false;
        }
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            boolean ok = (c >= 'a' && c <= 'z')
                    || (c >= 'A' && c <= 'Z')
                    || (c >= '0' && c <= '9')
                    || c == '.'
                    || c == '_'
                    || c == '-';
            if (!ok) {
                return false;
            }
        }
        return true;
    }

    private CtlV2Envelope startZygoteAgent(String zygoteService, String daemonService) {
        if (zygoteService == null || zygoteService.trim().isEmpty() || containsWhitespace(zygoteService)) {
            return error("ctl.zygote.start", 2, "ksu_dsapi_error=zygote_service_invalid");
        }
        if (daemonService == null || daemonService.trim().isEmpty() || containsWhitespace(daemonService)) {
            return error("ctl.zygote.start", 2, "ksu_dsapi_error=daemon_service_invalid");
        }

        String dexPath = resolveAdapterDexPath();
        if (dexPath.isEmpty()) {
            return error("ctl.zygote.start", 2, "ksu_dsapi_error=adapter_dex_missing");
        }
        String appProcessBin = findAppProcessBin();
        if (appProcessBin.isEmpty()) {
            return error("ctl.zygote.start", 2, "ksu_dsapi_error=app_process_missing");
        }

        String status = zygoteAgentStatusLine();
        String state = kvGet(status, "state", "stopped");
        String runningService = kvGet(status, "service", "");
        String runningDaemon = kvGet(status, "daemon_service", "");
        if ("running".equals(state) && zygoteService.equals(runningService) && daemonService.equals(runningDaemon)) {
            return ok("ctl.zygote.start", "ksu_dsapi_zygote " + status);
        }

        stopZygoteAgent();

        List<String> cmd = new ArrayList<String>();
        cmd.add(appProcessBin);
        cmd.add("/system/bin");
        cmd.add("--nice-name=shell");
        cmd.add("org.directscreenapi.adapter.AndroidAdapterMain");
        cmd.add("zygote-agent");
        cmd.add(zygoteService);
        cmd.add(daemonService);
        cmd.add(ZYGOTE_AGENT_READY_FILE);
        cmd.add(ZYGOTE_SCOPE_FILE);

        File logFile = new File(ZYGOTE_AGENT_LOG_FILE);
        File parent = logFile.getParentFile();
        if (parent != null && !parent.exists()) {
            parent.mkdirs();
        }

        Process process;
        try {
            ProcessBuilder pb = new ProcessBuilder(cmd);
            pb.environment().put("CLASSPATH", dexPath);
            pb.redirectErrorStream(true);
            pb.redirectOutput(ProcessBuilder.Redirect.appendTo(logFile));
            process = pb.start();
        } catch (Throwable t) {
            return error("ctl.zygote.start", 2, "ksu_dsapi_error=zygote_agent_spawn_failed");
        }

        int pidInt = getProcessPid(process);
        if (pidInt <= 0) {
            stopZygoteAgent();
            return error("ctl.zygote.start", 2, "ksu_dsapi_error=zygote_agent_pid_missing");
        }
        String pid = String.valueOf(pidInt);
        writeText(new File(ZYGOTE_AGENT_PID_FILE), pid + "\n");
        writeText(new File(ZYGOTE_AGENT_SERVICE_FILE), zygoteService + "\n");
        writeText(new File(ZYGOTE_AGENT_DAEMON_FILE), daemonService + "\n");

        int waitRounds = 60;
        while (waitRounds > 0) {
            if (!isPidAlive(pid)) {
                stopZygoteAgent();
                return error("ctl.zygote.start", 2, "ksu_dsapi_error=zygote_agent_start_failed");
            }
            String ready = readFirstLine(new File(ZYGOTE_AGENT_READY_FILE));
            String readyState = kvGet(ready, "state", "");
            String readyPid = kvGet(ready, "pid", "");
            String readyService = kvGet(ready, "zygote_service", "");
            String readyDaemon = kvGet(ready, "daemon_service", "");
            if ("ready".equals(readyState)
                    && pid.equals(readyPid)
                    && zygoteService.equals(readyService)
                    && daemonService.equals(readyDaemon)) {
                return ok("ctl.zygote.start", "ksu_dsapi_zygote " + zygoteAgentStatusLine());
            }
            sleepSilently(50L);
            waitRounds -= 1;
        }

        stopZygoteAgent();
        return error("ctl.zygote.start", 2, "ksu_dsapi_error=zygote_agent_ready_timeout");
    }

    private void stopZygoteAgent() {
        String pid = readPid(ZYGOTE_AGENT_PID_FILE);
        if (!pid.isEmpty() && isPidAlive(pid)) {
            runProcess(Arrays.asList("/system/bin/kill", pid), 2_000L);
            sleepSilently(120L);
            if (isPidAlive(pid)) {
                runProcess(Arrays.asList("/system/bin/kill", "-9", pid), 2_000L);
            }
        }
        new File(ZYGOTE_AGENT_PID_FILE).delete();
        new File(ZYGOTE_AGENT_READY_FILE).delete();
        new File(ZYGOTE_AGENT_SERVICE_FILE).delete();
        new File(ZYGOTE_AGENT_DAEMON_FILE).delete();
    }

    private String zygoteAgentStatusLine() {
        String service = readZygoteAgentService();
        if (service.isEmpty()) {
            service = ZYGOTE_AGENT_DEFAULT_SERVICE;
        }
        String daemonService = readZygoteDaemonService();
        if (daemonService.isEmpty()) {
            daemonService = serviceName;
        }
        String pid = readPid(ZYGOTE_AGENT_PID_FILE);
        if (!pid.isEmpty()) {
            if (isPidAlive(pid)) {
                String ready = readFirstLine(new File(ZYGOTE_AGENT_READY_FILE));
                String readyState = kvGet(ready, "state", "");
                String readyPid = kvGet(ready, "pid", "");
                if ("ready".equals(readyState) && pid.equals(readyPid)) {
                    return "state=running pid=" + pid + " service=" + sanitizeToken(service)
                            + " daemon_service=" + sanitizeToken(daemonService);
                }
                return "state=starting pid=" + pid + " service=" + sanitizeToken(service)
                        + " daemon_service=" + sanitizeToken(daemonService);
            }
            new File(ZYGOTE_AGENT_PID_FILE).delete();
            new File(ZYGOTE_AGENT_READY_FILE).delete();
            return "state=error pid=" + pid + " reason=stale_pid service=" + sanitizeToken(service)
                    + " daemon_service=" + sanitizeToken(daemonService);
        }
        return "state=stopped pid=- service=" + sanitizeToken(service)
                + " daemon_service=" + sanitizeToken(daemonService);
    }

    private String readZygoteAgentService() {
        return readFirstLine(new File(ZYGOTE_AGENT_SERVICE_FILE));
    }

    private String readZygoteDaemonService() {
        return readFirstLine(new File(ZYGOTE_AGENT_DAEMON_FILE));
    }

    private Context resolveBridgeContext() {
        if (bridgeContext != null) {
            return bridgeContext;
        }
        try {
            Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
            Object app = ReflectBridge.invokeStatic(activityThreadClass, "currentApplication");
            if (app instanceof Context) {
                bridgeContext = (Context) app;
                return bridgeContext;
            }
            Object thread = ReflectBridge.invokeStatic(activityThreadClass, "systemMain");
            Object systemContext = ReflectBridge.invoke(thread, "getSystemContext");
            if (systemContext instanceof Context) {
                bridgeContext = (Context) systemContext;
                return bridgeContext;
            }
        } catch (Throwable ignored) {
        }
        return null;
    }

    private boolean isPackageInstalled(String pkg) {
        if (pkg == null || pkg.trim().isEmpty()) {
            return false;
        }
        Context context = resolveBridgeContext();
        if (context == null) {
            return false;
        }
        PackageManager pm = context.getPackageManager();
        if (pm == null) {
            return false;
        }
        try {
            PackageInfo ignored = pm.getPackageInfo(pkg, 0);
            return true;
        } catch (Throwable ignored) {
            return false;
        }
    }

    private boolean isPackageInstalledViaShell(String pkg) {
        if (pkg == null || pkg.trim().isEmpty()) {
            return false;
        }
        CmdResult cmdPath = runProcess(Arrays.asList("/system/bin/cmd", "package", "path", pkg), 8_000L);
        if (cmdPath.exitCode == 0) {
            return true;
        }
        CmdResult pmPath = runProcess(Arrays.asList("/system/bin/pm", "path", pkg), 8_000L);
        return pmPath.exitCode == 0;
    }

    private File findManagerApkForInstall() {
        File moduleApk = new File(moduleRootDir(), "manager.apk");
        if (moduleApk.isFile()) {
            return moduleApk;
        }
        File fallbackApk = new File("/data/local/tmp/manager.apk");
        if (fallbackApk.isFile()) {
            return fallbackApk;
        }
        return null;
    }

    private boolean ensureManagerInstalled() {
        if (isPackageInstalled(MANAGER_PACKAGE) || isPackageInstalledViaShell(MANAGER_PACKAGE)) {
            return true;
        }
        File managerApk = findManagerApkForInstall();
        if (managerApk == null) {
            System.out.println("ksu_dsapi_error=manager_apk_missing_for_auto_install");
            return false;
        }

        List<List<String>> installCommands = new ArrayList<List<String>>();
        installCommands.add(Arrays.asList("/system/bin/pm", "install", "-r", "--user", "0", managerApk.getAbsolutePath()));
        installCommands.add(Arrays.asList("/system/bin/pm", "install", "-r", managerApk.getAbsolutePath()));
        for (List<String> command : installCommands) {
            CmdResult result = runProcess(command, 30_000L);
            if (result.exitCode == 0
                    && (isPackageInstalled(MANAGER_PACKAGE) || isPackageInstalledViaShell(MANAGER_PACKAGE))) {
                return true;
            }
        }

        runProcess(Arrays.asList("/system/bin/pm", "install-existing", "--user", "0", MANAGER_PACKAGE), 10_000L);
        if (isPackageInstalled(MANAGER_PACKAGE) || isPackageInstalledViaShell(MANAGER_PACKAGE)) {
            return true;
        }
        runProcess(Arrays.asList("/system/bin/cmd", "package", "install-existing", "--user", "0", MANAGER_PACKAGE), 10_000L);
        return isPackageInstalled(MANAGER_PACKAGE) || isPackageInstalledViaShell(MANAGER_PACKAGE);
    }

    private String resolveAdapterDexPath() {
        File current = new File(CURRENT_DIR + "/android/directscreen-adapter-dex.jar");
        if (current.isFile()) {
            return current.getAbsolutePath();
        }
        File module = new File(moduleRootDir() + "/android/directscreen-adapter-dex.jar");
        if (module.isFile()) {
            return module.getAbsolutePath();
        }
        return "";
    }

    private String findAppProcessBin() {
        String[] candidates = new String[]{
                "/system/bin/app_process64",
                "/system/bin/app_process",
                "/system/bin/app_process32"
        };
        for (String candidate : candidates) {
            File f = new File(candidate);
            if (f.isFile() && f.canExecute()) {
                return candidate;
            }
        }
        return "";
    }

    private String moduleRootDir() {
        File ctl = new File(ctlPath);
        File bin = ctl.getParentFile();
        if (bin == null) {
            return "/data/adb/modules/directscreenapi";
        }
        File root = bin.getParentFile();
        if (root == null) {
            return "/data/adb/modules/directscreenapi";
        }
        return root.getAbsolutePath();
    }

    private String resolveDsapictlBin() {
        File current = new File(CURRENT_DIR + "/bin/dsapictl");
        if (current.isFile()) {
            return current.getAbsolutePath();
        }
        File module = new File(moduleRootDir() + "/bin/dsapictl");
        if (module.isFile()) {
            return module.getAbsolutePath();
        }
        return "";
    }

    private String resolveDsapidBin() {
        File current = new File(CURRENT_DIR + "/bin/dsapid");
        if (current.isFile()) {
            return current.getAbsolutePath();
        }
        File module = new File(moduleRootDir() + "/bin/dsapid");
        if (module.isFile()) {
            return module.getAbsolutePath();
        }
        return "";
    }

    private String resolveActiveReleaseId() {
        String active = readFirstLine(new File(ACTIVE_RELEASE_FILE));
        if (!active.isEmpty()) {
            return active;
        }
        File current = new File(CURRENT_DIR);
        try {
            String canonical = current.getCanonicalPath();
            File releases = new File(RELEASES_DIR);
            String prefix = releases.getCanonicalPath() + File.separator;
            if (canonical.startsWith(prefix)) {
                return canonical.substring(prefix.length());
            }
        } catch (Throwable ignored) {
        }
        return "";
    }

    private String readEnabledToken() {
        String v = readFirstLine(new File(ENABLED_FILE));
        if ("0".equals(v)) {
            return "0";
        }
        return "1";
    }

    private static String readPid(String path) {
        if (path == null || path.trim().isEmpty()) {
            return "";
        }
        String raw = readFirstLine(new File(path));
        if (raw.isEmpty()) {
            return "";
        }
        for (int i = 0; i < raw.length(); i++) {
            char c = raw.charAt(i);
            if (c < '0' || c > '9') {
                return "";
            }
        }
        return raw;
    }

    private static CmdResult runProcess(List<String> cmd, long timeoutMs) {
        Process p = null;
        try {
            ProcessBuilder pb = new ProcessBuilder(cmd);
            pb.redirectErrorStream(true);
            p = pb.start();
            String out = readFully(p.getInputStream());

            boolean done = p.waitFor(timeoutMs, TimeUnit.MILLISECONDS);
            if (!done) {
                p.destroy();
                try {
                    p.waitFor(200, TimeUnit.MILLISECONDS);
                } catch (Throwable ignored) {
                }
                p.destroyForcibly();
                return new CmdResult(124, "ksu_dsapi_error=command_timeout");
            }
            int code = p.exitValue();
            return new CmdResult(code, out == null ? "" : out);
        } catch (Throwable t) {
            return new CmdResult(255,
                    "ksu_dsapi_error=command_exec_failed error=" + t.getClass().getName() + ":" + t.getMessage());
        } finally {
            if (p != null) {
                try {
                    p.destroy();
                } catch (Throwable ignored) {
                }
            }
        }
    }

    private static String readFirstLine(File file) {
        if (file == null || !file.isFile()) {
            return "";
        }
        BufferedReader br = null;
        try {
            br = new BufferedReader(new InputStreamReader(new FileInputStream(file), StandardCharsets.UTF_8));
            String line = br.readLine();
            if (line == null) {
                return "";
            }
            return line.trim();
        } catch (Throwable ignored) {
            return "";
        } finally {
            if (br != null) {
                try {
                    br.close();
                } catch (Throwable ignored) {
                }
            }
        }
    }

    private static List<String> readLines(File file) {
        if (file == null || !file.isFile()) {
            return new ArrayList<String>();
        }
        ArrayList<String> out = new ArrayList<String>();
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

    private static boolean writeText(File file, String text) {
        FileOutputStream fos = null;
        try {
            File parent = file.getParentFile();
            if (parent != null && !parent.exists()) {
                parent.mkdirs();
            }
            fos = new FileOutputStream(file, false);
            fos.write(text.getBytes(StandardCharsets.UTF_8));
            fos.flush();
            return true;
        } catch (Throwable ignored) {
            return false;
        } finally {
            if (fos != null) {
                try {
                    fos.close();
                } catch (Throwable ignored) {
                }
            }
        }
    }

    private static String readFully(InputStream input) throws IOException {
        BufferedReader br = new BufferedReader(new InputStreamReader(input, StandardCharsets.UTF_8));
        StringBuilder sb = new StringBuilder();
        String line;
        while ((line = br.readLine()) != null) {
            if (sb.length() > 0) {
                sb.append('\n');
            }
            sb.append(line);
        }
        return sb.toString();
    }

    private static int getProcessPid(Process process) {
        if (process == null) {
            return -1;
        }
        try {
            return process.pid() > Integer.MAX_VALUE ? -1 : (int) process.pid();
        } catch (Throwable ignored) {
        }
        try {
            java.lang.reflect.Field field = process.getClass().getDeclaredField("pid");
            field.setAccessible(true);
            return field.getInt(process);
        } catch (Throwable ignored) {
        }
        return -1;
    }

    private static boolean isPidAlive(String pidText) {
        if (pidText == null || pidText.trim().isEmpty()) {
            return false;
        }
        String pid = pidText.trim();
        for (int i = 0; i < pid.length(); i++) {
            char c = pid.charAt(i);
            if (c < '0' || c > '9') {
                return false;
            }
        }
        return new File("/proc/" + pid).exists();
    }

    private static boolean isUint(String value) {
        if (value == null || value.isEmpty()) {
            return false;
        }
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            if (c < '0' || c > '9') {
                return false;
            }
        }
        return true;
    }

    private static void sleepSilently(long ms) {
        try {
            Thread.sleep(ms);
        } catch (Throwable ignored) {
        }
    }

    private static String kvGet(String line, String key, String fallback) {
        if (line == null || line.isEmpty()) {
            return fallback;
        }
        String prefix = key + "=";
        String[] tokens = line.split(" ");
        for (String token : tokens) {
            if (token.startsWith(prefix)) {
                String v = token.substring(prefix.length());
                return v.isEmpty() ? fallback : v;
            }
        }
        return fallback;
    }

    private static String firstNonEmptyLine(String text) {
        if (text == null || text.isEmpty()) {
            return "";
        }
        String[] lines = text.split("\\r?\\n");
        for (String line : lines) {
            if (line != null && !line.trim().isEmpty()) {
                return line.trim();
            }
        }
        return "";
    }

    private static String normalizeDaemonOutput(String out) {
        if (out == null) {
            return "";
        }
        return out.trim();
    }

    private static CtlV2Envelope ok(String type, String body) {
        return new CtlV2Envelope(2, type, 0, "ok", body == null ? "" : body);
    }

    private static CtlV2Envelope error(String type, int code, String body) {
        return new CtlV2Envelope(2, type, code <= 0 ? 255 : code, "error", body == null ? "" : body);
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

    private static String sanitizeType(String raw) {
        if (raw == null || raw.trim().isEmpty()) {
            return "unknown";
        }
        String out = raw.trim().toLowerCase();
        out = out.replace(' ', '_').replace('\t', '_').replace('\r', '_').replace('\n', '_');
        return out.isEmpty() ? "unknown" : out;
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
                    + " service=" + sanitizeToken(serviceName)
                    + " reason=" + sanitizeToken(reason)
                    + "\n";
            fos = new FileOutputStream(file, false);
            fos.write(line.getBytes(StandardCharsets.UTF_8));
            fos.flush();
        } catch (Throwable t) {
            System.out.println("daemon_service_warn=ready_write_failed error=" + t.getClass().getName() + ":" + t.getMessage());
        } finally {
            if (fos != null) {
                try {
                    fos.close();
                } catch (Throwable ignored) {
                }
            }
        }
    }

    private static boolean enforceBridgeInterface(Parcel data) {
        if (data == null) {
            return false;
        }
        try {
            data.enforceInterface(BridgeContract.DESCRIPTOR_MANAGER);
            return true;
        } catch (Throwable ignored) {
        }
        return false;
    }

    private ParcelFileDescriptor openManagerApkFd() {
        File apk = findManagerApkForInstall();
        if (apk != null) {
            try {
                return ParcelFileDescriptor.open(apk, ParcelFileDescriptor.MODE_READ_ONLY);
            } catch (Throwable ignored) {
            }
        }
        return null;
    }

    private final class BridgeBinder extends Binder {
        @Override
        protected boolean onTransact(int code, Parcel data, Parcel reply, int flags) throws RemoteException {
            if (code == INTERFACE_TRANSACTION) {
                if (reply != null) {
                    reply.writeString(BridgeContract.DESCRIPTOR_MANAGER);
                }
                return true;
            }
            if (code == BridgeContract.TRANSACTION_GET_INFO) {
                if (data == null || reply == null || !enforceBridgeInterface(data)) {
                    return false;
                }
                reply.writeNoException();
                reply.writeInt(BridgeContract.INTERFACE_VERSION);
                reply.writeString(BridgeContract.INTERFACE_NAME);
                reply.writeStringArray(new String[]{
                        BridgeContract.FEATURE_EXEC_V2,
                        BridgeContract.FEATURE_MANAGER_APK_FD
                });
                return true;
            }
            if (code == BridgeContract.TRANSACTION_EXEC_V2) {
                if (data == null || reply == null) {
                    return false;
                }
                if (!enforceBridgeInterface(data)) {
                    return false;
                }
                String[] args = data.createStringArray();
                CtlV2Envelope result = execCtlV2(args);
                reply.writeNoException();
                reply.writeInt(result.resultVersion);
                reply.writeString(result.resultType);
                reply.writeInt(result.resultCode);
                reply.writeString(result.resultMessage);
                reply.writeString(result.resultBody);
                return true;
            }
            if (code == BridgeContract.TRANSACTION_GET_MANAGER_APK_FD) {
                if (data == null || reply == null || !enforceBridgeInterface(data)) {
                    return false;
                }
                ParcelFileDescriptor pfd = openManagerApkFd();
                reply.writeNoException();
                if (pfd == null) {
                    reply.writeInt(0);
                    return true;
                }
                reply.writeInt(1);
                pfd.writeToParcel(reply, 0);
                try {
                    pfd.close();
                } catch (Throwable ignored) {
                }
                return true;
            }
            return super.onTransact(code, data, reply, flags);
        }
    }

    private static final class CmdResult {
        final int exitCode;
        final String output;

        CmdResult(int exitCode, String output) {
            this.exitCode = exitCode;
            this.output = output == null ? "" : output;
        }
    }

    private static final class CtlV2Envelope {
        final int resultVersion;
        final String resultType;
        final int resultCode;
        final String resultMessage;
        final String resultBody;

        CtlV2Envelope(int resultVersion, String resultType, int resultCode, String resultMessage, String resultBody) {
            this.resultVersion = resultVersion;
            this.resultType = (resultType == null || resultType.trim().isEmpty()) ? "ctl.exec" : resultType;
            this.resultCode = resultCode;
            this.resultMessage = (resultMessage == null || resultMessage.trim().isEmpty())
                    ? (resultCode == 0 ? "ok" : "error")
                    : resultMessage;
            this.resultBody = resultBody == null ? "" : resultBody;
        }
    }
}
