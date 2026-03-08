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

## 4. 当前边界

- 当前实现完成了“裁决与链路可达性”闭环，尚未在目标进程内消费 daemon binder（后续注入业务逻辑阶段接入）。
- `BridgeControlServer` 的 manager 安装动作仍保留 `pm install` shell 调用（探测链路已迁到系统 API）。

## 5. 下一步

1. 在注入侧增加 daemon binder 消费链（模块查询/偏好读取/策略下发）。
2. 继续压缩 manager 安装链中的 shell 入口（向系统 API 或 daemon 原生事务下沉）。
3. 将 loader 决策日志接入 manager 日志页与统一事件流。
