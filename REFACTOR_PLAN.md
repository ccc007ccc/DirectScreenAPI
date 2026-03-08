# DirectScreenAPI Refactor Plan

> 目标：移除低价值回退逻辑、清理冗余路径、优化热路径拷贝与循环，同时保持接口与核心业务行为稳定。

## 0) 全局扫描与基线
- [x] 扫描项目结构、构建入口、代码规模
- [x] 识别 fallback/legacy/重复实现热点
- [x] 运行 Rust 基线测试（`core/rust`: `cargo test -q`）

## 1) Rust 核心路径重构
- [x] `runtime.rs`：移除渲染后端自动回退链，改为显式策略（失败即报错），避免静默降级
- [x] `runtime.rs`：减少帧对象与像素缓冲无谓 clone，优先按需切片/借用
- [x] `frame_fd.rs`：优化 DMABUF 提交路径，避免整块 `to_vec()` 复制
- [x] `filter.rs`：消除高频 filter 前后冗余内存复制

## 2) Android 适配层优化
- [x] `ScreenCaptureStreamer.java`：优化 plane->RGBA 紧凑拷贝路径，减少重复分支
- [x] `DaemonSession.java`：优化提交路径的行打包与通道写入，减少中间拷贝
- [x] `AndroidDisplayAdapter.java`：收敛反射回退链，保留单一主路径与明确失败输出

## 3) 脚本与构建链清理
- [x] `build_android_adapter.sh`：移除多级 fallback 输出目录策略，统一单一路径与显式失败
- [x] `build_android_manager.sh`：同上，统一构建输出与错误行为
- [x] `dsapi_presenter.sh` / `dsapi_android.sh`：删除日志/out_dir/缓存 dex 兜底分支，保留主执行链

## 4) 冗余与废弃清理清单
- [x] 删除确认无引用的废弃脚本或旧入口（已核验当前无可安全删除项）
- [x] 删除无用注释与重复说明（只清理改动文件内）

## 5) 持续验证与收尾
- [x] 每个模块改造后执行对应构建/测试
- [x] 运行仓库级关键校验（Rust tests + 关键脚本 dry-run）
- [x] 输出 `REFACTOR_REPORT.md`（变更摘要、性能收益、兼容性说明）

## 待删除候选（需“无引用验证”后执行）
- [x] 候选 A：`scripts/fix.sh`（已确认被 `scripts/dsapi.sh` 引用，保留）
- [x] 候选 B：重复输出目录 fallback 相关临时逻辑（在脚本改造后清理）
- [x] 候选 C：渲染后端自动回退路径注释与分支（Rust Runtime）
