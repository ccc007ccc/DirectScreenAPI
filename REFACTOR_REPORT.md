# DirectScreenAPI Refactor Report

## 目标达成概览
本轮完成了核心链路的增量重构，重点是移除静默回退并优化高频数据路径。
在不回滚现有仓库改动的前提下，保持了 Rust 测试通过与 Android 产物可构建。

## 已完成改造

### 1) Rust 运行时与提交链路
- `core/rust/src/engine/runtime.rs`
  - 移除 Vulkan 处理失败后自动降级到 CPU 的隐式回退，改为显式返回错误。
  - Vulkan 后端仅在 pipeline 变更时更新，避免每帧重复 clone/set。
  - 优化 `write_ppm_rgba`：改为按行写入 RGB，避免一次性分配整帧 `Vec<u8>`。
- `core/rust/src/bin/dsapid/frame_fd.rs`
  - DMABUF 提交路径重构为 `submit_dmabuf_tight_rgba(...)`。
  - 在 `stride == width` 主路径中直接使用 mmap 切片提交，移除中间整块 `to_vec()` 拷贝。
  - 保留非紧凑 stride 的行拷贝分支，确保格式兼容。
- `core/rust/src/backend/filter.rs`
  - 去除高斯两次卷积中的冗余权重累加与归一化除法（kernel 已归一化），减少热循环计算量。

### 2) Android 适配层
- `android/adapter/src/main/java/org/directscreenapi/adapter/DaemonSession.java`
  - 行打包写入路径改为复用 `rowPackScratchBuffer`，消除逐行 `ByteBuffer.wrap(...)` 对象创建。
- `android/adapter/src/main/java/org/directscreenapi/adapter/ScreenCaptureStreamer.java`
  - 删除 `shm_pack` 回退提交分支与对应冗余方法。
  - 对不满足主路径条件的 plane 布局改为显式报错，防止静默降级。
- `android/adapter/src/main/java/org/directscreenapi/adapter/AndroidDisplayAdapter.java`
  - 收敛 `invokeSingleDisplayIdMethod` 的多轮参数类型回退扫描为单轮扫描，降低反射开销。

### 3) 构建与运行脚本回退清理
- `scripts/build_android_adapter.sh`
  - 删除输出目录不可写时的 `_user` 回退策略，统一为显式失败。
- `scripts/build_android_manager.sh`
  - 删除输出目录不可写时回退路径策略，统一显式失败。
  - 清理无用变量/函数（`OUT_DIR_EXPLICIT`、`shell_quote`）。
- `scripts/dsapi_android.sh`
  - 删除 Android probe 输出目录 `_user` 回退逻辑，改为显式报错。
- `scripts/dsapi_presenter.sh`
  - 删除日志文件 fallback 路径。
  - 删除 Android 输出目录 `_user` 回退逻辑。
  - 删除构建失败后使用缓存 dex 的兜底，改为构建失败即退出。

## 验证结果
- Rust 单测（60s 超时约束内）：
  - `cd core/rust && timeout 60 cargo test -q` 通过（45+1+12 测试组全部通过）。
- Android Adapter 构建：
  - `./scripts/build_android_adapter.sh` 成功，生成 classes/dex/native bridge。
- Android Manager 构建：
  - `./scripts/build_android_manager.sh` 成功，生成并签名 APK。
- 脚本语法检查：
  - `sh -n` 校验通过（改动脚本）。

## 删除与冗余清理说明
- 已删除冗余代码分支：
  - `ScreenCaptureStreamer.java` 中 `ensureTightRgba` / `packPlaneToTightRgba` 及关联 fallback 提交分支。
- 未执行文件级删除：
  - `scripts/fix.sh` 仍保留（当前仍被 `scripts/dsapi.sh fix` 路由引用）。

## 剩余可继续优化项
- `filter.rs` 可继续做 SIMD/并行化优化（当前为标量实现）。
- Android 侧 `DisplayAdapter`/`ReflectBridge` 可引入 Method 缓存池进一步降低反射成本。

## 2026-03-08 增量更新（单 APK 自动安装链路）
- `ksu/module_template/bin/dsapi_ksu_lib.sh`
  - 恢复 `dsapi_manager_ensure_installed`，提供 UI 启动前自动补装能力。
  - `dsapi_manager_host_start` 改为自动补装后再拉起 host。
- `ksu/module_template/bin/dsapi_service_ctl.sh`
  - `ui start` 改为自动补装 manager，不再直接报未安装错误。
  - `ui install` 收敛为对共享自动安装函数的手动入口。
- `ksu/module_template/service.sh`
  - 开机服务阶段增加 manager 自动补装（失败仅记录日志，不阻断核心链路）。
- `android/adapter/.../BridgeControlServer.java`
  - `startManagerHost` 启动前增加自动补装。
  - 新增 manager APK 解析与 install-existing 补装路径，保留 AIDL 主链。
- `android/ksu_manager/AndroidManifest.xml`
  - 移除 `android:process="shell"`，回归单 APK 常规进程模型。
