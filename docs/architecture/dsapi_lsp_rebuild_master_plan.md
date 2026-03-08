# DSAPI 基于 LSP 思路的重构总方案（实时更新）

> 文档定位：本文件是 **DSAPI 重构主控文档**。  
> 目标：按 LSPosed（LSP）思路，重构成“核心常驻 + 模块热插拔 + 管理器可视化 + 无需重启设备装模块”框架。  
> 状态：进行中（会持续更新本文件任务状态与实施记录）。

---

## 1. 重构目标（硬指标）

1. 核心框架常驻稳定：`daemon + bridge + manager` 链路可恢复。
2. 模块热加载：安装/启停/更新模块 **不需要重启设备**。
3. 配置热生效：模块 env 变更后可 `reload` 生效，避免手工多步骤。
4. 统一控制平面：减少 shell 拼接与文本解析，向结构化协议迁移。
5. UI 按 LSPosed 风格重做：信息架构清晰、操作分层、风险操作有护栏。

---

## 2. LSP 思路映射到 DSAPI

LSPosed 的关键思想不是“某个 UI”，而是四层结构：

1. **Loader 层（薄）**：系统启动时只负责拉起核心。
2. **Core/Daemon 层（厚）**：统一状态、统一调度、统一策略。
3. **Service Contract 层（稳定接口）**：对 UI/注入侧提供可版本化 API。
4. **Manager/UI 层（可替换）**：只消费服务，不直控底层细节。

DSAPI 对应重构方向：

- `KSU 脚本` 从“大逻辑执行器”收敛为“引导器 + 兜底”。
- `dsapid` 扩展为 **Module Host**（模块注册、状态机、热重载协调）。
- `dsapi_service_ctl.sh` 从主控降级为“CLI 适配器”。
- `ksu_manager` 变为“LSPosed 风格管理器壳”，核心数据来自 daemon 接口。

---

## 3. 当前项目模块盘点（现状分析）

### 3.1 `core/rust`

- 现状：
  - `dsapid` 已有稳定二进制协议、控制/数据面、事件驱动模型。
  - `RuntimeEngine` 已涵盖显示/触控/渲染/键盘事件。
- 问题：
  - 模块生命周期不在 daemon 内部统一管理（主要在 shell 层拼装）。
  - 缺少“模块注册表 + reload 协调器 + 结构化 module API”。

### 3.2 `android/adapter`

- 现状：
  - 有 bridge server、presenter、screen stream、cap-ui。
- 问题：
  - 与 KSU 脚本强耦合（通过命令拼装和文本回传）。
  - 缺少统一服务契约（模块状态/日志/操作结果未结构化）。

### 3.3 `android/ksu_manager`

- 现状：
  - 已有主页/模块/设置三页雏形，功能可用。
- 问题：
  - UI 是命令驱动 + 高频轮询，缺少仓库层和统一状态模型。
  - 信息架构与 LSPosed 仍有差距（状态、模块、日志、设置耦合偏重）。

### 3.4 `ksu/module_template` + `ksu/module_examples`

- 现状：
  - 已有 module zip、action、env、capability 规范。
- 问题（可用性差的主要来源）：
  - 关键状态大量由 shell 解析拼接，易受输出格式波动影响。
  - `runtime_seed_sync` 在多个入口执行，存在竞态和时序风险。
  - 模块安装后缺少统一“自动接管/热生效”策略（依赖人工动作串联）。
  - action 无统一超时/错误码规范，失败可观测性不足。

### 3.5 `scripts/*`

- 现状：
  - 命令入口统一在 `dsapi.sh`，运维可用。
- 问题：
  - 本地脚本路径与设备端 KSU 路径各有一套生命周期逻辑，重复维护。

---

## 4. 目标架构（V2）

```text
┌──────────────────────────────────────────┐
│ KSU Loader (薄层)                        │
│ post-fs-data/service: 仅拉起 dsapi-core  │
└───────────────┬──────────────────────────┘
                │
┌───────────────▼──────────────────────────┐
│ DSAPI Core Daemon (厚层, 单一事实源)      │
│ - Engine Runtime                          │
│ - Module Registry                         │
│ - Module Lifecycle FSM                    │
│ - Reload Coordinator                      │
│ - Event Bus / Log Stream                  │
└───────────────┬──────────────────────────┘
                │ Stable API (binary/AIDL)
      ┌─────────┴─────────┐
      │                   │
┌─────▼────────────────┐  ┌───────────────────────▼─────┐
│ CLI Adapter          │  │ Manager App (LSP 风格 UI)   │
│ dsapi_service_ctl    │  │ Home / Modules / Logs / Set │
└──────────────────────┘  └─────────────────────────────┘
```

---

## 5. 关键技术路径

## 5.1 模块热加载（不重启设备）

1. `install-zip` 进入 **事务安装流程**：
   - staging 解包 -> 校验 -> 原子替换 -> 注册表更新。
2. 安装完成后由 daemon 判定策略：
   - 新装模块：按 `MODULE_AUTO_START` 决定是否拉起。
   - 已运行模块更新：执行 `reload`（stop->start 或热更新回调）。
3. 全流程写入结构化事件日志（install/start/reload/error）。

## 5.2 配置热生效

- 新增统一命令：`module reload <id>`、`module reload-all`。
- `env-set` 可选触发 `--apply-now`（默认对运行模块自动 reload）。

## 5.3 控制面收敛

- 保留 `dsapi_service_ctl.sh`，但只做参数适配，不承载核心状态机。
- 模块状态、动作结果迁移为结构化响应（避免 grep/sed 脆弱链路）。

## 5.4 KSU 稳定性治理

- `runtime_seed_sync` 迁移为“受锁的一次性/按版本触发”流程。
- 引入全局互斥锁（`flock` 或 lockfile）保护安装、激活、同步路径。
- 所有 action 统一超时与退出码规范，支持 UI 直接展示失败原因。

## 5.5 UI（按 LSPosed 风格重做）

- 目标信息架构：
  - 首页：核心状态、桥接状态、关键告警、快速操作。
  - 模块页：模块列表、启停、作用域/配置入口、实时状态。
  - 日志页：系统日志、模块日志、失败筛选。
  - 设置页：全局参数、桥接参数、实验功能开关。
- 交互原则：
  - 危险操作二次确认（停止核心、移除模块、清空配置）。
  - 列表状态颜色与文案对齐（running/stopped/error/disabled）。
  - 避免 1s 轮询，优先事件刷新 + 手动下拉刷新。

---

## 6. 分阶段任务清单（实时更新）

## 阶段 A：基线梳理与设计冻结

- [x] A1. 全项目模块盘点（core/android/ksu/scripts）。
- [x] A2. 明确 LSP 风格目标架构与改造边界。
- [x] A3. 建立本“重构主控文档”。
- [x] A4. 输出 V2 协议草案（module registry / lifecycle / reload）。

## 阶段 B：核心控制面重构（daemon 优先）

- [x] B1. 在 daemon 侧实现 Module Registry（统一状态源，新增持久化文件与启动恢复）。
- [x] B2. 实现模块生命周期 FSM（installed/enabled/running/error...，并接入 `scope` 拒绝错误落盘）。
- [x] B3. 实现 `module reload` 与 `reload-all`（daemon 原生 `MODULE_RELOAD*`）。
- [ ] B4. 将关键模块命令返回改为结构化结果（兼容旧入口）。
- [ ] B5. 加入安装/更新事务锁，消除 runtime/module 并发竞态。

## 阶段 C：KSU 可用性修复（短期止血）

- [x] C1. 收敛 `runtime_seed_sync` 触发时机（避免重复覆盖）。
- [x] C2. 模块安装后自动策略（auto-start / running-then-reload）。
- [x] C3. action 执行超时/退出码规范化。
- [x] C4. 错误日志聚合（UI 可直接显示最近失败链路）。

## 阶段 D：Manager UI（LSP 风格）

- [x] D1. 重构页面结构：Home / Modules / Logs / Settings。
- [x] D2. 引入统一状态仓库层（避免页面各自拼命令）。
- [ ] D3. 模块详情页（状态、action、env、日志）。
- [ ] D4. 危险操作护栏与可回溯操作记录。
- [ ] D5. 视觉与交互统一（接近 LSPosed 的管理体验）。

## 阶段 E：迁移与收尾

- [ ] E1. 旧命令兼容层 + deprecate 提示。
- [ ] E2. 文档全量更新（commands/spec/ops）。
- [ ] E3. 端到端回归（安装模块、热更新、恢复链路）。
- [ ] E4. 发布一个“可用性优先”的里程碑版本。

---

## 7. 实时进度日志（每轮更新）

- 2026-03-06
  - 完成：项目现状盘点与问题归因。
  - 完成：确定“基于 LSP 思路”的 DSAPI V2 架构方向。
  - 完成：新增本主控文档，作为后续实施与对齐基线。
  - 新增需求：UI 需要做成类似 LSPosed，已纳入阶段 D。
  - 完成：`runtime_seed_sync` 增加互斥锁 + marker 快速跳过，降低竞态和重复同步。
  - 完成：模块安装新增自动策略（运行中模块更新后自动重启；支持 `MODULE_AUTO_START`）。
  - 完成：新增 `module reload <id>` / `module reload-all`（脚本层热生效入口）。
  - 完成：新增 `docs/architecture/dsapi_module_runtime_v2_spec.md`（V2 契约草案）。
  - 完成：脚本新增 `errors last|clear`、`ksu_dsapi_last_error` 状态输出、`reload-all failed_ids`。
  - 完成：Manager UI 扩展为四页（Home/Modules/Logs/Settings），新增 Logs 页面。
  - 完成：引入 `ManagerRepository`，主页/模块页/设置页切换为统一数据仓库调用。
  - 完成：设备侧 smoke（`status`/`module list`/`reload-all`/`errors last`）通过，最近错误链路可读。
  - 完成：`scripts/build_android_manager.sh` 构建通过，产物 `artifacts/ksu_module/android_manager/dsapi-manager.apk`。
  - 完成：`core/rust` 新增 `engine/module_registry.rs`（ModuleRegistry + FSM + 错误模型 + reload-all 聚合）。
  - 完成：`RuntimeEngine` 接入 registry 基础接口（upsert/list/get/reload/reload-all/last_error/event_seq/count）。
  - 完成：`READY` 响应增强（`module_count/module_event_seq/module_error`），为 manager/ctl 读 daemon 状态准备入口。
  - 完成：`status` 输出接入 daemon READY 指标（`daemon_module_registry_*`），便于 shell/UI 感知 daemon 内状态。
  - 完成：Manager Repository 增加 daemon registry 字段解析（首页/日志页可见）。
  - 验证：`cargo test -p directscreen-core module_registry` 通过；`ping_version_and_ready_binary_commands` 通过。
- 2026-03-07
  - 完成：Manager 三页轮询生命周期统一为“前台启动刷新、后台停止刷新”（`Main/Modules/Logs`）。
  - 完成：`ModulesActivity` 接入基于 `daemon_module_registry_event_seq` 的增量刷新与内存缓存，降低 N+1 命令压力。
  - 完成：`ModulesActivity` 将 `module action-list` 改为按需拉取（点击 Action 时加载），移除刷新阶段全量 `action-list`。
  - 完成：`ctl_wire::read_response` EOF 边界修复，新增 payload 丢失单测，避免“有头无体”被静默吞掉。
  - 完成：binary 读帧路径补 `Interrupted` 重试（client `ctl_wire` + daemon `control_dispatch`），提升信号中断场景稳定性。
  - 完成：统一 socket 协议探测从单次 peek 升级为“短帧重试 + 超时退出”，降低二进制短分片误判概率。
  - 完成：`dsapid` 分发层新增“dispatch 饱和 -> overflow thread 兜底”，降低长连接场景下短命令被立即 `ERR BUSY` 的概率。
  - 完成：Bridge 控制面升级为 LSP 风格单契约（`IManagerService + get_info + exec_v2`），Manager 端移除候选 service 探测与 legacy 自动降级。
  - 完成：KSU/Manager 配置侧移除 legacy service 常量与桥接候选服务池，桥接服务名改为“仅用户配置值 + 契约校验”。
  - 完成：`dsapi_service_ctl.sh` 增加显式 `--v2` 包络输出（`result_version/result_type/result_code/result_message` + 原正文），为 CLI 向统一契约迁移提供入口。
  - 完成：Bridge `exec_v2` 强制调用 `ctl --v2` 并解析/校验 v2 包络，再回填 Binder 返回字段；Manager `exec_v2` 增加 `result_version` 与 `result_type` 严格校验。
  - 验证：`scripts/build_android_manager.sh` 通过，产物保持可构建。
  - 验证：`scripts/build_android_adapter.sh` 通过，产物保持可构建。
  - 验证：`scripts/build_ksu_module.sh` 通过，产物 `artifacts/ksu_module/directscreenapi-ksu.zip`。
  - 验证：`cargo test -p directscreen-core read_response_ -- --nocapture` 通过。
  - 验证：`cargo test -p directscreen-core socket_probe_waits_for_full_magic_prefix -- --nocapture` 通过。
  - 验证：`cargo test -p directscreen-core ping_version_and_ready_binary_commands -- --nocapture` 通过。
  - 验证：`cargo test -p directscreen-core module_registry -- --nocapture` 通过。
  - 验证：`cargo test -p directscreen-core -- --nocapture` 全量通过（含 `dsapid` 13 项测试）。
  - 完成：daemon 新增 `ModuleRuntime`（`module_root/state/disabled/registry/scope` 配置化），并由 `RuntimeEngine` 统一托管模块状态。
  - 完成：daemon 控制面新增 `MODULE_*` RPC：`SYNC/LIST/STATUS/DETAIL/START/STOP/RELOAD/RELOAD_ALL/DISABLE/ENABLE/REMOVE/ACTION_LIST/ACTION_RUN/ENV_LIST/ENV_SET/ENV_UNSET/SCOPE_LIST/SCOPE_SET/SCOPE_CLEAR`。
  - 完成：新增 scope（包/用户）数据模型与裁决链路：`module_scope.db` 持久化；执行链路在 `start/reload/action-run` 前进行 allow/deny 裁决；拒绝写入 `E_SCOPE_DENIED`。
  - 完成：模块执行下沉 daemon：`module` 子命令默认走 `dsapi_daemon_cmd MODULE_*`，`dsapi_service_ctl.sh` 不再 shell 主导生命周期与 action 执行。
  - 完成：统一持久化状态源：daemon 持久化 `module_registry.db + module_scope.db`，`status/module_count` 改读 daemon registry 指标。
  - 完成：`dsapid` 配置扩展：`--module-root-dir/--module-state-root-dir/--module-disabled-dir/--module-registry-file/--module-scope-file/--module-action-timeout-sec`。
  - 完成：`dsapictl/ctl_wire` 扩展 `MODULE_*` 命令与可变长度响应体；支持多行结果透传。
  - 完成：KSU `ui start` 主路径切换为 Manager Activity（`am start`），移除 root `app_process + addView` 注入路径对可用性的硬依赖。
  - 完成：KSU 新增 `manager.apk` 自动安装与版本戳校验（按指纹更新，不再每次 action 重装）。
  - 完成：`runtime_seed_sync.marker` 增加 seed 内容指纹，修复同 release id 更新不生效问题。
  - 验证：`cargo test -p directscreen-core -- --nocapture` 全量通过（54 + 13 + 其余 bin 测试）。
  - 验证：`sh -n ksu/module_template/action.sh`、`sh -n ksu/module_template/bin/dsapi_service_ctl.sh`、`sh -n ksu/module_template/bin/dsapi_ksu_lib.sh` 通过。
  - 验证：`scripts/build_ksu_module.sh` 通过（zip 产物可生成）。
- 2026-03-08
  - 完成：移除 `ksu/module_template/bin/dsapi_manager_bridge_handler.sh` 与 `dsapi_manager_bridge_server.sh` 旧 socket/nc 桥接脚本链。
  - 完成：控制面状态输出去 socket 字段（`status` / `core.daemon detail`）。
  - 完成：Manager 侧移除页面自动轮询（`Main/Modules/Logs` 不再 `postDelayed` 周期刷新）。
  - 完成：Bridge descriptor 统一到 `org.directscreenapi.daemon.IDaemonService`，并同步 KSU service 名称可用性校验。
  - 完成：Bridge 控制面去 shell 化（Java 直连实现 `module zip-list/install-zip/install-builtin` 与 `runtime activate/install/remove`）。
  - 完成：移除 `BridgeControlServer` 中 `execCtlV2ViaShell` 与 v2 包络反解析 dead code。
  - 完成：`DsapiCtlClient` 契约提示同步为 `IDaemonService + exec_v2`。
  - 完成：新增 `ParasiticManagerHost`，`ui start/stop/status` 主路径切换为 `app_process --nice-name=shell` 宿主启动。
  - 完成：新增 `ZygoteAgentServer` + `ZygoteAgentContract`，提供 `daemon_binder` 代理服务（`zygote-agent`）。
  - 完成：`dsapi_service_ctl.sh` 增加 `zygote start|stop|restart|status`，`status` 输出纳入 `ksu_dsapi_zygote`。
  - 完成：zygote scope 裁决链（`policy-list/set/clear`）落地，统一持久化 `state/zygote_scope.db` 并接入 `should_inject` 事务。
  - 完成：新增 `zygisk loader` native 骨架 + 构建脚本，并接入 KSU 打包自动嵌入（`zygisk/<abi>.so`）。
  - 新增说明文档：`docs/architecture/dsapi_zygisk_loader_notes.md`。
  - 验证：`scripts/build_android_adapter.sh`、`scripts/build_android_manager.sh`、`scripts/build_ksu_module.sh` 均通过。
  - 新增执行记录：`docs/architecture/dsapi_aidl_no_socket_rebuild.md`。
  - 新增阶段文档：`docs/architecture/dsapi_zygote_parasitic_manager_plan.md`。

---

## 8. 下一步（马上执行）

1. 推进 **zygote 注入链 PoC（阶段 F）**：补齐 `zygisk/riru -> loader -> daemon` 最小闭环，先完成注入握手与进程筛选策略。  
2. 推进 **寄生 manager（阶段 G）**：将 manager 启动路径从 `am start` 迁移为 `app_process + manager dex` 宿主模式，保持 Binder 契约不变。  
3. 推进 **AIDL 直连 daemon 收尾（阶段 H）**：将 UI 状态探测/包安装/前台控制进一步替换为 Binder/系统 API 调用，继续压缩 shell 路径。  
4. 持续仓库优化：分批删除无效兼容代码与未使用文件，保持“无轮询、无回滚、单主路径”。  
