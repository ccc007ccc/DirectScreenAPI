# DirectScreenAPI KSU 模块

本目录用于产出 KernelSU 可安装模块 zip，目标是让 DSAPI 核心以 root 常驻服务方式运行，
同时支持 runtime/capability 热更新，尽量减少 KSU 模块重装频率。

## 目录说明

- `module_template/`：KSU 模块模板文件
- `capability_examples/`：外部 capability 示例（默认不内置到核心）
- `../scripts/build_ksu_module.sh`：打包脚本（根目录入口）
- `../scripts/dsapi_ksu_preflight.sh`：设备预检脚本

## 快速使用

```sh
./scripts/dsapi.sh ksu preflight
./scripts/dsapi.sh ksu pack
```

产物默认在：`artifacts/ksu_module/directscreenapi-ksu.zip`

说明：

- `pack` 默认会自动尝试嵌入 `manager.apk`（优先本机构建，失败自动回退到 `~/ubuntu-chroot/rootfs` 容器构建）。
- 如需禁用 manager APK 嵌入，可用：`DSAPI_KSU_WITH_MANAGER_APK=0 ./scripts/dsapi.sh ksu pack`

## 外置模块独立打包

核心 KSU 包不再内置 `module_zips`。模块请独立打包、独立更新：

```sh
# 单模块
./scripts/build_ksu_module_zip.sh dsapi.demo.touch_ui

# 全部模块（逐个独立 zip）
./scripts/build_ksu_module_zip.sh --all
```

默认输出目录：`artifacts/ksu_module_zips/`

## 安装后控制

```sh
# 核心状态
su -c /system/bin/sh /data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh status

# capability 列表/操作
su -c /system/bin/sh /data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh capability list
su -c /system/bin/sh /data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh capability start core.daemon
su -c /system/bin/sh /data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh capability add /data/adb/modules/directscreenapi/capability_examples/inject.window.sh

# 启动可视化管理 UI（由 app_process 拉起，桌面无图标）
su -c /system/bin/sh /data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh ui start 1200

# 可选：自动补装失败时手动安装 manager APK
su -c /system/bin/sh /data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh ui install

# zygote 注入代理状态 / 策略（包+用户 scope）
su -c /system/bin/sh /data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh zygote status
su -c /system/bin/sh /data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh zygote policy-list
su -c /system/bin/sh /data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh zygote policy-set com.example.app 0 allow
```

在 KernelSU 模块详情页点击 `Action`：

- 自动补装并拉起 `DSAPI Manager`，主路径为 `app_process` 寄生 host。
- 桥接与 zygote agent 默认随核心一并拉起。

也可通过统一入口：

```sh
./scripts/dsapi.sh ksu ctl status
./scripts/dsapi.sh ksu ui-start 1200
# 可选：自动补装失败时手动补装
./scripts/dsapi.sh ksu ui-install
```

## 设计原则

- 核心模块默认只内置 `core.daemon`
- 注入/输入法/窗口等能力走外部 capability 热插拔
- 协议见：`docs/guides/ksu_capability_module_spec.md`
- 模块打包默认尝试嵌入 `zygisk` loader（可用 `DSAPI_KSU_WITH_ZYGISK=0` 关闭）

## 参考资料

- KernelSU Module Guide: `https://kernelsu.org/guide/module.html`
- KernelSU Module WebUI: `https://kernelsu.org/guide/module-webui.html`
- KernelSU Module Config: `https://kernelsu.org/guide/module-config.html`
