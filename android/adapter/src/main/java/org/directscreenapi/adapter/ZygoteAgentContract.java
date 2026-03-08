package org.directscreenapi.adapter;

import android.os.IBinder;

final class ZygoteAgentContract {
    static final String DESCRIPTOR = "org.directscreenapi.daemon.IZygoteAgent";

    static final int TRANSACTION_GET_INFO = IBinder.FIRST_CALL_TRANSACTION;
    static final int TRANSACTION_GET_DAEMON_BINDER = IBinder.FIRST_CALL_TRANSACTION + 1;
    static final int TRANSACTION_SHOULD_INJECT = IBinder.FIRST_CALL_TRANSACTION + 2;

    static final int INTERFACE_VERSION = 1;
    static final String INTERFACE_NAME = "dsapi.zygote_agent";
    static final String FEATURE_DAEMON_BINDER = "daemon_binder";
    static final String FEATURE_SCOPE_DECIDER = "scope_decider";

    private ZygoteAgentContract() {
    }
}
