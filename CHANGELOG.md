# 变更日志

## 未发布

- 新增统一脚本入口 `scripts/dsapi.sh`（daemon/presenter/touch/android/frame/build/check）
- 删除 10 个旧包装脚本（`daemon/presenter/touch` 的 `start|stop|status|cmd`），统一收敛到 `dsapi.sh`
- 删除 `android_display_probe.sh` / `daemon_sync_display.sh` / `daemon_frame_pull.sh`，对应能力并入 `dsapi.sh`
- 删除 `daemon_presenter_run.sh` / `daemon_touch_bridge_run.sh`，`run` 逻辑并入 `dsapi.sh`
- 删除历史 AWK 转换脚本 `touch_event_to_dsapi.awk`（已不在主链路）
- 新增 `docs/guides/commands.md`，集中维护 `dsapi.sh` 命令入口与迁移映射
- `dsapid` 新增并发连接上限 `DSAPI_MAX_CONNECTIONS` / `--max-connections`，超限返回 `ERR BUSY`
- 新增 Android 显示探测实现（`AndroidDisplayAdapter`）
- 新增 Android 适配入口（`AndroidAdapterMain`）
- 新增 Android 适配构建脚本与显示探测脚本
- 新增显示参数同步脚本（同步到 `dsapid` 的 `DISPLAY_SET`）
- 新增 Android 适配中文指南并更新索引
- 新增触控会话状态机（多点触控 + 决策锁定）
- 新增 daemon 触控命令（`TOUCH_*`）
- 新增 C ABI 触控接口（`dsapi_touch_*`）
- 新增触控路由指南文档
- 新增真实触摸桥接脚本（`getevent` -> `TOUCH_*`）
- 新增持久流式客户端 `dsapistream`
- 触摸桥接支持显示参数变化自动重映射
- `dsapid` 支持并发连接处理
- 新增渲染统计命令（`RENDER_SUBMIT` / `RENDER_GET`）
- 新增像素帧提交命令（`RENDER_FRAME_SUBMIT_RGBA` / `RENDER_FRAME_GET` / `RENDER_FRAME_CLEAR`）
- 新增呈现命令（`RENDER_PRESENT` / `RENDER_PRESENT_GET`）作为内存主链路
- 新增调试导出命令 `RENDER_DUMP_PPM`
- 新增帧分块读取命令 `RENDER_FRAME_READ_BASE64`
- 新增帧拉取命令（`dsapi.sh frame pull`），可按二进制协议拉取完整 RGBA 帧
- C ABI 增加渲染接口（统计、帧提交、分块读取、呈现状态）

## 0.1.0 - 基石重构版本

- 初始化全新仓库结构（不迁移历史实现）
- 实现 Rust 核心状态模型与路由引擎
- 定义 C ABI v0.1 接口与 C 示例
- 增加命令行工具 `dsapi`
- 增加守护进程 `dsapid` 与控制端 `dsapictl`
- 增加构建与质量检查脚本
- 完成首版架构、API、运维与治理文档
