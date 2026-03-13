# DSAPI Capability / Module 规范（KSU）

本文定义 DSAPI 在 KSU 模块中的 capability 与 module 扩展协议。
核心原则：`KSU 核心只保留 core.daemon`，其余能力全部走热插拔 module/capability。

## 1. 生命周期与目录

- 运行根目录：`/data/adb/dsapi`
- 当前 runtime：`/data/adb/dsapi/runtime/current`
- 内置 capability：`/data/adb/dsapi/runtime/current/capabilities/*.sh`
- 自定义 capability：`/data/adb/dsapi/capabilities/custom/*.sh`
- 已删除标记：`/data/adb/dsapi/state/capabilities_disabled/<cap_id>.disabled`

内置 capability 由 runtime 发布；自定义 capability 可在设备侧动态安装。

## 2. 脚本接口（必须实现）

每个 capability 文件必须是 POSIX `sh` 脚本，并可被 `source`。

### 2.1 元信息变量

- `CAP_ID`：唯一 ID，推荐 `<domain>.<name>`，如 `inject.window`
- `CAP_NAME`：展示名称
- `CAP_KIND`：类型，如 `core` / `inject` / `filter`
- `CAP_DESC`：简要说明

### 2.2 行为函数

- `cap_start`
- `cap_stop`
- `cap_status`
- `cap_detail`（可选）

`cap_status` 必须返回键值对格式，至少包含：

- `state=<running|stopped|missing|error|removed|unknown>`
- `pid=<pid or ->`
- `reason=<text>`

示例：`state=running pid=1234 reason=-`

## 3. Module ZIP 规范（新增）

统一安装入口：`dsapi_service_ctl.sh module install-zip <zip>`

### 3.1 目录结构

每个 module ZIP 至少包含：

- `dsapi.module`：模块元信息
- `capabilities/*.sh`：可选，capability 脚本集合
- `actions/*.sh`：可选，action 脚本集合（可多按钮）
- `env.spec`：可选，环境变量模板
- `env.values`：可选，用户自定义变量持久化

示例：

```text
dsapi.demo.touch_ui/
├── dsapi.module
├── env.spec
├── env.values
├── capabilities/
│   └── dsapi.demo.touch_ui.sh
└── actions/
    ├── start.sh
    ├── stop.sh
    ├── status.sh
    └── quick_start.sh
```

### 3.2 `dsapi.module` 字段

推荐字段：

- `MODULE_ID`：模块唯一 ID（必填）
- `MODULE_NAME`：展示名
- `MODULE_KIND`：模块类型（demo/system/inject/...）
- `MODULE_VERSION`：版本
- `MODULE_DESC`：描述
- `MAIN_CAP_ID`：主 capability ID（可选）
- `MODULE_AUTO_START`：`1/true/...` 表示安装后自动启动（可选）

### 3.3 Action 元信息

每个 `actions/*.sh` 可声明：

- `ACTION_NAME`：UI 按钮名
- `ACTION_DANGER`：`1` 表示危险操作，管理器需二次确认

### 3.4 Env 模板格式

`env.spec` 每行格式：

`KEY|DEFAULT|TYPE|LABEL|DESC`

示例：

`TOUCH_DEMO_FILTER_CHAIN|1,24,12|text|Filter Chain|默认开启高斯链`

## 4. 运行时注入环境变量

框架在调用 capability 时会注入：

- `DSAPI_BASE_DIR` / `DSAPI_RUN_DIR` / `DSAPI_LOG_DIR` / `DSAPI_STATE_DIR`
- `DSAPI_RUNTIME_DIR` / `DSAPI_RELEASES_DIR` / `DSAPI_ACTIVE_RELEASE`
- `DSAPI_ACTIVE_DSAPID` / `DSAPI_ACTIVE_DSAPICTL`
- `DSAPI_DAEMON_PID_FILE` / `DSAPI_DAEMON_SOCKET`
- `DSAPI_ACTIVE_ADAPTER_DEX` / `DSAPI_APP_PROCESS_BIN`
- `DSAPI_MODULES_DIR` / `DSAPI_MODULE_STATE_ROOT`（module 运行上下文）
- `DSAPI_MODULE_ID` / `DSAPI_MODULE_DIR` / `DSAPI_MODULE_STATE_DIR`（模块 action/capability 调用时）

## 5. 管理命令

统一入口：`/data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh`

- `capability list`
- `capability start <id>`
- `capability stop <id>`
- `capability remove <id>`
- `capability enable <id>`
- `capability status <id>`
- `capability detail <id>`
- `capability add <file>`

module 相关命令：

- `module install-zip <zip>`
- `module list|start|stop|reload|enable|disable|remove|status|detail <id>`
- `module reload-all`
- `module action-list <id>`
- `module action-run <id> <action>`
- `module env-list <id>`
- `module env-set <id> <key> <value>`
- `module env-unset <id> <key>`
- `errors last|clear`

新增状态输出（`status` 命令）：

- `ksu_dsapi_last_error state=<none|present> scope=<...> code=<...> message=<...> detail=<...> ts=<...>`
- `daemon_ready_state=<starting|ready|stopping|->`
- `daemon_module_registry_count=<n>`
- `daemon_module_registry_event_seq=<seq>`
- `daemon_module_registry_error=<0|1>`

`reload-all` 输出扩展：

- `module_action=reloaded_all total=<n> failed=<n> failed_ids=<id1,id2,...|->`

Action 超时控制：

- `DSAPI_MODULE_ACTION_TIMEOUT_SEC`（默认 60，最小 1）
- 若超时返回，输出 `ksu_dsapi_error=module_action_timeout ...`

`list` 输出行为：

- `capability_row=<id>|<name>|<kind>|<source>|<state>|<pid>|<reason>`

该行格式为 UI 与自动化脚本稳定消费接口。

## 6. 发布与更新策略

- KSU 模块尽量少更新：仅承载启动器与基础控制层。
- 核心 KSU 包不再内置 `module_zips`，模块更新走独立 ZIP 分发。
- 业务能力通过 runtime release 与 capability 热更新。
- 外部模块独立打包：
  - `./scripts/build_ksu_module_zip.sh <module_id>`
  - `./scripts/build_ksu_module_zip.sh --all`
- runtime 安装/切换：
  - `runtime install <release_id> <dir>`
  - `runtime activate <release_id>`
  - `runtime list`

## 7. 兼容策略

当前内核目标为“精简高性能”，不保留旧协议回退。
capability 应遵循同样原则：

- 无功能即明确 `state=missing`
- 不做静默失败
- 可观测输出优先

## 8. 示例位置

- 核心内置示例：`ksu/module_template/capabilities/core.daemon.sh`
- 外部扩展示例：`ksu/module_examples/dsapi.demo.touch_ui/`（MODULE_ID=`dsapi.demo.touch_ui`）、`ksu/module_examples/system.ime/`
- 旧 capability 占位示例：`ksu/capability_examples/inject.window.sh`、`ksu/capability_examples/inject.ime.sh`
- 安装后示例路径：`/data/adb/modules/directscreenapi/capability_examples/*.sh`
