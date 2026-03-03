use crate::api::{
    Decision, DisplayState, RectRegion, RenderFrameChunk, RenderFrameInfo, RenderStats,
    RouteResult, Status, TouchEvent, RENDER_MAX_CHUNK_BYTES, RENDER_MAX_FRAME_BYTES,
    TOUCH_MAX_POINTERS,
};
use crate::backend::filter::{apply_filter_pipeline_rgba, FilterPipeline};
use crate::backend::vulkan::VulkanBackend;
use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPresentInfo {
    pub present_seq: u64,
    pub frame_seq: u64,
    pub width: u32,
    pub height: u32,
    pub byte_len: u32,
    pub checksum_fnv1a32: u32,
}

#[derive(Debug, Clone, Copy)]
struct ActiveTouch {
    x: f32,
    y: f32,
    routed: RouteResult,
}

#[derive(Debug, Clone)]
struct StoredRenderFrame {
    info: RenderFrameInfo,
    pixels_rgba8: Arc<[u8]>,
    origin_x: i32,
    origin_y: i32,
}

type FrameSnapshot = (RenderFrameInfo, Arc<[u8]>, i32, i32);

#[derive(Debug)]
struct RouteState {
    default_decision: Decision,
    regions: Vec<RectRegion>,
}

impl Default for RouteState {
    fn default() -> Self {
        Self {
            default_decision: Decision::Pass,
            regions: Vec::new(),
        }
    }
}

impl RouteState {
    fn route_point(&self, x: f32, y: f32) -> RouteResult {
        for region in self.regions.iter().rev() {
            if region.contains(x, y) {
                return RouteResult {
                    decision: region.decision,
                    region_id: region.region_id,
                };
            }
        }

        RouteResult {
            decision: self.default_decision,
            region_id: -1,
        }
    }
}

#[derive(Debug, Default)]
struct TouchState {
    active_touches: HashMap<i32, ActiveTouch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterRuntimeInfo {
    pub backend_kind: u32,
    pub gpu_active: bool,
}

const FILTER_BACKEND_CPU: u32 = 0;
const FILTER_BACKEND_VULKAN: u32 = 1;

#[derive(Debug, Default)]
struct RenderState {
    render: RenderStats,
    render_frame_seq: u64,
    last_render_frame: Option<StoredRenderFrame>,
    present_seq: u64,
    dump_seq: u64,
    last_present: Option<RenderPresentInfo>,
}

#[derive(Debug)]
pub struct RuntimeEngine {
    display: RwLock<DisplayState>,
    route: RwLock<RouteState>,
    touch: Mutex<TouchState>,
    render: RwLock<RenderState>,
    display_signal_seq: Mutex<u64>,
    display_signal_cv: Condvar,
    frame_signal_seq: Mutex<u64>,
    frame_signal_cv: Condvar,
    filter_pipeline: RwLock<FilterPipeline>,
    vulkan_backend: Mutex<Option<VulkanBackend>>,
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
            display: RwLock::new(DisplayState::default()),
            route: RwLock::new(RouteState::default()),
            touch: Mutex::new(TouchState::default()),
            render: RwLock::new(RenderState::default()),
            display_signal_seq: Mutex::new(0),
            display_signal_cv: Condvar::new(),
            frame_signal_seq: Mutex::new(0),
            frame_signal_cv: Condvar::new(),
            filter_pipeline: RwLock::new(FilterPipeline::default()),
            vulkan_backend: Mutex::new(init_vulkan_backend()),
            render_output_dir: render_output_dir.into(),
        }
    }

    pub fn set_display_state(&self, display: DisplayState) -> Result<(), Status> {
        display.validate()?;
        let mut guard = self.display.write().map_err(|_| Status::InternalError)?;
        let changed = *guard != display;
        *guard = display;
        drop(guard);

        if changed {
            // 显示尺寸/旋转变更后，旧帧尺寸可能与新显示不匹配，先清掉避免过渡帧拉伸。
            self.clear_render_frame();
            self.notify_display_changed();
        }
        Ok(())
    }

    pub fn display_state(&self) -> DisplayState {
        match self.display.read() {
            Ok(v) => *v,
            Err(_) => DisplayState::default(),
        }
    }

    pub fn display_signal_seq(&self) -> u64 {
        match self.display_signal_seq.lock() {
            Ok(v) => *v,
            Err(_) => 0,
        }
    }

    pub fn wait_for_display_after(
        &self,
        last_seq: u64,
        timeout_ms: u32,
    ) -> Result<Option<(u64, DisplayState)>, Status> {
        let mut seq = self
            .display_signal_seq
            .lock()
            .map_err(|_| Status::InternalError)?;
        if *seq <= last_seq {
            let timeout = Duration::from_millis(timeout_ms as u64);
            let (next, _) = self
                .display_signal_cv
                .wait_timeout_while(seq, timeout, |v| *v <= last_seq)
                .map_err(|_| Status::InternalError)?;
            seq = next;
        }

        if *seq <= last_seq {
            return Ok(None);
        }

        let next_seq = *seq;
        drop(seq);
        Ok(Some((next_seq, self.display_state())))
    }

    pub fn set_default_decision(&self, decision: Decision) {
        if let Ok(mut guard) = self.route.write() {
            guard.default_decision = decision;
        }
    }

    pub fn clear_regions(&self) {
        if let Ok(mut guard) = self.route.write() {
            guard.regions.clear();
        }
    }

    pub fn add_region_rect(&self, region: RectRegion) -> Result<(), Status> {
        region.validate()?;
        let mut guard = self.route.write().map_err(|_| Status::InternalError)?;
        guard.regions.push(region);
        Ok(())
    }

    pub fn route_point(&self, x: f32, y: f32) -> RouteResult {
        match self.route.read() {
            Ok(guard) => guard.route_point(x, y),
            Err(_) => RouteResult::passthrough(),
        }
    }

    pub fn touch_down(&self, event: TouchEvent) -> Result<RouteResult, Status> {
        event.validate()?;

        let routed = {
            let route = self.route.read().map_err(|_| Status::InternalError)?;
            route.route_point(event.x, event.y)
        };

        let mut touch = self.touch.lock().map_err(|_| Status::InternalError)?;
        if touch.active_touches.contains_key(&event.pointer_id) {
            return Err(Status::InvalidArgument);
        }
        if touch.active_touches.len() >= TOUCH_MAX_POINTERS {
            return Err(Status::OutOfRange);
        }

        touch.active_touches.insert(
            event.pointer_id,
            ActiveTouch {
                x: event.x,
                y: event.y,
                routed,
            },
        );
        Ok(routed)
    }

    pub fn touch_move(&self, event: TouchEvent) -> Result<RouteResult, Status> {
        event.validate()?;
        let mut touch = self.touch.lock().map_err(|_| Status::InternalError)?;
        let entry = touch
            .active_touches
            .get_mut(&event.pointer_id)
            .ok_or(Status::OutOfRange)?;
        entry.x = event.x;
        entry.y = event.y;
        Ok(entry.routed)
    }

    pub fn touch_up(&self, event: TouchEvent) -> Result<RouteResult, Status> {
        event.validate()?;
        let mut touch = self.touch.lock().map_err(|_| Status::InternalError)?;
        let entry = touch
            .active_touches
            .remove(&event.pointer_id)
            .ok_or(Status::OutOfRange)?;
        Ok(entry.routed)
    }

    pub fn touch_cancel(&self, pointer_id: i32) -> Result<RouteResult, Status> {
        if pointer_id < 0 {
            return Err(Status::OutOfRange);
        }
        let mut touch = self.touch.lock().map_err(|_| Status::InternalError)?;
        let entry = touch
            .active_touches
            .remove(&pointer_id)
            .ok_or(Status::OutOfRange)?;
        Ok(entry.routed)
    }

    pub fn clear_touches(&self) {
        if let Ok(mut touch) = self.touch.lock() {
            touch.active_touches.clear();
        }
    }

    pub fn active_touch_count(&self) -> usize {
        match self.touch.lock() {
            Ok(touch) => touch.active_touches.len(),
            Err(_) => 0,
        }
    }

    pub fn submit_render_stats(
        &self,
        draw_calls: u32,
        frost_passes: u32,
        text_calls: u32,
    ) -> RenderStats {
        match self.render.write() {
            Ok(mut render) => {
                render.render.frame_seq = render.render.frame_seq.saturating_add(1);
                render.render.draw_calls = draw_calls;
                render.render.frost_passes = frost_passes;
                render.render.text_calls = text_calls;
                render.render
            }
            Err(_) => RenderStats::default(),
        }
    }

    pub fn render_stats(&self) -> RenderStats {
        match self.render.read() {
            Ok(render) => render.render,
            Err(_) => RenderStats::default(),
        }
    }

    pub fn set_filter_pipeline(&self, pipeline: FilterPipeline) -> Result<(), Status> {
        let mut guard = self
            .filter_pipeline
            .write()
            .map_err(|_| Status::InternalError)?;
        *guard = pipeline;
        Ok(())
    }

    pub fn clear_filter_pipeline(&self) -> Result<(), Status> {
        self.set_filter_pipeline(FilterPipeline::default())
    }

    pub fn filter_state_snapshot(&self) -> Result<(FilterRuntimeInfo, FilterPipeline), Status> {
        let pipeline = self
            .filter_pipeline
            .read()
            .map_err(|_| Status::InternalError)?
            .clone();
        let info = self.filter_runtime_info();
        Ok((info, pipeline))
    }

    pub fn filter_runtime_info(&self) -> FilterRuntimeInfo {
        match self.vulkan_backend.lock() {
            Ok(guard) => {
                if let Some(vk) = guard.as_ref() {
                    FilterRuntimeInfo {
                        backend_kind: FILTER_BACKEND_VULKAN,
                        gpu_active: vk.gpu_path_active(),
                    }
                } else {
                    FilterRuntimeInfo {
                        backend_kind: FILTER_BACKEND_CPU,
                        gpu_active: false,
                    }
                }
            }
            Err(_) => FilterRuntimeInfo {
                backend_kind: FILTER_BACKEND_CPU,
                gpu_active: false,
            },
        }
    }

    pub fn submit_render_frame_rgba(
        &self,
        width: u32,
        height: u32,
        pixels_rgba8: &[u8],
    ) -> Result<RenderFrameInfo, Status> {
        self.submit_render_frame_rgba_at(width, height, pixels_rgba8, 0, 0)
    }

    pub fn submit_render_frame_rgba_at(
        &self,
        width: u32,
        height: u32,
        pixels_rgba8: &[u8],
        origin_x: i32,
        origin_y: i32,
    ) -> Result<RenderFrameInfo, Status> {
        if width == 0 || height == 0 {
            return Err(Status::InvalidArgument);
        }

        let expected_len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4usize))
            .ok_or(Status::OutOfRange)?;
        if expected_len > RENDER_MAX_FRAME_BYTES {
            return Err(Status::OutOfRange);
        }
        if pixels_rgba8.len() != expected_len {
            return Err(Status::InvalidArgument);
        }

        let mut pixels_rgba8 = pixels_rgba8.to_vec();
        self.apply_filter_pipeline(width, height, &mut pixels_rgba8)?;

        let checksum = compute_render_checksum(&pixels_rgba8);
        let pixels_rgba8: Arc<[u8]> = pixels_rgba8.into();

        let info = {
            let mut render = self.render.write().map_err(|_| Status::InternalError)?;
            render.render_frame_seq = render.render_frame_seq.saturating_add(1);
            let info = RenderFrameInfo {
                frame_seq: render.render_frame_seq,
                width,
                height,
                byte_len: expected_len as u32,
                checksum_fnv1a32: checksum,
            };
            render.last_render_frame = Some(StoredRenderFrame {
                info,
                pixels_rgba8,
                origin_x,
                origin_y,
            });
            info
        };

        self.notify_new_frame(info.frame_seq);
        Ok(info)
    }

    pub fn wait_for_frame_after(
        &self,
        last_frame_seq: u64,
        timeout_ms: u32,
    ) -> Result<Option<RenderFrameInfo>, Status> {
        if let Some(info) = self.render_frame_info() {
            if info.frame_seq > last_frame_seq {
                return Ok(Some(info));
            }
        }

        let timeout = Duration::from_millis(timeout_ms as u64);
        let mut seq = self
            .frame_signal_seq
            .lock()
            .map_err(|_| Status::InternalError)?;
        if *seq <= last_frame_seq {
            let (next, _) = self
                .frame_signal_cv
                .wait_timeout_while(seq, timeout, |v| *v <= last_frame_seq)
                .map_err(|_| Status::InternalError)?;
            seq = next;
        }
        drop(seq);

        let frame = self.render_frame_info();
        Ok(frame.filter(|v| v.frame_seq > last_frame_seq))
    }

    pub fn wait_for_frame_after_and_present(
        &self,
        last_frame_seq: u64,
        timeout_ms: u32,
    ) -> Result<Option<FrameSnapshot>, Status> {
        if let Some(snapshot) = self.present_latest_if_newer(last_frame_seq)? {
            return Ok(Some(snapshot));
        }

        let timeout = Duration::from_millis(timeout_ms as u64);
        let mut seq = self
            .frame_signal_seq
            .lock()
            .map_err(|_| Status::InternalError)?;
        if *seq <= last_frame_seq {
            let (next, _) = self
                .frame_signal_cv
                .wait_timeout_while(seq, timeout, |v| *v <= last_frame_seq)
                .map_err(|_| Status::InternalError)?;
            seq = next;
        }
        drop(seq);

        self.present_latest_if_newer(last_frame_seq)
    }

    pub fn render_frame_info(&self) -> Option<RenderFrameInfo> {
        self.render
            .read()
            .ok()
            .and_then(|r| r.last_render_frame.as_ref().map(|v| v.info))
    }

    pub fn clear_render_frame(&self) {
        if let Ok(mut render) = self.render.write() {
            render.last_render_frame = None;
        }
    }

    pub fn render_frame_byte_len(&self) -> Option<usize> {
        self.render
            .read()
            .ok()
            .and_then(|r| r.last_render_frame.as_ref().map(|f| f.pixels_rgba8.len()))
    }

    pub fn render_frame_snapshot(&self) -> Option<(RenderFrameInfo, Arc<[u8]>)> {
        self.render.read().ok().and_then(|r| {
            r.last_render_frame
                .as_ref()
                .map(|f| (f.info, f.pixels_rgba8.clone()))
        })
    }

    pub fn render_frame_origin(&self) -> Option<(i32, i32)> {
        self.render.read().ok().and_then(|r| {
            r.last_render_frame
                .as_ref()
                .map(|f| (f.origin_x, f.origin_y))
        })
    }

    pub fn render_frame_read_chunk(
        &self,
        offset: usize,
        max_bytes: usize,
    ) -> Result<RenderFrameChunk, Status> {
        if max_bytes == 0 || max_bytes > RENDER_MAX_CHUNK_BYTES {
            return Err(Status::OutOfRange);
        }

        let render = self.render.read().map_err(|_| Status::InternalError)?;
        let frame = render
            .last_render_frame
            .as_ref()
            .ok_or(Status::OutOfRange)?;
        let total = frame.pixels_rgba8.len();
        if offset >= total {
            return Err(Status::OutOfRange);
        }
        let end = offset.saturating_add(max_bytes).min(total);
        let bytes = frame.pixels_rgba8[offset..end].to_vec();

        Ok(RenderFrameChunk {
            frame_seq: frame.info.frame_seq,
            total_bytes: frame.info.byte_len,
            offset: offset as u32,
            chunk_bytes: bytes,
        })
    }

    pub fn render_present(&self) -> Result<RenderPresentInfo, Status> {
        let mut render = self.render.write().map_err(|_| Status::InternalError)?;
        let frame_info = render
            .last_render_frame
            .as_ref()
            .map(|f| f.info)
            .ok_or(Status::OutOfRange)?;
        render.present_seq = render.present_seq.saturating_add(1);

        let info = RenderPresentInfo {
            present_seq: render.present_seq,
            frame_seq: frame_info.frame_seq,
            width: frame_info.width,
            height: frame_info.height,
            byte_len: frame_info.byte_len,
            checksum_fnv1a32: frame_info.checksum_fnv1a32,
        };
        render.last_present = Some(info.clone());
        Ok(info)
    }

    pub fn render_present_get(&self) -> Option<RenderPresentInfo> {
        self.render.read().ok().and_then(|r| r.last_present.clone())
    }

    pub fn render_dump_ppm(&self) -> Result<String, Status> {
        let (frame_info, pixels, dump_seq) = {
            let mut render = self.render.write().map_err(|_| Status::InternalError)?;
            let (frame_info, pixels) = {
                let frame = render
                    .last_render_frame
                    .as_ref()
                    .ok_or(Status::OutOfRange)?;
                (frame.info, frame.pixels_rgba8.clone())
            };
            render.dump_seq = render.dump_seq.saturating_add(1);
            (frame_info, pixels, render.dump_seq)
        };

        create_dir_all(&self.render_output_dir).map_err(|_| Status::InternalError)?;

        let file_name = format!(
            "frame_dump_{:08}_frame_{:08}.ppm",
            dump_seq, frame_info.frame_seq
        );
        let path = self.render_output_dir.join(file_name);
        write_ppm_rgba(
            path.as_path(),
            frame_info.width,
            frame_info.height,
            pixels.as_ref(),
        )
        .map_err(|_| Status::InternalError)?;
        Ok(path.display().to_string())
    }

    fn apply_filter_pipeline(
        &self,
        width: u32,
        height: u32,
        pixels_rgba8: &mut [u8],
    ) -> Result<(), Status> {
        let pipeline = self
            .filter_pipeline
            .read()
            .map_err(|_| Status::InternalError)?
            .clone();
        if pipeline.passes.is_empty() {
            return Ok(());
        }

        let mut backend = self
            .vulkan_backend
            .lock()
            .map_err(|_| Status::InternalError)?;
        if let Some(vk) = backend.as_mut() {
            vk.set_filter_pipeline(pipeline.clone());
            if vk.process_frame_rgba(width, height, pixels_rgba8).is_ok() {
                return Ok(());
            }
            // Vulkan 后端异常时回退 CPU 路径，避免影响主渲染链路稳定性。
            *backend = None;
        }

        let _report = apply_filter_pipeline_rgba(&pipeline, width, height, pixels_rgba8)?;
        Ok(())
    }

    fn notify_new_frame(&self, frame_seq: u64) {
        if let Ok(mut seq) = self.frame_signal_seq.lock() {
            *seq = frame_seq;
            self.frame_signal_cv.notify_all();
        }
    }

    fn present_latest_if_newer(
        &self,
        last_frame_seq: u64,
    ) -> Result<Option<FrameSnapshot>, Status> {
        let mut render = self.render.write().map_err(|_| Status::InternalError)?;
        let (frame_info, pixels, origin_x, origin_y) = {
            let Some(frame) = render.last_render_frame.as_ref() else {
                return Ok(None);
            };
            if frame.info.frame_seq <= last_frame_seq {
                return Ok(None);
            }
            (
                frame.info,
                frame.pixels_rgba8.clone(),
                frame.origin_x,
                frame.origin_y,
            )
        };

        render.present_seq = render.present_seq.saturating_add(1);
        render.last_present = Some(RenderPresentInfo {
            present_seq: render.present_seq,
            frame_seq: frame_info.frame_seq,
            width: frame_info.width,
            height: frame_info.height,
            byte_len: frame_info.byte_len,
            checksum_fnv1a32: frame_info.checksum_fnv1a32,
        });
        Ok(Some((frame_info, pixels, origin_x, origin_y)))
    }

    fn notify_display_changed(&self) {
        if let Ok(mut seq) = self.display_signal_seq.lock() {
            *seq = seq.saturating_add(1);
            self.display_signal_cv.notify_all();
        }
    }
}

fn init_vulkan_backend() -> Option<VulkanBackend> {
    let mode = std::env::var("DSAPI_FILTER_BACKEND").unwrap_or_else(|_| "auto".to_string());
    if mode.eq_ignore_ascii_case("cpu") {
        return None;
    }
    VulkanBackend::new().ok()
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

fn fnv1a32(data: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for b in data {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(0x01000193u32);
    }
    hash
}

fn render_checksum_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let raw = std::env::var("DSAPI_RENDER_FRAME_CHECKSUM").unwrap_or_else(|_| "1".to_string());
        !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        )
    })
}

fn compute_render_checksum(data: &[u8]) -> u32 {
    if render_checksum_enabled() {
        fnv1a32(data)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_prefers_last_added_region() {
        let engine = RuntimeEngine::default();
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
        let engine = RuntimeEngine::default();
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
        let engine = RuntimeEngine::default();
        let res = engine.touch_move(TouchEvent {
            pointer_id: 3,
            x: 1.0,
            y: 1.0,
        });
        assert_eq!(res, Err(Status::OutOfRange));
    }

    #[test]
    fn render_submit_increments_frame_seq_and_updates_stats() {
        let engine = RuntimeEngine::default();

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
        let engine = RuntimeEngine::default();
        let pixels = vec![
            255u8, 0u8, 0u8, 255u8, 0u8, 255u8, 0u8, 255u8, 0u8, 0u8, 255u8, 255u8, 255u8, 255u8,
            255u8, 255u8,
        ];
        let first = engine
            .submit_render_frame_rgba(2, 2, &pixels)
            .expect("submit first frame");
        assert_eq!(first.frame_seq, 1);
        assert_eq!(first.width, 2);
        assert_eq!(first.height, 2);
        assert_eq!(first.byte_len, 16);
        assert_ne!(first.checksum_fnv1a32, 0);
        assert_eq!(engine.render_frame_byte_len(), Some(16));

        let second = engine
            .submit_render_frame_rgba(2, 2, &pixels)
            .expect("submit second frame");
        assert_eq!(second.frame_seq, 2);

        let got = engine.render_frame_info().expect("frame info should exist");
        assert_eq!(got.frame_seq, 2);

        engine.clear_render_frame();
        assert_eq!(engine.render_frame_info(), None);
        assert_eq!(engine.render_frame_byte_len(), None);
    }

    #[test]
    fn render_frame_submit_applies_gaussian_blur_pipeline() {
        use crate::backend::filter::{FilterPass, GaussianBlurPass};

        let engine = RuntimeEngine::default();
        engine
            .set_filter_pipeline(FilterPipeline {
                passes: vec![FilterPass::GaussianBlur(GaussianBlurPass {
                    radius: 1,
                    sigma: 1.0,
                })],
            })
            .expect("set filter pipeline");

        let mut pixels = vec![0u8; 5 * 5 * 4];
        let center = ((2usize * 5usize) + 2usize) * 4usize;
        pixels[center] = 255;
        pixels[center + 1] = 255;
        pixels[center + 2] = 255;
        pixels[center + 3] = 255;

        engine
            .submit_render_frame_rgba(5, 5, &pixels)
            .expect("submit filtered frame");
        let (_, output) = engine
            .render_frame_snapshot()
            .expect("render frame snapshot should exist");

        let center_after = output[center];
        let left = ((2usize * 5usize) + 1usize) * 4usize;
        assert!(center_after < 255);
        assert!(output[left] > 0);
    }

    #[test]
    fn render_frame_rejects_invalid_length() {
        let engine = RuntimeEngine::default();
        let pixels = [1u8; 15];
        let out = engine.submit_render_frame_rgba(2, 2, &pixels);
        assert_eq!(out, Err(Status::InvalidArgument));
    }

    #[test]
    fn render_present_updates_metadata_without_disk_io() {
        let engine = RuntimeEngine::new_with_render_output_dir("artifacts/test_render_present");
        let pixels = vec![
            255u8, 0u8, 0u8, 255u8, 0u8, 255u8, 0u8, 255u8, 0u8, 0u8, 255u8, 255u8, 255u8, 255u8,
            255u8, 255u8,
        ];
        let frame = engine
            .submit_render_frame_rgba(2, 2, &pixels)
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
        let engine = RuntimeEngine::default();
        let out = engine.render_present();
        assert_eq!(out, Err(Status::OutOfRange));
    }

    #[test]
    fn render_dump_ppm_writes_file() {
        let engine = RuntimeEngine::new_with_render_output_dir("artifacts/test_render_dump");
        let pixels = vec![
            255u8, 0u8, 0u8, 255u8, 0u8, 255u8, 0u8, 255u8, 0u8, 0u8, 255u8, 255u8, 255u8, 255u8,
            255u8, 255u8,
        ];
        engine
            .submit_render_frame_rgba(2, 2, &pixels)
            .expect("submit frame");
        let path = engine.render_dump_ppm().expect("dump ppm");
        assert!(std::path::Path::new(&path).exists());
    }

    #[test]
    fn render_frame_read_chunk_reads_partial_bytes() {
        let engine = RuntimeEngine::default();
        let pixels = (0u8..16u8).collect::<Vec<u8>>();
        let frame = engine
            .submit_render_frame_rgba(2, 2, &pixels)
            .expect("submit frame");

        let a = engine
            .render_frame_read_chunk(0, 6)
            .expect("read first chunk");
        assert_eq!(a.frame_seq, frame.frame_seq);
        assert_eq!(a.total_bytes, 16);
        assert_eq!(a.offset, 0);
        assert_eq!(a.chunk_bytes, pixels[0..6].to_vec());

        let b = engine
            .render_frame_read_chunk(6, 6)
            .expect("read second chunk");
        assert_eq!(b.offset, 6);
        assert_eq!(b.chunk_bytes, pixels[6..12].to_vec());

        let c = engine
            .render_frame_read_chunk(12, 16)
            .expect("read last chunk");
        assert_eq!(c.offset, 12);
        assert_eq!(c.chunk_bytes, pixels[12..16].to_vec());
    }

    #[test]
    fn render_frame_read_chunk_rejects_invalid_range() {
        let engine = RuntimeEngine::default();
        let pixels = vec![1u8; 16];
        engine
            .submit_render_frame_rgba(2, 2, &pixels)
            .expect("submit frame");
        assert_eq!(
            engine.render_frame_read_chunk(16, 1),
            Err(Status::OutOfRange)
        );
        assert_eq!(
            engine.render_frame_read_chunk(0, 0),
            Err(Status::OutOfRange)
        );
    }

    #[test]
    fn wait_for_frame_wakes_on_new_frame() {
        let engine = RuntimeEngine::default();
        assert_eq!(
            engine
                .wait_for_frame_after(0, 1)
                .expect("wait without frame"),
            None
        );

        let pixels = vec![255u8; 16];
        let frame = engine
            .submit_render_frame_rgba(2, 2, &pixels)
            .expect("submit frame");
        let waited = engine
            .wait_for_frame_after(0, 10)
            .expect("wait with frame")
            .expect("new frame expected");
        assert_eq!(waited.frame_seq, frame.frame_seq);
    }

    #[test]
    fn wait_for_frame_after_and_present_keeps_frame_consistent() {
        let engine = RuntimeEngine::default();
        assert_eq!(
            engine
                .wait_for_frame_after_and_present(0, 1)
                .expect("wait without frame"),
            None
        );

        let pixels = vec![9u8; 16];
        let frame = engine
            .submit_render_frame_rgba(2, 2, &pixels)
            .expect("submit frame");

        let (waited_frame, waited_pixels, waited_origin_x, waited_origin_y) = engine
            .wait_for_frame_after_and_present(0, 10)
            .expect("wait with frame")
            .expect("frame expected");
        assert_eq!(waited_frame.frame_seq, frame.frame_seq);
        assert_eq!(waited_pixels.len(), frame.byte_len as usize);
        assert_eq!(waited_origin_x, 0);
        assert_eq!(waited_origin_y, 0);

        let present = engine.render_present_get().expect("present state exists");
        assert_eq!(present.frame_seq, waited_frame.frame_seq);
    }

    #[test]
    fn wait_for_display_times_out_without_change() {
        let engine = RuntimeEngine::default();
        assert_eq!(
            engine
                .wait_for_display_after(0, 1)
                .expect("wait for display"),
            None
        );
    }

    #[test]
    fn wait_for_display_wakes_on_change() {
        let engine = RuntimeEngine::default();
        let initial_seq = engine.display_signal_seq();

        engine
            .set_display_state(DisplayState {
                width: 1200,
                height: 2400,
                refresh_hz: 120.0,
                density_dpi: 420,
                rotation: 1,
            })
            .expect("set display");

        let (seq, display) = engine
            .wait_for_display_after(initial_seq, 10)
            .expect("wait for display")
            .expect("display change expected");
        assert!(seq > initial_seq);
        assert_eq!(display.width, 1200);
        assert_eq!(display.height, 2400);
        assert_eq!(display.rotation, 1);
    }

    #[test]
    fn display_change_clears_cached_render_frame() {
        let engine = RuntimeEngine::default();
        let pixels = vec![255u8; 16];
        engine
            .submit_render_frame_rgba(2, 2, &pixels)
            .expect("submit frame");
        assert!(engine.render_frame_info().is_some());

        engine
            .set_display_state(DisplayState {
                width: 1200,
                height: 2400,
                refresh_hz: 120.0,
                density_dpi: 420,
                rotation: 1,
            })
            .expect("set display");

        assert_eq!(engine.render_frame_info(), None);
    }
}
