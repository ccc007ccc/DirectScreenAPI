# DirectScreenAPI 修复清单（2026-03-01）

> 本清单是本轮修复的唯一事实源。每项必须含“动作 + 验收”。

## 状态说明

- `[ ]` 待处理
- `[-]` 处理中
- `[x]` 已完成

## P0：核心稳定性

- [x] `daemon` 连接计数泄漏修复  
  动作：`dispatch_accepted_client` 在 `set_*_timeout` 失败路径回收连接配额。  
  验收：异常路径不会导致 `active_connections` 长期偏高或误报 `ERR BUSY`。

- [x] `RENDER_FRAME_BIND_FD` 内存占用收敛  
  动作：绑定 fd 改为小容量起步 + 按需扩容，禁止每连接预分配 64MiB。  
  验收：空闲连接不产生大块共享内存占用；大帧仍可按需工作。

- [x] `WAIT_BOUND_PRESENT` 原子语义修复  
  动作：在 runtime 提供“快照+present”原子接口，数据面只走单次调用。  
  验收：返回帧信息与 present 元数据始终同帧一致。

## P1：Android 稳定性

- [x] 显示监听初始化失败回滚  
  动作：`initDisplayListener` 部分失败时执行 unregister/quit 回收。  
  验收：初始化失败不遗留 listener 线程或注册状态。

- [x] `SurfaceLayerSession.create` 失败回滚  
  动作：创建流程增加事务/对象回滚，异常时 remove/release。  
  验收：创建失败后无遗留 SurfaceControl/Surface 资源。

- [x] `RgbaFramePresenter` 关闭并发安全  
  动作：`shutdown` 增加幂等保护，避免 hook 与主循环并发重复释放。  
  验收：重复触发 `shutdown` 不抛错、不双重释放。

- [x] 反射方法选择与代理返回值稳健化  
  动作：显示探测方法按参数类型筛选；`DisplayListener` 代理处理
  `equals/hashCode/toString`。  
  验收：跨版本重载下仍能稳定命中，代理不会返回类型不匹配值。

- [x] 帧头一致性校验补齐  
  动作：`DaemonSession.frameWaitBoundPresent` 校验
  `byteLen == width * height * 4`。  
  验收：异常头在绘制前被拒绝并重连。

## P2：工程化与文档一致性

- [x] 构建目标目录语义统一  
  动作：`build_core.sh` 与脚本入口统一 `target` 目录变量。  
  验收：设置 `DSAPI_TARGET_DIR` 或 `CARGO_TARGET_DIR` 后构建与执行路径一致。

- [x] CI 可复现性增强  
  动作：CI 固定镜像版本与 Rust 版本；`cargo` 命令加 `--locked`。  
  验收：同提交在不同时间重复构建结果一致。

- [x] 发布流程补版本/ABI 联动校验  
  动作：发布文档增加 `Cargo.toml`、`DSAPI_ABI_VERSION`、C 头宏的一致性核查。  
  验收：发布步骤可防止 tag/二进制/ABI 文档漂移。

- [x] 文档与默认值纠偏  
  动作：修正文档中 `DSAPI_PRESENTER_POLL_MS`、touch 故障码等偏差。  
  验收：文档示例与实际脚本输出一致。

## 进度日志

- 2026-03-01：建立本轮清单，重置状态为待处理。
- 2026-03-01：修复 `dispatch_accepted_client` 连接配额泄漏，异常路径回收计数。
- 2026-03-01：`RENDER_FRAME_BIND_FD` 改为按当前帧大小与最小容量（512KiB）绑定，移除 64MiB 固定预分配。
- 2026-03-01：新增 `wait_for_frame_after_and_present` 原子接口，`WAIT_BOUND_PRESENT` 改为单次一致性路径。
- 2026-03-01：`RgbaFramePresenter` 显示监听初始化增加失败回滚（unregister/quit）。
- 2026-03-01：`RgbaFramePresenter.shutdown` 增加幂等保护，避免并发重复释放。
- 2026-03-01：`DisplayListener` 代理补齐 `equals/hashCode/toString` 返回语义。
- 2026-03-01：`AndroidDisplayAdapter` 显示探测按参数类型选择重载，减少跨版本反射不确定性。
- 2026-03-01：`SurfaceLayerSession.create` 增加失败回滚，避免 SurfaceControl/Surface 资源遗留。
- 2026-03-01：`DaemonSession.frameWaitBoundPresent` 新增 `byteLen == width*height*4` 校验。
- 2026-03-01：统一 `build_core.sh`/`build_c_example.sh` 的 target 目录语义并兼容 `DSAPI_TARGET_DIR`。
- 2026-03-01：CI 固定 `ubuntu-24.04` 与 Rust `1.93.1`，并去除 C example job 的重复构建步骤。
- 2026-03-01：`check/fix` 增加 `--locked`；发布文档补齐版本/ABI 联动核查；修正文档默认值与故障码。
- 2026-03-01：回归通过：`./scripts/check.sh`、`cargo test --workspace`、`./scripts/build_android_adapter.sh`。
