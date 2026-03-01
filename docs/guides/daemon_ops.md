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
./scripts/dsapi.sh daemon cmd PING
./scripts/dsapi.sh daemon cmd DISPLAY_GET
./scripts/dsapi.sh daemon cmd ROUTE_ADD_RECT 9 block 100 100 280 280
./scripts/dsapi.sh daemon cmd ROUTE_POINT 120 120
./scripts/dsapi.sh daemon cmd TOUCH_DOWN 1 120 120
./scripts/dsapi.sh daemon cmd TOUCH_MOVE 1 300 300
./scripts/dsapi.sh daemon cmd TOUCH_UP 1 300 300
./scripts/dsapi.sh daemon cmd TOUCH_COUNT
./scripts/dsapi.sh daemon cmd RENDER_SUBMIT 12 2 3
./scripts/dsapi.sh daemon cmd RENDER_GET
./scripts/dsapi.sh daemon cmd RENDER_FRAME_GET
./scripts/dsapi.sh daemon cmd RENDER_FRAME_WAIT 0 16
./scripts/dsapi.sh daemon cmd RENDER_PRESENT
./scripts/dsapi.sh daemon cmd RENDER_PRESENT_GET
./scripts/dsapi.sh daemon cmd RENDER_DUMP_PPM
./scripts/dsapi.sh frame pull artifacts/frame/latest.rgba
./scripts/dsapi.sh daemon cmd RENDER_FRAME_CLEAR
```

`dsapi.sh daemon cmd` / `dsapictl` 连接的是 **control socket**（默认 `artifacts/run/dsapi.sock`）。

`RENDER_FRAME_SUBMIT_RGBA_RAW` 需要先读写握手再发送二进制 body，不适合
`dsapi.sh daemon cmd` 这种逐行文本工具；请使用长连接客户端（如 FrostUI runtime）。

`RENDER_FRAME_GET_RAW` 会返回二进制 RGBA body，同样不适合 `dsapi.sh daemon cmd`。

`RENDER_FRAME_GET_FD` 依赖 Unix Socket ancillary fd（`SCM_RIGHTS`），
`dsapi.sh daemon cmd` / `dsapictl` 不支持该能力，需由支持 fd 透传的客户端调用（如 Android presenter）。

二进制命令与触控流走 **data socket**（默认 `artifacts/run/dsapi.data.sock`）。

同步 Android 实际显示参数：

```sh
./scripts/dsapi.sh android sync-display
```

启动触摸桥接：

```sh
./scripts/dsapi.sh touch start
./scripts/dsapi.sh touch status
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

可用于多实例隔离运行与路径定制。
