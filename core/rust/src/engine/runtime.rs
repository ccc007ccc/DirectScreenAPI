use crate::api::{
    Decision, DisplayState, RectRegion, RenderFrameInfo, RenderStats, RouteResult, Status,
    TouchEvent,
};
use crate::domain::RuntimeState;
use std::fs::{create_dir_all, File};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPresentInfo {
    pub present_seq: u64,
    pub frame_seq: u64,
    pub width: u32,
    pub height: u32,
    pub byte_len: u32,
    pub checksum_fnv1a32: u32,
}

#[derive(Debug)]
pub struct RuntimeEngine {
    state: RuntimeState,
    present_seq: u64,
    dump_seq: u64,
    last_present: Option<RenderPresentInfo>,
    render_output_dir: PathBuf,
}

impl Default for RuntimeEngine {
    fn default() -> Self {
        Self::new_with_render_output_dir("artifacts/render")
    }
}

impl RuntimeEngine {
    pub fn new_with_render_output_dir(render_output_dir: impl Into<PathBuf>) -> Self {
        Self {
            state: RuntimeState::default(),
            present_seq: 0,
            dump_seq: 0,
            last_present: None,
            render_output_dir: render_output_dir.into(),
        }
    }

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

    pub fn render_present(&mut self) -> Result<RenderPresentInfo, Status> {
        let (frame_info, _pixels) = self
            .state
            .render_frame_snapshot()
            .ok_or(Status::OutOfRange)?;
        self.present_seq = self.present_seq.saturating_add(1);

        let info = RenderPresentInfo {
            present_seq: self.present_seq,
            frame_seq: frame_info.frame_seq,
            width: frame_info.width,
            height: frame_info.height,
            byte_len: frame_info.byte_len,
            checksum_fnv1a32: frame_info.checksum_fnv1a32,
        };
        self.last_present = Some(info.clone());
        Ok(info)
    }

    pub fn render_present_get(&self) -> Option<RenderPresentInfo> {
        self.last_present.clone()
    }

    pub fn render_dump_ppm(&mut self) -> Result<String, Status> {
        let (frame_info, pixels) = self
            .state
            .render_frame_snapshot()
            .ok_or(Status::OutOfRange)?;
        self.dump_seq = self.dump_seq.saturating_add(1);
        create_dir_all(&self.render_output_dir).map_err(|_| Status::InternalError)?;

        let file_name = format!(
            "frame_dump_{:08}_frame_{:08}.ppm",
            self.dump_seq, frame_info.frame_seq
        );
        let path = self.render_output_dir.join(file_name);
        write_ppm_rgba(path.as_path(), frame_info.width, frame_info.height, &pixels)
            .map_err(|_| Status::InternalError)?;
        Ok(path.display().to_string())
    }
}

fn write_ppm_rgba(path: &Path, width: u32, height: u32, rgba8: &[u8]) -> std::io::Result<()> {
    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4usize))
        .ok_or_else(|| std::io::Error::other("ppm_size_overflow"))?;
    if rgba8.len() != expected_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "ppm_input_len_mismatch",
        ));
    }

    let mut f = File::create(path)?;
    let header = format!("P6\n{} {}\n255\n", width, height);
    f.write_all(header.as_bytes())?;

    let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3usize);
    for px in rgba8.chunks_exact(4) {
        rgb.push(px[0]);
        rgb.push(px[1]);
        rgb.push(px[2]);
    }
    f.write_all(&rgb)?;
    Ok(())
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

    #[test]
    fn render_present_updates_metadata_without_disk_io() {
        let mut engine = RuntimeEngine::new_with_render_output_dir("artifacts/test_render_present");
        let pixels = vec![
            255u8, 0u8, 0u8, 255u8, 0u8, 255u8, 0u8, 255u8, 0u8, 0u8, 255u8, 255u8, 255u8, 255u8,
            255u8, 255u8,
        ];
        let frame = engine
            .submit_render_frame_rgba(2, 2, pixels)
            .expect("submit frame");
        let presented = engine.render_present().expect("present frame");
        assert_eq!(presented.present_seq, 1);
        assert_eq!(presented.frame_seq, frame.frame_seq);
        assert_eq!(presented.width, 2);
        assert_eq!(presented.height, 2);
        assert_eq!(presented.byte_len, 16);

        let got = engine
            .render_present_get()
            .expect("present state should exist");
        assert_eq!(got, presented);
    }

    #[test]
    fn render_present_without_frame_returns_out_of_range() {
        let mut engine = RuntimeEngine::default();
        let out = engine.render_present();
        assert_eq!(out, Err(Status::OutOfRange));
    }

    #[test]
    fn render_dump_ppm_writes_file() {
        let mut engine = RuntimeEngine::new_with_render_output_dir("artifacts/test_render_dump");
        let pixels = vec![
            255u8, 0u8, 0u8, 255u8, 0u8, 255u8, 0u8, 255u8, 0u8, 0u8, 255u8, 255u8, 255u8, 255u8,
            255u8, 255u8,
        ];
        engine
            .submit_render_frame_rgba(2, 2, pixels)
            .expect("submit frame");
        let path = engine.render_dump_ppm().expect("dump ppm");
        assert!(std::path::Path::new(&path).exists());
    }
}
