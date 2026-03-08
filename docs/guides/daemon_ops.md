# 守护进程运维

## 启动

```sh
./scripts/dsapi.sh daemon start
```

## 查看状态

```sh
./scripts/dsapi.sh daemon status
```

## 发送命令

```sh
./scripts/dsapi.sh daemon cmd READY
./scripts/dsapi.sh daemon cmd VERSION
./scripts/dsapi.sh daemon cmd DISPLAY_GET
./scripts/dsapi.sh daemon cmd TOUCH_MOVE 1 300 300
./scripts/dsapi.sh daemon cmd TOUCH_COUNT
./scripts/dsapi.sh daemon cmd TOUCH_CLEAR
./scripts/dsapi.sh daemon cmd RENDER_SUBMIT 12 2 3
./scripts/dsapi.sh daemon cmd RENDER_GET
./scripts/dsapi.sh frame pull artifacts/frame/latest.rgba
```

`dsapi.sh daemon cmd` / `dsapictl` 连接的是 **control socket**（默认 `artifacts/run/dsapi.sock`），
并自动使用二进制协议（`DSAP` header）。默认统一单 socket 模式下，
帧拉取同样通过该路径完成（`RENDER_FRAME_BIND_SHM + RENDER_FRAME_WAIT_SHM_PRESENT`）。

同步 Android 实际显示参数：

```sh
./scripts/dsapi.sh android sync-display
```

启动触摸桥接：

```sh
./scripts/dsapi.sh touch start
./scripts/dsapi.sh touch status
```

启动 root 全屏采集流（写入 daemon 当前帧）：

```sh
./scripts/dsapi.sh android sync-display
DSAPI_ANDROID_OUT_DIR=artifacts/android_user DSAPI_SCREEN_RUN_AS_ROOT=1 ./scripts/dsapi.sh screen start
./scripts/dsapi.sh screen status
./scripts/dsapi.sh frame pull artifacts/frame/screen_latest.rgba
```

Supervisor 启动（推荐统一进程编排）：

```sh
DSAPI_SUPERVISE_PRESENTER=1 DSAPI_SUPERVISE_INPUT=1 ./scripts/dsapi.sh daemon start
```

## 停止

```sh
./scripts/dsapi.sh touch stop
./scripts/dsapi.sh daemon stop
```

## 可配置环境变量

- `DSAPI_SOCKET_PATH`
- `DSAPI_CONTROL_SOCKET_PATH`
- `DSAPI_DATA_SOCKET_PATH`
- `DSAPI_UNIFIED_SOCKET`
- `DSAPI_PID_FILE`
- `DSAPI_LOG_FILE`
- `DSAPI_TOUCH_BRIDGE_PID_FILE`
- `DSAPI_TOUCH_BRIDGE_LOG_FILE`
- `DSAPI_TOUCH_DEVICE`
- `DSAPI_TOUCH_RUN_AS_ROOT`
- `DSAPI_TOUCH_AUTO_SYNC_DISPLAY`
- `DSAPI_TOUCH_SYNC_DISPLAY_EVERY_SEC`
- `DSAPI_TOUCH_MONITOR_INTERVAL_SEC`
- `DSAPI_RENDER_OUTPUT_DIR`
- `DSAPI_SUPERVISE_PRESENTER`（`1` 时由 daemon 托管 presenter）
- `DSAPI_SUPERVISE_INPUT`（`1` 时由 daemon 托管 input bridge）
- `DSAPI_SUPERVISE_PRESENTER_CMD`
- `DSAPI_SUPERVISE_INPUT_CMD`
- `DSAPI_SUPERVISE_RESTART_MS`
- `DSAPI_MAX_CONNECTIONS`
- `DSAPI_SCREEN_RUN_AS_ROOT`
- `DSAPI_SCREEN_TARGET_FPS`
- `DSAPI_SCREEN_SUBMIT_MODE`（`dmabuf` 或 `shm`）
- `DSAPI_HWBUFFER_BRIDGE_LIB`（`dmabuf` 模式 JNI 桥接库路径）
- `DSAPI_SCREEN_LOG_FILE`
- `DSAPI_SCREEN_PID_FILE`
- `DSAPI_FRAME_PULL_TIMEOUT_MS`
- `DSAPI_FRAME_WAIT_TIMEOUT_MS`
- `DSAPI_FRAME_PULL_RETRIES`
- `DSAPI_FRAME_PULL_RETRY_DELAY_MS`

可用于多实例隔离运行与路径定制。
