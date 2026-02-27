use crate::api::{Decision, DisplayState, RectRegion, RouteResult, Status, TouchEvent};
use crate::domain::RuntimeState;

#[derive(Debug, Default)]
pub struct RuntimeEngine {
    state: RuntimeState,
}

impl RuntimeEngine {
    pub fn set_display_state(&mut self, display: DisplayState) -> Result<(), Status> {
        self.state.set_display(display)
    }

    pub fn display_state(&self) -> DisplayState {
        self.state.display
    }

    pub fn set_default_decision(&mut self, decision: Decision) {
        self.state.default_decision = decision;
    }

    pub fn clear_regions(&mut self) {
        self.state.clear_regions();
    }

    pub fn add_region_rect(&mut self, region: RectRegion) -> Result<(), Status> {
        self.state.add_rect_region(region)
    }

    pub fn route_point(&self, x: f32, y: f32) -> RouteResult {
        self.state.route_point(x, y)
    }

    pub fn touch_down(&mut self, event: TouchEvent) -> Result<RouteResult, Status> {
        self.state.touch_down(event)
    }

    pub fn touch_move(&mut self, event: TouchEvent) -> Result<RouteResult, Status> {
        self.state.touch_move(event)
    }

    pub fn touch_up(&mut self, event: TouchEvent) -> Result<RouteResult, Status> {
        self.state.touch_up(event)
    }

    pub fn touch_cancel(&mut self, pointer_id: i32) -> Result<RouteResult, Status> {
        self.state.touch_cancel(pointer_id)
    }

    pub fn clear_touches(&mut self) {
        self.state.clear_touches();
    }

    pub fn active_touch_count(&self) -> usize {
        self.state.active_touch_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_prefers_last_added_region() {
        let mut engine = RuntimeEngine::default();
        engine
            .add_region_rect(RectRegion {
                region_id: 1,
                decision: Decision::Pass,
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            })
            .expect("region 1");
        engine
            .add_region_rect(RectRegion {
                region_id: 2,
                decision: Decision::Block,
                x: 10.0,
                y: 10.0,
                w: 20.0,
                h: 20.0,
            })
            .expect("region 2");

        let res = engine.route_point(15.0, 15.0);
        assert_eq!(res.decision, Decision::Block);
        assert_eq!(res.region_id, 2);
    }

    #[test]
    fn touch_stream_locks_decision_until_up() {
        let mut engine = RuntimeEngine::default();
        engine
            .add_region_rect(RectRegion {
                region_id: 9,
                decision: Decision::Block,
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            })
            .expect("region");

        let down = engine
            .touch_down(TouchEvent {
                pointer_id: 1,
                x: 10.0,
                y: 10.0,
            })
            .expect("touch down");
        assert_eq!(down.decision, Decision::Block);
        assert_eq!(down.region_id, 9);

        engine.clear_regions();
        engine
            .add_region_rect(RectRegion {
                region_id: 12,
                decision: Decision::Pass,
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            })
            .expect("region2");

        let moved = engine
            .touch_move(TouchEvent {
                pointer_id: 1,
                x: 20.0,
                y: 20.0,
            })
            .expect("touch move");
        assert_eq!(moved.decision, Decision::Block);
        assert_eq!(moved.region_id, 9);

        let up = engine
            .touch_up(TouchEvent {
                pointer_id: 1,
                x: 25.0,
                y: 25.0,
            })
            .expect("touch up");
        assert_eq!(up.decision, Decision::Block);
        assert_eq!(up.region_id, 9);
        assert_eq!(engine.active_touch_count(), 0);
    }

    #[test]
    fn touch_move_unknown_pointer_returns_error() {
        let mut engine = RuntimeEngine::default();
        let res = engine.touch_move(TouchEvent {
            pointer_id: 3,
            x: 1.0,
            y: 1.0,
        });
        assert_eq!(res, Err(Status::OutOfRange));
    }
}
