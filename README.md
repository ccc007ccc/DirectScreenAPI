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
- 触控会话状态机（多点触控、按指针锁定路由决策）
- 触摸桥接自动重映射（分辨率/旋转变化自动更新坐标映射）
- 渲染统计上报协议（`RENDER_SUBMIT` / `RENDER_GET`）
- 像素帧上报协议（`RENDER_FRAME_SUBMIT_RGBA_RAW` / `RENDER_FRAME_GET`）
- 像素帧读取协议（`RENDER_FRAME_GET_RAW`）
- 共享内存取帧协议（`RENDER_FRAME_GET_FD`，fd 透传）
- 事件驱动帧等待协议（`RENDER_FRAME_WAIT` / `RENDER_FRAME_WAIT_BOUND_PRESENT`）
- 呈现协议（`RENDER_PRESENT` / `RENDER_PRESENT_GET`，内存主链路）
- 调试导出协议（`RENDER_DUMP_PPM`，仅调试用）
- 二进制触控流协议（`STREAM_TOUCH_V1`）
- 控制/数据双 socket 分离（控制面文本命令与数据面二进制流解耦）
- C ABI 渲染接口（帧提交/分块读取/呈现状态）

## 当前未实现

- OpenGL ES 后端
- Vulkan 后端
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

默认会使用 `su -c` 执行显示探测；如果只想在当前用户运行，可设置 `DSAPI_RUN_AS_ROOT=0`。

触控路由会话（稳定链路）：

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh daemon cmd ROUTE_CLEAR
./scripts/dsapi.sh daemon cmd ROUTE_ADD_RECT 9 block 0 0 300 300
./scripts/dsapi.sh daemon cmd TOUCH_DOWN 1 100 100
./scripts/dsapi.sh daemon cmd TOUCH_MOVE 1 600 600
./scripts/dsapi.sh daemon cmd TOUCH_UP 1 600 600
./scripts/dsapi.sh daemon cmd TOUCH_COUNT
./scripts/dsapi.sh daemon cmd RENDER_SUBMIT 18 4 3
./scripts/dsapi.sh daemon cmd RENDER_GET
./scripts/dsapi.sh daemon cmd RENDER_FRAME_GET
./scripts/dsapi.sh daemon cmd RENDER_FRAME_WAIT 0 16
./scripts/dsapi.sh daemon cmd RENDER_PRESENT
./scripts/dsapi.sh daemon cmd RENDER_PRESENT_GET
./scripts/dsapi.sh daemon cmd RENDER_DUMP_PPM
./scripts/dsapi.sh frame pull artifacts/frame/latest.rgba
./scripts/dsapi.sh daemon cmd RENDER_FRAME_CLEAR
./scripts/dsapi.sh daemon stop
```

`RENDER_PRESENT` 不走磁盘；只有 `RENDER_DUMP_PPM` 会导出文件。  
导出目录默认 `artifacts/render`，可通过 `DSAPI_RENDER_OUTPUT_DIR` 覆盖。

`RENDER_FRAME_SUBMIT_RGBA_RAW` 为高性能通道（**数据 socket**）：先发送
`RENDER_FRAME_SUBMIT_RGBA_RAW <w> <h> <byte_len>`，收到 `OK READY`
后再发送 `byte_len` 原始 RGBA 字节流，最后读取结果行。`dsapi.sh daemon cmd`
是控制面逐行文本工具，不适用于该二进制命令。

`RENDER_FRAME_GET_RAW` 也属于二进制响应命令（**数据 socket**）：先返回
`OK <frame_seq> <w> <h> <byte_len>`，随后直接返回 `byte_len` 原始 RGBA 字节流。

`RENDER_FRAME_GET_FD`（**数据 socket**）通过 Unix Socket ancillary fd 传输共享内存句柄。
当前 presenter 的推荐路径是先调用 `RENDER_FRAME_BIND_FD` 绑定持久 fd，
后续循环调用 `RENDER_FRAME_WAIT_BOUND_PRESENT` 等待并拉取呈现后帧元数据，
复用同一映射缓冲。
`dsapi.sh daemon cmd` 与 `dsapictl` 不支持上述 fd 透传命令。

真实触摸采集桥接（native input -> daemon binary stream）：

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh android sync-display
./scripts/dsapi.sh touch start
./scripts/dsapi.sh touch status
./scripts/dsapi.sh touch stop
./scripts/dsapi.sh daemon stop
```

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

Apache-2.0
