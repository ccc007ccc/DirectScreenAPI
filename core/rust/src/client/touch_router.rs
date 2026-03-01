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

use evdev::{Device, EventType, InputEvent, InputEventKind, Key, Synchronization};

use super::DsapiClient;
#[path = "touch_router/device.rs"]
mod touch_router_device;
#[path = "touch_router/event.rs"]
mod touch_router_event;
#[path = "touch_router/io.rs"]
mod touch_router_io;
#[path = "touch_router/map.rs"]
mod touch_router_map;
#[path = "touch_router/passthrough.rs"]
mod touch_router_passthrough;
#[path = "touch_router/routing.rs"]
mod touch_router_routing;
use touch_router_device::{build_virtual_touch_device, read_touch_axis_caps};
use touch_router_event::{kind_to_phase, to_client_touch_event};
use touch_router_io::{close_fd, close_fd_value, make_wake_pipe, set_fd_nonblocking, wait_ready};
use touch_router_map::normalize_axis;
use touch_router_passthrough::{build_passthrough_frame_events, has_passthrough_active_touch};
use touch_router_routing::resolve_pointer_route;

const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;

const TOUCH_PACKET_DOWN: u8 = 1;
const TOUCH_PACKET_MOVE: u8 = 2;
const TOUCH_PACKET_UP: u8 = 3;

#[derive(Debug, Clone)]
pub struct TouchRouterConfig {
    pub control_socket_path: String,
    pub data_socket_path: String,
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
    let client =
        DsapiClient::connect_with_data_socket(&cfg.control_socket_path, &cfg.data_socket_path)
            .map_err(|e| io::Error::other(format!("touch_client_connect:{}", e)))?;
    let mut touch_stream = client
        .subscribe_touch(&cfg.route_name)
        .map_err(|e| io::Error::other(format!("touch_subscribe:{}", e)))?;

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
            .map_err(|e| io::Error::other(format!("touch_clear:{}", e)))?;
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
                        return Err(io::Error::other(format!("fetch_events_failed:{}", e)));
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

                                let route = resolve_pointer_route(
                                    p.kind,
                                    p.pointer_id,
                                    x,
                                    y,
                                    snapshot,
                                    &hit_test,
                                    &mut pointer_routes,
                                );

                                frame_slot_routes.insert(*slot_id, route);
                                if route == PointerRoute::Ui {
                                    if let Some(event) =
                                        to_client_touch_event(p.kind, p.pointer_id, x, y)
                                    {
                                        touch_stream.send_event(event).map_err(|e| {
                                            io::Error::other(format!("touch_send_failed:{}", e))
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

fn current_display_transform(
    display_transform: &Arc<Mutex<TouchDisplayTransform>>,
) -> TouchDisplayTransform {
    match display_transform.lock() {
        Ok(v) => *v,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

fn touch_latency_ms(event_time: SystemTime) -> f32 {
    match SystemTime::now().duration_since(event_time) {
        Ok(d) => d.as_secs_f32() * 1000.0,
        Err(_) => 0.0,
    }
}
