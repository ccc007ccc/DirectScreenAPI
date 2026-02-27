# 守护进程模型

## 目的

`dsapid` 作为唯一状态持有进程，避免多 CLI 进程导致状态漂移。

## 传输层

- Unix Domain Socket（默认 `artifacts/run/dsapi.sock`）
- 单行命令请求
- 单行响应结果
- 并发连接处理（触摸桥接与控制命令可并行）

## 协议命令（v0.1+）

- `PING`
- `VERSION`
- `DISPLAY_GET`
- `DISPLAY_SET <w> <h> <hz> <dpi> <rot>`
- `ROUTE_DEFAULT <pass|block>`
- `ROUTE_CLEAR`
- `ROUTE_ADD_RECT <id> <pass|block> <x> <y> <w> <h>`
- `ROUTE_POINT <x> <y>`
- `TOUCH_DOWN <pointer_id> <x> <y>`
- `TOUCH_MOVE <pointer_id> <x> <y>`
- `TOUCH_UP <pointer_id> <x> <y>`
- `TOUCH_CANCEL <pointer_id>`
- `TOUCH_CLEAR`
- `TOUCH_COUNT`
- `RENDER_SUBMIT <draw_calls> <frost_passes> <text_calls>`
- `RENDER_GET`
- `SHUTDOWN`

## 响应格式

- 成功：`OK ...`
- 失败：`ERR <STATUS_NAME>`

## 生命周期管理

- 启动：`scripts/daemon_start.sh`
- 状态：`scripts/daemon_status.sh`
- 停止：`scripts/daemon_stop.sh`

## 安全基线

- v0.1 不执行任何特权副作用动作
- 状态仅驻留进程内，重启后重建
- 非法命令不会改变内部状态
