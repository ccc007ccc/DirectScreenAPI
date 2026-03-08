# Android 适配层接线指南

本文描述 Android 侧“超薄适配层”的可用链路：获取真实显示参数、同步到 Rust 守护进程状态机，将 RGBA 帧真正呈现到屏幕，并在 root 场景下采集系统全屏帧。

## 目标

- 将 Android 平台能力限制在适配层，不下沉核心策略
- 通过 `app_process` 调用 Java 探测 / 呈现逻辑
- 将探测结果标准化后写入 `dsapid`（`DISPLAY_SET`）
- 将 `RENDER_FRAME_*` 帧数据接入 `SurfaceControl`，实现真实上屏

## 前置条件

- 已安装 `javac`、`jar`、`d8`
- 设备可执行 `app_process`
- 若需 root 场景执行，系统可用 `su`
- 建议准备 `android.jar`（便于使用类型安全 Android API）：
  `./scripts/fetch_android_jar.sh 35`

## 关键脚本

- `scripts/build_android_adapter.sh`：编译 Java 适配层并产出 dex jar
- `scripts/fetch_android_jar.sh`：从官方仓库下载指定 API 的 `android.jar`
- `scripts/dsapi.sh android probe`：执行显示探测
- `scripts/dsapi.sh android sync-display`：将探测结果同步到 daemon
- `scripts/dsapi.sh presenter run`：前台运行上屏 presenter（事件驱动拉帧）
- `scripts/dsapi.sh presenter start`：后台启动 presenter
- `scripts/dsapi.sh presenter stop`：停止 presenter
- `scripts/dsapi.sh presenter status`：查询 presenter 状态
- `scripts/dsapi.sh screen run`：前台运行 root 全屏采集流
- `scripts/dsapi.sh screen start`：后台启动 root 全屏采集流
- `scripts/dsapi.sh screen stop`：停止全屏采集流
- `scripts/dsapi.sh screen status`：查询全屏采集流状态
- `AndroidAdapterMain cap-ui`：拉起 capability 管理界面（供 KSU 控制层调用）

## 快速使用

1. 构建 Android 适配产物

```sh
./scripts/build_android_adapter.sh
```

2. 探测显示参数

```sh
./scripts/dsapi.sh android probe display-kv
```

3. 启动守护进程并同步显示状态

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh android sync-display
./scripts/dsapi.sh daemon cmd DISPLAY_GET
```

4. 启动上屏 presenter 并提交测试帧

```sh
./scripts/dsapi.sh presenter start
./scripts/dsapi.sh presenter status
```

5. 启动 root 全屏采集流（全屏帧回灌到 daemon）

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh android sync-display
DSAPI_ANDROID_OUT_DIR=artifacts/android_user DSAPI_SCREEN_RUN_AS_ROOT=1 ./scripts/dsapi.sh screen start
./scripts/dsapi.sh frame pull artifacts/frame/screen_latest.rgba
```

## 输出格式约定

`display-kv` 输出如下键值，供同步脚本解析：

- `width`
- `height`
- `refresh_hz`
- `density_dpi`
- `rotation`

## 环境变量

- `DSAPI_ANDROID_OUT_DIR`：Android 构建产物目录，默认 `artifacts/android`
- `DSAPI_ANDROID_BUILD_MODE`：`display_probe`（默认）、`presenter` 或 `all`
- `DSAPI_CONTROL_SOCKET_PATH`：控制面 socket，默认 `artifacts/run/dsapi.sock`
- `DSAPI_DATA_SOCKET_PATH`：数据会话 socket，默认继承控制 socket（统一单 socket 模式）
- `DSAPI_UNIFIED_SOCKET`：是否启用统一单 socket（默认 `1`）
- `DSAPI_RUN_AS_ROOT`：是否使用 `su -c` 执行 probe，默认 `1`
- `DSAPI_APP_PROCESS_BIN`：指定 `app_process` 可执行路径，默认 `app_process`
- `DSAPI_PRESENTER_WAIT_TIMEOUT_MS`：presenter 帧等待超时（`RENDER_FRAME_WAIT_SHM_PRESENT`），`0` 表示无限阻塞等待新帧（默认 `0`）
- `DSAPI_PRESENTER_LAYER_Z`：presenter 图层 Z 值，默认 `1000000`
- `DSAPI_PRESENTER_LAYER_NAME`：presenter 图层名，默认 `DirectScreenAPI`
- `DSAPI_SCREEN_RUN_AS_ROOT`：全屏采集是否 root 运行，默认继承 `DSAPI_RUN_AS_ROOT`
- `DSAPI_SCREEN_TARGET_FPS`：全屏采集提交帧率上限（实际取决于系统与负载），默认 `60`
- `DSAPI_SCREEN_SUBMIT_MODE`：全屏采集提交模式，默认 `dmabuf`（可选 `shm`）
- `DSAPI_HWBUFFER_BRIDGE_LIB`：`HardwareBuffer` JNI 桥接库路径，默认 `<out_dir>/libdsapi_hwbuffer_bridge.so`
- `DSAPI_ANDROID_BUILD_NATIVE`：是否构建 JNI 桥接库，默认 `1`
- `DSAPI_ANDROID_JAR`：`javac` 使用的 `android.jar` 路径，默认
  `third_party/android-sdk/platforms/android-35/android.jar`
- `DSAPI_SCREEN_LOG_FILE` / `DSAPI_SCREEN_PID_FILE`：全屏采集日志与 pid 文件路径
- `DSAPI_SCREEN_PRECHECK`：是否在非 root 预检 `app_process`（root 场景默认 `0`）

## 故障排查

- `android_adapter_error=javac_not_found`：缺少 Java 编译器
- `android_adapter_error=d8_not_found`：缺少 dex 构建工具
- `android_probe_error=app_process_not_found`：系统未找到 `app_process`
- `display_sync_error=invalid_probe_output`：probe 输出格式不符合约定
- `presenter_error=daemon_control_socket_missing`：daemon 套接字不可用
- `presenter_status=failed`：presenter 初始化失败，查看 `artifacts/run/dsapi_presenter.log`
- `screen_stream_error=daemon_control_socket_missing`：采集流连接 daemon 失败
- `screen_stream_error=hwbuffer_bridge_missing`：`dmabuf` 模式下未找到 JNI 桥接库
- `screen_stream_error=on_image_available_failed ... Broken pipe`：数据面会话断开，需重启采集流或 daemon

## 当前边界

- 已实现：显示参数探测与同步
- 已实现：`SurfaceControl` RGBA 上屏 presenter
- 已实现：presenter 通过 `RENDER_FRAME_BIND_SHM + RENDER_FRAME_WAIT_SHM_PRESENT` 事件驱动共享内存取帧（携带 offset 元数据）
- 已实现：root 全屏采集流（`VirtualDisplay + ImageReader`）支持 `RENDER_FRAME_SUBMIT_DMABUF + SCM_RIGHTS` 上行（默认）与 `RENDER_FRAME_SUBMIT_SHM`
- 已实现：`cap-ui` 可视化 capability 管理界面（启用/停用/删除/恢复/详情）
- 已定义：触控会话接口契约（Down/Move/Up/Cancel）
- 未实现：通用应用级回调桥接（目前以 demo 与控制面为主）

该边界用于保证重构阶段的稳定性，后续能力扩展在此基础上逐步推进。
