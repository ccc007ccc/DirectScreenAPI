package org.directscreenapi.manager;

import android.app.Service;
import android.content.Intent;
import android.os.Binder;
import android.os.IBinder;
import android.os.Parcel;
import android.os.RemoteException;

/**
 * 寄生 host 注入入口：
 *
 * - host 进程 bind 到此 Service，把 dsapi.core 的 binder 句柄写进来
 * - Manager 后续执行 ctl 直接使用该 binder，不再依赖 ServiceManager.find
 */
public final class HostHandshakeService extends Service {
    static final String DESCRIPTOR = "org.directscreenapi.manager.IHostHandshake";
    static final int TRANSACTION_SET_CORE = 1;
    static final int TRANSACTION_GET_INFO = 2;

    private static final int UID_ROOT = 0;
    private static final int UID_SYSTEM = 1000;
    private static final int UID_SHELL = 2000;

    private final IBinder binder = new Binder() {
        @Override
        protected boolean onTransact(int code, Parcel data, Parcel reply, int flags) throws RemoteException {
            if (code == INTERFACE_TRANSACTION) {
                if (reply != null) {
                    reply.writeString(DESCRIPTOR);
                }
                return true;
            }

            if (code == TRANSACTION_GET_INFO) {
                if (data == null || reply == null) {
                    return false;
                }
                data.enforceInterface(DESCRIPTOR);
                reply.writeNoException();
                reply.writeInt(0);
                reply.writeString("ok");
                reply.writeString(InjectedCoreBinder.debugLine());
                return true;
            }

            if (code == TRANSACTION_SET_CORE) {
                if (data == null || reply == null) {
                    return false;
                }
                data.enforceInterface(DESCRIPTOR);
                int uid = Binder.getCallingUid();
                if (uid != UID_ROOT && uid != UID_SYSTEM && uid != UID_SHELL) {
                    throw new SecurityException("permission_denied uid=" + uid);
                }
                IBinder core = data.readStrongBinder();
                String serviceName = data.readString();
                InjectedCoreBinder.set(core, "uid=" + uid + " service=" + (serviceName == null ? "" : serviceName));

                reply.writeNoException();
                reply.writeInt(0);
                reply.writeString("ok");
                return true;
            }

            return super.onTransact(code, data, reply, flags);
        }
    };

    @Override
    public IBinder onBind(Intent intent) {
        return binder;
    }
}

