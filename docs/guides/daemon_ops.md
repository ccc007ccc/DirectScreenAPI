# 守护进程运维

## 启动

```sh
./scripts/daemon_start.sh
```

## 查看状态

```sh
./scripts/daemon_status.sh
```

## 发送命令

```sh
./scripts/daemon_cmd.sh PING
./scripts/daemon_cmd.sh DISPLAY_GET
./scripts/daemon_cmd.sh ROUTE_ADD_RECT 9 block 100 100 280 280
./scripts/daemon_cmd.sh ROUTE_POINT 120 120
./scripts/daemon_cmd.sh TOUCH_DOWN 1 120 120
./scripts/daemon_cmd.sh TOUCH_MOVE 1 300 300
./scripts/daemon_cmd.sh TOUCH_UP 1 300 300
./scripts/daemon_cmd.sh TOUCH_COUNT
./scripts/daemon_cmd.sh RENDER_SUBMIT 12 2 3
./scripts/daemon_cmd.sh RENDER_GET
./scripts/daemon_cmd.sh RENDER_FRAME_SUBMIT_RGBA 1 1 /wAA/w==
./scripts/daemon_cmd.sh RENDER_FRAME_GET
./scripts/daemon_cmd.sh RENDER_FRAME_WAIT 0 16
./scripts/daemon_cmd.sh RENDER_FRAME_READ_BASE64 0 1024
./scripts/daemon_cmd.sh RENDER_PRESENT
./scripts/daemon_cmd.sh RENDER_PRESENT_GET
./scripts/daemon_cmd.sh RENDER_DUMP_PPM
./scripts/daemon_frame_pull.sh artifacts/frame/latest.rgba
./scripts/daemon_cmd.sh RENDER_FRAME_CLEAR
```

`RENDER_FRAME_SUBMIT_RGBA_RAW` 需要先读写握手再发送二进制 body，不适合
`daemon_cmd.sh` 这种逐行文本工具；请使用长连接客户端（如 FrostUI runtime）。

`RENDER_FRAME_GET_RAW` 会返回二进制 RGBA body，同样不适合 `daemon_cmd.sh`。

`RENDER_FRAME_GET_FD` 依赖 Unix Socket ancillary fd（`SCM_RIGHTS`），
`daemon_cmd.sh` / `dsapictl` 不支持该能力，需由支持 fd 透传的客户端调用（如 Android presenter）。

同步 Android 实际显示参数：

```sh
./scripts/daemon_sync_display.sh
```

启动触摸桥接：

```sh
./scripts/daemon_touch_bridge_start.sh
./scripts/daemon_touch_bridge_status.sh
```

Supervisor 启动（推荐统一进程编排）：

```sh
DSAPI_SUPERVISE_PRESENTER=1 DSAPI_SUPERVISE_INPUT=1 ./scripts/daemon_start.sh
```

## 停止

```sh
./scripts/daemon_touch_bridge_stop.sh
./scripts/daemon_stop.sh
```

## 可配置环境变量

- `DSAPI_SOCKET_PATH`
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

可用于多实例隔离运行与路径定制。
