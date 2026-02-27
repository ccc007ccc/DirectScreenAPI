# DirectScreenAPI

DirectScreenAPI 是一个面向 Android 特权场景的“基础能力层”项目，目标是先打磨稳定内核，再扩展上层能力。

本仓库当前定位：`基石重构阶段（v0.1.0）`

## 设计目标

- 稳定优先：先保证状态模型、协议和生命周期可控
- 可恢复：出现异常时可确定性回收
- 可组合：上层可扩展窗口系统、AI 控制框架、容器桥接
- 多语言接入：以 C ABI 作为长期稳定边界

## 当前已实现

- Rust 核心状态引擎
- C ABI 基线接口
- 矩形区域路由（后添加优先）
- 显示状态严格校验
- 最小 CLI（`dsapi`）
- 守护进程模型（`dsapid`）与控制端（`dsapictl`）
- Android 超薄适配层显示探测（Java 反射 + `app_process`）
- Android 显示参数同步管线（`android_display_probe.sh` -> `DISPLAY_SET`）
- 触控会话状态机（多点触控、按指针锁定路由决策）

## 当前未实现

- OpenGL ES 后端
- Vulkan 后端
- Android SurfaceControl 真正渲染适配
- 屏幕采集与输入注入闭环

## 目录结构

- `core/rust`：核心领域模型、运行时、FFI
- `bridge/c`：C 头文件与 C 示例
- `android`：Android 适配层（当前已接入显示探测）
- `docs`：架构、API、运维与治理文档
- `scripts`：构建、检查、守护进程脚本

## 快速开始

```sh
cd core/rust
cargo build --release
cargo test
cargo run --bin dsapi -- version
```

守护进程模式：

```sh
./scripts/daemon_start.sh
./scripts/daemon_status.sh
./scripts/daemon_cmd.sh PING
./scripts/daemon_cmd.sh DISPLAY_GET
./scripts/daemon_stop.sh
```

Android 显示探测与同步：

```sh
./scripts/build_android_adapter.sh
./scripts/android_display_probe.sh display-kv
./scripts/daemon_start.sh
./scripts/daemon_sync_display.sh
./scripts/daemon_cmd.sh DISPLAY_GET
```

默认会使用 `su -c` 执行显示探测；如果只想在当前用户运行，可设置 `DSAPI_RUN_AS_ROOT=0`。

触控路由会话（稳定链路）：

```sh
./scripts/daemon_start.sh
./scripts/daemon_cmd.sh ROUTE_CLEAR
./scripts/daemon_cmd.sh ROUTE_ADD_RECT 9 block 0 0 300 300
./scripts/daemon_cmd.sh TOUCH_DOWN 1 100 100
./scripts/daemon_cmd.sh TOUCH_MOVE 1 600 600
./scripts/daemon_cmd.sh TOUCH_UP 1 600 600
./scripts/daemon_cmd.sh TOUCH_COUNT
./scripts/daemon_stop.sh
```

真实触摸采集桥接（getevent -> daemon）：

```sh
./scripts/daemon_start.sh
./scripts/daemon_sync_display.sh
./scripts/daemon_touch_bridge_start.sh
./scripts/daemon_touch_bridge_status.sh
./scripts/daemon_touch_bridge_stop.sh
./scripts/daemon_stop.sh
```

C 示例：

```sh
./scripts/build_c_example.sh
./artifacts/bin/dsapi_example
```

## 版本与发布

- 变更记录：`CHANGELOG.md`
- 发布流程：`docs/governance/release.md`
- 路线图：`docs/governance/roadmap.md`

## 许可证

Apache-2.0
