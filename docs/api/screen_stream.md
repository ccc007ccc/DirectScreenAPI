# Root 全屏采集流 API

本文描述 `dsapi.sh screen ...` 提供的 root 全屏采集能力。

## 能力定义

- 采集来源：Android `VirtualDisplay + ImageReader`（root 运行）
- 数据去向：通过统一 socket 的 `RENDER_FRAME_SUBMIT_SHM/DMABUF` 回灌
- 可见效果：daemon 当前帧变为系统全屏帧，可由 `frame pull` 拉取

## 命令

```sh
./scripts/dsapi.sh screen start
./scripts/dsapi.sh screen stop
./scripts/dsapi.sh screen status
./scripts/dsapi.sh screen run
./scripts/dsapi.sh screen bench [samples]
```

## 最小流程

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh android sync-display
DSAPI_ANDROID_OUT_DIR=artifacts/android_user DSAPI_SCREEN_RUN_AS_ROOT=1 ./scripts/dsapi.sh screen start
./scripts/dsapi.sh frame pull artifacts/frame/screen_latest.rgba
```

## 环境变量

- `DSAPI_SCREEN_RUN_AS_ROOT`：是否 root 运行（默认继承 `DSAPI_RUN_AS_ROOT`）
- `DSAPI_SCREEN_TARGET_FPS`：提交帧率上限（默认 `60`）
- `DSAPI_SCREEN_PRECHECK`：`app_process` 预检（root 场景默认 `0`）
- `DSAPI_SCREEN_LOG_FILE`：日志路径（默认 `artifacts/run/dsapi_screen.log`）
- `DSAPI_SCREEN_PID_FILE`：pid 路径（默认 `artifacts/run/dsapi_screen.pid`）
- `DSAPI_APP_PROCESS_BIN`：指定 `app_process` 可执行文件
- `DSAPI_ANDROID_OUT_DIR`：Android 适配产物目录
- `DSAPI_FRAME_PULL_ENGINE`：`auto|rust|python`，控制 `frame pull` 后端选择（默认 `auto`）
- `DSAPI_FRAME_PULL_BIN`：Rust 后端二进制路径（默认 `target/release/dsapiframepull`）

## 错误语义

- `screen_stream_error=daemon_control_socket_missing`：控制 socket 不可用
- `screen_stream_error=on_image_available_failed ...`：图像回调或提交链路失败
- `screen_stream_status=failed`：采集流初始化失败，需查看日志定位

## 性能说明

- 该链路面向“可持续串流”的低拷贝路径（共享内存提交）。
- 与 `screencap` 的绝对速度会受分辨率、刷新率、SoC 负载影响，建议使用 `./scripts/dsapi.sh screen bench` 在目标设备实测。
- 高负载下若 `frame pull` 出现超时，可降低 `DSAPI_SCREEN_TARGET_FPS`，并调整：
  - `DSAPI_FRAME_PULL_TIMEOUT_MS`（socket 超时，默认 `30000` ms）
  - `DSAPI_FRAME_WAIT_TIMEOUT_MS`（单次等帧超时，默认 `250` ms）
  - `DSAPI_FRAME_PULL_RETRIES`（重试次数，默认 `4`）
  - `DSAPI_FRAME_PULL_RETRY_DELAY_MS`（重试间隔，默认 `80` ms）
