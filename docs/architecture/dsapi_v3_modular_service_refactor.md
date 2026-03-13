# DSAPI V3 模块化服务框架重构清单（唯一路线，实时更新）

本文档定位：

- 这是 DSAPI V3 的“唯一路线”设计冻结与实施清单。
- 目标是：高性能、高稳定、强解耦、强扩展。
- 本轮明确约束：不再维护/引入其他路线；不做回退分支；不做兼容旧协议的自动降级。


## 0. 硬约束（不可违反）

1. 只允许 Binder/AIDL（或等价的稳定 Binder 契约）作为控制面与服务发现手段。
1. 禁止新增 Unix socket/nc 文本协议路径；既有 socket 相关代码进入“待删除”清单，迁移完成后删除。
1. 禁止新增“周期轮询刷新”作为主路径。
1. Fail-closed：关键链路失败必须明确失败，不允许悄悄回落旧实现。
1. 单一事实源：持久化状态只能有一个来源（统一为一个 DB/文件），禁止 registry + 文件双轨。
1. 不兼容低版本（按当前仓库 targetSdk=35 与 Java 21 约束推进）。


## 1. 目标架构（V3）

一句话：Core 是一个稳定的 Binder 平台，模块既可以是“应用层”，也可以是“接口提供者”。

### 1.1 角色划分

- Core（核心常驻）
  - 只做：权限/Scope 裁决、模块生命周期、服务注册表、窗口/输入等底盘能力、统一状态与事件。
  - 对外只暴露稳定的 Binder 契约，不暴露内部实现细节。
  - 不内置 demo/业务实现（demo 作为模块分发）。
- Module（可热插拔）
  - 可以是应用：提供 UI/窗口/交互等“业务层”。
  - 也可以是接口：提供某个 Binder Service 给其他模块/Manager 使用。
  - 模块之间通过 Core 的服务注册表解耦；模块不直接依赖 Core 的内部代码或 runtime/current 的“共享资产”。
- Manager（UI）
  - 只消费 Core 的 Binder 契约（事件驱动刷新）。
  - UI 体验对齐 LSPosed 的信息架构与操作护栏，但不参与核心决策逻辑。

### 1.2 关键能力：服务注册表（模块即接口）

Core 提供 `ServiceRegistry`：

- Provider 模块向 Core 注册：`register(serviceId, version, binder, meta)`
- Consumer 通过 Core 获取：`get(serviceId, minVersion)`，拿到 Provider 的 `IBinder` 句柄后直接 Binder 调用。
- Core 只做：版本协商、Scope 校验、死亡监听（binder death）、依赖拉起与状态广播。

这使得“模块即接口”成立：接口不再固化在 Core/Manager，而可由模块提供。


## 2. 进程与启动模型（V3）

### 2.1 必须常驻的进程

1. `dsapi.core`（Core Daemon，Binder 服务端）
1. `dsapi.zygote.injector`（Zygote 注入代理，Binder 服务端）

可选：

- `manager-host`（寄生 host，仅在隐藏 API 受限的机型上作为辅助；默认尽量不需要）

### 2.2 KSU 的职责（收敛为 Loader）

- KSU 脚本只负责：
  - 创建目录与权限（layout）
  - 激活/升级 runtime（只做一次性同步，受全局锁保护）
  - 拉起 `dsapi.core` 与 `dsapi.zygote.injector`
- KSU 不负责：
  - 模块生命周期状态机
  - 服务注册表
  - UI 数据拼装

### 2.3 SELinux（不修改 policy 的路线）

约束：不随 DSAPI 模块分发 `sepolicy.rule/service_contexts`，避免被软件检测或造成系统侧策略变化。

影响：普通 app 进程可能无法直接 `ServiceManager.getService("dsapi.core")`。

路线：对齐 LSPosed 的思路，避免让 untrusted_app 去查找自定义 service：

1. Manager 默认不做 `ServiceManager.getService("dsapi.core")` 作为唯一路径
1. 走 zygote：Zygisk 在 zygote 权限阶段拿到 `dsapi.core` 的 binder 句柄，并在 Manager 进程内写入：
   - `preAppSpecialize` 获取 binder 并保存
   - `postAppSpecialize` 等待 ClassLoader 就绪后调用 `InjectedCoreBinder.setFromZygisk(...)`
1. `parasitic host + HostHandshakeService` 仅作为诊断/辅助路径（例如某些 ROM Zygisk 行为异常时排查用）

说明：这不是降级，而是把“特权能力”固定在特权域内，UI 只做展示与交互。


## 3. 状态模型（单一状态源）

统一持久化为 `state/core_state.db`（文件名暂定）：

- modules：模块元信息、启用状态、运行态、最后错误、更新时间
- scopes：包/用户 scope 规则与裁决结果缓存（可选）
- services：已注册服务（短期内存 + 持久化元数据）
- runtime：当前 active release、版本与兼容性信息

约束：

- 任何 UI/ctl 输出均从 Core 的状态源读取，不允许读多个文件拼装。


## 4. 性能路线（窗口系统）

目标链路（只允许这一条）：

1. 禁止整帧 RGBA 传输（删除 SHM 像素流与“全帧 push”方案）。
1. 只传绘制命令/状态（Binder/AIDL）。
1. Presenter 内部用 EGL（优先）/Vulkan（后续）直接渲染到 SurfaceControl。
1. 全链路 VSync 驱动 + 脏区更新 + 事务仅在几何变化时提交。


## 5. 改造清单（按依赖顺序）

说明：本清单按“先打底、后拔高”的顺序排列；每项完成后必须更新本文件勾选与记录关键变更点。

### 5.1 Core 契约冻结（第一优先级）

- [ ] 定义 V3 的 Binder/AIDL 契约：
  - Core：`ICore`（status/health/events）
  - Module：`IModuleManager`（install/update/remove/enable/disable/start/stop/action-run）
  - Registry：`IServiceRegistry`（register/get/list/watch）
  - Window：`IWindowService`（create/update/destroy，后续）
- [ ] 定义契约版本策略（interfaceVersion + feature flags + minVersion）。
- [ ] 定义错误码规范（稳定、可展示、可诊断）。

### 5.2 Core Daemon 实现（替代 socket 控制面）

- [ ] Core 作为唯一控制面入口：Manager/ctl/modules 全部改走 Binder。
- [ ] 事件驱动：提供 module/service/runtime 的事件订阅回调，UI 不轮询。
- [ ] 统一状态源：落盘 `core_state.db`，删除/停用旧 `module_registry.db + module_scope.db` 双轨（迁移期允许只读导入一次）。

### 5.3 模块执行模型（模块即接口）

- [ ] 模块包规范升级：
  - `dsapi.module` 增加：`PROVIDES_SERVICES`/`REQUIRES_SERVICES`（含版本约束）
  - 模块可包含：`android/*.dex` 或 `bin/*`（实现载荷必须随模块分发）
- [ ] Module Host（在 Core 内）：
  - 按需拉起 Provider 模块
  - Provider 退出/崩溃自动下线服务并发事件
  - 依赖图管理（requires -> autostart dependencies）
- [ ] 将 touchdemo/gpu_demo 等 demo 完全外置为模块（Core 不再内置其实现二进制/DEX）。

### 5.4 Zygote 注入链（严格 fail-closed）

- [ ] Zygisk loader -> zygote-agent -> core 三段链路稳定化：
  - shouldInject 裁决：scope + 进程类型过滤
  - getDaemonBinder/ getCoreBinder：注入侧直连 Core，不经过 shell

### 5.5 UI 重构（LSPosed 风格）

- [ ] Manager UI 只绑定 Core Binder（Repository 层禁止拼 shell 命令）。
- [ ] 信息架构固定：Home/Modules/Logs/Settings + 模块详情页（scope/env/actions/log）。
- [ ] 操作护栏：危险操作二次确认 + 操作记录。


## 6. 待删除清单（迁移完成后执行，需用户确认）

说明：删除属于高风险操作，本段只记录候选项，真正删除前必须再次核对并征得确认。

- [ ] 任何 socket 控制面：`dsapi.sock` / `DaemonSession` / `dsapictl --socket` 等相关路径。
- [ ] 任何整帧 RGBA/SHM 像素流 presenter（仅保留 GPU 命令流）。
- [ ] 任何 Manager 页面周期轮询 ticker。


## 7. 本轮实施日志（实时更新）

- 2026-03-12
  - 建立 V3 “模块即接口”重构清单与唯一路线约束。
  - 落地：在 `android/adapter` 的 Binder bridge 内加入 V3 CoreContract 占位与 ServiceRegistry（register/get/list/unregister）。
  - 构建：`scripts/build_android_adapter.sh` 改为强制使用 `ensure_d8_from_r8.sh` 提供的 d8（支持 Java 21 classfile）。
  - 落地：新增 `HelloServiceDemo`（provider/consumer）用于验证“模块即接口：Provider 注册 Binder，Consumer 直连调用”的技术路径。
  - 落地：新增外置模块样例 `dsapi.demo.hello_provider` / `dsapi.demo.hello_consumer`，通过 `app_process + AndroidAdapterMain hello-*` 验证模块侧可提供/消费 ServiceRegistry 服务。

- 2026-03-13
  - 规范：Binder Core serviceName 固定为 `dsapi.core`，并在 KSU/Manager 两端限制 bridge service 必须为 `dsapi.*` 命名空间，避免遗留配置（例如 `assetatlas`）导致 UI/模块互相不一致。
  - 收敛：GPU demo 控制面统一走 bridge Binder（`demo aidl ...` 以及 `demo start/stop/status/cmd` 均转发到 Binder），删除旧的独立进程 demo 路线，避免“双模式”。
  - 性能：`TouchUiGpuPresenter` 渲染线程改为 `Choreographer` VSync 驱动（替代 sleep 限帧），并支持 `frame_rate=current/auto` 跟随系统刷新率。
  - 稳定：runtime release 目录默认固定为 `stable`（避免 /data/adb/dsapi/runtime/releases 目录累积且避免被旧 releaseId 覆盖），seed sync marker 引入 `module.prop versionCode` 作为变更检测来源，保证内容更新可被同步。
  - 走 zygote：Manager 核心 binder 注入改为 Zygisk 主路径：
    - `preAppSpecialize` 获取 `daemon_binder` 并保存。
    - `postAppSpecialize` 等待 Application 与 ClassLoader 就绪后调用 `InjectedCoreBinder.setFromZygisk(...)`。
    - Manager 不再依赖 `ServiceManager.find`，无需修改 SELinux policy。
