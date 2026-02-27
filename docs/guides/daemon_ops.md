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
```

## 停止

```sh
./scripts/daemon_stop.sh
```

## 可配置环境变量

- `DSAPI_SOCKET_PATH`
- `DSAPI_PID_FILE`
- `DSAPI_LOG_FILE`

可用于多实例隔离运行与路径定制。
