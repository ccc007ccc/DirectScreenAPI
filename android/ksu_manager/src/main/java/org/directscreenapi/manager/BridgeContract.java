package org.directscreenapi.manager;

import android.os.IBinder;

final class BridgeContract {
    static final String DESCRIPTOR_MANAGER = "org.directscreenapi.daemon.IDaemonService";

    static final int TRANSACTION_GET_INFO = IBinder.FIRST_CALL_TRANSACTION;
    static final int TRANSACTION_EXEC_V2 = IBinder.FIRST_CALL_TRANSACTION + 1;
    static final int TRANSACTION_GET_MANAGER_APK_FD = IBinder.FIRST_CALL_TRANSACTION + 2;

    static final int INTERFACE_VERSION_V2 = 2;
    static final String FEATURE_EXEC_V2 = "exec_v2";
    static final String FEATURE_MANAGER_APK_FD = "manager_apk_fd";

    private BridgeContract() {
    }
}
