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
```

同步 Android 实际显示参数：

```sh
./scripts/daemon_sync_display.sh
```

启动触摸桥接：

```sh
./scripts/daemon_touch_bridge_start.sh
./scripts/daemon_touch_bridge_status.sh
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

可用于多实例隔离运行与路径定制。
