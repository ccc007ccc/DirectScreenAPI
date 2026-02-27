# 触摸桥接指南

本文描述如何把 Android 设备真实触摸事件接入 DirectScreenAPI daemon。

## 目标

- 采集 `/dev/input/event*` 的多点触摸事件
- 映射到当前显示坐标系
- 通过持久 socket 客户端写入 `TOUCH_*` 协议

## 组件

- `scripts/daemon_touch_bridge_start.sh`
- `scripts/daemon_touch_bridge_status.sh`
- `scripts/daemon_touch_bridge_stop.sh`
- `scripts/daemon_touch_bridge_run.sh`
- `scripts/touch_event_to_dsapi.awk`
- `target/release/dsapistream`

## 快速使用

```sh
./scripts/daemon_start.sh
./scripts/daemon_sync_display.sh
./scripts/daemon_touch_bridge_start.sh
./scripts/daemon_touch_bridge_status.sh
```

停止：

```sh
./scripts/daemon_touch_bridge_stop.sh
./scripts/daemon_stop.sh
```

## 默认行为

- 默认使用 root 采集：`DSAPI_TOUCH_RUN_AS_ROOT=1`
- 自动探测触摸设备（`ABS_MT_POSITION_X/Y` + `ABS_MT_TRACKING_ID`）
- 运行时按周期检查显示状态，分辨率/旋转变化后自动重建映射管线
- 启动时执行一次 `TOUCH_CLEAR`，退出时再次清理，避免粘连触点

## 关键环境变量

- `DSAPI_TOUCH_DEVICE`：手动指定触摸设备，如 `/dev/input/event6`
- `DSAPI_TOUCH_RUN_AS_ROOT`：是否使用 `su -c`（默认 `1`）
- `DSAPI_TOUCH_AUTO_SYNC_DISPLAY`：是否自动调用 `daemon_sync_display.sh`（默认 `1`）
- `DSAPI_TOUCH_SYNC_DISPLAY_EVERY_SEC`：自动同步显示周期，默认 `1`
- `DSAPI_TOUCH_MONITOR_INTERVAL_SEC`：显示变化监视周期，默认 `1`
- `DSAPI_TOUCH_BRIDGE_PID_FILE`：桥接 PID 文件路径
- `DSAPI_TOUCH_BRIDGE_LOG_FILE`：桥接日志路径
- `DSAPI_GETEVENT_BIN`：`getevent` 可执行路径，默认 `/system/bin/getevent`

## 性能说明

- 触摸命令通过 `dsapistream` 持续读取标准输入，不再每条事件启动新进程
- `dsapid` 支持并发连接，触摸桥接与控制命令互不阻塞

## 故障排查

- `touch_bridge_error=touch_device_not_found`
  - 无法自动识别触摸设备，手动设置 `DSAPI_TOUCH_DEVICE`
- `touch_bridge_error=invalid_display_response`
  - daemon 未启动或 display 状态异常
- `touch_bridge_status=failed_start`
  - 查看日志文件定位原因
- `touch_bridge_status=display_changed`
  - 说明已检测到显示参数变化并自动重建映射
