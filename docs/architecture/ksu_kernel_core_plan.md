# KSU 内核化重构方案（精简高性能）

## 目标

- DSAPI 核心以 KSU 模块常驻，不依赖前台 APK。
- 模块低频更新：业务能力通过 runtime/capability 热更新。
- 闲时低功耗：无业务时走阻塞等待，不做高频轮询。
- 管理可视化：提供由 `app_process` 拉起的隐藏管理 UI（非桌面图标应用）。

## 架构分层

1. KSU 薄壳层
- `post-fs-data.sh`：初始化目录、同步 runtime seed。
- `service.sh`：按 `enabled` 状态决定是否拉起 `core.daemon`。
- `bin/dsapi_service_ctl.sh`：统一控制面（daemon/runtime/capability/ui）。

2. Runtime 层
- 位置：`/data/adb/dsapi/runtime/releases/<id>`
- 激活：`/data/adb/dsapi/runtime/current -> .../<id>`
- 内容：`bin/dsapid`、`bin/dsapictl`、`android/directscreen-adapter-dex.jar`、`capabilities/`

3. Capability 层（热插拔）
- 核心只内置 `core.daemon`
- 扩展能力（窗口注入、IME、滤镜链）通过外部 capability 安装
- 接口规范：`docs/guides/ksu_capability_module_spec.md`

4. UI 层（隐藏管理界面）
- 入口：`AndroidAdapterMain cap-ui`
- 启动：`dsapi_service_ctl.sh ui start`
- 特性：实时展示 capability 状态，可启用/停用/删除/恢复/查看详情

## 当前状态

- [x] 统一 socket 协议主路径（control 承载 frame opcode + fd）
- [x] KSU 基础目录与 daemon 生命周期管理
- [x] runtime seed + active release 激活机制
- [x] capability 控制协议（list/start/stop/remove/enable/detail/add）
- [x] cap-ui（app_process 可视化管理页）
- [ ] 窗口/IME 注入 capability 正式实现（当前应走外部插件）

## 一次成功校验清单

安装前：
- `./scripts/dsapi.sh ksu preflight` 返回 `status=ok`

安装后：
- `.../dsapi_service_ctl.sh status` 返回 `ksu_dsapi_status=running`
- `.../dsapi_service_ctl.sh cmd READY` 返回 `OK`
- `.../dsapi_service_ctl.sh capability list` 至少包含 `core.daemon`
- `.../dsapi_service_ctl.sh ui start` 可拉起隐藏管理界面

## 风险与后续

- ROM SELinux 策略差异可能影响 overlay window 添加和注入能力。
- `app_process` UI 仅用于管理，不承担高帧渲染路径。
- 后续注入 capability 建议优先走 Zygisk/LSPosed，保持与 KSU 核心解耦。
