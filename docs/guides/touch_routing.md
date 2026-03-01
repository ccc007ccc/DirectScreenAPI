# 触控路由指南

本文描述 DirectScreenAPI 当前触控链路的核心语义，目标是优先保证稳定性与可预测性。

## 设计原则

- 每个 `pointer_id` 独立建会话
- 在 `TOUCH_DOWN` 时完成路由命中与决策锁定
- `TOUCH_MOVE`、`TOUCH_UP`、`TOUCH_CANCEL` 复用同一决策
- 非法事件不修改内部状态

该设计避免了“第一次点击穿透、后续事件被阻断”这类链路抖动问题。

## 命令语义

- `TOUCH_DOWN <pointer_id> <x> <y>`
  - 创建触控会话
  - 返回 `OK <decision> <region_id>`
- `TOUCH_MOVE <pointer_id> <x> <y>`
  - 更新触点坐标
  - 返回会话锁定决策
- `TOUCH_UP <pointer_id> <x> <y>`
  - 结束触控会话
  - 返回会话锁定决策
- `TOUCH_CANCEL <pointer_id>`
  - 取消触控会话
  - 返回会话锁定决策
- `TOUCH_COUNT`
  - 返回当前活跃触点数量
- `TOUCH_CLEAR`
  - 清理全部触控会话

## 错误语义

- `ERR INVALID_ARGUMENT`
  - 参数个数错误
  - 坐标不是有效浮点数
  - 对已存在 `pointer_id` 重复执行 `TOUCH_DOWN`
- `ERR OUT_OF_RANGE`
  - `pointer_id < 0`
  - 未知触点执行 `MOVE/UP/CANCEL`
  - 活跃触点超过上限（当前固定 16）

## 示例

```sh
./scripts/dsapi.sh daemon start
./scripts/dsapi.sh daemon cmd ROUTE_CLEAR
./scripts/dsapi.sh daemon cmd ROUTE_ADD_RECT 9 block 0 0 300 300
./scripts/dsapi.sh daemon cmd TOUCH_DOWN 1 100 100
./scripts/dsapi.sh daemon cmd TOUCH_MOVE 1 600 600
./scripts/dsapi.sh daemon cmd TOUCH_UP 1 600 600
./scripts/dsapi.sh daemon cmd TOUCH_COUNT
./scripts/dsapi.sh daemon stop
```

在上面的流程中，`MOVE` 和 `UP` 仍会返回与 `DOWN` 一致的决策结果。
