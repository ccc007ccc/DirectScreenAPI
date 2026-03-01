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
- `screen start|stop|status|run|bench [samples]`
- `touch start|stop|status|run`
- `android probe [display-kv|display-line]`
- `android sync-display`
- `frame pull <out_rgba_path>`
- `build core|android|c-example|framepull`
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

Screen stream（root，全屏采集 -> daemon 当前帧）：

```sh
./scripts/dsapi.sh android sync-display
DSAPI_ANDROID_OUT_DIR=artifacts/android_user DSAPI_SCREEN_RUN_AS_ROOT=1 ./scripts/dsapi.sh screen start
./scripts/dsapi.sh screen status
./scripts/dsapi.sh frame pull artifacts/frame/screen_latest.rgba
./scripts/dsapi.sh screen bench 10
./scripts/dsapi.sh screen stop
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

可选快速取帧 helper（非必须、按需构建）：

```sh
./scripts/dsapi.sh build framepull
DSAPI_FRAME_PULL_ENGINE=rust ./scripts/dsapi.sh frame pull artifacts/frame/latest.rgba
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
- `daemon_screen_stream_start.sh` -> `dsapi.sh screen start`
- `daemon_screen_stream_stop.sh` -> `dsapi.sh screen stop`

## 注意事项

- `dsapi.sh daemon cmd` 通过 `dsapictl` 走控制面二进制协议，不再支持旧文本控制协议。
- `frame pull` 已切换为 `RENDER_FRAME_BIND_SHM + RENDER_FRAME_WAIT_SHM_PRESENT` 的零拷贝路径。
- `frame pull` 默认 `DSAPI_FRAME_PULL_ENGINE=auto`：若存在 `target/release/dsapiframepull`
  则优先使用轻量 Rust helper；否则自动回退 Python 实现。
- `frame pull` 默认带重试（`DSAPI_FRAME_PULL_RETRIES=4`、`DSAPI_FRAME_PULL_RETRY_DELAY_MS=80`），
  可通过 `DSAPI_FRAME_WAIT_TIMEOUT_MS` 调整单次等帧超时。
