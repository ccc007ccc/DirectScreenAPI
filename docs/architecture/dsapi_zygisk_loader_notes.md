# DSAPI Zygisk Loader 进展说明

## 1. 目标

- 给 zygote 注入链提供可打包、可编译、可迭代的 native 承载位。
- 在不引入回滚与轮询前提下，逐步对齐 LSP 的注入路径。

## 2. 已落地（第一阶段）

- 新增 native 源码：`ksu/zygisk_loader/dsapi_zygisk_loader.cpp`
  - 导出 `zygisk_module_entry`
  - 提供 `dsapi_should_inject(...)` 供桥接复用
  - 初版过滤条件：isolated / child_zygote / no_data_dir
- 新增构建脚本：`scripts/build_zygisk_loader.sh`
  - 产物：`artifacts/ksu_module/zygisk_loader/libdsapi_zygisk_loader.so`
- KSU 打包接入：`scripts/build_ksu_module.sh`
  - 默认自动尝试构建并嵌入 `zygisk/<abi>.so`
  - 可用 `DSAPI_KSU_WITH_ZYGISK=0` 关闭

## 3. 已落地（第二阶段）

- 新增 `ksu/zygisk_loader/zygisk.hpp`（对齐 Magisk API v5 公共头）。
- `dsapi_zygisk_loader.cpp` 改为 `zygisk::ModuleBase` 实现，走 `preAppSpecialize`。
- loader 在 `preAppSpecialize` 中通过 Binder 直连 `zygote-agent`：
  - `GET_INFO`：接口版本与 feature 校验。
  - `SHOULD_INJECT`：包/用户/sandbox 条件裁决。
  - `GET_DAEMON_BINDER`：校验 daemon binder 可达。
- 若注入裁决失败，loader 走 fail-closed（拒绝注入）并记录日志，不做旧路径降级。

## 3.1 已落地（第三阶段：Manager 注入链）

- Manager 进程默认不再依赖 `ServiceManager.getService("dsapi.core")`（避免 `service_manager find` 被 SELinux 拦截）。
- Zygisk loader 在 `preAppSpecialize`（zygote 权限阶段）通过 `zygote-agent` 获取 `daemon_binder`：
  - 若目标进程为 `org.directscreenapi.manager`，则保存 binder 句柄（`GlobalRef`）。
  - 为保证 `postAppSpecialize` 可继续执行注入线程，Manager 进程不设置 `DLCLOSE_MODULE_LIBRARY`。
- 为提高冷启动可靠性：当 `zygote-agent` 不可用时，Manager 允许直接 `ServiceManager.getService("dsapi.core")` 获取 binder（仍在 zygote 权限阶段进行）。
- Zygisk loader 在 `postAppSpecialize` 启动一次性线程：
  - 等待 `ActivityThread.currentApplication()` 可用。
  - 获取 `Application.getClassLoader()`，用该 ClassLoader 加载 `InjectedCoreBinder`。
  - 调用 `InjectedCoreBinder.setFromZygisk(IBinder, "zygisk")` 写入 binder 句柄。

## 4. 当前边界

- 当前实现完成了“裁决 + 链路可达性 + Manager 自动注入 binder”闭环。
- 仍待补齐：在其他目标进程（非 Manager）内消费 daemon binder（窗口/输入等业务注入阶段）。
- `BridgeControlServer` 的 manager 安装动作仍保留 `pm install` shell 调用（探测链路已迁到系统 API）。

## 5. 下一步

1. 在注入侧增加 daemon binder 消费链（模块查询/偏好读取/策略下发）。
2. 继续压缩 manager 安装链中的 shell 入口（向系统 API 或 daemon 原生事务下沉）。
3. 将 loader 决策日志接入 manager 日志页与统一事件流。
