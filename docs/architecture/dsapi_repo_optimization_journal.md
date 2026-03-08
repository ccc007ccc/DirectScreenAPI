# DSAPI 仓库优化追踪笔记（实时更新）

> 目标：围绕“高性能 + 高稳定性”，对全仓逐目录审查、优化与清理。  
> 策略：先标注风险与引用，再执行变更，变更后立即构建/测试验证。  
> 基线：`docs/architecture/_file_inventory.txt` 当前统计 `324` 条。

---

## 1. 目录分布（用于分批推进）

- `third_party/android-sdk`: 171（外部依赖，默认只做版本与来源核验，不做业务改造）
- `core/rust`: 38（核心稳定性与协议路径，优先级 P0）
- `ksu/module_examples`: 18（示例清理与规范收敛，优先级 P2）
- `android/adapter`: 13（桥接链路与能力层，优先级 P1）
- `android/ksu_manager`: 12（现代化 UI 与交互稳定性，优先级 P1）
- `ksu/module_template`: 10（KSU 可用性与脚本收敛，优先级 P0）
- `docs/architecture`: 10（方案与追踪文档，持续更新）
- `docs/guides`: 8（用户文档与命令语义对齐）

---

## 2. 当前阶段与行动顺序

1. `core/rust`：协议健壮性、分流可靠性、长连接公平性。
2. `android/ksu_manager`：去轮询、增量刷新、UI 组件收敛。
3. `ksu/module_template + scripts`：脚本重复逻辑下沉、错误模型统一。
4. `android/adapter`：桥接路径对齐 daemon 新契约。
5. 文档与无用项清理：先标注候选，再低风险删除并验证。

---

## 3. 变更流水（按时间追加）

### 2026-03-07

- ✅ `android/ksu_manager`
  - `Main/Modules/Logs` 统一为前台轮询、后台停止。
  - `ModulesActivity` 增量刷新：基于 `daemon_module_registry_event_seq` + 本地缓存，减少周期性全量命令。
  - `ModulesActivity` 的 `module action-list` 改为按需拉取（仅点击 Action 时请求），刷新路径不再全量 N+1。
  - Binder 客户端改为单契约严格模式：仅 `IManagerService + get_info + exec_v2`，移除候选服务探测和 legacy 自动降级。
  - `ManagerConfig` 移除 `LEGACY_BRIDGE_SERVICE` 映射，配置层不再隐式改写桥接服务名。
  - 验证：`scripts/build_android_manager.sh` 通过。

- ✅ `core/rust`
  - `ctl_wire::read_response` 修复 payload EOF 边界（有头无体返回 `UnexpectedEof`）。
  - binary 读帧路径补 `Interrupted` 重试（`ctl_wire` + `control_dispatch`）。
  - 增加单测：
    - `read_response_returns_none_on_clean_eof`
    - `read_response_payload_missing_returns_unexpected_eof`
  - 统一 socket 协议探测改为短帧重试 + 超时退出，降低分片误判。
  - `dsapid` 分发层新增 `dispatch queue busy -> overflow thread` 兜底，缓解长连接占满 worker 时的短命令饥饿。
  
- ✅ `android/adapter`
  - Bridge 服务端契约切换为单 descriptor（`IManagerService`）+ `get_info/exec_v2`。
  - `exec_v2` 路径强制执行 `dsapi_service_ctl --v2`，并在服务端解析 v2 包络后再返回给 Binder 调用方。
  - 构建脚本补齐新契约文件 `BridgeContract.java` 白名单。
  - 验证：`scripts/build_android_adapter.sh` 通过。

- ✅ `ksu/module_template`
  - `dsapi_ksu_lib.sh` 移除 legacy descriptor 判断与候选 service 池自动挑选逻辑。
  - 桥接启动改为“仅使用请求 service + `IManagerService` 契约可用性校验”，冲突即失败返回。
  - `dsapi_service_ctl.sh` 新增显式 `--v2` 输出包络，统一返回字段：`result_version/result_type/result_code/result_message`。
  - 验证：`sh -n ksu/module_template/bin/dsapi_ksu_lib.sh` 通过。
  - 验证：`sh -n ksu/module_template/bin/dsapi_service_ctl.sh` 通过。
  - 验证：`scripts/build_ksu_module.sh` 通过（zip 产物可生成）。

- ✅ `android/ksu_manager`
  - `exec_v2` 客户端增加包络严格校验（`result_version>=2` 且 `result_type` 非空），异常包络直接判定失败。
  - 增加单测：
    - `socket_probe_waits_for_full_magic_prefix`
  - 验证：
    - `cargo test -p directscreen-core read_response_ -- --nocapture`
    - `cargo test -p directscreen-core socket_probe_waits_for_full_magic_prefix -- --nocapture`
    - `cargo test -p directscreen-core ping_version_and_ready_binary_commands -- --nocapture`
    - `cargo test -p directscreen-core module_registry -- --nocapture`

- ✅ `core/rust + ksu/module_template`（本轮架构下沉）
  - daemon 新增 `ModuleRuntime` 并由 `RuntimeEngine` 托管，模块状态不再依赖 shell 即时计算。
  - 新增 daemon 模块 RPC 控制面（`MODULE_*`），覆盖模块列表/状态/生命周期/action/env/scope。
  - 新增 scope（包/用户）持久化文件 `module_scope.db` 与裁决链路（执行前 allow/deny）。
  - 新增 daemon 持久化状态源 `module_registry.db`，并在 `sync_from_disk` 时统一对齐模块目录。
  - `dsapictl/ctl_wire` 支持 `MODULE_*` 请求与可变长度响应体，多行文本结果可透传给 shell/UI。
  - `dsapi_service_ctl.sh` 的 `module` 子命令改为 daemon-first（严格模式，不再 shell 主导执行路径）。
  - `dsapi_ksu_lib.sh` 启动 daemon 时注入模块路径与持久化文件参数。
  - `action.sh` 调整为 LSP 核心动作：仅重启 bridge，不再自动拉起 UI；UI 由 `ui start` 手动打开。
  - 修复 UI 主阻断：`ui start` 不再走 root `app_process + addView` 注入，改为 Manager Activity 拉起（`am start`）。
  - 修复 Manager 安装链路：新增 `manager.apk` 自动安装/激活与版本戳校验（仅指纹变化时触发更新，不再每次 action 重装）。
  - 修复 runtime 同步：`runtime_seed_sync.marker` 增加种子内容指纹，解决“同 release id 内容更新却不生效”。
  - 验证：
    - `cargo test -p directscreen-core -- --nocapture`
    - `sh -n ksu/module_template/action.sh`
    - `sh -n ksu/module_template/bin/dsapi_service_ctl.sh`
    - `sh -n ksu/module_template/bin/dsapi_ksu_lib.sh`
    - `scripts/build_ksu_module.sh`

### 2026-03-08

- ✅ `ksu/module_template`（进程清理稳定性）
  - 新增 `dsapi_stop_pid` / `dsapi_stop_pidfile_process`，统一“先 PIDFILE，再 `pidof` 精确名”的回收路径。
  - `manager_bridge_stop / manager_host_stop / zygote_agent_stop` 全部移除 `pkill -f` 模糊匹配。
  - `manager-host` 与 `zygote-agent` 启动进程改为唯一 `--nice-name`：
    - `DSAPIManagerHost`
    - `DSAPIZygoteAgent`
  - 目的：避免误杀 `su/sh` 调用链导致 Action/终端会话异常中断（用户侧表现为“执行后崩”）。
  - 验证：
    - `sh -n ksu/module_template/bin/dsapi_ksu_lib.sh`
    - 设备侧 `ui start/status/stop` 全链路通过
    - 设备侧 `zygote start/status/stop` 全链路通过
    - 设备侧 `module action-run test.touch_demo start/status/stop/status` 全链路通过

---

## 4. 候选优化清单（先标注，后执行）

### P0（稳定性）

- [ ] `core/rust/bin/dsapid.rs`：dispatch pool 仍为 `sync_channel(0)+try_send`，高并发长连接场景可能放大拒绝率，需要“长短连接隔离”方案。
- [x] `core/rust/bin/dsapid.rs`：dispatch busy 增加 overflow thread 兜底，降低饥饿与直接拒绝（后续继续做长短连接隔离）。
- [ ] `core/rust/bin/dsapid/control_dispatch.rs`：sendmsg/写入路径补齐短写重试与截断可观测性。
- [ ] `core/rust` Vulkan 相关路径：补有界等待与降级策略，避免极端卡死。
- [ ] `core/rust/engine/module_runtime.rs`：补模块级写锁与 install/reload 并发互斥（当前以单 mutex 保护，仍需细化事务锁）。

### P1（性能与架构）

- [ ] `android/ksu_manager`：抽取底部导航/状态 chip/日志预览公共组件，减少重复代码。
- [x] `android/ksu_manager`：模块 Action 延迟加载（按需 action-list），继续压缩 N+1。
- [ ] `ksu/module_template/bin/dsapi_service_ctl.sh`：与脚本侧 daemon 生命周期逻辑去重，收敛到单一路径。

### P2（清理）

- [ ] 建立“可删除候选列表（文件、理由、引用关系、风险等级）”。
- [ ] 先处理低风险冗余（示例/过期文档/重复脚本片段），每次删除后执行对应构建与测试。

---

## 5. 明确不做（当前阶段）

- 不做回滚机制与旧方案恢复逻辑。
- 不在未验证引用关系前直接删除核心文件或脚本。
- 不引入新的轮询路径替代事件驱动路径。
