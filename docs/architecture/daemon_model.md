# 守护进程模型

## 目的

`dsapid` 是唯一状态持有进程，负责统一控制/帧/输入通道，避免多进程状态漂移。

## 传输层

- Unix Domain Socket 统一通道（默认）：`artifacts/run/dsapi.sock`
- 协议：固定长度二进制帧（`DSAP` header + payload）
- 帧交换：SHM + `SCM_RIGHTS`（仅传 fd 与元数据）
- 触控流：`STREAM_TOUCH_V1` 握手后发送固定 16B 二进制包

## 控制面（二进制协议 v1）

控制面不再接受文本命令，统一使用如下二进制头：

- `magic: u32`（`0x50415344`，即 `DSAP`）
- `version: u16`（当前 `1`）
- `opcode: u16`
- `payload_len: u32`
- `seq: u64`

已实现 opcode：

- `PING`
- `VERSION`
- `READY_GET`
- `SHUTDOWN`
- `DISPLAY_GET`
- `DISPLAY_SET`
- `TOUCH_CLEAR`
- `TOUCH_COUNT`
- `TOUCH_MOVE`
- `RENDER_SUBMIT`
- `RENDER_GET`

响应统一为二进制：

- 同头结构（复用 `opcode/seq`）
- payload 固定 `68B`：`status(i32) + values([u64; 8])`

## 帧交换（SHM 零拷贝）

统一通道内支持以下命令：

- `RENDER_FRAME_BIND_SHM`
  - 返回：`OK SHM_BOUND <capacity> <offset>` + `SCM_RIGHTS` 传输 memfd
- `RENDER_FRAME_WAIT_SHM_PRESENT <last_frame_seq> <timeout_ms>`
  - 返回：
    - 命中：`OK <frame_seq> <w> <h> RGBA8888 <byte_len> <checksum> <offset>`
    - 超时：`OK TIMEOUT`
  - 语义：`timeout_ms=0` 表示无限阻塞等待（事件唤醒）
- `RENDER_FRAME_SUBMIT_DMABUF <w> <h> <stride> <format> <usage> <byte_len> <byte_offset> [origin_x origin_y]`
  - 请求后需附带一次 `SCM_RIGHTS` fd 发送（同连接下一次写）
  - 返回：`OK <frame_seq> <w> <h> RGBA8888 <byte_len> <checksum>`

像素数据始终驻留共享内存，socket 只承载控制元数据与 fd。

## 并发模型

- 主线程：`mio` + `epoll` 监听 listener（阻塞等待，无固定周期轮询）
- 处理线程：固定大小 `DispatchPool`（不再 thread-per-connection）
- 背压策略：超出并发能力时返回 `ERR BUSY`

## 安全基线

- `SO_PEERCRED` UID 鉴权（默认仅允许同 UID）
- 白名单通过 `DSAPI_ALLOWED_UIDS` 配置
- 协议参数全部做长度/边界校验，非法请求返回错误，不修改内部状态
