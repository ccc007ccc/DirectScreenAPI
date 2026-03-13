package org.directscreenapi.adapter;

import android.os.Binder;
import android.os.IBinder;
import android.os.Parcel;

import java.lang.reflect.Method;

/**
 * “模块即接口”概念验证：
 *
 * - provider: 在独立进程中创建一个 Binder 服务并注册到 Core ServiceRegistry。
 * - consumer: 从 Core ServiceRegistry 获取该 Binder，并直接 transact 调用。
 *
 * 注意：这是 demo，用于验证技术路径；后续会抽离为模块 SDK + 模块进程模型。
 */
final class HelloServiceDemo {
    private static final String HELLO_DESCRIPTOR = "org.directscreenapi.demo.IHelloService";
    private static final int TX_ECHO = IBinder.FIRST_CALL_TRANSACTION;

    private HelloServiceDemo() {
    }

    static int runProvider(String coreServiceName, String serviceId, int version, String meta, String responsePrefix) {
        String core = normalizeServiceName(coreServiceName);
        String id = serviceId == null ? "" : serviceId.trim();
        if (!DsapiServiceRegistry.serviceIdValid(id)) {
            System.out.println("hello_provider_error=service_id_invalid id=" + sanitize(id));
            return 2;
        }
        if (version < 0) version = 0;

        IBinder coreBinder;
        try {
            coreBinder = queryServiceBinder(core);
        } catch (Throwable t) {
            System.out.println("hello_provider_error=core_query_failed service=" + sanitize(core)
                    + " error=" + t.getClass().getName() + ":" + sanitize(t.getMessage()));
            return 2;
        }
        if (coreBinder == null) {
            System.out.println("hello_provider_error=core_missing service=" + sanitize(core));
            return 2;
        }

        final String prefix = responsePrefix == null ? "echo:" : responsePrefix;
        final Binder helloBinder = new Binder() {
            @Override
            protected boolean onTransact(int code, Parcel data, Parcel reply, int flags) throws android.os.RemoteException {
                if (code == INTERFACE_TRANSACTION) {
                    if (reply != null) {
                        reply.writeString(HELLO_DESCRIPTOR);
                    }
                    return true;
                }
                if (code == TX_ECHO) {
                    if (data == null || reply == null) return false;
                    try {
                        data.enforceInterface(HELLO_DESCRIPTOR);
                    } catch (Throwable ignored) {
                        return false;
                    }
                    String in = data.readString();
                    String out = prefix + (in == null ? "" : in);
                    reply.writeNoException();
                    reply.writeString(out);
                    return true;
                }
                return super.onTransact(code, data, reply, flags);
            }
        };

        int rc = registryRegister(coreBinder, id, version, helloBinder, meta);
        if (rc != 0) {
            System.out.println("hello_provider_error=registry_register_failed rc=" + rc + " service_id=" + sanitize(id));
            return 2;
        }

        System.out.println("hello_provider_status=running core=" + sanitize(core)
                + " service_id=" + sanitize(id)
                + " version=" + version
                + " descriptor=" + HELLO_DESCRIPTOR);

        // 作为服务提供者常驻。
        while (true) {
            try {
                Thread.sleep(60_000L);
            } catch (InterruptedException ignored) {
            }
        }
    }

    static int runConsumer(String coreServiceName, String serviceId, String message) {
        String core = normalizeServiceName(coreServiceName);
        String id = serviceId == null ? "" : serviceId.trim();
        if (id.isEmpty()) {
            System.out.println("hello_consumer_error=service_id_missing");
            return 2;
        }

        IBinder coreBinder;
        try {
            coreBinder = queryServiceBinder(core);
        } catch (Throwable t) {
            System.out.println("hello_consumer_error=core_query_failed service=" + sanitize(core)
                    + " error=" + t.getClass().getName() + ":" + sanitize(t.getMessage()));
            return 2;
        }
        if (coreBinder == null) {
            System.out.println("hello_consumer_error=core_missing service=" + sanitize(core));
            return 2;
        }

        IBinder svc = registryGetBinder(coreBinder, id);
        if (svc == null) {
            System.out.println("hello_consumer_error=service_not_found id=" + sanitize(id));
            return 2;
        }

        Parcel data = null;
        Parcel reply = null;
        try {
            data = Parcel.obtain();
            reply = Parcel.obtain();
            data.writeInterfaceToken(HELLO_DESCRIPTOR);
            data.writeString(message == null ? "" : message);
            boolean ok = svc.transact(TX_ECHO, data, reply, 0);
            if (!ok) {
                System.out.println("hello_consumer_error=transact_failed");
                return 2;
            }
            reply.readException();
            String out = reply.readString();
            System.out.println("hello_consumer_reply=" + sanitize(out));
            return 0;
        } catch (Throwable t) {
            System.out.println("hello_consumer_error=exception error=" + t.getClass().getName() + ":" + sanitize(t.getMessage()));
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

    private static int registryRegister(IBinder coreBinder, String serviceId, int version, IBinder binder, String meta) {
        Parcel data = null;
        Parcel reply = null;
        try {
            data = Parcel.obtain();
            reply = Parcel.obtain();
            data.writeInterfaceToken(CoreContract.DESCRIPTOR_CORE);
            data.writeString(serviceId);
            data.writeInt(version);
            data.writeStrongBinder(binder);
            data.writeString(meta == null ? "" : meta);
            boolean ok = coreBinder.transact(CoreContract.TRANSACTION_REGISTRY_REGISTER, data, reply, 0);
            if (!ok) return 2;
            reply.readException();
            int rc = reply.readInt();
            String msg = reply.readString();
            if (rc != 0) {
                System.out.println("hello_provider_warn=registry_register_failed rc=" + rc + " msg=" + sanitize(msg));
            }
            return rc;
        } catch (Throwable t) {
            System.out.println("hello_provider_warn=registry_register_exception error=" + t.getClass().getName() + ":" + sanitize(t.getMessage()));
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

    private static IBinder registryGetBinder(IBinder coreBinder, String serviceId) {
        Parcel data = null;
        Parcel reply = null;
        try {
            data = Parcel.obtain();
            reply = Parcel.obtain();
            data.writeInterfaceToken(CoreContract.DESCRIPTOR_CORE);
            data.writeString(serviceId);
            boolean ok = coreBinder.transact(CoreContract.TRANSACTION_REGISTRY_GET, data, reply, 0);
            if (!ok) return null;
            reply.readException();
            int rc = reply.readInt();
            reply.readString(); // msg
            IBinder binder = reply.readStrongBinder();
            reply.readInt(); // version
            reply.readString(); // meta
            return rc == 0 ? binder : null;
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
        Method getService = serviceManager.getMethod("getService", String.class);
        Object binder = getService.invoke(null, serviceName);
        if (binder instanceof IBinder) {
            return (IBinder) binder;
        }
        return null;
    }

    private static String normalizeServiceName(String serviceName) {
        String service = serviceName == null ? "" : serviceName.trim();
        if (service.isEmpty()) {
            service = "dsapi.core";
        }
        return service;
    }

    private static String sanitize(String raw) {
        if (raw == null) return "-";
        String s = raw.trim();
        if (s.isEmpty()) return "-";
        return s.replace('\n', '_')
                .replace('\r', '_')
                .replace('\t', '_')
                .replace(' ', '_');
    }
}
