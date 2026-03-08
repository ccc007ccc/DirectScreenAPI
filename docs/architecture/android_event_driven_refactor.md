# Android 事件驱动重构总纲

## 目标

以 Android 系统思维重构 DirectScreenAPI 运行时，核心目标：

- 去轮询：主链路不再用固定周期 `sleep` / 超时轮询拉帧。
- 低功耗：空闲时线程阻塞等待事件，尽量不唤醒 CPU。
- 高性能：帧到达后直接唤醒呈现，减少无效上下文切换与空转。
- 精简内核：不保留旧兼容回退逻辑，保留当前版本必要能力。

## Android 思维原则

- 生命周期驱动：组件在“有事件时工作、无事件时休眠”。
- 单一事实源：`dsapid` 维护状态，presenter/screen-stream 仅消费或提交。
- 显式唤醒：通过 socket 数据、condvar、内核阻塞 IO 触发。
- 快速退出：关闭连接/发 shutdown 即可打断阻塞等待，避免僵尸进程。

## 模块清单

1. Rust RuntimeEngine（帧/显示等待原语）
2. Rust dsapid（监听、分发、子进程监管）
3. Android presenter（上屏消费）
4. Android screen-stream（系统采集回灌）
5. Shell 脚本与运行参数（默认策略与运维入口）
6. 文档与验证（规范和性能验收）

## 现状轮询点与问题

1. `core/rust/src/bin/dsapid.rs`
- 主循环 `poll(..., 250ms)` + 异常 `sleep(20ms)`，空闲仍定时唤醒。
- dispatch worker `recv_timeout(200ms)`，无任务时持续超时唤醒。
- supervise worker `try_wait + sleep(250ms)`，对子进程状态轮询。

2. `android/adapter/.../RgbaFramePresenter.java`
- `frameWaitBoundPresent(lastSeq, pollMs)` 默认 `pollMs=2`，高频超时轮询。
- display sync fallback 定时触发，产生额外无效工作。

3. `android/adapter/.../ScreenCaptureStreamer.java`
- keepalive 线程 `sleep(1000)` + `frameWaitBoundPresent(...,1)`，纯轮询保活。

4. `android/adapter/.../DaemonSession.java`
- 读失败瞬态重试含 backoff sleep，长时间链路噪声时会产生周期唤醒。

## 重构设计（目标态）

1. 等待语义统一
- 约定 `timeout_ms == 0` 表示“无限阻塞等待事件”（不是立即返回）。
- 应用于 `DISPLAY_WAIT`、`RENDER_FRAME_WAIT_SHM_PRESENT`。

2. dsapid 主循环/分发
- listener 事件循环改为 `poll(..., None)`（无限阻塞）。
- dispatch worker 改 `recv()` 阻塞等待任务。
- 退出通过 `shutdown + drop sender + join` 触发线程收敛。

3. 子进程监管
- 移除 `try_wait + sleep` 轮询；改为 `child.wait()` 阻塞等待退出。
- daemon 退出时先 `SIGTERM`，短等待后必要时 `SIGKILL`，保证可回收。

4. presenter
- 默认 `wait_timeout_ms=0`，完全依赖新帧事件唤醒。
- 仅在 display listener 事件触发时同步显示参数，不做固定周期 fallback。
- 退出时关闭 daemon session，利用 socket 关闭打断阻塞读。

5. screen-stream
- 移除 keepalive 线程；由 ImageReader 回调驱动提交。
- 无帧时不唤醒、不发保活命令。

## 分模块重构清单

### M1 RuntimeEngine（等待原语）
- [x] `wait_for_display_after` 支持 `timeout_ms=0` 无限阻塞。
- [x] `wait_for_frame_after` 支持 `timeout_ms=0` 无限阻塞。
- [x] `wait_for_frame_after_and_present` 支持 `timeout_ms=0` 无限阻塞。
- [x] 补充单测：`timeout_ms=0` 的唤醒行为。

### M2 dsapid（核心守护）
- [x] 主循环改为无超时阻塞 `poll`。
- [x] dispatch worker 改阻塞 `recv`。
- [x] supervise worker 改阻塞 `wait`，去掉 250ms 轮询。
- [x] 删除异常路径定时 `sleep`（保留必要错误处理）。

### M3 presenter（上屏）
- [x] 参数名从 `poll_ms` 语义改为 `wait_timeout_ms`。
- [x] 默认 `wait_timeout_ms=0`（事件驱动）。
- [x] 去掉 display fallback 定时逻辑，仅 listener 触发同步。

### M4 screen-stream（采集）
- [x] 删除 keepalive 线程及相关命令轮询。
- [x] 保证 submit 失败可快速触发关闭/重连策略。

### M5 脚本与文档
- [x] `DSAPI_PRESENTER_POLL_MS` 迁移为 `DSAPI_PRESENTER_WAIT_TIMEOUT_MS`。
- [x] 更新 `android_adapter.md`、`daemon_model.md` 与命令帮助。
- [x] 输出“事件驱动默认配置”的运维建议。

### M6 READY 握手 + 启动状态机
- [x] 控制面新增 `READY_GET` opcode。
- [x] `dsapid` 引入生命周期状态：`starting -> ready -> stopping`。
- [x] 控制面 `READY` 响应返回 `state_code/boot_id/uptime_ms/pid`。
- [x] 启动脚本探测由 `PING` 切换为 `READY`，并在 presenter 启动前强制等待 ready。
- [x] `SHUTDOWN` 触发 `mio::Waker` 唤醒阻塞 poll，确保立即退出。

### M7 DMA-BUF / AHardwareBuffer（当前阶段）
- [x] 已有 `AHardwareBuffer -> dma-buf fd` C bridge（`android/adapter/native`）并补齐 JNI 导出封装。
- [-] 守护进程新增 `RENDER_FRAME_SUBMIT_DMABUF` 与 `SCM_RIGHTS` fd 接收提交流程。
- [-] `screen-stream` 端接入 AHB fd 导出并通过统一 socket 发送（默认 `dmabuf` 模式）。
- [x] 完成 `dsapid` 端 `recvmsg + SCM_RIGHTS` 接收、mmap/stride 展开提交核心。
- [ ] 在 `RgbaFramePresenter` 打通端到端验证并输出性能数据。

## 验收指标

- 空闲唤醒：presenter/daemon 在无事件时不应出现固定周期日志/调用。
- CPU：空闲态长期维持接近 0%（设备测量允许轻微抖动）。
- 帧率：120Hz 设备在无滤镜链压力下可逼近刷新上限。
- 退出：`daemon stop`、`presenter stop` 无残留窗口层与孤儿进程。

## 实施进度

- [x] 建立本总纲文档并接入文档索引。
- [x] M1 RuntimeEngine
- [x] M2 dsapid
- [x] M3 presenter
- [x] M4 screen-stream
- [x] M5 脚本与文档收尾 + check 验证
- [x] M6 READY 握手 + 启动状态机
- [-] M7 DMA-BUF / AHardwareBuffer

## 本轮结果

- 已通过 `./scripts/dsapi.sh check`（本地全量检查通过）。
- 运行时主路径（daemon/presenter/screen-stream）已去除固定周期轮询，改为事件驱动阻塞等待。

## 剩余说明（非核心轮询）

- `dsapi_touch_demo` 与 `client/render` 仍保留帧率节流 `sleep`（仅演示/客户端节拍控制，不属于内核守护路径）。
- `dsapid` 保留故障重启退避 `sleep` 与停机信号宽限窗口（仅异常恢复与退出路径，不属于空闲轮询）。

## 文件级优化台账（持续更新）

1. `core/rust/src/engine/protocol.rs`
- 已完成：`READY_GET` 二进制命令接入。
- 下一步：为 DMA-BUF 数据面增加统一统计字段（提交路径、复制字节数）。

2. `core/rust/src/bin/dsapid.rs`
- 已完成：生命周期状态机、`Waker` 唤醒、阻塞 poll。
- 下一步：将统一 socket 的读路径升级到 `recvmsg` 流式解析，支持 fd 上行。

3. `core/rust/src/bin/dsapid/control_dispatch.rs`
- 已完成：READY 响应覆写为真实生命周期快照。
- 下一步：补充 READY 状态切换日志与状态跃迁单测。

4. `core/rust/src/bin/dsapid/data_dispatch.rs`
- 已完成：新增 `RENDER_FRAME_SUBMIT_DMABUF` 命令与 fd 提交分支。
- 下一步：将数据面升级为统一二进制头，进一步减少解析和状态分支。

5. `core/rust/src/bin/dsapid/frame_fd.rs`
- 已完成：SHM 零拷贝绑定与等待。
- 已完成：DMA-BUF 提交处理器（`recvmsg/SCM_RIGHTS + mmap + stride/offset`）。
- 下一步：在宽行距场景引入 SIMD 行压缩，减少 CPU copy 时间。

6. `android/adapter/src/main/java/.../DaemonSession.java`
- 已完成：`READY/READY_GET` 控制命令支持。
- 已完成：新增 fd 提交接口（`setFileDescriptorsForSend + RENDER_FRAME_SUBMIT_DMABUF`）。
- 下一步：接入 `HardwareBuffer` 导出 fd 直接上行。

7. `android/adapter/src/main/java/.../ScreenCaptureStreamer.java`
- 已完成：事件回调驱动提交、移除 keepalive 轮询。
- 已完成：限帧判断前移；`Plane -> SHM` 直写路径（按行写入）替代“先整帧打包再提交”。
- 已完成：引入 `Image.getHardwareBuffer()` + JNI 导出，默认 `dmabuf` 提交流。
- 下一步：补充端到端帧率/功耗基准，确认 `dmabuf` 模式稳定性。

8. `android/adapter/src/main/java/.../RgbaFramePresenter.java`
- 已完成：阻塞等帧、显示监听驱动同步。
- 下一步：渲染路径从 `Bitmap+Canvas` 迁移到更低开销的 buffer 直提交流程。
