use super::{TouchPhase, TOUCH_PACKET_DOWN, TOUCH_PACKET_MOVE, TOUCH_PACKET_UP};
use crate::client::TouchEvent;

pub(super) fn to_client_touch_event(
    kind: u8,
    pointer_id: i32,
    x: f32,
    y: f32,
) -> Option<TouchEvent> {
    if pointer_id < 0 {
        return None;
    }

    let id = pointer_id as u32;
    let sx = x.max(0.0).round() as u32;
    let sy = y.max(0.0).round() as u32;

    match kind {
        TOUCH_PACKET_DOWN => Some(TouchEvent::Down { id, x: sx, y: sy }),
        TOUCH_PACKET_MOVE => Some(TouchEvent::Move { id, x: sx, y: sy }),
        TOUCH_PACKET_UP => Some(TouchEvent::Up { id }),
        _ => None,
    }
}

pub(super) fn kind_to_phase(kind: u8) -> Option<TouchPhase> {
    match kind {
        TOUCH_PACKET_DOWN => Some(TouchPhase::Down),
        TOUCH_PACKET_MOVE => Some(TouchPhase::Move),
        TOUCH_PACKET_UP => Some(TouchPhase::Up),
        _ => None,
    }
}
