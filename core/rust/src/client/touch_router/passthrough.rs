use std::collections::HashMap;

use evdev::{EventType, InputEvent, InputEventKind, Key};

use super::{PointerRoute, SlotState, ABS_MT_SLOT};

pub(super) fn build_passthrough_frame_events(
    frame_events: &[InputEvent],
    frame_start_slot: i32,
    frame_slot_routes: &HashMap<i32, PointerRoute>,
    slots: &HashMap<i32, SlotState>,
    pointer_routes: &HashMap<i32, PointerRoute>,
) -> Vec<InputEvent> {
    let mut out: Vec<InputEvent> = Vec::new();
    let mut cur_slot = frame_start_slot;
    let mut emitted_slot: Option<i32> = None;

    for ev in frame_events {
        match ev.kind() {
            InputEventKind::AbsAxis(axis) => {
                if axis.0 == ABS_MT_SLOT {
                    cur_slot = ev.value();
                    continue;
                }

                // 只透传 MT 轴；ABS_X/ABS_Y 等全局轴会混入 UI 指针状态，不能直接透传。
                if !is_mt_axis(axis.0) {
                    continue;
                }

                let route =
                    slot_route_for_frame(cur_slot, frame_slot_routes, slots, pointer_routes);
                if route != PointerRoute::PassThrough {
                    continue;
                }

                if emitted_slot != Some(cur_slot) {
                    out.push(InputEvent::new(EventType::ABSOLUTE, ABS_MT_SLOT, cur_slot));
                    emitted_slot = Some(cur_slot);
                }
                out.push(*ev);
            }
            // 触摸相关按键由路由器重建，避免 UI 触点污染系统侧状态。
            InputEventKind::Key(key) if is_touch_related_key(key) => {}
            InputEventKind::Synchronization(_) => {}
            _ => out.push(*ev),
        }
    }

    out
}

pub(super) fn has_passthrough_active_touch(
    slots: &HashMap<i32, SlotState>,
    pointer_routes: &HashMap<i32, PointerRoute>,
) -> bool {
    slots.values().any(|slot| {
        if !slot.active || slot.pointer_id < 0 {
            return false;
        }
        *pointer_routes
            .get(&slot.pointer_id)
            .unwrap_or(&PointerRoute::PassThrough)
            == PointerRoute::PassThrough
    })
}

fn slot_route_for_frame(
    slot_id: i32,
    frame_slot_routes: &HashMap<i32, PointerRoute>,
    slots: &HashMap<i32, SlotState>,
    pointer_routes: &HashMap<i32, PointerRoute>,
) -> PointerRoute {
    if let Some(route) = frame_slot_routes.get(&slot_id) {
        return *route;
    }

    let Some(slot) = slots.get(&slot_id) else {
        return PointerRoute::PassThrough;
    };

    if !slot.active || slot.pointer_id < 0 {
        return PointerRoute::PassThrough;
    }

    *pointer_routes
        .get(&slot.pointer_id)
        .unwrap_or(&PointerRoute::PassThrough)
}

fn is_mt_axis(code: u16) -> bool {
    code >= ABS_MT_SLOT
}

fn is_touch_related_key(key: Key) -> bool {
    matches!(
        key,
        Key::BTN_TOUCH
            | Key::BTN_TOOL_FINGER
            | Key::BTN_TOOL_DOUBLETAP
            | Key::BTN_TOOL_TRIPLETAP
            | Key::BTN_TOOL_QUADTAP
            | Key::BTN_TOOL_QUINTTAP
            | Key::BTN_TOOL_PEN
            | Key::BTN_TOOL_RUBBER
            | Key::BTN_TOOL_BRUSH
            | Key::BTN_TOOL_PENCIL
            | Key::BTN_TOOL_AIRBRUSH
            | Key::BTN_TOOL_MOUSE
            | Key::BTN_TOOL_LENS
    )
}
