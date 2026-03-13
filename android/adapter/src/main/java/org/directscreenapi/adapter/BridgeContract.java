package org.directscreenapi.adapter;

import android.os.IBinder;

final class BridgeContract {
    // V3：桥接进程对外暴露的是 Core 契约（稳定标识），不再使用旧的 daemon 描述符。
    static final String DESCRIPTOR_MANAGER = CoreContract.DESCRIPTOR_CORE;

    static final int TRANSACTION_GET_INFO = IBinder.FIRST_CALL_TRANSACTION;
    static final int TRANSACTION_EXEC_V2 = IBinder.FIRST_CALL_TRANSACTION + 1;
    static final int TRANSACTION_GET_MANAGER_APK_FD = IBinder.FIRST_CALL_TRANSACTION + 2;

    static final int INTERFACE_VERSION = 3;
    static final String INTERFACE_NAME = "dsapi.core";
    static final String FEATURE_EXEC_V2 = "exec_v2";
    static final String FEATURE_MANAGER_APK_FD = "manager_apk_fd";

    private BridgeContract() {
    }
}
