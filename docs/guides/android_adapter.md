# Android 适配层接线指南

本文描述当前 Android 侧“超薄适配层”的可用链路：获取真实显示参数，并同步到 Rust 守护进程状态机。

## 目标

- 将 Android 平台能力限制在适配层，不下沉核心策略
- 通过 `app_process` 调用 Java 探测逻辑
- 将探测结果标准化后写入 `dsapid`（`DISPLAY_SET`）

## 前置条件

- 已安装 `javac`、`jar`、`d8`
- 设备可执行 `app_process`
- 若需 root 场景执行，系统可用 `su`

## 关键脚本

- `scripts/build_android_adapter.sh`：编译 Java 适配层并产出 dex jar
- `scripts/android_display_probe.sh`：执行显示探测
- `scripts/daemon_sync_display.sh`：将探测结果同步到 daemon

## 快速使用

1. 构建 Android 适配产物

```sh
./scripts/build_android_adapter.sh
```

2. 探测显示参数

```sh
./scripts/android_display_probe.sh display-kv
```

3. 启动守护进程并同步显示状态

```sh
./scripts/daemon_start.sh
./scripts/daemon_sync_display.sh
./scripts/daemon_cmd.sh DISPLAY_GET
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
- `DSAPI_RUN_AS_ROOT`：是否使用 `su -c` 执行 probe，默认 `1`
- `DSAPI_APP_PROCESS_BIN`：指定 `app_process` 可执行路径，默认 `app_process`

## 故障排查

- `android_adapter_error=javac_not_found`：缺少 Java 编译器
- `android_adapter_error=d8_not_found`：缺少 dex 构建工具
- `android_probe_error=app_process_not_found`：系统未找到 `app_process`
- `display_sync_error=invalid_probe_output`：probe 输出格式不符合约定

## 当前边界

- 已实现：显示参数探测与同步
- 未实现：触摸输入适配、渲染提交适配、事件回调桥接

该边界用于保证重构阶段的稳定性，后续能力扩展在此基础上逐步推进。
