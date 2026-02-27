# C ABI（v0.1）

头文件：`bridge/c/include/directscreen_api.h`

## 上下文生命周期

- `dsapi_context_create`
- `dsapi_context_destroy`

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
