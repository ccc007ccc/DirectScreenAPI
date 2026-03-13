package org.directscreenapi.manager;

import java.io.IOException;
import java.util.ArrayList;
import java.util.List;

import android.os.IBinder;
import android.os.Parcel;

final class DsapiCtlClient {
    private final ManagerConfig config;

    DsapiCtlClient(ManagerConfig config) {
        this.config = config;
    }

    CmdResult run(String... args) {
        BridgeExecAttempt bridgeAttempt = runViaBridge(args);
        if (bridgeAttempt.result != null) {
            return withFriendlyHint(bridgeAttempt.result);
        }

        StringBuilder out = new StringBuilder();
        out.append("bridge_offline=1");
        if (bridgeAttempt.error != null) {
            out.append("\nbridge_error=")
                    .append(bridgeAttempt.error.getClass().getName())
                    .append(':')
                    .append(bridgeAttempt.error.getMessage());
        }
        out.append("\nfix_hint=当前管理器仅支持 LSP 风格单契约: ICoreService + exec_v2");
        out.append("\nfix_hint2=请确认 bridge 已升级并重启: /data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh bridge restart ")
                .append(config.bridgeService);
        out.append("\nfix_hint_service=当前 binder_service=").append(config.bridgeService);
        out.append("\nfix_hint3=查看桥接日志: /data/adb/dsapi/log/manager_bridge.log");
        return withFriendlyHint(new CmdResult(255, out.toString()));
    }

    static List<String> splitLines(String text) {
        List<String> out = new ArrayList<String>();
        if (text == null || text.isEmpty()) {
            return out;
        }
        String[] parts = text.split("\\r?\\n");
        for (String line : parts) {
            if (!line.isEmpty()) {
                out.add(line);
            }
        }
        return out;
    }

    static String findLineByPrefix(List<String> lines, String prefix) {
        if (lines == null || prefix == null) {
            return null;
        }
        for (String line : lines) {
            if (line.startsWith(prefix)) {
                return line;
            }
        }
        return null;
    }

    static String parseAfterPrefixToken(String line, String prefix) {
        if (line == null || prefix == null || !line.startsWith(prefix)) {
            return "";
        }
        String tail = line.substring(prefix.length());
        int sp = tail.indexOf(' ');
        if (sp < 0) {
            return tail;
        }
        return tail.substring(0, sp);
    }

    static String kvGet(String line, String key) {
        if (line == null || key == null) {
            return "";
        }
        String[] tokens = line.split(" ");
        String prefix = key + "=";
        for (String token : tokens) {
            if (token.startsWith(prefix)) {
                return token.substring(prefix.length());
            }
        }
        return "";
    }

    private CmdResult withFriendlyHint(CmdResult in) {
        if (in == null) {
            return new CmdResult(255, "command_error=null_result");
        }
        String output = in.output == null ? "" : in.output;
        if (output.contains("ctl_not_found")
                || (output.contains("dsapi_service_ctl.sh") && output.contains("No such file or directory"))) {
            output = output
                    + "\nfix_hint=未找到 dsapi_service_ctl.sh，请在 KSU 模块页重新点击 Action 或确认模块 ID=directscreenapi";
        }
        return new CmdResult(in.exitCode, output);
    }

    private BridgeExecAttempt runViaBridge(String... args) {
        for (String arg : args) {
            if (arg == null || arg.indexOf('\n') >= 0 || arg.indexOf('\r') >= 0 || arg.indexOf('\t') >= 0) {
                return BridgeExecAttempt.failure(new IllegalArgumentException("bridge_bad_arg"));
            }
        }

        String serviceName = config.bridgeService == null ? "" : config.bridgeService.trim();
        if (serviceName.isEmpty()) {
            return BridgeExecAttempt.failure(new IOException("bridge_service_missing"));
        }

        return runViaBinder(serviceName, args);
    }

    private BridgeExecAttempt runViaBinder(String serviceName, String... args) {
        try {
            IBinder binder = queryServiceBinder(serviceName);
            if (binder == null) {
                return BridgeExecAttempt.failure(new IOException("bridge_service_not_found"));
            }

            BridgeInfo info = transactGetInfo(binder);
            if (info == null) {
                return BridgeExecAttempt.failure(new IOException("bridge_contract_info_unavailable"));
            }
            if (!info.supportsExecV2) {
                return BridgeExecAttempt.failure(new IOException("bridge_contract_exec_v2_missing"));
            }

            CmdResult result = transactExecV2(binder, BridgeContract.DESCRIPTOR_MANAGER, args);
            if (result == null) {
                return BridgeExecAttempt.failure(new IOException("bridge_transact_exec_v2_failed"));
            }
            return BridgeExecAttempt.success(result);
        } catch (Throwable t) {
            return BridgeExecAttempt.failure(t);
        }
    }

    private static BridgeInfo transactGetInfo(IBinder binder) {
        Parcel data = null;
        Parcel reply = null;
        try {
            data = Parcel.obtain();
            reply = Parcel.obtain();
            data.writeInterfaceToken(BridgeContract.DESCRIPTOR_MANAGER);
            boolean ok = binder.transact(BridgeContract.TRANSACTION_GET_INFO, data, reply, 0);
            if (!ok) {
                return null;
            }
            reply.readException();
            int version = reply.readInt();
            String interfaceName = reply.readString();
            String[] features = reply.createStringArray();

            boolean supportsExecV2 = version >= BridgeContract.INTERFACE_VERSION_V2
                    && containsFeature(features, BridgeContract.FEATURE_EXEC_V2);
            return new BridgeInfo(version, interfaceName, supportsExecV2);
        } catch (Throwable ignored) {
            return null;
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
    }

    private static boolean containsFeature(String[] features, String expected) {
        if (features == null || expected == null || expected.isEmpty()) {
            return false;
        }
        for (String feature : features) {
            if (expected.equals(feature)) {
                return true;
            }
        }
        return false;
    }

    private static CmdResult transactExecV2(IBinder binder, String descriptor, String... args) {
        Parcel data = null;
        Parcel reply = null;
        try {
            data = Parcel.obtain();
            reply = Parcel.obtain();
            data.writeInterfaceToken(descriptor);
            data.writeStringArray(args);
            boolean ok = binder.transact(BridgeContract.TRANSACTION_EXEC_V2, data, reply, 0);
            if (!ok) {
                return null;
            }
            reply.readException();
            int resultVersion = reply.readInt();
            String resultType = reply.readString();
            int resultCode = reply.readInt();
            String resultMessage = reply.readString();
            String body = reply.readString();

            if (resultVersion < BridgeContract.INTERFACE_VERSION_V2) {
                return null;
            }
            if (resultType == null || resultType.trim().isEmpty()) {
                return null;
            }

            StringBuilder out = new StringBuilder();
            out.append("result_version=").append(resultVersion);
            out.append("\nresult_type=").append(resultType == null ? "-" : resultType);
            out.append("\nresult_code=").append(resultCode);
            out.append("\nresult_message=").append(resultMessage == null ? "-" : resultMessage);
            if (body != null && !body.isEmpty()) {
                out.append('\n').append(body);
            }
            return new CmdResult(resultCode, out.toString());
        } catch (Throwable ignored) {
            return null;
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
    }

    private static IBinder queryServiceBinder(String serviceName) throws Exception {
        Class<?> serviceManager = Class.forName("android.os.ServiceManager");
        java.lang.reflect.Method getService = serviceManager.getMethod("getService", String.class);
        Object binder = getService.invoke(null, serviceName);
        if (binder instanceof IBinder) {
            return (IBinder) binder;
        }
        return null;
    }

    static String joinSpace(String[] array) {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < array.length; i++) {
            if (i > 0) {
                sb.append(' ');
            }
            sb.append(array[i]);
        }
        return sb.toString();
    }

    static final class CmdResult {
        final int exitCode;
        final String output;

        CmdResult(int exitCode, String output) {
            this.exitCode = exitCode;
            this.output = output == null ? "" : output;
        }
    }

    private static final class BridgeExecAttempt {
        final CmdResult result;
        final Throwable error;

        private BridgeExecAttempt(CmdResult result, Throwable error) {
            this.result = result;
            this.error = error;
        }

        static BridgeExecAttempt success(CmdResult result) {
            return new BridgeExecAttempt(result, null);
        }

        static BridgeExecAttempt failure(Throwable error) {
            return new BridgeExecAttempt(null, error);
        }
    }

    private static final class BridgeInfo {
        final int interfaceVersion;
        final String interfaceName;
        final boolean supportsExecV2;

        BridgeInfo(int interfaceVersion, String interfaceName, boolean supportsExecV2) {
            this.interfaceVersion = interfaceVersion;
            this.interfaceName = interfaceName == null ? "" : interfaceName;
            this.supportsExecV2 = supportsExecV2;
        }
    }
}
