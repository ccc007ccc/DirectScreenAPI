# DSAPI Zygote 注入链 + 寄生 Manager + AIDL 直连（执行计划）

## 1. 目标约束

- 对齐 LSP 主路径：`zygote 注入链 -> daemon 服务 -> manager 直连 AIDL`。
- 不走回滚/降级路径，不新增轮询方案。
- shell 仅保留最薄引导层（post-fs-data/service/action CLI），核心控制与状态全部收敛到 daemon/bridge。

## 2. 当前基线（2026-03-08）

- Core 契约已统一为 `org.directscreenapi.core.ICoreService`。
- Manager 与 Bridge 已通过 `exec_v2` 进行 AIDL 控制面通信。
- Bridge 侧已去除 `module install/runtime activate` 的 shell 回落，改为 Java 直连文件事务 + daemon RPC。
- 仍待替换的 shell 依赖：
  - manager 安装探测仍使用 `pm/cmd package`。

## 2.1 本轮推进（2026-03-08 夜）

- `ui start/stop` 主链已切换为 `ParasiticManagerHost`（`app_process --nice-name=DSAPIManagerHost`）。
- 新增 `zygote-agent` Binder 代理服务：
  - descriptor: `org.directscreenapi.daemon.IZygoteAgent`
  - transact: `get_info` / `get_daemon_binder` / `should_inject`
- `BridgeControlServer` 已接入 `zygote start|stop|restart|status`。
- KSU 控制面已接入 `zygote` 子命令，并在 `status` 输出 `ksu_dsapi_zygote`。
- 新增 zygote scope 持久化：`state/zygote_scope.db`（包/用户 allow|deny）。
- 新增 `zygisk loader` native 骨架与打包路径（`zygisk/<abi>.so`）。

## 2.2 增量推进（2026-03-08 深夜）

- `zygisk loader` 升级为真实 `zygisk::ModuleBase`：
  - 引入 `zygisk.hpp`（API v5）。
  - `preAppSpecialize` 中通过 Binder 调 `zygote-agent`：
    - `get_info`（接口版本/feature 校验）
    - `should_inject`
    - `get_daemon_binder`
  - 注入裁决失败走 fail-closed，不自动降级旧链路。
- `BridgeControlServer` 优化：
  - `isPackageInstalled` 改为 `PackageManager` 查询。
  - `isUiRunning` 改为 `ActivityManager` 任务栈探测。
  - 去掉 `dumpsys activity` / `pm path` 作为 UI/安装探测主路径。
- Bridge/zygote-agent 增加服务覆盖自愈：
  - 使用 `ServiceManager.registerForNotifications` 监听同名服务注册事件。
  - 当发现 descriptor 不匹配时，自动重注册本地 Binder 服务（无轮询）。
- KSU `ui` 命令脚本去除 activity 前台探测依赖：
  - 删除 `dumpsys activity` 路径。
  - `ui` 状态统一以 `parasitic_host` 主路径为准。
  - `ui start` 去除自动安装 manager 行为，改为失败即报错（无隐式降级）。

## 2.3 底层拉起链路修正（2026-03-08 夜）

- `ParasiticManagerHost` 改为 **底层直连 `ActivityTaskManager` Binder** 拉起 manager：
  - 不再依赖 `Context.startActivity` 主路径（system context 下存在权限拒绝风险）。
  - 不再使用 `cmd activity` / `am start` 命令回退路径。
- 反射调用 `startActivity/startActivityAsUser`，按参数类型动态构造调用参数：
  - `IApplicationThread/IBinder/Bundle/ProfilerInfo` 走 `null`。
  - `callingPackage` 固定使用 `org.directscreenapi.manager`。
  - `requestCode=-1`，`flags=0`，`userId=0`（asUser 方法尾位 int）。
- 实测口径：
  - `manager_host_info=atm_start ... result=0` 即视为启动成功。
  - `manager_host.ready state=ready` 与 `ui status=running` 作为端到端验收标准。

## 3. 目标架构（阶段化）

### 阶段 F：zygote 注入链最小闭环

1. 建立注入入口（zygisk/riru 二选一，优先 zygisk）。
2. fork 前后注入判定：过滤 isolated/child zygote/无 data dir 进程。
3. 注入侧与 daemon 建立 AIDL 握手：
   - 获取模块列表/作用域策略/开关状态。
   - 注入 manager binder 常量通道。
4. system_server 重启后的 bridge 重挂策略。

### 阶段 G：寄生 manager 启动链

1. manager 改为宿主进程模式（`app_process + manager dex`），避免显式桌面 app 启动依赖。
2. manager UI 仅消费 AIDL 数据，不直控 shell。
3. 进程名与生命周期行为向 LSP 对齐（可见宿主进程、可热重连 daemon）。

### 阶段 H：AIDL 直连收尾

1. UI 相关控制从 `am/pm` 迁移到系统 Binder API。
2. Bridge 事务扩展：补齐 manager 自更新/状态查询必要接口。
3. shell `dsapi_service_ctl.sh` 仅作为 CLI 适配层，逻辑不再主导状态机。

## 4. 任务清单（持续更新）

- [x] F1. 明确 zygisk 注入最小代码骨架（native + java loader）。
- [x] F2. 定义注入侧 AIDL 代理接口（当前完成 `IZygoteAgent` 第一版：daemon binder 透传）。
- [x] F3. 注入进程判定规则与黑名单机制落地（isolated/child_zygote/no_data_dir 初版）。
- [x] F3.1 loader 接入 `zygote-agent` Binder 事务裁决（`should_inject` + `get_daemon_binder`）。
- [ ] F4. bridge 在 system_server 重启后的重注册与死亡监听。
- [x] F4. bridge/zygote-agent 通过服务注册通知实现同名服务覆盖自愈（事件驱动）。
- [x] G1. 设计寄生 manager 引导类与 dex 装载路径（`ParasiticManagerHost`）。
- [x] G2. 替换 `ui start` 的 `am start` 主路径。
- [ ] G3. manager UI 全面改为仓库 + AIDL push/event 驱动，避免轮询。
- [ ] H1. 替换 manager 安装动作中的 `pm install` 路径（当前仅剩安装动作，探测已迁移系统 API）。
- [ ] H2. 将剩余 shell fallback 清零或收敛到 CLI 层。
- [ ] H3. 端到端稳定性验证（冷启动、热重载、system_server 重启、模块安装/卸载）。

## 5. 验收口径

- 冷启动后无需手工操作即可恢复 daemon + bridge。
- manager 与模块控制全链路 AIDL 可用，shell 仅作为外部命令入口。
- 模块安装/启停/作用域修改均无需重启设备。
- 关键链路 24h 稳定运行，无轮询高频开销与无回滚分支。
