# Android 适配层

该目录用于承载 Android 平台能力接线，不承载核心策略。

当前状态：

- 已有接口契约（Display/Input/Render）
- 已接入显示探测实现（`AndroidDisplayAdapter`）
- 已提供命令入口（`AndroidAdapterMain`）
- 已可通过脚本构建并被 `app_process` 调用

设计约束：

- 核心状态与策略必须保留在 Rust 核心层
- Android 侧只做能力适配，保持薄层与可替换
- 任何平台私有 API 使用都集中在适配层
