use std::collections::HashMap;
use std::sync::Arc;

use super::{PointerRoute, TOUCH_PACKET_DOWN, TOUCH_PACKET_MOVE, TOUCH_PACKET_UP};

pub(super) fn resolve_pointer_route<S>(
    kind: u8,
    pointer_id: i32,
    x: f32,
    y: f32,
    snapshot: S,
    hit_test: &Arc<dyn Fn(S, f32, f32) -> bool + Send + Sync>,
    pointer_routes: &mut HashMap<i32, PointerRoute>,
) -> PointerRoute
where
    S: Copy,
{
    match kind {
        TOUCH_PACKET_DOWN => {
            let route = if hit_test(snapshot, x, y) {
                PointerRoute::Ui
            } else {
                PointerRoute::PassThrough
            };
            pointer_routes.insert(pointer_id, route);
            route
        }
        TOUCH_PACKET_MOVE => *pointer_routes
            .get(&pointer_id)
            .unwrap_or(&PointerRoute::PassThrough),
        TOUCH_PACKET_UP => {
            let route = *pointer_routes
                .get(&pointer_id)
                .unwrap_or(&PointerRoute::PassThrough);
            pointer_routes.remove(&pointer_id);
            route
        }
        _ => PointerRoute::PassThrough,
    }
}
