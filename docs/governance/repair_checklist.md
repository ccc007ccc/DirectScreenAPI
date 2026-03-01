# DirectScreenAPI 高性能改造清单（2026-03-01）

> 本清单为本轮改造唯一事实源，按“无回退、删旧路径”执行。

## 状态说明

- `[ ]` 待处理
- `[-]` 处理中
- `[x]` 已完成

## 任务 1：共享内存与零拷贝机制改造（已硬切换）

- [x] 数据面仅保留 SHM 取帧协议
  动作：`dsapid/data_dispatch.rs` 仅接受 `RENDER_FRAME_BIND_SHM` 与 `RENDER_FRAME_WAIT_SHM_PRESENT`。
  验收：旧 `RENDER_FRAME_GET_FD / BIND_FD / GET_BOUND / WAIT_BOUND_PRESENT / GET_RAW` 分发路径已删除。

- [x] Rust 共享内存生命周期模型落地
  动作：`frame_fd.rs` 重构为 `SharedFrameMeta + MappedSharedRegion + BoundFrameFd`。
  验收：`memfd_create + mmap(MAP_SHARED)` 持续映射，`Drop` 自动 `munmap/close`。

- [x] 数据通道 socket 仅传输 fd + 元数据
  动作：绑定时通过 `SCM_RIGHTS` 发 memfd；等待时返回 `seq/w/h/byte_len/checksum/offset`。
  验收：像素不再经 socket body 传输。

- [x] 配套脚本切换至 SHM 路径
  动作：`scripts/dsapi_frame.sh` 改为 `BIND_SHM + WAIT_SHM_PRESENT` 并通过 `recvmsg` 收 fd。
  验收：`frame pull` 不再依赖 `RENDER_FRAME_GET_RAW`。

## 任务 2：协议二进制化/零分配解析（已硬切换）

- [x] 控制面协议全量二进制化
  动作：`engine/protocol.rs` 保留二进制协议实现，删除 `execute_command` 文本命令执行路径。
  验收：控制面仅接受 `DSAP` 二进制帧，响应为固定 `status + [u64;8]`。

- [x] 控制面命令集明确收敛
  动作：实现并暴露 `PING / VERSION / SHUTDOWN / DISPLAY_GET / DISPLAY_SET / TOUCH_CLEAR / TOUCH_COUNT / TOUCH_MOVE / RENDER_SUBMIT / RENDER_GET`。
  验收：`dsapictl` 与 `dsapistream` 全部通过二进制 opcode 通信。

- [x] 移除旧文本解析热路径分配
  动作：删除文本命令解析；`ctl_wire::parse_command_line` 改为迭代器切片解析，无 token `Vec` 分配。
  验收：控制命令解析不再走 `split_whitespace().collect()`。

- [x] 控制面读取缓冲区复用
  动作：`control_dispatch.rs` 改为单次分配 frame buffer，循环复用读取 header/payload。
  验收：每条命令不再重复分配拼接 frame 缓冲。

## 任务 3：异步 I/O（epoll 事件驱动）迁移

- [x] `mio` 主循环保留并稳定运行
  动作：`dsapid.rs` 使用 `Poll + SourceFd + Token` 监听 control/data listener。
  验收：主线程事件驱动，多路复用正常。

- [x] 固定线程池分发保留
  动作：`DispatchPool` 处理连接任务，拒绝超载连接并返回 `ERR BUSY`。
  验收：不再 thread-per-connection。

## 任务 4：Android 硬件渲染桥接设计

- [x] AHardwareBuffer JNI/C 桥接结构与导出实现
  动作：`android/adapter/native/dsapi_hwbuffer_bridge.h/.c` 导出 `fd + desc`。
  验收：Java `HardwareBuffer` 可转 native handle fd。

- [x] C ABI 与 Rust FFI 接入 AHB 描述符
  动作：`directscreen_api.h` + `ffi/mod.rs` 增加 `dsapi_hwbuffer_desc_t` 与提交接口。
  验收：Rust 核心可接收平台硬件缓冲区描述。

- [x] Android presenter 数据面无回退
  动作：`DaemonSession.java` 仅使用 `RENDER_FRAME_BIND_SHM + RENDER_FRAME_WAIT_SHM_PRESENT`。
  验收：已删除旧 `BIND_FD/WAIT_BOUND_PRESENT` 回退逻辑。

## 验证结果

- [x] `cargo fmt --all`
- [x] `cargo check --workspace`
- [x] `cargo test --workspace`（42 passed）
- [x] `./scripts/build_android_adapter.sh`

## 关键变更文件

- `core/rust/src/engine/protocol.rs`
- `core/rust/src/engine/mod.rs`
- `core/rust/src/util/ctl_wire.rs`
- `core/rust/src/bin/dsapid/control_dispatch.rs`
- `core/rust/src/bin/dsapid/data_dispatch.rs`
- `core/rust/src/bin/dsapid/frame_fd.rs`
- `core/rust/src/bin/dsapid/parse.rs`
- `core/rust/src/bin/dsapid/config.rs`
- `core/rust/src/bin/dsapid.rs`
- `core/rust/src/bin/dsapictl.rs`
- `core/rust/src/bin/dsapistream.rs`
- `android/adapter/src/main/java/org/directscreenapi/adapter/DaemonSession.java`
- `android/adapter/native/dsapi_hwbuffer_bridge.h`
- `android/adapter/native/dsapi_hwbuffer_bridge.c`
- `bridge/c/include/directscreen_api.h`
- `core/rust/src/ffi/mod.rs`
- `scripts/dsapi_frame.sh`
- `docs/architecture/daemon_model.md`
- `docs/guides/android_adapter.md`
