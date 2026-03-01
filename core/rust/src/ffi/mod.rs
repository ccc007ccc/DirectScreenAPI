use std::ffi::c_char;
use std::ptr;

use crate::api::{
    Decision, DisplayState, RectRegion, RenderFrameInfo, RenderStats, RouteResult, Status,
    TouchEvent, DSAPI_ABI_VERSION, RENDER_MAX_FRAME_BYTES,
};
use crate::engine::{RenderPresentInfo, RuntimeEngine};

#[repr(C)]
pub struct DsapiContext {
    engine: RuntimeEngine,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DsapiDisplayState {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f32,
    pub density_dpi: u32,
    pub rotation: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DsapiRouteResult {
    pub decision: i32,
    pub region_id: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DsapiRenderStats {
    pub frame_seq: u64,
    pub draw_calls: u32,
    pub frost_passes: u32,
    pub text_calls: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DsapiRenderFrameInfo {
    pub frame_seq: u64,
    pub width: u32,
    pub height: u32,
    pub byte_len: u32,
    pub checksum_fnv1a32: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DsapiRenderPresentInfo {
    pub present_seq: u64,
    pub frame_seq: u64,
    pub width: u32,
    pub height: u32,
    pub byte_len: u32,
    pub checksum_fnv1a32: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DsapiRenderFrameChunk {
    pub frame_seq: u64,
    pub total_bytes: u32,
    pub offset: u32,
    pub chunk_len: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DsapiHwBufferDesc {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
    pub usage: u64,
    pub byte_offset: u32,
    pub byte_len: u32,
}

const VERSION_CSTR: &[u8] = concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes();

impl From<DsapiDisplayState> for DisplayState {
    fn from(v: DsapiDisplayState) -> Self {
        Self {
            width: v.width,
            height: v.height,
            refresh_hz: v.refresh_hz,
            density_dpi: v.density_dpi,
            rotation: v.rotation,
        }
    }
}

impl From<DisplayState> for DsapiDisplayState {
    fn from(v: DisplayState) -> Self {
        Self {
            width: v.width,
            height: v.height,
            refresh_hz: v.refresh_hz,
            density_dpi: v.density_dpi,
            rotation: v.rotation,
        }
    }
}

impl From<RouteResult> for DsapiRouteResult {
    fn from(v: RouteResult) -> Self {
        Self {
            decision: v.decision as i32,
            region_id: v.region_id,
        }
    }
}

impl From<RenderStats> for DsapiRenderStats {
    fn from(v: RenderStats) -> Self {
        Self {
            frame_seq: v.frame_seq,
            draw_calls: v.draw_calls,
            frost_passes: v.frost_passes,
            text_calls: v.text_calls,
        }
    }
}

impl From<RenderFrameInfo> for DsapiRenderFrameInfo {
    fn from(v: RenderFrameInfo) -> Self {
        Self {
            frame_seq: v.frame_seq,
            width: v.width,
            height: v.height,
            byte_len: v.byte_len,
            checksum_fnv1a32: v.checksum_fnv1a32,
        }
    }
}

impl From<RenderPresentInfo> for DsapiRenderPresentInfo {
    fn from(v: RenderPresentInfo) -> Self {
        Self {
            present_seq: v.present_seq,
            frame_seq: v.frame_seq,
            width: v.width,
            height: v.height,
            byte_len: v.byte_len,
            checksum_fnv1a32: v.checksum_fnv1a32,
        }
    }
}

fn touch_event_from(pointer_id: i32, x: f32, y: f32) -> TouchEvent {
    TouchEvent { pointer_id, x, y }
}

fn with_engine_mut(ctx: *mut DsapiContext, f: impl FnOnce(&RuntimeEngine) -> Status) -> Status {
    if ctx.is_null() {
        return Status::NullPointer;
    }

    let ctx_ref = unsafe { &*ctx };
    f(&ctx_ref.engine)
}

fn with_engine_ref(ctx: *mut DsapiContext, f: impl FnOnce(&RuntimeEngine) -> Status) -> Status {
    if ctx.is_null() {
        return Status::NullPointer;
    }

    let ctx_ref = unsafe { &*ctx };
    f(&ctx_ref.engine)
}

#[no_mangle]
pub extern "C" fn dsapi_version() -> *const c_char {
    VERSION_CSTR.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn dsapi_abi_version() -> u32 {
    DSAPI_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn dsapi_context_create() -> *mut DsapiContext {
    Box::into_raw(Box::new(DsapiContext {
        engine: RuntimeEngine::default(),
    }))
}

#[no_mangle]
/// # Safety
///
/// `out_ctx` must be a valid, writable pointer.
pub unsafe extern "C" fn dsapi_context_create_with_abi(
    abi_version: u32,
    out_ctx: *mut *mut DsapiContext,
) -> i32 {
    if out_ctx.is_null() {
        return Status::NullPointer as i32;
    }
    if abi_version != DSAPI_ABI_VERSION {
        return Status::InvalidArgument as i32;
    }
    unsafe {
        ptr::write(out_ctx, dsapi_context_create());
    }
    Status::Ok as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be either null or a pointer returned by `dsapi_context_create`
/// that has not been freed before. Passing any other pointer is undefined behavior.
pub unsafe extern "C" fn dsapi_context_destroy(ctx: *mut DsapiContext) {
    if ctx.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(ctx));
    }
}

#[no_mangle]
pub extern "C" fn dsapi_set_default_decision(ctx: *mut DsapiContext, decision: i32) -> i32 {
    let decision = match Decision::from_i32(decision) {
        Some(v) => v,
        None => return Status::InvalidArgument as i32,
    };

    with_engine_mut(ctx, |engine| {
        engine.set_default_decision(decision);
        Status::Ok
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `display` must be a valid, readable pointer to `DsapiDisplayState`.
pub unsafe extern "C" fn dsapi_set_display_state(
    ctx: *mut DsapiContext,
    display: *const DsapiDisplayState,
) -> i32 {
    if display.is_null() {
        return Status::NullPointer as i32;
    }

    let display = unsafe { *display };
    with_engine_mut(ctx, |engine| {
        match engine.set_display_state(display.into()) {
            Ok(()) => Status::Ok,
            Err(e) => e,
        }
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_display` must be a valid, writable pointer to `DsapiDisplayState`.
pub unsafe extern "C" fn dsapi_get_display_state(
    ctx: *mut DsapiContext,
    out_display: *mut DsapiDisplayState,
) -> i32 {
    if out_display.is_null() {
        return Status::NullPointer as i32;
    }

    with_engine_ref(ctx, |engine| {
        let state: DsapiDisplayState = engine.display_state().into();
        unsafe {
            ptr::write(out_display, state);
        }
        Status::Ok
    }) as i32
}

#[no_mangle]
pub extern "C" fn dsapi_region_clear(ctx: *mut DsapiContext) -> i32 {
    with_engine_mut(ctx, |engine| {
        engine.clear_regions();
        Status::Ok
    }) as i32
}

#[no_mangle]
pub extern "C" fn dsapi_region_add_rect(
    ctx: *mut DsapiContext,
    region_id: i32,
    decision: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> i32 {
    let decision = match Decision::from_i32(decision) {
        Some(v) => v,
        None => return Status::InvalidArgument as i32,
    };

    let region = RectRegion {
        region_id,
        decision,
        x,
        y,
        w,
        h,
    };

    with_engine_mut(ctx, |engine| match engine.add_region_rect(region) {
        Ok(()) => Status::Ok,
        Err(e) => e,
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_result` must be a valid, writable pointer to `DsapiRouteResult`.
pub unsafe extern "C" fn dsapi_route_point(
    ctx: *mut DsapiContext,
    x: f32,
    y: f32,
    out_result: *mut DsapiRouteResult,
) -> i32 {
    if out_result.is_null() {
        return Status::NullPointer as i32;
    }

    with_engine_ref(ctx, |engine| {
        let routed: DsapiRouteResult = engine.route_point(x, y).into();
        unsafe {
            ptr::write(out_result, routed);
        }
        Status::Ok
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_result` must be a valid, writable pointer to `DsapiRouteResult`.
pub unsafe extern "C" fn dsapi_touch_down(
    ctx: *mut DsapiContext,
    pointer_id: i32,
    x: f32,
    y: f32,
    out_result: *mut DsapiRouteResult,
) -> i32 {
    if out_result.is_null() {
        return Status::NullPointer as i32;
    }

    let event = touch_event_from(pointer_id, x, y);
    with_engine_mut(ctx, |engine| match engine.touch_down(event) {
        Ok(routed) => {
            unsafe {
                ptr::write(out_result, routed.into());
            }
            Status::Ok
        }
        Err(e) => e,
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_result` must be a valid, writable pointer to `DsapiRouteResult`.
pub unsafe extern "C" fn dsapi_touch_move(
    ctx: *mut DsapiContext,
    pointer_id: i32,
    x: f32,
    y: f32,
    out_result: *mut DsapiRouteResult,
) -> i32 {
    if out_result.is_null() {
        return Status::NullPointer as i32;
    }

    let event = touch_event_from(pointer_id, x, y);
    with_engine_mut(ctx, |engine| match engine.touch_move(event) {
        Ok(routed) => {
            unsafe {
                ptr::write(out_result, routed.into());
            }
            Status::Ok
        }
        Err(e) => e,
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_result` must be a valid, writable pointer to `DsapiRouteResult`.
pub unsafe extern "C" fn dsapi_touch_up(
    ctx: *mut DsapiContext,
    pointer_id: i32,
    x: f32,
    y: f32,
    out_result: *mut DsapiRouteResult,
) -> i32 {
    if out_result.is_null() {
        return Status::NullPointer as i32;
    }

    let event = touch_event_from(pointer_id, x, y);
    with_engine_mut(ctx, |engine| match engine.touch_up(event) {
        Ok(routed) => {
            unsafe {
                ptr::write(out_result, routed.into());
            }
            Status::Ok
        }
        Err(e) => e,
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_result` must be a valid, writable pointer to `DsapiRouteResult`.
pub unsafe extern "C" fn dsapi_touch_cancel(
    ctx: *mut DsapiContext,
    pointer_id: i32,
    out_result: *mut DsapiRouteResult,
) -> i32 {
    if out_result.is_null() {
        return Status::NullPointer as i32;
    }

    with_engine_mut(ctx, |engine| match engine.touch_cancel(pointer_id) {
        Ok(routed) => {
            unsafe {
                ptr::write(out_result, routed.into());
            }
            Status::Ok
        }
        Err(e) => e,
    }) as i32
}

#[no_mangle]
pub extern "C" fn dsapi_touch_clear(ctx: *mut DsapiContext) -> i32 {
    with_engine_mut(ctx, |engine| {
        engine.clear_touches();
        Status::Ok
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_count` must be a valid, writable pointer to `u32`.
pub unsafe extern "C" fn dsapi_touch_count(ctx: *mut DsapiContext, out_count: *mut u32) -> i32 {
    if out_count.is_null() {
        return Status::NullPointer as i32;
    }

    with_engine_ref(ctx, |engine| {
        unsafe {
            ptr::write(out_count, engine.active_touch_count() as u32);
        }
        Status::Ok
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_stats` must be a valid, writable pointer to `DsapiRenderStats`.
pub unsafe extern "C" fn dsapi_render_submit_stats(
    ctx: *mut DsapiContext,
    draw_calls: u32,
    frost_passes: u32,
    text_calls: u32,
    out_stats: *mut DsapiRenderStats,
) -> i32 {
    if out_stats.is_null() {
        return Status::NullPointer as i32;
    }

    with_engine_mut(ctx, |engine| {
        let stats: DsapiRenderStats = engine
            .submit_render_stats(draw_calls, frost_passes, text_calls)
            .into();
        unsafe {
            ptr::write(out_stats, stats);
        }
        Status::Ok
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_stats` must be a valid, writable pointer to `DsapiRenderStats`.
pub unsafe extern "C" fn dsapi_render_get_stats(
    ctx: *mut DsapiContext,
    out_stats: *mut DsapiRenderStats,
) -> i32 {
    if out_stats.is_null() {
        return Status::NullPointer as i32;
    }

    with_engine_ref(ctx, |engine| {
        let stats: DsapiRenderStats = engine.render_stats().into();
        unsafe {
            ptr::write(out_stats, stats);
        }
        Status::Ok
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `pixels_rgba8` must be a valid readable pointer of `pixels_len` bytes.
/// `out_info` must be a valid writable pointer to `DsapiRenderFrameInfo`.
pub unsafe extern "C" fn dsapi_render_submit_frame_rgba(
    ctx: *mut DsapiContext,
    width: u32,
    height: u32,
    pixels_rgba8: *const u8,
    pixels_len: u32,
    out_info: *mut DsapiRenderFrameInfo,
) -> i32 {
    if pixels_rgba8.is_null() || out_info.is_null() {
        return Status::NullPointer as i32;
    }

    let len = match usize::try_from(pixels_len) {
        Ok(v) => v,
        Err(_) => return Status::OutOfRange as i32,
    };
    if width == 0 || height == 0 {
        return Status::InvalidArgument as i32;
    }
    let expected_len = match (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4usize))
    {
        Some(v) => v,
        None => return Status::OutOfRange as i32,
    };
    if expected_len > RENDER_MAX_FRAME_BYTES {
        return Status::OutOfRange as i32;
    }
    if len != expected_len {
        return Status::InvalidArgument as i32;
    }
    let bytes = unsafe { std::slice::from_raw_parts(pixels_rgba8, len) }.to_vec();

    with_engine_mut(ctx, |engine| {
        match engine.submit_render_frame_rgba(width, height, bytes) {
            Ok(info) => {
                let info: DsapiRenderFrameInfo = info.into();
                unsafe {
                    ptr::write(out_info, info);
                }
                Status::Ok
            }
            Err(e) => e,
        }
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `desc` and `out_info` must be valid pointers.
/// `fd` must be a readable dma-buf/file descriptor that maps at least `desc->byte_len` bytes.
pub unsafe extern "C" fn dsapi_render_submit_frame_ahb_fd(
    ctx: *mut DsapiContext,
    fd: i32,
    desc: *const DsapiHwBufferDesc,
    out_info: *mut DsapiRenderFrameInfo,
) -> i32 {
    if desc.is_null() || out_info.is_null() {
        return Status::NullPointer as i32;
    }
    if fd < 0 {
        return Status::InvalidArgument as i32;
    }

    let desc = unsafe { *desc };
    if desc.width == 0 || desc.height == 0 || desc.stride == 0 {
        return Status::InvalidArgument as i32;
    }
    // AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM
    if desc.format != 1 {
        return Status::InvalidArgument as i32;
    }

    let map_len = match usize::try_from(desc.byte_len) {
        Ok(v) => v,
        Err(_) => return Status::OutOfRange as i32,
    };
    if map_len == 0 || map_len > RENDER_MAX_FRAME_BYTES {
        return Status::OutOfRange as i32;
    }

    let offset = match usize::try_from(desc.byte_offset) {
        Ok(v) => v,
        Err(_) => return Status::OutOfRange as i32,
    };
    if offset >= map_len {
        return Status::InvalidArgument as i32;
    }

    let expected_len = match (desc.width as usize)
        .checked_mul(desc.height as usize)
        .and_then(|v| v.checked_mul(4usize))
    {
        Some(v) => v,
        None => return Status::OutOfRange as i32,
    };
    let end = match offset.checked_add(expected_len) {
        Some(v) => v,
        None => return Status::OutOfRange as i32,
    };
    if end > map_len {
        return Status::InvalidArgument as i32;
    }

    let mapped = unsafe {
        libc::mmap(
            ptr::null_mut(),
            map_len,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    if mapped == libc::MAP_FAILED {
        return Status::InternalError as i32;
    }

    let bytes = unsafe {
        let base = mapped as *const u8;
        let frame_ptr = base.add(offset);
        std::slice::from_raw_parts(frame_ptr, expected_len).to_vec()
    };
    let _ = unsafe { libc::munmap(mapped, map_len) };

    with_engine_mut(ctx, |engine| {
        match engine.submit_render_frame_rgba(desc.width, desc.height, bytes) {
            Ok(info) => {
                unsafe {
                    ptr::write(out_info, info.into());
                }
                Status::Ok
            }
            Err(e) => e,
        }
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_info` must be a valid writable pointer to `DsapiRenderFrameInfo`.
pub unsafe extern "C" fn dsapi_render_get_frame_info(
    ctx: *mut DsapiContext,
    out_info: *mut DsapiRenderFrameInfo,
) -> i32 {
    if out_info.is_null() {
        return Status::NullPointer as i32;
    }

    with_engine_ref(ctx, |engine| match engine.render_frame_info() {
        Some(info) => {
            let info: DsapiRenderFrameInfo = info.into();
            unsafe {
                ptr::write(out_info, info);
            }
            Status::Ok
        }
        None => Status::OutOfRange,
    }) as i32
}

#[no_mangle]
pub extern "C" fn dsapi_render_clear_frame(ctx: *mut DsapiContext) -> i32 {
    with_engine_mut(ctx, |engine| {
        engine.clear_render_frame();
        Status::Ok
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_chunk` must be a valid writable pointer to `DsapiRenderFrameChunk`.
/// `out_bytes` must be a valid writable pointer of `out_bytes_cap` bytes.
pub unsafe extern "C" fn dsapi_render_frame_read_chunk(
    ctx: *mut DsapiContext,
    offset: u32,
    max_bytes: u32,
    out_chunk: *mut DsapiRenderFrameChunk,
    out_bytes: *mut u8,
    out_bytes_cap: u32,
) -> i32 {
    if out_chunk.is_null() || out_bytes.is_null() {
        return Status::NullPointer as i32;
    }

    let offset = match usize::try_from(offset) {
        Ok(v) => v,
        Err(_) => return Status::OutOfRange as i32,
    };
    let max_bytes = match usize::try_from(max_bytes) {
        Ok(v) => v,
        Err(_) => return Status::OutOfRange as i32,
    };
    let out_bytes_cap = match usize::try_from(out_bytes_cap) {
        Ok(v) => v,
        Err(_) => return Status::OutOfRange as i32,
    };

    with_engine_ref(ctx, |engine| {
        match engine.render_frame_read_chunk(offset, max_bytes) {
            Ok(chunk) => {
                if chunk.chunk_bytes.len() > out_bytes_cap {
                    return Status::OutOfRange;
                }
                unsafe {
                    ptr::copy_nonoverlapping(
                        chunk.chunk_bytes.as_ptr(),
                        out_bytes,
                        chunk.chunk_bytes.len(),
                    );
                    ptr::write(
                        out_chunk,
                        DsapiRenderFrameChunk {
                            frame_seq: chunk.frame_seq,
                            total_bytes: chunk.total_bytes,
                            offset: chunk.offset,
                            chunk_len: chunk.chunk_bytes.len() as u32,
                        },
                    );
                }
                Status::Ok
            }
            Err(e) => e,
        }
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_present` must be a valid writable pointer to `DsapiRenderPresentInfo`.
pub unsafe extern "C" fn dsapi_render_present(
    ctx: *mut DsapiContext,
    out_present: *mut DsapiRenderPresentInfo,
) -> i32 {
    if out_present.is_null() {
        return Status::NullPointer as i32;
    }

    with_engine_mut(ctx, |engine| match engine.render_present() {
        Ok(info) => {
            let info: DsapiRenderPresentInfo = info.into();
            unsafe {
                ptr::write(out_present, info);
            }
            Status::Ok
        }
        Err(e) => e,
    }) as i32
}

#[no_mangle]
/// # Safety
///
/// `ctx` must be a valid context pointer from `dsapi_context_create`.
/// `out_present` must be a valid writable pointer to `DsapiRenderPresentInfo`.
pub unsafe extern "C" fn dsapi_render_get_present(
    ctx: *mut DsapiContext,
    out_present: *mut DsapiRenderPresentInfo,
) -> i32 {
    if out_present.is_null() {
        return Status::NullPointer as i32;
    }

    with_engine_ref(ctx, |engine| match engine.render_present_get() {
        Some(info) => {
            let info: DsapiRenderPresentInfo = info.into();
            unsafe {
                ptr::write(out_present, info);
            }
            Status::Ok
        }
        None => Status::OutOfRange,
    }) as i32
}
