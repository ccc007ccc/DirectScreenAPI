# DSAPI Module Runtime V2 协议草案

> 状态：Draft（用于指导 `daemon + ksu + manager` 重构实现）

## 1. 目标

1. 将模块生命周期从 shell 拼接迁移到统一控制面（最终由 daemon 托管）。
2. 支持模块安装/启停/重载的热生效，不要求设备重启。
3. 给 Manager 提供稳定可解析的结构化响应，降低 grep/sed 脆弱链路。

## 2. V2 总体分层

```text
KSU Loader(薄) -> Runtime Host(厚, daemon) -> Service Contract(V2) -> Manager/CLI
```

- Loader：仅负责启动核心、准备环境目录、兜底恢复。
- Runtime Host：维护 registry、FSM、事务锁、错误聚合。
- Service Contract：稳定版本化输出（V2），兼容 V1 文本接口。
- Manager：只消费 Contract，不直接拼接底层状态。

## 3. Registry 数据模型

`ModuleRecord`（建议存储于 daemon 内存 + 持久化快照）：

- `id`: string，模块唯一标识
- `name`: string
- `kind`: string
- `version`: string
- `enabled`: bool
- `state`: enum
- `reason`: string
- `main_cap`: string
- `action_count`: int
- `auto_start`: bool
- `installed_at`: epoch_ms
- `updated_at`: epoch_ms
- `last_error`: `ErrorRecord | null`

`ErrorRecord`：

- `scope`: string（`module.install` / `module.lifecycle` / `module.action` / `daemon.lifecycle`）
- `code`: string
- `message`: string
- `detail`: string
- `ts`: string（本地时间戳）

## 4. 生命周期 FSM（V2）

状态集合：

- `installed`
- `disabled`
- `running`
- `ready`
- `degraded`
- `stopped`
- `error`
- `removed`

核心迁移：

1. `install`：`none -> installed`
2. `start`：`installed|stopped|error -> running|ready|degraded`
3. `stop`：`running|ready|degraded -> stopped`
4. `reload`：`running|ready|degraded|stopped -> running|ready|degraded|error`
5. `disable/enable`：`* <-> disabled`
6. `remove`：`* -> removed`

失败统一进入 `error`，并写入 `ErrorRecord`。

## 5. V2 返回契约

为了兼容现有 V1 KV 输出，V2 采用“KV 包络 + JSON 主体”：

固定包络字段：

- `result_version=2`
- `result_type=<module.list|module.reload|module.action.run|...>`
- `result_code=<0=ok,非0失败>`
- `result_message=<ok|error_xxx>`
- `result_json=<JSON对象或数组>`

当前已落地的 daemon 状态补充（READY）：

- `module_count`：daemon 内 registry 条目数
- `module_event_seq`：registry 变更序号（自增）
- `module_error`：最近错误标记（`0/1`）

示例：`module reload <id>`

```text
result_version=2
result_type=module.reload
result_code=0
result_message=ok
result_json={"module_id":"dsapi.demo.touch_ui","from":"running","to":"ready","reloaded":true}
```

示例：`module list`

```text
result_version=2
result_type=module.list
result_code=0
result_message=ok
result_json=[{"id":"system.ime","state":"ready","enabled":true},{"id":"dsapi.demo.touch_ui","state":"degraded","enabled":true}]
```

## 6. 错误码规范（首版）

- `E_MODULE_NOT_FOUND`
- `E_MODULE_DISABLED`
- `E_ACTION_NOT_FOUND`
- `E_ACTION_TIMEOUT`
- `E_ACTION_FAILED`
- `E_INSTALL_INVALID_ZIP`
- `E_INSTALL_TXN_FAILED`
- `E_RELOAD_FAILED`
- `E_DAEMON_NOT_READY`
- `E_BRIDGE_OFFLINE`

约定：

- `result_code` 仍保持 shell 兼容退出码；
- 机器消费优先看 `result_message` + `result_json.error.code`。

## 7. 事务与并发

必须串行化的路径：

1. `install/update/remove`
2. `reload-all`
3. `runtime_seed_sync`

建议锁模型：

- 全局写锁：`runtime.write.lock`
- 模块级锁：`module.<id>.lock`
- 日志写入无锁，但错误状态文件采用原子替换（tmp -> mv）

## 8. 与现有实现的映射

已落地（当前仓库）：

- `module reload <id>` / `reload-all`
- `MODULE_AUTO_START`
- action 超时参数：`DSAPI_MODULE_ACTION_TIMEOUT_SEC`
- 最近错误聚合：`errors last|clear` + `status` 内 `ksu_dsapi_last_error`

待迁移到 daemon：

1. module registry 与 FSM 真正入驻 `core/rust`
2. shell 改为“薄适配器”，只转发命令与显示结果
3. manager 改为以 `result_version=2` 为主读取

## 9. 迁移策略

1. **Phase-1（兼容期）**：同时输出 V1 + V2。
2. **Phase-2（切换期）**：Manager 默认读 V2，V1 仅保留脚本兜底。
3. **Phase-3（收敛期）**：移除高耦合文本拼接逻辑，保留最小兼容接口。

## 10. Manager 对接要求

1. 所有页面统一通过 Repository 拉取 `status/module/errors`。
2. 模块页必须支持 `reload` / `reload-all` 并显示 `failed_ids`。
3. 日志页默认展示最近错误链路，可执行 `errors clear`。
4. 危险操作保留确认弹窗与操作记录（后续接入事件流）。
