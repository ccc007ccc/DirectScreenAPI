# DSAPI AIDL 直连与 Socket 清理执行记录

## 1. 目标

- 将管理链路固定为 `Manager -> AIDL(Binder) -> Bridge Service`。
- 删除旧的 `nc/socket` 桥接脚本路径。
- 去除 Manager 页面自动轮询，改为事件触发/手动刷新。
- 清理控制面输出中对 socket 的暴露。

## 2. 本轮已完成

- 已删除旧桥接脚本：
  - `ksu/module_template/bin/dsapi_manager_bridge_handler.sh`
  - `ksu/module_template/bin/dsapi_manager_bridge_server.sh`
- 已清理构建与权限脚本中对上述脚本的引用：
  - `scripts/build_ksu_module.sh`
  - `ksu/module_template/bin/dsapi_ksu_lib.sh`
- 已清理控制面 socket 暴露字段：
  - `ksu/module_template/bin/dsapi_ksu_lib.sh`
  - `ksu/module_template/bin/dsapi_service_ctl.sh`
  - `ksu/module_template/capabilities/core.daemon.sh`
  - `ksu/module_template/module.prop.in`
- 已移除 Manager 自动轮询：
  - `MainActivity.java`（移除 `RefreshTicker` 周期调度）
  - `ModulesActivity.java`（移除周期 ticker 与 tick 计数驱动）
  - `LogsActivity.java`（移除 `RefreshTicker` 周期调度）

## 3. 验证结果

- `sh -n` 语法检查通过：
  - `dsapi_ksu_lib.sh`
  - `dsapi_service_ctl.sh`
  - `core.daemon.sh`
  - `uninstall.sh`
- 构建通过：
  - `scripts/build_android_manager.sh`
  - `scripts/build_ksu_module.sh`
- 设备侧验证通过：
  - `action.sh`：`mode=lsp_core`
  - `status`：`ksu_dsapi_status` 不再输出 socket 字段
  - `capability detail core.daemon`：不再输出 socket 字段

## 4. 本轮补充（减少 shell 依赖）

- Core contract descriptor 统一为：
  - `org.directscreenapi.core.ICoreService`
- Bridge 服务端 `BridgeControlServer` 已将以下链路改为 Java 直连（不再回落 `ctl.sh --v2`）：
  - `module install-zip/install`
  - `runtime activate/install/remove`
  - 注：`module zip-list/install-builtin` 已在“外置模块化”阶段移除（核心包不再内置 `module_zips`）。
- 管理 UI 启动主路径改为 `parasitic manager host`（`app_process --nice-name=DSAPIManagerHost`）：
  - `ui start/stop/status` 优先走 `manager-host`，保留 activity 状态探测作为补充视图。
- 新增 zygote 注入链前置代理：
  - `zygote-agent`（Binder service）对外提供 `daemon_binder` 透传能力，用于注入侧后续直连 daemon。
- 新增 zygote scope 裁决链：
  - `zygote policy-list/policy-set/policy-clear`（包+用户 allow/deny）统一落盘 `state/zygote_scope.db`。
  - `zygote-agent` 新增 `should_inject` 事务（过滤 isolated/child_zygote/no_data_dir 并应用 scope 规则）。
- 新增 `zygisk loader` native 骨架与打包链：
  - `scripts/build_zygisk_loader.sh`
  - `ksu/zygisk_loader/dsapi_zygisk_loader.cpp`
  - `build_ksu_module.sh` 默认尝试嵌入 `zygisk/<abi>.so`
- Bridge 控制面移除 `execCtlV2ViaShell/parseCtlV2Envelope` 旧 shell 兜底逻辑。
- Manager 提示文案同步为 `ICoreService + exec_v2`。
- 构建验证通过：
  - `scripts/build_android_adapter.sh`
  - `scripts/build_android_manager.sh`
  - `scripts/build_ksu_module.sh`

## 4.1 本轮补充（zygote 链路做实）

- `zygisk loader` 已切换为真实 `zygisk::ModuleBase` 实现（API v5）：
  - 新增 `ksu/zygisk_loader/zygisk.hpp`（公共头落库）。
  - `preAppSpecialize` 通过 Binder 直连 `zygote-agent`：
    - `get_info`（接口版本与 feature 校验）
    - `should_inject`
    - `get_daemon_binder`
  - 裁决失败按 fail-closed 处理，不自动回落旧方案。
- `BridgeControlServer` 进一步减少 shell 依赖：
  - `isPackageInstalled` 改为 `PackageManager` 查询。
  - `isUiRunning` 改为 `ActivityManager` 任务栈查询。
  - `dumpsys activity` / `pm path` 不再作为主探测路径。
- Bridge 与 zygote-agent 增加服务覆盖自愈：
  - 使用 `ServiceManager.registerForNotifications` 监听同名服务注册。
  - 发现 descriptor 不匹配后自动重注册本地 Binder（事件驱动，无轮询）。
- KSU `ui` 控制脚本进一步收敛到寄生 host 主路径：
  - `ui status` 不再依赖 `dumpsys activity` 探测 activity 前台状态。
  - `ui start` 不再依赖 `am` 可用性检查。
  - `ui start` 不再自动执行 manager 安装（去掉隐式 `pm install` 路径）。
  - 若未安装 manager，改为显式报错并提供 `ui install` 手动入口。

## 5. 后续约束

- 控制面禁止恢复 `nc/socket` 文本协议桥接链。
- Manager 端禁止恢复页面级周期轮询。
- Bridge 控制面不再引入新的 shell 回落分支（仅保留必要系统命令调用路径）。
