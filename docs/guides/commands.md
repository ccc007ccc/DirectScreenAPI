# 命令入口参考（`dsapi.sh`）

当前脚本入口统一为 `./scripts/dsapi.sh`。

## 核心用法

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh daemon status
./scripts/dsapi.sh daemon cmd PING
./scripts/dsapi.sh daemon stop
```

## 子命令

- `daemon start|stop|status|cmd <COMMAND ...>`
- `presenter start|stop|status|run`
- `touch start|stop|status|run`
- `android probe [display-kv|display-line]`
- `android sync-display`
- `frame pull <out_rgba_path>`
- `build core|android|c-example`
- `check`
- `fix`

## 常见流程

守护进程 + 控制命令：

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh daemon cmd DISPLAY_GET
./scripts/dsapi.sh daemon stop
```

Android 显示同步：

```sh
./scripts/dsapi.sh build android
./scripts/dsapi.sh android probe display-kv
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh android sync-display
```

Presenter：

```sh
./scripts/dsapi.sh presenter start
./scripts/dsapi.sh presenter status
./scripts/dsapi.sh presenter stop
```

Touch bridge：

```sh
./scripts/dsapi.sh touch start
./scripts/dsapi.sh touch status
./scripts/dsapi.sh touch stop
```

取帧：

```sh
./scripts/dsapi.sh frame pull artifacts/frame/latest.rgba
```

门禁校验与修复：

```sh
./scripts/dsapi.sh check
./scripts/dsapi.sh fix
```

## 旧脚本迁移

- `daemon_start.sh` -> `dsapi.sh daemon start`
- `daemon_stop.sh` -> `dsapi.sh daemon stop`
- `daemon_status.sh` -> `dsapi.sh daemon status`
- `daemon_cmd.sh` -> `dsapi.sh daemon cmd ...`
- `daemon_presenter_start.sh` -> `dsapi.sh presenter start`
- `daemon_presenter_stop.sh` -> `dsapi.sh presenter stop`
- `daemon_presenter_status.sh` -> `dsapi.sh presenter status`
- `daemon_touch_bridge_start.sh` -> `dsapi.sh touch start`
- `daemon_touch_bridge_stop.sh` -> `dsapi.sh touch stop`
- `daemon_touch_bridge_status.sh` -> `dsapi.sh touch status`
- `android_display_probe.sh` -> `dsapi.sh android probe ...`
- `daemon_sync_display.sh` -> `dsapi.sh android sync-display`
- `daemon_frame_pull.sh` -> `dsapi.sh frame pull ...`
- `daemon_presenter_run.sh` -> `dsapi.sh presenter run`
- `daemon_touch_bridge_run.sh` -> `dsapi.sh touch run`

## 注意事项

- `dsapi.sh daemon cmd` 走控制面 socket，不适合二进制命令（如 `RENDER_FRAME_GET_RAW`）。
- 需要 fd 透传的命令（如 `RENDER_FRAME_GET_FD`）应由支持 `SCM_RIGHTS` 的客户端调用。
