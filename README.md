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
- Android 显示参数同步管线（`dsapi.sh android probe` -> `dsapi.sh android sync-display` -> `DISPLAY_SET`）
- Android SurfaceControl 实际上屏 presenter（`RENDER_FRAME_*` -> 屏幕）
- Android root 全屏采集流（`VirtualDisplay + ImageReader` -> `RENDER_FRAME_SUBMIT_SHM`）
- 触控会话状态机（多点触控、按指针锁定路由决策）
- 触摸桥接自动重映射（分辨率/旋转变化自动更新坐标映射）
- 渲染统计上报协议（`RENDER_SUBMIT` / `RENDER_GET`）
- 共享内存取帧协议（`RENDER_FRAME_BIND_SHM` / `RENDER_FRAME_WAIT_SHM_PRESENT`）
- 呈现协议（`RENDER_PRESENT` / `RENDER_PRESENT_GET`，内存主链路）
- 调试导出协议（`RENDER_DUMP_PPM`，仅调试用）
- 二进制触控流协议（`STREAM_TOUCH_V1`）
- 控制/数据双 socket 分离（控制面二进制命令与数据面 SHM/触控流解耦）
- C ABI 渲染接口（帧提交/分块读取/呈现状态）

## 当前未实现

- OpenGL ES 后端
- Vulkan 后端
- 输入注入闭环

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

统一脚本入口：`./scripts/dsapi.sh`  
命令参考：`docs/guides/commands.md`

守护进程模式：

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh daemon status
./scripts/dsapi.sh daemon cmd PING
./scripts/dsapi.sh daemon cmd DISPLAY_GET
./scripts/dsapi.sh daemon stop
```

Supervisor 模式（由 `dsapid` 托管 presenter + input）：

```sh
DSAPI_SUPERVISE_PRESENTER=1 DSAPI_SUPERVISE_INPUT=1 ./scripts/dsapi.sh daemon start
./scripts/dsapi.sh daemon status
./scripts/dsapi.sh daemon stop
```

Android 显示探测与同步：

```sh
./scripts/dsapi.sh build android
./scripts/dsapi.sh android probe display-kv
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh android sync-display
./scripts/dsapi.sh daemon cmd DISPLAY_GET
```

Android 实际上屏 presenter（默认前台层）：

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh android sync-display
./scripts/dsapi.sh presenter start
./scripts/dsapi.sh presenter status
./scripts/dsapi.sh presenter stop
./scripts/dsapi.sh daemon stop
```

Android root 全屏采集流（将系统全屏帧写入 daemon 当前帧）：

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh android sync-display
DSAPI_ANDROID_OUT_DIR=artifacts/android_user DSAPI_SCREEN_RUN_AS_ROOT=1 ./scripts/dsapi.sh screen start
./scripts/dsapi.sh screen status
./scripts/dsapi.sh frame pull artifacts/frame/screen_latest.rgba
./scripts/dsapi.sh screen stop
./scripts/dsapi.sh daemon stop
```

默认会使用 `su -c` 执行显示探测；如果只想在当前用户运行，可设置 `DSAPI_RUN_AS_ROOT=0`。

触控路由会话（稳定链路）：

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh daemon cmd TOUCH_MOVE 1 600 600
./scripts/dsapi.sh daemon cmd TOUCH_COUNT
./scripts/dsapi.sh daemon cmd RENDER_SUBMIT 18 4 3
./scripts/dsapi.sh daemon cmd RENDER_GET
./scripts/dsapi.sh frame pull artifacts/frame/latest.rgba
./scripts/dsapi.sh daemon stop
```

上面 `frame pull` 已切换为 SHM 路径（`RENDER_FRAME_BIND_SHM + RENDER_FRAME_WAIT_SHM_PRESENT`），
默认读取 daemon 当前帧；若开启 `screen start`，当前帧即为系统全屏采集帧。
拉帧支持重试与等帧超时调节：`DSAPI_FRAME_PULL_RETRIES`、`DSAPI_FRAME_PULL_RETRY_DELAY_MS`、
`DSAPI_FRAME_WAIT_TIMEOUT_MS`、`DSAPI_FRAME_PULL_TIMEOUT_MS`。
`frame pull` 默认会自动探测快速后端：若存在 `target/release/dsapiframepull` 则优先使用，
否则自动回退 Python 实现。可通过 `DSAPI_FRAME_PULL_ENGINE=rust|python|auto` 强制指定。

按需构建快速后端：

```sh
./scripts/dsapi.sh build framepull
DSAPI_FRAME_PULL_ENGINE=rust ./scripts/dsapi.sh frame pull artifacts/frame/latest.rgba
```

控制面已支持统一二进制协议（含 `RENDER_FRAME_*`）；默认单 socket 模式下，
同一路径可承载控制命令、SHM 帧交换与触控流。

真实触摸采集桥接（native input -> daemon binary stream）：

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh android sync-display
./scripts/dsapi.sh touch start
./scripts/dsapi.sh touch status
./scripts/dsapi.sh touch stop
./scripts/dsapi.sh daemon stop
```

触摸阻断/透传 + 渲染窗口 Demo（可拖动、缩放、关闭，窗口显示 FPS）：

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh android sync-display
./scripts/dsapi.sh presenter start
cargo run --release --manifest-path core/rust/Cargo.toml --bin dsapi_touch_demo --
```

可选参数：

- `--device /dev/input/eventX`：手动指定触摸设备（默认自动探测）
- `--no-touch-router`：仅渲染（不接管触摸）
- `--no-soft-keyboard`：关闭 demo 内置软键盘面板
- `--fps 90`：设置渲染帧率上限
- `--window-x/--window-y/--window-w/--window-h`：设置窗口初始位置与尺寸
- `--control-socket/--data-socket`：自定义 socket 路径

C 示例：

```sh
./scripts/dsapi.sh build c-example
./artifacts/bin/dsapi_example
```

## 版本与发布

- 变更记录：`CHANGELOG.md`
- 发布流程：`docs/governance/release.md`
- 路线图：`docs/governance/roadmap.md`

## 许可证

MIT
