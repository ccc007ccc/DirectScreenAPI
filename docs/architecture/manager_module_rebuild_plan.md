# DSAPI Manager / Module 全量重构清单

目标：重构为低学习成本、多页面管理器 + 可扩展模块体系，覆盖你提出的全部要求。

## 1. UI 重构目标

- [x] 管理器改为多页面结构：主页 / 模块页 / 设置页
- [x] 按钮减量，按任务分区（核心控制、模块控制、设置）
- [x] 危险操作二次确认（停止核心、移除模块、禁用模块、重置设置）
- [x] 视觉风格按 Material 3 思路统一（层级、圆角、色彩、卡片状态）

## 2. 模块系统目标

- [x] 定义 DSAPI 模块 ZIP 规范（元信息、capability、action、env）
- [x] 控制层支持 ZIP 导入安装
- [x] 模块支持生命周期读取（installed/enabled/running/stopped/error）
- [x] 模块支持 action 多按钮（类似 KSU Action）
- [x] 模块支持环境变量模板与用户配置持久化

## 3. 功能模块目标

- [x] 实现系统输入法调用模块（IME）
- [x] 将 `dsapi_touch_demo_test` 做成测试模块，可快速 Action 启停
- [x] 测试模块可被正确停止并回收进程
- [x] 去掉 `dsapi_touch_demo_test` 默认自动关闭
- [x] 默认开启 blur（并可在管理器里调参）

## 4. 管理器能力目标

- [x] 模块页支持“添加 ZIP 模块”
- [x] 模块页显示模块生命周期、action 按钮、状态摘要
- [x] 设置页可读取模块 env 清单并编辑保存
- [x] 设置变更后可应用到模块运行时

## 5. 实施顺序

1. 扩展 `dsapi_ksu_lib.sh` 与 `dsapi_service_ctl.sh`（模块协议与命令）
2. 实现两个标准模块（IME / TouchDemo）
3. 调整 `dsapi_touch_demo_test.sh` 默认策略（禁自动退出 + 默认 blur）
4. Manager 改多 Activity 页面 + 二次确认 + 模块导入
5. 联调模块 action/env/lifecycle，全链路验证

## 6. 验证清单

- [x] `module install-zip` 可导入模块
- [x] `module list` 正确显示生命周期
- [x] `module action run` 可执行模块动作
- [x] `module env list/set` 可读写并持久化
- [x] 管理器里可完成导入、启停、设置、删除流程
- [x] `dsapi_touch_demo_test` 不再默认定时退出
- [x] blur 默认生效且可配置

## 7. 产出文件

- 管理器多页面：
  - `android/ksu_manager/src/main/java/org/directscreenapi/manager/MainActivity.java`
  - `android/ksu_manager/src/main/java/org/directscreenapi/manager/ModulesActivity.java`
  - `android/ksu_manager/src/main/java/org/directscreenapi/manager/SettingsActivity.java`
- 管理器公共层：
  - `android/ksu_manager/src/main/java/org/directscreenapi/manager/ManagerConfig.java`
  - `android/ksu_manager/src/main/java/org/directscreenapi/manager/DsapiCtlClient.java`
  - `android/ksu_manager/src/main/java/org/directscreenapi/manager/CtlParsers.java`
  - `android/ksu_manager/src/main/java/org/directscreenapi/manager/UiStyles.java`
- 标准模块示例：
  - `ksu/module_examples/test.touch_demo/*`
  - `ksu/module_examples/system.ime/*`
- 构建与默认策略：
  - `scripts/build_ksu_module.sh`
  - `scripts/dsapi_touch_demo_test.sh`
