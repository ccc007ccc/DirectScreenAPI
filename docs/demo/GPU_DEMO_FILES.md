# GPU Demo 文件清单（隔离测试阶段）

更新时间：2026-03-13

## 目的

当前阶段只验证 GPU 窗口渲染可行性与性能，不与 DSAPI 主控制链路深度耦合。

## Demo 专属文件

- `AndroidAdapterMain.java`
  - 命令入口：`gpu-demo`
- `GpuVsyncDemo.java`
  - 薄门面，保持调用稳定
- `experimental/GpuVsyncDemoExperiment.java`
  - GPU demo 核心实现（EGL/GLES + VSync）

## 共享但被 demo 使用的文件

- `SurfaceLayerSession.java`
  - 提供 `SurfaceControl` 图层与 `Surface` 对象
- `AndroidDisplayAdapter.java`
  - 读取显示参数（分辨率/刷新率）
- `ReflectBridge.java`
  - 反射调用桥接

## 构建入口

- `scripts/build_android_adapter.sh`
  - 已纳入 `experimental/GpuVsyncDemoExperiment.java`

## 控制入口（测试态）

1. 直接命令入口：
   - `AndroidAdapterMain gpu-demo [width] [height] [z_layer] [layer_name] [run_seconds]`
   - `AndroidAdapterMain bridge-exec [service] <ctl_args...>`
2. AIDL 控制入口（bridge `exec_v2`）：
   - `demo status`
   - `demo start [width] [height] [z_layer] [layer_name] [run_seconds]`
   - `demo stop`
   - `demo cmd <原始命令>`
   - `demo set-pos <x> <y>`
   - `demo set-view <w> <h>`
   - `demo set-color <r> <g> <b> [a]`
   - `demo set-speed <speed>`
3. Shell 快捷入口：
   - `dsapi_service_ctl.sh demo aidl [service] <status|start|stop|cmd|set-pos|set-view|set-color|set-speed ...>`
   - `dsapi_service_ctl.sh demo <status|start|stop|cmd|set-pos|set-view|set-color|set-speed ...>`（等价于默认 service 的 `demo aidl`）

## KSU 模块示例入口

- 新增示例模块：`dsapi.demo.gpu_window`（源码目录：`ksu/module_examples/dsapi.demo.gpu_window`）
- 独立打包命令：`./scripts/build_ksu_module_zip.sh dsapi.demo.gpu_window`
- 打包产物：`artifacts/ksu_module_zips/dsapi.demo.gpu_window.zip`
- 适用目的：在 KSU Action 菜单直接验证 AIDL inproc demo 控制链路
- Action 清单：
  - `start.sh`：启动 GPU demo（AIDL）
  - `status.sh`：查询运行状态（AIDL）
  - `stop.sh`：停止 demo（AIDL）
  - `set_pos.sh`：调整位置（AIDL）
  - `set_view.sh`：调整尺寸（AIDL）
  - `set_color.sh`：调整颜色（AIDL）
  - `set_speed.sh`：调整动画速度（AIDL）

## AIDL 直连示例（Core 服务 dsapi.core）

- 查询 demo 状态：
  - `CLASSPATH=<adapter_dex.jar> app_process /system/bin org.directscreenapi.adapter.AndroidAdapterMain bridge-exec dsapi.core demo status`
- 启动 demo：
  - `... bridge-exec dsapi.core demo start 0 0 1000000 DirectScreenAPI-GPU 0`
- 动态控制：
  - `... bridge-exec dsapi.core demo set-pos 120 240`
  - `... bridge-exec dsapi.core demo set-view 900 600`
  - `... bridge-exec dsapi.core demo set-color 64 180 255 255`
  - `... bridge-exec dsapi.core demo set-speed 1.25`
- 停止 demo：
  - `... bridge-exec dsapi.core demo stop`

## 自动化冒烟

- 脚本：`scripts/demo_aidl_smoke.sh`
- 用法：
  - `./scripts/demo_aidl_smoke.sh`（默认 core service: `dsapi.core`）
  - `./scripts/demo_aidl_smoke.sh <service>`

## 隔离原则

1. Demo 不写入 daemon 状态机，不改 module 行为。
2. Demo 不新增 dsapid 协议，不依赖 `RENDER_FRAME_SUBMIT_SHM`。
3. Demo 只通过 `AndroidAdapterMain gpu-demo` 启动，作为性能基准。
4. 待验证通过后，再按文档分阶段并入主窗口系统。

## 当前运行形态

- 统一形态：demo 在 bridge 进程内线程运行（`mode=inproc`），不再额外拉起独立 app_process。
- `dsapi_service_ctl.sh demo ...` 默认也会转发到 bridge Binder（避免出现“双模式”与行为分歧）。
