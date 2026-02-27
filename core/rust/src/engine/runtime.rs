use crate::api::{
    Decision, DisplayState, RectRegion, RenderFrameInfo, RenderStats, RouteResult, Status,
    TouchEvent,
};
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

    pub fn submit_render_stats(
        &mut self,
        draw_calls: u32,
        frost_passes: u32,
        text_calls: u32,
    ) -> RenderStats {
        self.state
            .submit_render_stats(draw_calls, frost_passes, text_calls)
    }

    pub fn render_stats(&self) -> RenderStats {
        self.state.render_stats()
    }

    pub fn submit_render_frame_rgba(
        &mut self,
        width: u32,
        height: u32,
        pixels_rgba8: Vec<u8>,
    ) -> Result<RenderFrameInfo, Status> {
        self.state
            .submit_render_frame_rgba(width, height, pixels_rgba8)
    }

    pub fn render_frame_info(&self) -> Option<RenderFrameInfo> {
        self.state.render_frame_info()
    }

    pub fn clear_render_frame(&mut self) {
        self.state.clear_render_frame();
    }

    pub fn render_frame_byte_len(&self) -> Option<usize> {
        self.state.render_frame_byte_len()
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

    #[test]
    fn render_submit_increments_frame_seq_and_updates_stats() {
        let mut engine = RuntimeEngine::default();

        let first = engine.submit_render_stats(7, 1, 2);
        assert_eq!(first.frame_seq, 1);
        assert_eq!(first.draw_calls, 7);
        assert_eq!(first.frost_passes, 1);
        assert_eq!(first.text_calls, 2);

        let second = engine.submit_render_stats(12, 3, 4);
        assert_eq!(second.frame_seq, 2);
        assert_eq!(second.draw_calls, 12);
        assert_eq!(second.frost_passes, 3);
        assert_eq!(second.text_calls, 4);

        let got = engine.render_stats();
        assert_eq!(got, second);
    }

    #[test]
    fn render_frame_submit_get_and_clear() {
        let mut engine = RuntimeEngine::default();
        let pixels = vec![
            255u8, 0u8, 0u8, 255u8, 0u8, 255u8, 0u8, 255u8, 0u8, 0u8, 255u8, 255u8, 255u8, 255u8,
            255u8, 255u8,
        ];
        let first = engine
            .submit_render_frame_rgba(2, 2, pixels.clone())
            .expect("submit first frame");
        assert_eq!(first.frame_seq, 1);
        assert_eq!(first.width, 2);
        assert_eq!(first.height, 2);
        assert_eq!(first.byte_len, 16);
        assert_ne!(first.checksum_fnv1a32, 0);
        assert_eq!(engine.render_frame_byte_len(), Some(16));

        let second = engine
            .submit_render_frame_rgba(2, 2, pixels)
            .expect("submit second frame");
        assert_eq!(second.frame_seq, 2);

        let got = engine.render_frame_info().expect("frame info should exist");
        assert_eq!(got.frame_seq, 2);

        engine.clear_render_frame();
        assert_eq!(engine.render_frame_info(), None);
        assert_eq!(engine.render_frame_byte_len(), None);
    }

    #[test]
    fn render_frame_rejects_invalid_length() {
        let mut engine = RuntimeEngine::default();
        let out = engine.submit_render_frame_rgba(2, 2, vec![1u8; 15]);
        assert_eq!(out, Err(Status::InvalidArgument));
    }
}
