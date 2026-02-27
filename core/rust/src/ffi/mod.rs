use std::ffi::c_char;
use std::ptr;
use std::sync::Mutex;

use crate::api::{Decision, DisplayState, RectRegion, RouteResult, Status, TouchEvent};
use crate::engine::RuntimeEngine;
use crate::DIRECTSCREEN_CORE_VERSION;

#[repr(C)]
pub struct DsapiContext {
    engine: Mutex<RuntimeEngine>,
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

const VERSION_CSTR: &[u8] = b"0.1.0\0";

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

fn touch_event_from(pointer_id: i32, x: f32, y: f32) -> TouchEvent {
    TouchEvent { pointer_id, x, y }
}

fn with_engine_mut(ctx: *mut DsapiContext, f: impl FnOnce(&mut RuntimeEngine) -> Status) -> Status {
    if ctx.is_null() {
        return Status::NullPointer;
    }

    let ctx_ref = unsafe { &*ctx };
    match ctx_ref.engine.lock() {
        Ok(mut guard) => f(&mut guard),
        Err(_) => Status::InternalError,
    }
}

fn with_engine_ref(ctx: *mut DsapiContext, f: impl FnOnce(&RuntimeEngine) -> Status) -> Status {
    if ctx.is_null() {
        return Status::NullPointer;
    }

    let ctx_ref = unsafe { &*ctx };
    match ctx_ref.engine.lock() {
        Ok(guard) => f(&guard),
        Err(_) => Status::InternalError,
    }
}

#[no_mangle]
pub extern "C" fn dsapi_version() -> *const c_char {
    let _ = DIRECTSCREEN_CORE_VERSION;
    VERSION_CSTR.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn dsapi_context_create() -> *mut DsapiContext {
    Box::into_raw(Box::new(DsapiContext {
        engine: Mutex::new(RuntimeEngine::default()),
    }))
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
