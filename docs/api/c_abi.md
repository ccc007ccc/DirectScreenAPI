# C ABI（v0.1）

头文件：`bridge/c/include/directscreen_api.h`

## 上下文生命周期

- `dsapi_abi_version`
- `dsapi_context_create`
- `dsapi_context_create_with_abi`
- `dsapi_context_destroy`

### 并发与生命周期约束

- `dsapi_context_t*` 只能由 `dsapi_context_create*` 创建，并且只能销毁一次。
- `dsapi_context_destroy(ctx)` 不能与任何其它 `dsapi_*` 调用并发执行。
- 调用方传入的输出指针必须可写；输入指针必须可读；错误指针属于未定义行为（UB）。

## 显示状态接口

- `dsapi_set_display_state`
- `dsapi_get_display_state`

## 路由接口

- `dsapi_set_default_decision`
- `dsapi_region_clear`
- `dsapi_region_add_rect`
- `dsapi_route_point`

## 触控会话接口

- `dsapi_touch_down`
- `dsapi_touch_move`
- `dsapi_touch_up`
- `dsapi_touch_cancel`
- `dsapi_touch_clear`
- `dsapi_touch_count`

## 渲染接口

- `dsapi_render_submit_stats`
- `dsapi_render_get_stats`
- `dsapi_render_submit_frame_rgba`
- `dsapi_render_submit_frame_ahb_fd`
- `dsapi_render_get_frame_info`
- `dsapi_render_clear_frame`
- `dsapi_render_frame_read_chunk`
- `dsapi_render_present`
- `dsapi_render_get_present`

### Android 硬件缓冲区桥接（新增）

- `dsapi_hwbuffer_desc_t`：描述 `AHardwareBuffer` 的核心元数据（宽高、stride、format、usage、偏移、长度）。
- `dsapi_render_submit_frame_ahb_fd`：通过 dma-buf fd + 元数据提交帧。

说明：

- 当前接口优先保障 ABI 与调用路径稳定，零拷贝策略由守护进程共享内存链路与后续平台后端逐步启用。
- 建议 JNI 层传入 `AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM`（值 `1`）并保证 `byte_len` 覆盖有效像素区。

## 状态码

- `0`: OK
- `1`: NULL_POINTER
- `2`: INVALID_ARGUMENT
- `3`: OUT_OF_RANGE
- `4`: INTERNAL_ERROR

## 兼容策略

- 已发布符号不做原地破坏式修改
- 破坏性变更必须提升 ABI 主版本
- 新增接口必须补文档与错误语义
- 建议优先调用 `dsapi_context_create_with_abi(DSAPI_ABI_VERSION, ...)` 做运行时 ABI 协商
