# LSP（LSPosed）实现结构与技术路径分析

## 1. 分析范围与样本

- 本文中的 `LSP` 指的是 **LSPosed Framework**（不是 Language Server Protocol）。
- 参考样本：`https://github.com/LSPosed/LSPosed`（本地按 `--depth=1` 拉取的 `master` 快照）。
- 分析目标：梳理 LSPosed 的目录结构、实现方法、运行时技术路径（从模块安装到进程注入再到模块回调）。

## 2. 顶层结构（模块职责分层）

- `magisk-loader/`
  - Root 模块加载与注入入口（Zygisk / Riru 双实现）。
  - 负责在 zygote fork 前后与 daemon 建立通信、拉取 `lspd.dex`、触发核心初始化。
- `daemon/`
  - 常驻服务端，负责配置、作用域、模块列表、日志、通知、manager 协调。
  - 提供 Binder/AIDL 服务给注入进程和管理端。
- `core/`
  - Hook 核心与 Xposed 兼容层（`XposedBridge`、`XposedInit`、hooker 集合）。
  - Native 桥（`HookBridge`/`ResourcesHook`/`NativeAPI`）对接 LSPlant。
- `app/`
  - Manager UI（模块管理、作用域配置、日志、仓库、设置等）。
- `services/`
  - AIDL 接口契约拆分：
    - `manager-service`：管理端 API（启停模块、设 scope、日志、重启等）。
    - `daemon-service`：注入侧 API（模块列表、偏好、注入 manager binder 等）。
- `hiddenapi/`
  - `bridge + stubs`，统一封装系统隐藏 API 访问，减轻反射和版本差异影响。
- `dex2oat/`
  - dex2oat wrapper 与兼容处理（SELinux/挂载/inline 参数控制）。
- `external/`
  - 关键三方能力：`lsplant`、`dobby` 等。

## 3. 构建与打包方法

- 根工程通过 `settings.gradle.kts` 聚合子模块。
- `magisk-loader/build.gradle.kts` 在打包阶段将以下产物装配到一个 Magisk/KSU 模块 zip：
  - `manager.apk`
  - `daemon.apk`
  - `framework/lspd.dex`
  - `lib*/liblspd.so`
  - `magisk_module/*` 脚本与策略文件
- `magisk_module/customize.sh` 负责安装时解压、校验 hash、按架构放置 so、部署脚本。
- `magisk_module/post-fs-data.sh` 与 `service.sh` 在开机阶段启动 `daemon` 脚本。

## 4. 启动与注入主路径（核心链路）

### 4.1 开机拉起 daemon

- `magisk_module/post-fs-data.sh` / `service.sh` 最终执行 `magisk_module/daemon`。
- `daemon` 脚本通过 `app_process` 启动 Java 主类 `org.lsposed.lspd.Main`。
- `Main.main()` 进入 `ServiceManager.start(args)` 建立全局服务。

### 4.2 Zygote 注入入口（Riru / Zygisk）

- Zygisk 入口：`magisk-loader/src/main/jni/api/zygisk_main.cpp`
  - `onLoad()` 初始化 `MagiskLoader` 与配置桥。
  - 在 `pre/postAppSpecialize`、`pre/postServerSpecialize` 分别处理普通应用与 `system_server`。
- Riru 入口：`magisk-loader/src/main/jni/api/riru_main.cpp`
  - 对应 `forkAndSpecialize*` 与 `forkSystemServer*` 钩子。

### 4.3 每次 fork 后的注入动作

- 关键实现：`magisk-loader/src/main/jni/src/magisk_loader.cpp`
- 主要动作顺序：
  1. 判断是否跳过注入（如 isolated UID、child zygote、无 data dir）。
  2. 通过 `Service::RequestBinder` 向桥服务请求本进程可用的应用侧 Binder。
  3. 通过自定义事务获取 `lspd.dex` FD 与 obfuscation map。
  4. 用 `InMemoryDexClassLoader` 加载 `lspd.dex`。
  5. 初始化 LSPlant 与 Hook 注册（`InitArtHooker` + `InitHooks`）。
  6. 反射调用入口 `forkCommon(...)`（见 `magisk-loader/src/main/java/org/lsposed/lspd/core/Main.java`）。

## 5. Core Hook 实现方法

### 5.1 Java 入口与初始化

- `forkCommon(...)` 中先执行 `Startup.initXposed(...)`，再执行 `Startup.bootstrapXposed()`。
- `Startup` 会安装一组基础 hook：
  - `ActivityThread.attach`
  - `LoadedApk` 构造与 classloader 创建
  - `DexFile.open*`
  - `system_server` 特定路径 hook

### 5.2 模块装载策略

- 旧模块（legacy）：`XposedInit.loadLegacyModules()`
  - 读取 `assets/xposed_init`，兼容原 Xposed API。
- 新模块（modern）：`AttachHooker.afterHookedMethod()` 触发 `XposedInit.loadModules(...)`。
- 进程内包级回调：
  - 通过 `LoadedApkCtorHooker` / `LoadedApkCreateCLHooker` 在 classloader 就绪后触发 `XC_LoadPackage`。

### 5.3 Native Hook 桥

- `core/src/main/jni/src/jni/hook_bridge.cpp`
  - 提供 `hookMethod/unhookMethod/invokeOriginalMethod/deoptimizeMethod` 等 native 能力。
  - 维护回调优先级与快照，支持 modern/legacy 回调混合。
  - 底层调用 LSPlant（ART hook 内核）。

## 6. Daemon 控制面与配置面

### 6.1 服务编排

- `ServiceManager.start()` 初始化：
  - `LSPosedService`
  - `LSPApplicationService`
  - `LSPManagerService`
  - `LSPSystemServerService`
  - `LogcatService`
  - `Dex2OatService`（Android Q+）

### 6.2 AIDL 接口分工

- `ILSPApplicationService`
  - 给注入进程：获取模块列表、偏好路径、请求 injected manager binder。
- `ILSPManagerService`
  - 给管理端：启停模块、scope 管理、日志与系统操作接口。
- `ILSPosedService`
  - 系统桥接入口（请求 application service、派发 system_server context 等）。

### 6.3 作用域与模块数据库

- `ConfigManager` 使用 SQLite，核心表：
  - `modules`（模块启用态与 APK 路径）
  - `scope`（模块作用域：目标包+用户）
  - `configs`（模块偏好）
- 进程请求模块列表时按 `processName + uid` 精确过滤。

## 7. Manager 寄生注入（Parasitic Manager）

- 入口：`ParasiticManagerHooker.start()`
- 典型流程：
  1. 注入进程向 daemon 请求 `manager.apk` 的 `ParcelFileDescriptor`。
  2. 通过 Hook `ActivityThread/LoadedApk` 等路径，把 manager 代码“寄生”到目标进程展示。
  3. 通过 `Constants.setBinder()` 注入 manager binder，使 UI 可直接调用 `ILSPManagerService`。
- 这套方案的关键点：
  - 不依赖单独常驻 manager 进程。
  - 通过 binder 死亡监听与重拉起机制维持可用性。

## 8. 桥接通信与事务设计

- Native 侧桥服务在 `Service` 类中实现。
- 关键事务码：
  - Bridge transaction（与 `activity` 服务通道通信）
  - Dex transaction（下发 `lspd.dex` FD）
  - Obfuscation map transaction（下发混淆映射）
- `BridgeService` 在 daemon 侧负责把主服务 binder 挂到系统桥，并处理 system_server 重启后的重连。

## 9. 安全与稳定性机制

- 进程筛选与跳过：
  - 跳过 isolated / child zygote / 无 data dir 等不稳定目标。
- Binder 心跳：
  - `LSPApplicationService` 通过 `DeathRecipient` 跟踪进程存活，自动清理状态。
- 管理端 APK 签名校验：
  - `InstallerVerifier.verifyInstallerSignature(...)` 强校验 `manager.apk` 证书。
- SELinux 与 dex2oat 兼容：
  - `Dex2OatService` 监控策略状态，动态 mount/unmount wrapper，避免失配导致崩溃。
- 混淆映射下发：
  - 通过 obfuscation map 减少静态特征暴露并对齐运行时类名解析。

## 10. 端到端技术路径（时序）

```text
Magisk/KSU 安装模块
  -> customize.sh 解包+校验+部署产物
  -> post-fs-data/service 启动 daemon
  -> daemon(Main->ServiceManager) 初始化服务与桥
  -> Zygisk/Riru 在 zygote fork 前后回调 MagiskLoader
  -> 注入进程向 daemon 请求 Binder + lspd.dex + obfuscation map
  -> InMemoryDexClassLoader 加载 core dex
  -> 初始化 LSPlant + 注册 Native/Java Hook
  -> 调用 forkCommon
  -> initXposed + bootstrapXposed
  -> 按 scope 加载 legacy/modern 模块
  -> 在 LoadedApk/Attach 等时机触发模块回调
  -> manager UI 通过 ILSPManagerService 做配置与控制
```

## 11. 对你当前 DSAPI 方案可借鉴点

- 把“注入平面”和“控制平面”分开：
  - 注入侧只做轻量握手和执行。
  - 配置与策略集中到 daemon 统一裁决。
- 用 AIDL/Binder 做明确契约层：
  - 区分“注入进程接口”和“管理端接口”。
- 用 scope（进程/包/用户）做精确放行：
  - 避免全局注入带来的性能和稳定性风险。
- 对 APK/桥接服务做强约束：
  - 签名校验、心跳机制、死亡重连、事务幂等。

## 12. 关键源码索引（便于二次深入）

- 注入入口：`magisk-loader/src/main/jni/api/zygisk_main.cpp`
- 注入执行：`magisk-loader/src/main/jni/src/magisk_loader.cpp`
- fork 入口：`magisk-loader/src/main/java/org/lsposed/lspd/core/Main.java`
- 核心启动：`core/src/main/java/org/lsposed/lspd/core/Startup.java`
- Native Hook 桥：`core/src/main/jni/src/jni/hook_bridge.cpp`
- daemon 主控：`daemon/src/main/java/org/lsposed/lspd/service/ServiceManager.java`
- 应用侧服务：`daemon/src/main/java/org/lsposed/lspd/service/LSPApplicationService.java`
- 管理侧服务：`daemon/src/main/java/org/lsposed/lspd/service/LSPManagerService.java`
- 配置中心：`daemon/src/main/java/org/lsposed/lspd/service/ConfigManager.java`
- 寄生 manager：`magisk-loader/src/main/java/org/lsposed/lspd/util/ParasiticManagerHooker.java`
- 安装脚本：`magisk-loader/magisk_module/customize.sh`

