# 守护进程模型

## 目的

`dsapid` 作为唯一状态持有进程，避免多 CLI 进程导致状态漂移。

## 传输层

- Unix Domain Socket（默认 `artifacts/run/dsapi.sock`）
- 单行命令请求
- 单行响应结果
- 二进制帧提交扩展（`RENDER_FRAME_SUBMIT_RGBA_RAW` 先握手再传定长 body）
- 共享内存取帧扩展（`RENDER_FRAME_GET_FD`：头行 + `SCM_RIGHTS` 传输帧 fd）
- 事件驱动帧等待（`RENDER_FRAME_WAIT`）
- 二进制触摸流扩展（`STREAM_TOUCH_V1` 握手后发送定长二进制包）
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
- `RENDER_FRAME_SUBMIT_RGBA_RAW <width> <height> <byte_len>`（收到 `OK READY` 后发送 `byte_len` 原始 RGBA8）
- `RENDER_FRAME_SUBMIT_RGBA <width> <height> <base64_rgba8>`
- `RENDER_FRAME_GET_RAW`（`OK <frame_seq> <w> <h> <byte_len>` 后紧跟 `byte_len` 原始 RGBA8）
- `RENDER_FRAME_GET_FD`（`OK <frame_seq> <w> <h> <byte_len>` + `SCM_RIGHTS` 帧 fd）
- `RENDER_FRAME_GET`
- `RENDER_FRAME_WAIT <last_frame_seq> <timeout_ms>`
- `RENDER_FRAME_READ_BASE64 <offset> <max_bytes>`
- `RENDER_FRAME_CLEAR`
- `RENDER_PRESENT`
- `RENDER_PRESENT_GET`
- `RENDER_DUMP_PPM`
- `STREAM_TOUCH_V1`（`OK STREAM_TOUCH_V1` 后进入二进制触摸流）
- `SHUTDOWN`

## 响应格式

- 成功：`OK ...`
- 失败：`ERR <STATUS_NAME>`
- `RENDER_FRAME_WAIT` 超时：`OK TIMEOUT`

## 生命周期管理

- 启动：`scripts/daemon_start.sh`
- 状态：`scripts/daemon_status.sh`
- 停止：`scripts/daemon_stop.sh`

## 安全基线

- v0.1 不执行任何特权副作用动作
- 状态仅驻留进程内，重启后重建
- 非法命令不会改变内部状态
