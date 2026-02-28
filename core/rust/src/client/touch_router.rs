use std::collections::{BTreeMap, HashMap};
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::sync::mpsc::Sender;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::SystemTime;

use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{
    AbsInfo, AbsoluteAxisType, AttributeSet, Device, EventType, InputEvent, InputEventKind, Key,
    PropType, Synchronization, UinputAbsSetup,
};

use super::{DsapiClient, TouchEvent};

const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;

const TOUCH_PACKET_DOWN: u8 = 1;
const TOUCH_PACKET_MOVE: u8 = 2;
const TOUCH_PACKET_UP: u8 = 3;

#[derive(Debug, Clone)]
pub struct TouchRouterConfig {
    pub socket_path: String,
    pub route_name: String,
    pub device_path: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub rotation: i32,
    pub quiet: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct TouchAxisCaps {
    pub x_min: i32,
    pub x_max: i32,
    pub y_min: i32,
    pub y_max: i32,
    pub slot_min: i32,
    pub slot_max: i32,
    pub tracking_min: i32,
    pub tracking_max: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct TouchMapConfig {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub width: i32,
    pub height: i32,
    pub rotation: i32,
}

#[derive(Debug, Clone, Copy)]
struct TouchDisplayTransform {
    width: i32,
    height: i32,
    rotation: i32,
}

impl TouchMapConfig {
    pub fn map_point(&self, raw_x: i32, raw_y: i32) -> (f32, f32) {
        let ux = normalize_axis(raw_x, self.min_x, self.max_x);
        let uy = normalize_axis(raw_y, self.min_y, self.max_y);

        let (lx, ly) = match self.rotation {
            1 => (uy, 1.0 - ux),
            2 => (1.0 - ux, 1.0 - uy),
            3 => (1.0 - uy, ux),
            _ => (ux, uy),
        };

        (
            lx * (self.width.saturating_sub(1)) as f32,
            ly * (self.height.saturating_sub(1)) as f32,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Down,
    Move,
    Up,
    Clear,
}

#[derive(Debug, Clone, Copy)]
pub struct TouchMessage {
    pub phase: TouchPhase,
    pub pointer_id: i32,
    pub x: f32,
    pub y: f32,
    pub latency_ms: f32,
}

#[derive(Debug)]
pub struct TouchRouterHandle {
    shutdown: Arc<AtomicBool>,
    wake_write_fd: RawFd,
    display_transform: Arc<Mutex<TouchDisplayTransform>>,
    join: Option<JoinHandle<()>>,
}

impl TouchRouterHandle {
    pub fn signal_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if self.wake_write_fd >= 0 {
            let buf = [1u8; 1];
            let _ = unsafe {
                libc::write(
                    self.wake_write_fd,
                    buf.as_ptr() as *const libc::c_void,
                    buf.len(),
                )
            };
        }
    }

    pub fn shutdown_and_join(&mut self) {
        self.signal_shutdown();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        close_fd(&mut self.wake_write_fd);
    }

    pub fn update_display_transform(
        &self,
        screen_width: u32,
        screen_height: u32,
        rotation: i32,
    ) -> io::Result<()> {
        if screen_width == 0 || screen_height == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "screen_size_invalid",
            ));
        }
        if !(0..=3).contains(&rotation) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "rotation_must_be_0_3",
            ));
        }
        let mut guard = match self.display_transform.lock() {
            Ok(v) => v,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.width = i32::try_from(screen_width).unwrap_or(i32::MAX);
        guard.height = i32::try_from(screen_height).unwrap_or(i32::MAX);
        guard.rotation = rotation;
        Ok(())
    }
}

impl Drop for TouchRouterHandle {
    fn drop(&mut self) {
        self.shutdown_and_join();
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct SlotState {
    active: bool,
    pointer_id: i32,
    raw_x: i32,
    raw_y: i32,
}

#[derive(Debug, Clone, Copy)]
struct PendingTouch {
    kind: u8,
    pointer_id: i32,
    raw_x: i32,
    raw_y: i32,
    event_time: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerRoute {
    Ui,
    PassThrough,
}

pub fn spawn_touch_router<S, F>(
    cfg: TouchRouterConfig,
    route_state: Arc<Mutex<S>>,
    hit_test: F,
    tx: Sender<TouchMessage>,
) -> io::Result<TouchRouterHandle>
where
    S: Copy + Send + 'static,
    F: Fn(S, f32, f32) -> bool + Send + Sync + 'static,
{
    if cfg.route_name.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "route_name_empty",
        ));
    }
    if cfg.screen_width == 0 || cfg.screen_height == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "screen_size_invalid",
        ));
    }
    if !(0..=3).contains(&cfg.rotation) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "rotation_must_be_0_3",
        ));
    }

    let (wake_read_fd, wake_write_fd) = make_wake_pipe()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let thread_shutdown = Arc::clone(&shutdown);
    let hit_test = Arc::new(hit_test);
    let display_transform = Arc::new(Mutex::new(TouchDisplayTransform {
        width: i32::try_from(cfg.screen_width).unwrap_or(i32::MAX),
        height: i32::try_from(cfg.screen_height).unwrap_or(i32::MAX),
        rotation: cfg.rotation,
    }));
    let thread_transform = Arc::clone(&display_transform);

    let join = thread::spawn(move || {
        let run_result = run_touch_router(
            cfg,
            route_state,
            hit_test,
            thread_shutdown,
            thread_transform,
            wake_read_fd,
            tx,
        );
        close_fd_value(wake_read_fd);
        if let Err(e) = run_result {
            eprintln!("touch_router_error=run_failed err={}", e);
        }
    });

    Ok(TouchRouterHandle {
        shutdown,
        wake_write_fd,
        display_transform,
        join: Some(join),
    })
}

fn run_touch_router<S>(
    cfg: TouchRouterConfig,
    route_state: Arc<Mutex<S>>,
    hit_test: Arc<dyn Fn(S, f32, f32) -> bool + Send + Sync>,
    shutdown: Arc<AtomicBool>,
    display_transform: Arc<Mutex<TouchDisplayTransform>>,
    wake_read_fd: RawFd,
    tx: Sender<TouchMessage>,
) -> io::Result<()>
where
    S: Copy + Send + 'static,
{
    let client = DsapiClient::connect(&cfg.socket_path)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("touch_client_connect:{}", e)))?;
    let mut touch_stream = client
        .subscribe_touch(&cfg.route_name)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("touch_subscribe:{}", e)))?;

    let mut device = Device::open(&cfg.device_path)?;
    device.grab()?;

    let run_result = (|| -> io::Result<()> {
        let caps = read_touch_axis_caps(&device)?;
        let init_transform = current_display_transform(&display_transform);
        let map_cfg = TouchMapConfig {
            min_x: caps.x_min,
            max_x: caps.x_max,
            min_y: caps.y_min,
            max_y: caps.y_max,
            width: init_transform.width,
            height: init_transform.height,
            rotation: init_transform.rotation,
        };

        if !cfg.quiet {
            eprintln!(
                "touch_router_status=running device={} abs_x=[{},{}] abs_y=[{},{}] slot=[{},{}] tracking=[{},{}] rotation={}",
                cfg.device_path,
                caps.x_min,
                caps.x_max,
                caps.y_min,
                caps.y_max,
                caps.slot_min,
                caps.slot_max,
                caps.tracking_min,
                caps.tracking_max,
                map_cfg.rotation
            );
        }

        set_fd_nonblocking(device.as_raw_fd())?;
        let mut virtual_device = build_virtual_touch_device(&device)?;

        touch_stream
            .clear()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("touch_clear:{}", e)))?;
        let _ = tx.send(TouchMessage {
            phase: TouchPhase::Clear,
            pointer_id: -1,
            x: 0.0,
            y: 0.0,
            latency_ms: 0.0,
        });

        let mut cur_slot: i32 = 0;
        let mut slots: HashMap<i32, SlotState> = HashMap::new();
        let mut pending: BTreeMap<i32, PendingTouch> = BTreeMap::new();
        let mut pointer_routes: HashMap<i32, PointerRoute> = HashMap::new();
        let mut frame_events: Vec<InputEvent> = Vec::new();
        let mut frame_start_slot: i32 = cur_slot;
        let mut passthrough_btn_touch_down = false;

        loop {
            if shutdown.load(Ordering::SeqCst) {
                break;
            }

            let device_ready = wait_ready(device.as_raw_fd(), wake_read_fd)?;
            if !device_ready {
                break;
            }

            loop {
                let events = match device.fetch_events() {
                    Ok(iter) => iter.collect::<Vec<_>>(),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        break;
                    }
                    Err(e) => {
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("fetch_events_failed:{}", e),
                        ));
                    }
                };

                if events.is_empty() {
                    break;
                }

                for ev in events {
                    let event_time = ev.timestamp();
                    match ev.kind() {
                        InputEventKind::AbsAxis(axis) => {
                            frame_events.push(ev);
                            if axis.0 == ABS_MT_SLOT {
                                cur_slot = ev.value();
                                continue;
                            }

                            if axis.0 == ABS_MT_TRACKING_ID {
                                let slot = slots.entry(cur_slot).or_default();
                                if ev.value() < 0 {
                                    if slot.active {
                                        pending.insert(
                                            cur_slot,
                                            PendingTouch {
                                                kind: TOUCH_PACKET_UP,
                                                pointer_id: slot.pointer_id,
                                                raw_x: slot.raw_x,
                                                raw_y: slot.raw_y,
                                                event_time,
                                            },
                                        );
                                    }
                                    slot.active = false;
                                    slot.pointer_id = -1;
                                } else {
                                    slot.active = true;
                                    slot.pointer_id = ev.value();
                                    pending.insert(
                                        cur_slot,
                                        PendingTouch {
                                            kind: TOUCH_PACKET_DOWN,
                                            pointer_id: slot.pointer_id,
                                            raw_x: slot.raw_x,
                                            raw_y: slot.raw_y,
                                            event_time,
                                        },
                                    );
                                }
                                continue;
                            }

                            if axis.0 == ABS_MT_POSITION_X {
                                let slot = slots.entry(cur_slot).or_default();
                                slot.raw_x = ev.value();
                                if slot.active {
                                    pending
                                        .entry(cur_slot)
                                        .and_modify(|p| {
                                            p.raw_x = slot.raw_x;
                                            p.event_time = event_time;
                                        })
                                        .or_insert(PendingTouch {
                                            kind: TOUCH_PACKET_MOVE,
                                            pointer_id: slot.pointer_id,
                                            raw_x: slot.raw_x,
                                            raw_y: slot.raw_y,
                                            event_time,
                                        });
                                }
                                continue;
                            }

                            if axis.0 == ABS_MT_POSITION_Y {
                                let slot = slots.entry(cur_slot).or_default();
                                slot.raw_y = ev.value();
                                if slot.active {
                                    pending
                                        .entry(cur_slot)
                                        .and_modify(|p| {
                                            p.raw_y = slot.raw_y;
                                            p.event_time = event_time;
                                        })
                                        .or_insert(PendingTouch {
                                            kind: TOUCH_PACKET_MOVE,
                                            pointer_id: slot.pointer_id,
                                            raw_x: slot.raw_x,
                                            raw_y: slot.raw_y,
                                            event_time,
                                        });
                                }
                                continue;
                            }
                        }
                        InputEventKind::Synchronization(sync)
                            if sync == Synchronization::SYN_REPORT =>
                        {
                            let snapshot = match route_state.lock() {
                                Ok(guard) => *guard,
                                Err(poisoned) => *poisoned.into_inner(),
                            };
                            let transform = current_display_transform(&display_transform);
                            let map_cfg = TouchMapConfig {
                                min_x: caps.x_min,
                                max_x: caps.x_max,
                                min_y: caps.y_min,
                                max_y: caps.y_max,
                                width: transform.width,
                                height: transform.height,
                                rotation: transform.rotation,
                            };

                            let mut frame_slot_routes: HashMap<i32, PointerRoute> = HashMap::new();

                            for (slot_id, p) in pending.iter() {
                                if p.pointer_id < 0 {
                                    continue;
                                }
                                let (x, y) = map_cfg.map_point(p.raw_x, p.raw_y);

                                let route = match p.kind {
                                    TOUCH_PACKET_DOWN => {
                                        let route = if hit_test(snapshot, x, y) {
                                            PointerRoute::Ui
                                        } else {
                                            PointerRoute::PassThrough
                                        };
                                        pointer_routes.insert(p.pointer_id, route);
                                        route
                                    }
                                    TOUCH_PACKET_MOVE => *pointer_routes
                                        .get(&p.pointer_id)
                                        .unwrap_or(&PointerRoute::PassThrough),
                                    TOUCH_PACKET_UP => {
                                        let route = *pointer_routes
                                            .get(&p.pointer_id)
                                            .unwrap_or(&PointerRoute::PassThrough);
                                        pointer_routes.remove(&p.pointer_id);
                                        route
                                    }
                                    _ => PointerRoute::PassThrough,
                                };

                                frame_slot_routes.insert(*slot_id, route);
                                if route == PointerRoute::Ui {
                                    if let Some(event) =
                                        to_client_touch_event(p.kind, p.pointer_id, x, y)
                                    {
                                        touch_stream.send_event(event).map_err(|e| {
                                            io::Error::new(
                                                io::ErrorKind::Other,
                                                format!("touch_send_failed:{}", e),
                                            )
                                        })?;
                                    }

                                    if let Some(phase) = kind_to_phase(p.kind) {
                                        let _ = tx.send(TouchMessage {
                                            phase,
                                            pointer_id: p.pointer_id,
                                            x,
                                            y,
                                            latency_ms: touch_latency_ms(p.event_time),
                                        });
                                    }
                                }
                            }

                            let mut passthrough_frame_events = build_passthrough_frame_events(
                                &frame_events,
                                frame_start_slot,
                                &frame_slot_routes,
                                &slots,
                                &pointer_routes,
                            );

                            let passthrough_active =
                                has_passthrough_active_touch(&slots, &pointer_routes);
                            if passthrough_active != passthrough_btn_touch_down {
                                passthrough_frame_events.push(InputEvent::new(
                                    EventType::KEY,
                                    Key::BTN_TOUCH.code(),
                                    if passthrough_active { 1 } else { 0 },
                                ));
                                passthrough_btn_touch_down = passthrough_active;
                            }

                            if !passthrough_frame_events.is_empty() {
                                virtual_device.emit(&passthrough_frame_events)?;
                            }

                            frame_events.clear();
                            pending.clear();
                            frame_start_slot = cur_slot;
                        }
                        _ => {
                            frame_events.push(ev);
                        }
                    }
                }
            }
        }

        Ok(())
    })();

    let _ = touch_stream.clear();
    if let Err(e) = device.ungrab() {
        eprintln!("touch_router_error=device_ungrab_failed err={}", e);
    }

    run_result
}

fn build_passthrough_frame_events(
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

                let route = slot_route_for_frame(cur_slot, frame_slot_routes, slots, pointer_routes);
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

fn has_passthrough_active_touch(
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

fn is_mt_axis(code: u16) -> bool {
    code >= ABS_MT_SLOT
}

fn current_display_transform(
    display_transform: &Arc<Mutex<TouchDisplayTransform>>,
) -> TouchDisplayTransform {
    match display_transform.lock() {
        Ok(v) => *v,
        Err(poisoned) => *poisoned.into_inner(),
    }
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

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn normalize_axis(raw: i32, min_v: i32, max_v: i32) -> f32 {
    if max_v <= min_v {
        return 0.0;
    }
    let num = (raw - min_v) as f32;
    let den = (max_v - min_v) as f32;
    clamp01(num / den)
}

fn set_fd_nonblocking(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }

    let ret = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

fn read_touch_axis_caps(device: &Device) -> io::Result<TouchAxisCaps> {
    let Some(supported_abs) = device.supported_absolute_axes() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "device_has_no_absolute_axes",
        ));
    };

    let required_axes = [
        AbsoluteAxisType::ABS_MT_POSITION_X,
        AbsoluteAxisType::ABS_MT_POSITION_Y,
        AbsoluteAxisType::ABS_MT_SLOT,
        AbsoluteAxisType::ABS_MT_TRACKING_ID,
    ];

    for axis in required_axes {
        if !supported_abs.contains(axis) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing_required_abs_axis:{}", axis.0),
            ));
        }
    }

    let abs_state = device.get_abs_state()?;
    let x = abs_state[AbsoluteAxisType::ABS_MT_POSITION_X.0 as usize];
    let y = abs_state[AbsoluteAxisType::ABS_MT_POSITION_Y.0 as usize];
    let slot = abs_state[AbsoluteAxisType::ABS_MT_SLOT.0 as usize];
    let tracking = abs_state[AbsoluteAxisType::ABS_MT_TRACKING_ID.0 as usize];

    if x.maximum <= x.minimum || y.maximum <= y.minimum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid_abs_xy_range",
        ));
    }

    Ok(TouchAxisCaps {
        x_min: x.minimum,
        x_max: x.maximum,
        y_min: y.minimum,
        y_max: y.maximum,
        slot_min: slot.minimum,
        slot_max: slot.maximum,
        tracking_min: tracking.minimum,
        tracking_max: tracking.maximum,
    })
}

fn build_virtual_touch_device(device: &Device) -> io::Result<VirtualDevice> {
    let mut key_set: AttributeSet<Key> = device
        .supported_keys()
        .map(|keys| keys.iter().collect())
        .unwrap_or_else(AttributeSet::new);
    key_set.insert(Key::BTN_TOUCH);

    let mut props: AttributeSet<PropType> = device.properties().iter().collect();
    props.insert(PropType::DIRECT);

    let Some(abs_axes) = device.supported_absolute_axes() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "device_has_no_absolute_axes",
        ));
    };

    let abs_state = device.get_abs_state()?;
    let device_name = format!(
        "DirectScreenAPI Touch Clone ({})",
        device.name().unwrap_or("unknown")
    );

    let mut builder = VirtualDeviceBuilder::new()?
        .name(&device_name)
        .input_id(device.input_id())
        .with_properties(&props)?
        .with_keys(&key_set)?;

    for axis in abs_axes.iter() {
        let info = abs_state[axis.0 as usize];
        let setup = UinputAbsSetup::new(
            axis,
            AbsInfo::new(
                info.value,
                info.minimum,
                info.maximum,
                info.fuzz,
                info.flat,
                info.resolution,
            ),
        );
        builder = builder.with_absolute_axis(&setup)?;
    }

    builder.build()
}

fn to_client_touch_event(kind: u8, pointer_id: i32, x: f32, y: f32) -> Option<TouchEvent> {
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

fn kind_to_phase(kind: u8) -> Option<TouchPhase> {
    match kind {
        TOUCH_PACKET_DOWN => Some(TouchPhase::Down),
        TOUCH_PACKET_MOVE => Some(TouchPhase::Move),
        TOUCH_PACKET_UP => Some(TouchPhase::Up),
        _ => None,
    }
}

fn touch_latency_ms(event_time: SystemTime) -> f32 {
    match SystemTime::now().duration_since(event_time) {
        Ok(d) => d.as_secs_f32() * 1000.0,
        Err(_) => 0.0,
    }
}

fn make_wake_pipe() -> io::Result<(RawFd, RawFd)> {
    let mut fds = [0i32; 2];
    let ret = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((fds[0], fds[1]))
}

fn wait_ready(device_fd: RawFd, wake_read_fd: RawFd) -> io::Result<bool> {
    let mut fds = [
        libc::pollfd {
            fd: device_fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wake_read_fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];

    loop {
        let ret = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        break;
    }

    if (fds[1].revents & libc::POLLIN) != 0 {
        drain_fd(wake_read_fd);
        return Ok(false);
    }
    if (fds[1].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
        return Ok(false);
    }
    if (fds[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "device_poll_error_or_hup",
        ));
    }
    Ok((fds[0].revents & libc::POLLIN) != 0)
}

fn drain_fd(fd: RawFd) {
    let mut buf = [0u8; 64];
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n > 0 {
            if (n as usize) < buf.len() {
                break;
            }
            continue;
        }
        if n == 0 {
            break;
        }
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::WouldBlock || err.kind() == io::ErrorKind::Interrupted {
            break;
        }
        break;
    }
}

fn close_fd(fd: &mut RawFd) {
    if *fd >= 0 {
        unsafe {
            libc::close(*fd);
        }
        *fd = -1;
    }
}

fn close_fd_value(fd: RawFd) {
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
    }
}
