# DirectScreenAPI GPU 窗口系统改造记录

更新时间：2026-03-13

## 目标

1. 删除整帧 RGBA 像素流作为主数据面（不再以 `RENDER_FRAME_SUBMIT_SHM` 为主路径）。
2. 控制面统一走 AIDL/Binder（Manager/模块动作 -> Daemon）。
3. Presenter 内部改为 GPU 原生渲染（EGL/GLES 或 Vulkan）+ `SurfaceControl`。
4. 渲染调度改为 VSync 驱动，事务仅在几何变化时提交，支持脏区更新。

## 关键结论（AIDL 是否“性能最好”）

- AIDL/Binder 适合控制面（可靠、权限模型清晰、生命周期一致、低抖动）。
- AIDL/Binder 不适合高带宽像素数据面（大 payload 成本高）。
- 对窗口系统最佳实践：
  - 控制面：AIDL（命令、状态、事件订阅）。
  - 渲染面：Presenter 本地 GPU 渲染，不传整帧像素。
  - 如后续需要共享大资源（纹理/缓冲），优先 FD + 零拷贝句柄，不走大 Parcel。

## 当前已完成

- [x] 新增 GPU 基准入口：`AndroidAdapterMain gpu-demo [width] [height] [z_layer] [layer_name] [run_seconds]`
- [x] 新增 `GpuVsyncDemo`：
  - `SurfaceControl` 图层承载
  - EGL + GLES2 渲染
  - `Choreographer` VSync 驱动
  - 实时日志 `gpu_demo_perf fps=... frame_ms=...`
- [x] `touchdemo` Presenter 重构为纯 GPU 渲染：
  - `TouchUiGpuPresenter` 改为 `EGL14 + GLES20` 渲染线程
  - `Choreographer` VSync 驱动（替代 sleep 限帧）
  - 去除 `Canvas lock/unlock` 路径
  - 保留 `SurfaceControl` 几何事务（仅在位置/尺寸变化时提交）
- [x] `touchdemo` 数据面重构为状态流：
  - `dsapi_touch_demo` 强制 `state_pipe_required`
  - 删除 SHM 提交/CPU 软绘制代码路径（不再输出 `RENDER_FRAME_SUBMIT_SHM`）
  - 模块 helper 仅启动 `touch-ui-loop + --state-pipe`
- [x] `SurfaceLayerSession` 暴露 `surfaceObject()` 供 EGL WindowSurface 使用。
- [x] demo 实现迁移到 `experimental` 目录，与 DSAPI 主链路做最小耦合。
- [x] 新增 demo 文件台账：`docs/demo/GPU_DEMO_FILES.md`。
- [x] AIDL demo 控制域接入：`demo start/stop/status/cmd`（bridge `exec_v2`）。
- [x] 增加 demo 运行时命令：`SET_POS/SET_VIEW/SET_COLOR/SET_SPEED/STOP/PING/STATUS`。
- [x] 新增 shell -> AIDL 直连桥接：`dsapi_service_ctl.sh demo aidl ...`（调用 `AndroidAdapterMain bridge-exec`）。
- [x] 新增自动化冒烟脚本：`scripts/demo_aidl_smoke.sh`。
- [x] 新增 KSU 模块级测试入口：`ksu/module_examples/dsapi.demo.gpu_window`，支持在 action 菜单直接验证 `demo aidl` 控制链路。

## 下一阶段任务清单

### Phase 1：Demo 性能验证（当前）

- [ ] 在真实设备测试 `gpu-demo`（60/90/120Hz）并记录：
  - 平均 FPS
  - 帧抖动（frame_ms 波动）
  - CPU 占用
- [ ] 对比 `touchuidemo`（RGBA 流）与 `gpu-demo` 的拖动手感与功耗。
  - 注：touch UI 已迁移为“状态流 + GPU 渲染”，此处对比关注点改为“复杂 UI（含文字/按钮）vs 纯 shader demo”。

### Phase 2：命令流替换像素流

- [ ] 设计 `WindowCommand` 协议（创建/销毁窗口、位置、尺寸、可见性、动画、输入焦点）。
- [ ] 在 Daemon AIDL 层新增命令接口（oneway + 批处理）。
- [ ] Presenter 增加命令队列与状态机（窗口树 + 脏区收敛）。

### Phase 3：窗口系统实装

- [ ] 每窗口独立渲染节点（GPU），统一合成到 `SurfaceControl`。
- [ ] 几何变化才提交 `Transaction`，渲染变化只走 GPU draw。
- [ ] 输入链路打通（焦点、IME、手势、命中测试）并去轮询。

### Phase 4：收敛与清理

- [x] 删除 `touchdemo` RGBA SHM 主路径与相关废弃代码。
- [ ] 继续清理仓库中与 `touchdemo` 无关的历史像素流残留（其他组件）。
- [ ] 清理模块脚本中的旧参数与兼容分支。
- [ ] 更新 Manager UI 到新模型（窗口状态、性能指标、命令调试）。

## 里程碑验收标准

- [ ] `touchui` 功能迁移到 GPU 管线后，交互手感明显优于旧链路。
- [ ] 无轮询核心链路，VSync 驱动，稳定运行 30 分钟无崩溃。
- [ ] 模块启动后无需额外安装动作，KSU action 可直接启动 UI/窗口系统。
