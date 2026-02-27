use std::collections::HashMap;

use crate::api::{
    Decision, DisplayState, RectRegion, RenderFrameChunk, RenderFrameInfo, RenderStats,
    RouteResult, Status, TouchEvent, RENDER_MAX_CHUNK_BYTES, RENDER_MAX_FRAME_BYTES,
    TOUCH_MAX_POINTERS,
};

#[derive(Debug, Clone, Copy)]
struct ActiveTouch {
    x: f32,
    y: f32,
    routed: RouteResult,
}

#[derive(Debug, Clone)]
struct StoredRenderFrame {
    info: RenderFrameInfo,
    pixels_rgba8: Vec<u8>,
}

#[derive(Debug)]
pub struct RuntimeState {
    pub display: DisplayState,
    pub render: RenderStats,
    render_frame_seq: u64,
    pub default_decision: Decision,
    pub regions: Vec<RectRegion>,
    last_render_frame: Option<StoredRenderFrame>,
    active_touches: HashMap<i32, ActiveTouch>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            display: DisplayState::default(),
            render: RenderStats::default(),
            render_frame_seq: 0,
            default_decision: Decision::Pass,
            regions: Vec::new(),
            last_render_frame: None,
            active_touches: HashMap::new(),
        }
    }
}

impl RuntimeState {
    pub fn set_display(&mut self, display: DisplayState) -> Result<(), Status> {
        display.validate()?;
        self.display = display;
        Ok(())
    }

    pub fn clear_regions(&mut self) {
        self.regions.clear();
    }

    pub fn clear_touches(&mut self) {
        self.active_touches.clear();
    }

    pub fn add_rect_region(&mut self, region: RectRegion) -> Result<(), Status> {
        region.validate()?;
        self.regions.push(region);
        Ok(())
    }

    pub fn route_point(&self, x: f32, y: f32) -> RouteResult {
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

    pub fn touch_down(&mut self, event: TouchEvent) -> Result<RouteResult, Status> {
        event.validate()?;

        if self.active_touches.contains_key(&event.pointer_id) {
            return Err(Status::InvalidArgument);
        }
        if self.active_touches.len() >= TOUCH_MAX_POINTERS {
            return Err(Status::OutOfRange);
        }

        let routed = self.route_point(event.x, event.y);
        self.active_touches.insert(
            event.pointer_id,
            ActiveTouch {
                x: event.x,
                y: event.y,
                routed,
            },
        );
        Ok(routed)
    }

    pub fn touch_move(&mut self, event: TouchEvent) -> Result<RouteResult, Status> {
        event.validate()?;
        let touch = self
            .active_touches
            .get_mut(&event.pointer_id)
            .ok_or(Status::OutOfRange)?;

        touch.x = event.x;
        touch.y = event.y;
        Ok(touch.routed)
    }

    pub fn touch_up(&mut self, event: TouchEvent) -> Result<RouteResult, Status> {
        event.validate()?;
        let touch = self
            .active_touches
            .remove(&event.pointer_id)
            .ok_or(Status::OutOfRange)?;
        Ok(touch.routed)
    }

    pub fn touch_cancel(&mut self, pointer_id: i32) -> Result<RouteResult, Status> {
        if pointer_id < 0 {
            return Err(Status::OutOfRange);
        }
        let touch = self
            .active_touches
            .remove(&pointer_id)
            .ok_or(Status::OutOfRange)?;
        Ok(touch.routed)
    }

    pub fn active_touch_count(&self) -> usize {
        self.active_touches.len()
    }

    pub fn submit_render_stats(
        &mut self,
        draw_calls: u32,
        frost_passes: u32,
        text_calls: u32,
    ) -> RenderStats {
        self.render.frame_seq = self.render.frame_seq.saturating_add(1);
        self.render.draw_calls = draw_calls;
        self.render.frost_passes = frost_passes;
        self.render.text_calls = text_calls;
        self.render
    }

    pub fn render_stats(&self) -> RenderStats {
        self.render
    }

    pub fn submit_render_frame_rgba(
        &mut self,
        width: u32,
        height: u32,
        pixels_rgba8: Vec<u8>,
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

        self.render_frame_seq = self.render_frame_seq.saturating_add(1);
        let info = RenderFrameInfo {
            frame_seq: self.render_frame_seq,
            width,
            height,
            byte_len: expected_len as u32,
            checksum_fnv1a32: fnv1a32(&pixels_rgba8),
        };
        self.last_render_frame = Some(StoredRenderFrame { info, pixels_rgba8 });
        Ok(info)
    }

    pub fn render_frame_info(&self) -> Option<RenderFrameInfo> {
        self.last_render_frame.as_ref().map(|v| v.info)
    }

    pub fn clear_render_frame(&mut self) {
        self.last_render_frame = None;
    }

    pub fn render_frame_byte_len(&self) -> Option<usize> {
        self.last_render_frame
            .as_ref()
            .map(|f| f.pixels_rgba8.len())
    }

    pub fn render_frame_snapshot(&self) -> Option<(RenderFrameInfo, Vec<u8>)> {
        self.last_render_frame
            .as_ref()
            .map(|f| (f.info, f.pixels_rgba8.clone()))
    }

    pub fn render_frame_read_chunk(
        &self,
        offset: usize,
        max_bytes: usize,
    ) -> Result<RenderFrameChunk, Status> {
        if max_bytes == 0 || max_bytes > RENDER_MAX_CHUNK_BYTES {
            return Err(Status::OutOfRange);
        }

        let frame = self.last_render_frame.as_ref().ok_or(Status::OutOfRange)?;
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
}

fn fnv1a32(data: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for b in data {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(0x01000193u32);
    }
    hash
}
