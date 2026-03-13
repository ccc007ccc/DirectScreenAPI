package org.directscreenapi.adapter;

import android.os.IBinder;
import android.os.Parcel;

import java.lang.reflect.Method;

final class BridgeExecCli {
    private BridgeExecCli() {
    }

    static int run(String serviceName, String[] args) {
        if (args != null && args.length >= 1) {
            String cmd = args[0] == null ? "" : args[0].trim();
            if ("core-info".equals(cmd)) {
                return execCoreInfo(serviceName);
            }
            if ("registry".equals(cmd)) {
                String sub = args.length >= 2 && args[1] != null ? args[1].trim() : "";
                if ("list".equals(sub)) {
                    return execRegistryList(serviceName);
                }
                if ("get".equals(sub)) {
                    String serviceId = args.length >= 3 ? args[2] : "";
                    return execRegistryGet(serviceName, serviceId);
                }
                if ("unregister".equals(sub)) {
                    String serviceId = args.length >= 3 ? args[2] : "";
                    return execRegistryUnregister(serviceName, serviceId);
                }
                System.out.println("result_version=2");
                System.out.println("result_type=ctl.bridge_exec");
                System.out.println("result_code=2");
                System.out.println("result_message=error");
                System.out.println("ksu_dsapi_error=bridge_exec_usage registry_sub=" + sanitize(sub));
                return 2;
            }
        }

        String service = serviceName == null ? "" : serviceName.trim();
        if (service.isEmpty()) {
            service = "dsapi.core";
        }
        IBinder binder;
        try {
            binder = queryServiceBinder(service);
        } catch (Throwable t) {
            System.out.println("result_version=2");
            System.out.println("result_type=ctl.bridge_exec");
            System.out.println("result_code=2");
            System.out.println("result_message=error");
            System.out.println("ksu_dsapi_error=bridge_query_failed service=" + sanitize(service)
                    + " error=" + t.getClass().getName() + ":" + sanitize(t.getMessage()));
            return 2;
        }
        if (binder == null) {
            System.out.println("result_version=2");
            System.out.println("result_type=ctl.bridge_exec");
            System.out.println("result_code=2");
            System.out.println("result_message=error");
            System.out.println("ksu_dsapi_error=bridge_service_missing service=" + sanitize(service));
            return 2;
        }

        Parcel data = null;
        Parcel reply = null;
        try {
            data = Parcel.obtain();
            reply = Parcel.obtain();
            data.writeInterfaceToken(BridgeContract.DESCRIPTOR_MANAGER);
            data.writeStringArray(args == null ? new String[0] : args);
            boolean ok = binder.transact(BridgeContract.TRANSACTION_EXEC_V2, data, reply, 0);
            if (!ok) {
                System.out.println("result_version=2");
                System.out.println("result_type=ctl.bridge_exec");
                System.out.println("result_code=2");
                System.out.println("result_message=error");
                System.out.println("ksu_dsapi_error=bridge_transact_failed service=" + sanitize(service));
                return 2;
            }
            reply.readException();
            int resultVersion = reply.readInt();
            String resultType = reply.readString();
            int resultCode = reply.readInt();
            String resultMessage = reply.readString();
            String resultBody = reply.readString();

            System.out.println("result_version=" + resultVersion);
            System.out.println("result_type=" + sanitizeEmpty(resultType, "ctl.unknown"));
            System.out.println("result_code=" + resultCode);
            System.out.println("result_message=" + sanitizeEmpty(resultMessage, "error"));
            if (resultBody != null && !resultBody.trim().isEmpty()) {
                System.out.println(resultBody);
            }
            return resultCode <= 0 ? 0 : resultCode;
        } catch (Throwable t) {
            System.out.println("result_version=2");
            System.out.println("result_type=ctl.bridge_exec");
            System.out.println("result_code=2");
            System.out.println("result_message=error");
            System.out.println("ksu_dsapi_error=bridge_exec_exception service=" + sanitize(service)
                    + " error=" + t.getClass().getName() + ":" + sanitize(t.getMessage()));
            return 2;
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

    private static int execCoreInfo(String serviceName) {
        IBinder binder;
        try {
            binder = queryServiceBinder(normalizeServiceName(serviceName));
        } catch (Throwable t) {
            return printError("core_info_query_failed", serviceName, t);
        }
        if (binder == null) {
            return printError("core_info_service_missing", serviceName, null);
        }

        Parcel data = null;
        Parcel reply = null;
        try {
            data = Parcel.obtain();
            reply = Parcel.obtain();
            data.writeInterfaceToken(CoreContract.DESCRIPTOR_CORE);
            boolean ok = binder.transact(CoreContract.TRANSACTION_CORE_GET_INFO, data, reply, 0);
            if (!ok) {
                return printError("core_info_transact_failed", serviceName, null);
            }
            reply.readException();
            int version = reply.readInt();
            String name = reply.readString();
            String[] features = reply.createStringArray();

            System.out.println("result_version=2");
            System.out.println("result_type=ctl.core.info");
            System.out.println("result_code=0");
            System.out.println("result_message=ok");
            System.out.println("core_interface_version=" + version);
            System.out.println("core_interface_name=" + sanitizeEmpty(name, "-"));
            if (features != null && features.length > 0) {
                for (String f : features) {
                    System.out.println("core_feature=" + sanitizeEmpty(f, "-"));
                }
            }
            return 0;
        } catch (Throwable t) {
            return printError("core_info_exception", serviceName, t);
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

    private static int execRegistryList(String serviceName) {
        IBinder binder;
        try {
            binder = queryServiceBinder(normalizeServiceName(serviceName));
        } catch (Throwable t) {
            return printError("registry_list_query_failed", serviceName, t);
        }
        if (binder == null) {
            return printError("registry_list_service_missing", serviceName, null);
        }

        Parcel data = null;
        Parcel reply = null;
        try {
            data = Parcel.obtain();
            reply = Parcel.obtain();
            data.writeInterfaceToken(CoreContract.DESCRIPTOR_CORE);
            boolean ok = binder.transact(CoreContract.TRANSACTION_REGISTRY_LIST, data, reply, 0);
            if (!ok) {
                return printError("registry_list_transact_failed", serviceName, null);
            }
            reply.readException();
            int rc = reply.readInt();
            String msg = reply.readString();
            String[] rows = reply.createStringArray();

            System.out.println("result_version=2");
            System.out.println("result_type=ctl.core.registry.list");
            System.out.println("result_code=" + rc);
            System.out.println("result_message=" + sanitizeEmpty(msg, rc == 0 ? "ok" : "error"));
            if (rows != null) {
                for (String row : rows) {
                    if (row != null && !row.trim().isEmpty()) {
                        System.out.println("registry_row=" + row.trim());
                    }
                }
            }
            return rc == 0 ? 0 : 2;
        } catch (Throwable t) {
            return printError("registry_list_exception", serviceName, t);
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

    private static int execRegistryGet(String serviceName, String serviceIdRaw) {
        String serviceId = serviceIdRaw == null ? "" : serviceIdRaw.trim();
        if (serviceId.isEmpty()) {
            System.out.println("result_version=2");
            System.out.println("result_type=ctl.core.registry.get");
            System.out.println("result_code=2");
            System.out.println("result_message=error");
            System.out.println("ksu_dsapi_error=registry_get_service_id_missing");
            return 2;
        }

        IBinder binder;
        try {
            binder = queryServiceBinder(normalizeServiceName(serviceName));
        } catch (Throwable t) {
            return printError("registry_get_query_failed", serviceName, t);
        }
        if (binder == null) {
            return printError("registry_get_service_missing", serviceName, null);
        }

        Parcel data = null;
        Parcel reply = null;
        try {
            data = Parcel.obtain();
            reply = Parcel.obtain();
            data.writeInterfaceToken(CoreContract.DESCRIPTOR_CORE);
            data.writeString(serviceId);
            boolean ok = binder.transact(CoreContract.TRANSACTION_REGISTRY_GET, data, reply, 0);
            if (!ok) {
                return printError("registry_get_transact_failed", serviceName, null);
            }
            reply.readException();
            int rc = reply.readInt();
            String msg = reply.readString();
            IBinder svc = reply.readStrongBinder();
            int version = reply.readInt();
            String meta = reply.readString();

            System.out.println("result_version=2");
            System.out.println("result_type=ctl.core.registry.get");
            System.out.println("result_code=" + rc);
            System.out.println("result_message=" + sanitizeEmpty(msg, rc == 0 ? "ok" : "error"));
            System.out.println("service_id=" + sanitize(serviceId));
            System.out.println("service_version=" + version);
            System.out.println("service_meta=" + sanitizeEmpty(meta, ""));
            if (svc != null) {
                String desc = "";
                try {
                    desc = svc.getInterfaceDescriptor();
                } catch (Throwable ignored) {
                }
                System.out.println("service_binder=present descriptor=" + sanitizeEmpty(desc, "-"));
            } else {
                System.out.println("service_binder=absent");
            }
            return rc == 0 ? 0 : 2;
        } catch (Throwable t) {
            return printError("registry_get_exception", serviceName, t);
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

    private static int execRegistryUnregister(String serviceName, String serviceIdRaw) {
        String serviceId = serviceIdRaw == null ? "" : serviceIdRaw.trim();
        if (serviceId.isEmpty()) {
            System.out.println("result_version=2");
            System.out.println("result_type=ctl.core.registry.unregister");
            System.out.println("result_code=2");
            System.out.println("result_message=error");
            System.out.println("ksu_dsapi_error=registry_unregister_service_id_missing");
            return 2;
        }

        IBinder binder;
        try {
            binder = queryServiceBinder(normalizeServiceName(serviceName));
        } catch (Throwable t) {
            return printError("registry_unregister_query_failed", serviceName, t);
        }
        if (binder == null) {
            return printError("registry_unregister_service_missing", serviceName, null);
        }

        Parcel data = null;
        Parcel reply = null;
        try {
            data = Parcel.obtain();
            reply = Parcel.obtain();
            data.writeInterfaceToken(CoreContract.DESCRIPTOR_CORE);
            data.writeString(serviceId);
            boolean ok = binder.transact(CoreContract.TRANSACTION_REGISTRY_UNREGISTER, data, reply, 0);
            if (!ok) {
                return printError("registry_unregister_transact_failed", serviceName, null);
            }
            reply.readException();
            int rc = reply.readInt();
            String msg = reply.readString();

            System.out.println("result_version=2");
            System.out.println("result_type=ctl.core.registry.unregister");
            System.out.println("result_code=" + rc);
            System.out.println("result_message=" + sanitizeEmpty(msg, rc == 0 ? "ok" : "error"));
            System.out.println("service_id=" + sanitize(serviceId));
            return rc == 0 ? 0 : 2;
        } catch (Throwable t) {
            return printError("registry_unregister_exception", serviceName, t);
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

    private static String normalizeServiceName(String serviceName) {
        String service = serviceName == null ? "" : serviceName.trim();
        if (service.isEmpty()) {
            service = "dsapi.core";
        }
        return service;
    }

    private static int printError(String code, String serviceName, Throwable t) {
        System.out.println("result_version=2");
        System.out.println("result_type=ctl.bridge_exec");
        System.out.println("result_code=2");
        System.out.println("result_message=error");
        System.out.println("ksu_dsapi_error=" + sanitize(code) + " service=" + sanitize(serviceName));
        if (t != null) {
            System.out.println("error=" + t.getClass().getName() + ":" + sanitize(t.getMessage()));
        }
        return 2;
    }

    private static IBinder queryServiceBinder(String serviceName) throws Exception {
        Class<?> serviceManager = Class.forName("android.os.ServiceManager");
        Method getService = serviceManager.getMethod("getService", String.class);
        Object binder = getService.invoke(null, serviceName);
        if (binder instanceof IBinder) {
            return (IBinder) binder;
        }
        return null;
    }

    private static String sanitize(String raw) {
        if (raw == null || raw.trim().isEmpty()) {
            return "-";
        }
        return raw.trim()
                .replace('\n', '_')
                .replace('\r', '_')
                .replace('\t', '_')
                .replace(' ', '_');
    }

    private static String sanitizeEmpty(String raw, String fallback) {
        if (raw == null || raw.trim().isEmpty()) {
            return fallback;
        }
        return raw.trim();
    }
}
