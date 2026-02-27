# 变更日志

## 未发布

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

## 0.1.0 - 基石重构版本

- 初始化全新仓库结构（不迁移历史实现）
- 实现 Rust 核心状态模型与路由引擎
- 定义 C ABI v0.1 接口与 C 示例
- 增加命令行工具 `dsapi`
- 增加守护进程 `dsapid` 与控制端 `dsapictl`
- 增加构建与质量检查脚本
- 完成首版架构、API、运维与治理文档
