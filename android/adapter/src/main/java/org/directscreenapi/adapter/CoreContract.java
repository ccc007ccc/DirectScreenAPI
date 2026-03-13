package org.directscreenapi.adapter;

import android.os.IBinder;

/**
 * DSAPI V3 核心 Binder 契约（占位实现，后续会冻结为稳定接口）。
 *
 * 说明：
 * - 这里先用手写 Binder transaction 定义，避免引入 AIDL 编译工具链变更。
 * - 语义等价于 AIDL：强类型、可版本化、可扩展（interfaceVersion + features）。
 */
final class CoreContract {
    static final String DESCRIPTOR_CORE = "org.directscreenapi.core.ICoreService";

    static final int CORE_INTERFACE_VERSION = 1;
    static final String CORE_INTERFACE_NAME = "dsapi.core";
    static final String FEATURE_SERVICE_REGISTRY = "service_registry";

    // 使用较大偏移避免与 BridgeContract 现有 transaction 冲突。
    static final int TRANSACTION_CORE_GET_INFO = IBinder.FIRST_CALL_TRANSACTION + 100;

    static final int TRANSACTION_REGISTRY_REGISTER = IBinder.FIRST_CALL_TRANSACTION + 110;
    static final int TRANSACTION_REGISTRY_GET = IBinder.FIRST_CALL_TRANSACTION + 111;
    static final int TRANSACTION_REGISTRY_LIST = IBinder.FIRST_CALL_TRANSACTION + 112;
    static final int TRANSACTION_REGISTRY_UNREGISTER = IBinder.FIRST_CALL_TRANSACTION + 113;

    private CoreContract() {
    }
}

