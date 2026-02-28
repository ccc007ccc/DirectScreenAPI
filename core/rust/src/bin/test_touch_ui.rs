use font8x8::{UnicodeFonts, BASIC_FONTS};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{
    atomic::{AtomicI32, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tiny_skia::Pixmap;

use directscreen_core::client::{DsapiClient, TouchEvent as ClientTouchEvent};

const EV_SYN: u16 = 0x00;
const EV_ABS: u16 = 0x03;

const SYN_REPORT: u16 = 0;

const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;

const TOUCH_PACKET_DOWN: u8 = 1;
const TOUCH_PACKET_MOVE: u8 = 2;
const TOUCH_PACKET_UP: u8 = 3;

const TITLE_BAR_H: f32 = 64.0;
const MIN_BTN_SIZE: f32 = 36.0;
const CLOSE_BTN_SIZE: f32 = 40.0;
const RESIZE_HANDLE_SIZE: f32 = 28.0;
const RESIZE_HANDLE_HIT_SIZE: f32 = 72.0;
const BUBBLE_RADIUS: f32 = 44.0;
const CLICK_MOVE_THRESHOLD: f32 = 10.0;
const MIN_WINDOW_W: f32 = 100.0;
const MIN_WINDOW_H: f32 = 100.0;
const EVIOCGRAB_REQUEST: libc::c_int = 1074021776;

#[derive(Debug, Clone)]
struct Args {
    socket: String,
    device: String,
    max_x: i32,
    max_y: i32,
    rotation: Option<i32>,
    target_fps: u32,
    quiet: bool,
}

#[derive(Debug, Clone, Copy)]
struct TouchMapConfig {
    max_x: i32,
    max_y: i32,
    width: i32,
    height: i32,
    rotation: i32,
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

#[derive(Debug, Clone, Copy)]
enum TouchPhase {
    Down,
    Move,
    Up,
    Clear,
}

#[derive(Debug, Clone, Copy)]
struct TouchMessage {
    phase: TouchPhase,
    pointer_id: i32,
    x: f32,
    y: f32,
    latency_ms: f32,
}

#[derive(Debug, Clone, Copy)]
struct WindowRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Debug, Clone, Copy)]
struct BubbleState {
    cx: f32,
    cy: f32,
    r: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiMode {
    Window,
    Minimized,
}

#[derive(Debug, Clone, Copy)]
enum PressTarget {
    None,
    CloseButton,
    MinimizeButton,
    Bubble,
}

#[derive(Debug, Clone, Copy)]
enum DragKind {
    WindowMove {
        offset_x: f32,
        offset_y: f32,
    },
    Resize {
        start_w: f32,
        start_h: f32,
        start_x: f32,
        start_y: f32,
    },
    BubbleMove {
        offset_x: f32,
        offset_y: f32,
    },
}

#[derive(Debug, Clone, Copy)]
struct ActiveTouch {
    pointer_id: i32,
    start_x: f32,
    start_y: f32,
    moved: bool,
    press_target: PressTarget,
    drag_kind: Option<DragKind>,
}

#[derive(Debug, Clone, Copy)]
struct UiState {
    mode: UiMode,
    window: WindowRect,
    bubble: BubbleState,
    active: Option<ActiveTouch>,
    last_touch_x: f32,
    last_touch_y: f32,
    last_latency_ms: f32,
    exit_requested: bool,
}

fn set_input_grab(fd: RawFd, enabled: bool) -> io::Result<()> {
    let arg: libc::c_int = if enabled { 1 } else { 0 };
    let ret = unsafe { libc::ioctl(fd, EVIOCGRAB_REQUEST, arg) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn release_input_grab(grabbed_fd: &AtomicI32) {
    let fd = grabbed_fd.swap(-1, Ordering::SeqCst);
    if fd >= 0 {
        let _ = set_input_grab(fd, false);
    }
}

impl UiState {
    fn new(screen_w: f32, screen_h: f32) -> Self {
        let base_w = (screen_w * 0.64).clamp(280.0, (screen_w - 40.0).max(280.0));
        let base_h = (screen_h * 0.42).clamp(220.0, (screen_h - 40.0).max(220.0));
        let window = WindowRect {
            x: ((screen_w - base_w) * 0.5).max(0.0),
            y: ((screen_h - base_h) * 0.5).max(0.0),
            w: base_w,
            h: base_h,
        };

        let bubble = BubbleState {
            cx: (screen_w - 90.0).max(BUBBLE_RADIUS + 5.0),
            cy: (screen_h * 0.35).max(BUBBLE_RADIUS + 5.0),
            r: BUBBLE_RADIUS,
        };

        Self {
            mode: UiMode::Window,
            window,
            bubble,
            active: None,
            last_touch_x: 0.0,
            last_touch_y: 0.0,
            last_latency_ms: 0.0,
            exit_requested: false,
        }
    }

    fn on_touch(&mut self, msg: TouchMessage, screen_w: f32, screen_h: f32) {
        self.last_touch_x = msg.x;
        self.last_touch_y = msg.y;
        self.last_latency_ms = msg.latency_ms;

        match msg.phase {
            TouchPhase::Down => self.on_touch_down(msg, screen_w, screen_h),
            TouchPhase::Move => self.on_touch_move(msg, screen_w, screen_h),
            TouchPhase::Up => self.on_touch_up(msg, screen_w, screen_h),
            TouchPhase::Clear => {
                self.active = None;
            }
        }
    }

    fn on_touch_down(&mut self, msg: TouchMessage, _screen_w: f32, _screen_h: f32) {
        if self.active.is_some() {
            return;
        }

        match self.mode {
            UiMode::Window => {
                let (cx, cy, cw, ch) = self.close_button_rect();
                let (bx, by, bw, bh) = self.minimize_button_rect();
                let (rx, ry, rw, rh) = self.resize_hit_rect();
                let (tx, ty, tw, th) = self.title_bar_rect();

                if point_in_rect(msg.x, msg.y, cx, cy, cw, ch) {
                    self.active = Some(ActiveTouch {
                        pointer_id: msg.pointer_id,
                        start_x: msg.x,
                        start_y: msg.y,
                        moved: false,
                        press_target: PressTarget::CloseButton,
                        drag_kind: None,
                    });
                    return;
                }

                if point_in_rect(msg.x, msg.y, bx, by, bw, bh) {
                    self.active = Some(ActiveTouch {
                        pointer_id: msg.pointer_id,
                        start_x: msg.x,
                        start_y: msg.y,
                        moved: false,
                        press_target: PressTarget::MinimizeButton,
                        drag_kind: None,
                    });
                    return;
                }

                if point_in_rect(msg.x, msg.y, rx, ry, rw, rh) {
                    self.active = Some(ActiveTouch {
                        pointer_id: msg.pointer_id,
                        start_x: msg.x,
                        start_y: msg.y,
                        moved: false,
                        press_target: PressTarget::None,
                        drag_kind: Some(DragKind::Resize {
                            start_w: self.window.w,
                            start_h: self.window.h,
                            start_x: msg.x,
                            start_y: msg.y,
                        }),
                    });
                    return;
                }

                if point_in_rect(msg.x, msg.y, tx, ty, tw, th) {
                    self.active = Some(ActiveTouch {
                        pointer_id: msg.pointer_id,
                        start_x: msg.x,
                        start_y: msg.y,
                        moved: false,
                        press_target: PressTarget::None,
                        drag_kind: Some(DragKind::WindowMove {
                            offset_x: msg.x - self.window.x,
                            offset_y: msg.y - self.window.y,
                        }),
                    });
                }
            }
            UiMode::Minimized => {
                if point_in_circle(msg.x, msg.y, self.bubble.cx, self.bubble.cy, self.bubble.r) {
                    self.active = Some(ActiveTouch {
                        pointer_id: msg.pointer_id,
                        start_x: msg.x,
                        start_y: msg.y,
                        moved: false,
                        press_target: PressTarget::Bubble,
                        drag_kind: Some(DragKind::BubbleMove {
                            offset_x: msg.x - self.bubble.cx,
                            offset_y: msg.y - self.bubble.cy,
                        }),
                    });
                }
            }
        }
    }

    fn on_touch_move(&mut self, msg: TouchMessage, screen_w: f32, screen_h: f32) {
        let Some(mut active) = self.active else {
            return;
        };
        if active.pointer_id != msg.pointer_id {
            return;
        }

        let dx = msg.x - active.start_x;
        let dy = msg.y - active.start_y;
        if !active.moved && (dx * dx + dy * dy) >= (CLICK_MOVE_THRESHOLD * CLICK_MOVE_THRESHOLD) {
            active.moved = true;
        }

        if let Some(drag_kind) = active.drag_kind {
            match drag_kind {
                DragKind::WindowMove { offset_x, offset_y } => {
                    self.window.x = msg.x - offset_x;
                    self.window.y = msg.y - offset_y;
                    self.clamp_window(screen_w, screen_h);
                }
                DragKind::Resize {
                    start_w,
                    start_h,
                    start_x,
                    start_y,
                } => {
                    let min_w = MIN_WINDOW_W;
                    let min_h = MIN_WINDOW_H;
                    let max_w = (screen_w - self.window.x).max(min_w);
                    let max_h = (screen_h - self.window.y).max(min_h);
                    self.window.w = (start_w + (msg.x - start_x)).clamp(min_w, max_w);
                    self.window.h = (start_h + (msg.y - start_y)).clamp(min_h, max_h);
                    self.clamp_window(screen_w, screen_h);
                }
                DragKind::BubbleMove { offset_x, offset_y } => {
                    self.bubble.cx = msg.x - offset_x;
                    self.bubble.cy = msg.y - offset_y;
                    self.clamp_bubble(screen_w, screen_h);
                }
            }
        }

        self.active = Some(active);
    }

    fn on_touch_up(&mut self, msg: TouchMessage, screen_w: f32, screen_h: f32) {
        let Some(active) = self.active else {
            return;
        };
        if active.pointer_id != msg.pointer_id {
            return;
        }

        let clicked = !active.moved;
        let target = active.press_target;
        self.active = None;

        if !clicked {
            return;
        }

        match (self.mode, target) {
            (UiMode::Window, PressTarget::CloseButton) => {
                self.exit_requested = true;
            }
            (UiMode::Window, PressTarget::MinimizeButton) => {
                self.mode = UiMode::Minimized;
                self.clamp_bubble(screen_w, screen_h);
            }
            (UiMode::Minimized, PressTarget::Bubble) => {
                self.mode = UiMode::Window;
                self.clamp_window(screen_w, screen_h);
            }
            _ => {}
        }
    }

    fn title_bar_rect(&self) -> (f32, f32, f32, f32) {
        (
            self.window.x,
            self.window.y,
            self.window.w,
            TITLE_BAR_H.min(self.window.h),
        )
    }

    fn minimize_button_rect(&self) -> (f32, f32, f32, f32) {
        let padding = 12.0;
        let close_gap = 10.0;
        let close_left = self.window.x + self.window.w - CLOSE_BTN_SIZE - padding;
        (
            close_left - close_gap - MIN_BTN_SIZE,
            self.window.y + (TITLE_BAR_H - MIN_BTN_SIZE) * 0.5,
            MIN_BTN_SIZE,
            MIN_BTN_SIZE,
        )
    }

    fn close_button_rect(&self) -> (f32, f32, f32, f32) {
        let padding = 12.0;
        (
            self.window.x + self.window.w - CLOSE_BTN_SIZE - padding,
            self.window.y + (TITLE_BAR_H - CLOSE_BTN_SIZE) * 0.5,
            CLOSE_BTN_SIZE,
            CLOSE_BTN_SIZE,
        )
    }

    fn resize_handle_rect(&self) -> (f32, f32, f32, f32) {
        (
            self.window.x + self.window.w - RESIZE_HANDLE_SIZE,
            self.window.y + self.window.h - RESIZE_HANDLE_SIZE,
            RESIZE_HANDLE_SIZE,
            RESIZE_HANDLE_SIZE,
        )
    }

    fn resize_hit_rect(&self) -> (f32, f32, f32, f32) {
        (
            self.window.x + self.window.w - RESIZE_HANDLE_HIT_SIZE,
            self.window.y + self.window.h - RESIZE_HANDLE_HIT_SIZE,
            RESIZE_HANDLE_HIT_SIZE,
            RESIZE_HANDLE_HIT_SIZE,
        )
    }

    fn clamp_window(&mut self, screen_w: f32, screen_h: f32) {
        self.window.w = self.window.w.max(MIN_WINDOW_W);
        self.window.h = self.window.h.max(MIN_WINDOW_H);
        let max_x = (screen_w - self.window.w).max(0.0);
        let max_y = (screen_h - self.window.h).max(0.0);
        self.window.x = self.window.x.clamp(0.0, max_x);
        self.window.y = self.window.y.clamp(0.0, max_y);
    }

    fn clamp_bubble(&mut self, screen_w: f32, screen_h: f32) {
        let min_x = self.bubble.r + 2.0;
        let min_y = self.bubble.r + 2.0;
        let max_x = (screen_w - self.bubble.r - 2.0).max(min_x);
        let max_y = (screen_h - self.bubble.r - 2.0).max(min_y);
        self.bubble.cx = self.bubble.cx.clamp(min_x, max_x);
        self.bubble.cy = self.bubble.cy.clamp(min_y, max_y);
    }

    fn should_exit(&self) -> bool {
        self.exit_requested
    }
}

fn default_socket_path() -> String {
    "artifacts/run/dsapi.sock".to_string()
}

fn usage() {
    eprintln!("usage:");
    eprintln!(
        "  test_touch_ui --device <event_path> --max-x <n> --max-y <n> [--socket <path>] [--rotation <0-3>] [--target-fps <n>] [--quiet]"
    );
}

fn parse_i32_arg(v: &str, name: &str) -> Result<i32, String> {
    v.parse::<i32>()
        .map_err(|_| format!("invalid_{}_value", name))
}

fn parse_u32_arg(v: &str, name: &str) -> Result<u32, String> {
    v.parse::<u32>()
        .map_err(|_| format!("invalid_{}_value", name))
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut socket = default_socket_path();
    let mut device = String::new();
    let mut max_x = None::<i32>;
    let mut max_y = None::<i32>;
    let mut rotation = None::<i32>;
    let mut target_fps = 60u32;
    let mut quiet = false;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" => {
                if i + 1 >= args.len() {
                    return Err("missing_socket_path".to_string());
                }
                socket = args[i + 1].clone();
                i += 2;
            }
            "--device" => {
                if i + 1 >= args.len() {
                    return Err("missing_device_path".to_string());
                }
                device = args[i + 1].clone();
                i += 2;
            }
            "--max-x" => {
                if i + 1 >= args.len() {
                    return Err("missing_max_x".to_string());
                }
                max_x = Some(parse_i32_arg(&args[i + 1], "max_x")?);
                i += 2;
            }
            "--max-y" => {
                if i + 1 >= args.len() {
                    return Err("missing_max_y".to_string());
                }
                max_y = Some(parse_i32_arg(&args[i + 1], "max_y")?);
                i += 2;
            }
            "--rotation" => {
                if i + 1 >= args.len() {
                    return Err("missing_rotation".to_string());
                }
                rotation = Some(parse_i32_arg(&args[i + 1], "rotation")?);
                i += 2;
            }
            "--target-fps" => {
                if i + 1 >= args.len() {
                    return Err("missing_target_fps".to_string());
                }
                target_fps = parse_u32_arg(&args[i + 1], "target_fps")?;
                i += 2;
            }
            "--quiet" => {
                quiet = true;
                i += 1;
            }
            _ => return Err(format!("unknown_arg:{}", args[i])),
        }
    }

    if device.is_empty() {
        return Err("missing_device".to_string());
    }

    let max_x = max_x.ok_or_else(|| "missing_max_x".to_string())?;
    let max_y = max_y.ok_or_else(|| "missing_max_y".to_string())?;
    if max_x <= 0 || max_y <= 0 {
        return Err("max_x_max_y_must_be_positive".to_string());
    }

    if let Some(r) = rotation {
        if !(0..=3).contains(&r) {
            return Err("rotation_must_be_0_3".to_string());
        }
    }

    if target_fps == 0 {
        return Err("target_fps_must_be_positive".to_string());
    }

    Ok(Args {
        socket,
        device,
        max_x,
        max_y,
        rotation,
        target_fps,
        quiet,
    })
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn map_point(cfg: &TouchMapConfig, raw_x: i32, raw_y: i32) -> (f32, f32) {
    let ux = clamp01(raw_x as f32 / cfg.max_x as f32);
    let uy = clamp01(raw_y as f32 / cfg.max_y as f32);

    let (lx, ly) = match cfg.rotation {
        1 => (uy, 1.0 - ux),
        2 => (1.0 - ux, 1.0 - uy),
        3 => (1.0 - uy, ux),
        _ => (ux, uy),
    };

    (
        lx * (cfg.width.saturating_sub(1)) as f32,
        ly * (cfg.height.saturating_sub(1)) as f32,
    )
}

fn read_input_event(file: &mut File) -> io::Result<libc::input_event> {
    let mut ev = std::mem::MaybeUninit::<libc::input_event>::uninit();
    let ev_bytes = unsafe {
        std::slice::from_raw_parts_mut(
            ev.as_mut_ptr() as *mut u8,
            std::mem::size_of::<libc::input_event>(),
        )
    };
    file.read_exact(ev_bytes)?;
    Ok(unsafe { ev.assume_init() })
}

fn event_time_to_system_time(ev: &libc::input_event) -> SystemTime {
    let sec = i64::try_from(ev.time.tv_sec).unwrap_or(0).max(0) as u64;
    let usec = i64::try_from(ev.time.tv_usec)
        .unwrap_or(0)
        .clamp(0, 999_999) as u64;
    UNIX_EPOCH + Duration::from_secs(sec) + Duration::from_micros(usec)
}

fn touch_latency_ms(event_time: SystemTime) -> f32 {
    match SystemTime::now().duration_since(event_time) {
        Ok(d) => d.as_secs_f32() * 1000.0,
        Err(_) => 0.0,
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

fn to_client_touch_event(kind: u8, pointer_id: i32, x: f32, y: f32) -> Option<ClientTouchEvent> {
    if pointer_id < 0 {
        return None;
    }
    let id = pointer_id as u32;
    let sx = x.max(0.0).round() as u32;
    let sy = y.max(0.0).round() as u32;
    match kind {
        TOUCH_PACKET_DOWN => Some(ClientTouchEvent::Down { id, x: sx, y: sy }),
        TOUCH_PACKET_MOVE => Some(ClientTouchEvent::Move { id, x: sx, y: sy }),
        TOUCH_PACKET_UP => Some(ClientTouchEvent::Up { id }),
        _ => None,
    }
}

fn spawn_touch_thread(
    args: Args,
    map_cfg: TouchMapConfig,
    tx: Sender<TouchMessage>,
    grabbed_fd: Arc<AtomicI32>,
) {
    thread::spawn(move || {
        struct GrabReleaseGuard {
            grabbed_fd: Arc<AtomicI32>,
        }

        impl Drop for GrabReleaseGuard {
            fn drop(&mut self) {
                release_input_grab(self.grabbed_fd.as_ref());
            }
        }

        let _guard = GrabReleaseGuard {
            grabbed_fd: Arc::clone(&grabbed_fd),
        };

        let client = match DsapiClient::connect(&args.socket) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "touch_ui_error=touch_client_connect_failed socket={} err={}",
                    args.socket, e
                );
                return;
            }
        };

        let mut touch_stream = match client.subscribe_touch("test_touch_ui") {
            Ok(v) => v,
            Err(e) => {
                eprintln!("touch_ui_error=touch_subscribe_failed err={}", e);
                return;
            }
        };

        if let Err(e) = touch_stream.clear() {
            eprintln!("touch_ui_error=touch_clear_failed err={}", e);
            return;
        }
        let _ = tx.send(TouchMessage {
            phase: TouchPhase::Clear,
            pointer_id: -1,
            x: 0.0,
            y: 0.0,
            latency_ms: 0.0,
        });

        let mut input = match File::open(&args.device) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "touch_ui_error=open_device_failed device={} err={}",
                    args.device, e
                );
                return;
            }
        };
        let fd = input.as_raw_fd();
        if let Err(e) = set_input_grab(fd, true) {
            eprintln!(
                "touch_ui_error=device_grab_failed device={} fd={} err={}",
                args.device, fd, e
            );
            return;
        }
        grabbed_fd.store(fd, Ordering::SeqCst);

        if !args.quiet {
            eprintln!(
                "touch_ui_status=touch_thread_running device={} max_x={} max_y={} width={} height={} rotation={}",
                args.device,
                map_cfg.max_x,
                map_cfg.max_y,
                map_cfg.width,
                map_cfg.height,
                map_cfg.rotation
            );
        }

        let mut cur_slot: i32 = 0;
        let mut slots: HashMap<i32, SlotState> = HashMap::new();
        let mut pending: BTreeMap<i32, PendingTouch> = BTreeMap::new();

        loop {
            let ev = match read_input_event(&mut input) {
                Ok(v) => v,
                Err(e) => {
                    if e.kind() != io::ErrorKind::UnexpectedEof {
                        eprintln!("touch_ui_error=read_input_event_failed err={}", e);
                    }
                    return;
                }
            };

            let event_time = event_time_to_system_time(&ev);

            match ev.type_ {
                EV_ABS => match ev.code {
                    ABS_MT_SLOT => {
                        cur_slot = ev.value;
                    }
                    ABS_MT_TRACKING_ID => {
                        let slot = slots.entry(cur_slot).or_default();
                        if ev.value < 0 {
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
                            slot.pointer_id = ev.value;
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
                    }
                    ABS_MT_POSITION_X => {
                        let slot = slots.entry(cur_slot).or_default();
                        slot.raw_x = ev.value;
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
                    }
                    ABS_MT_POSITION_Y => {
                        let slot = slots.entry(cur_slot).or_default();
                        slot.raw_y = ev.value;
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
                    }
                    _ => {}
                },
                EV_SYN if ev.code == SYN_REPORT => {
                    if !pending.is_empty() {
                        for p in pending.values() {
                            if p.pointer_id < 0 {
                                continue;
                            }
                            let (x, y) = map_point(&map_cfg, p.raw_x, p.raw_y);
                            if let Some(event) = to_client_touch_event(p.kind, p.pointer_id, x, y) {
                                if let Err(e) = touch_stream.send_event(event) {
                                    eprintln!("touch_ui_error=touch_send_failed err={}", e);
                                    return;
                                }
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
                        pending.clear();
                    }
                }
                _ => {}
            }
        }
    });
}

fn point_in_rect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32) -> bool {
    px >= x && px <= (x + w) && py >= y && py <= (y + h)
}

fn point_in_circle(px: f32, py: f32, cx: f32, cy: f32, r: f32) -> bool {
    let dx = px - cx;
    let dy = py - cy;
    (dx * dx + dy * dy) <= r * r
}

fn set_pixel_blended(data: &mut [u8], width: i32, height: i32, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= width || y >= height {
        return;
    }

    let idx = ((y as usize) * (width as usize) + (x as usize)) * 4;
    let src_a = color[3] as u32;
    if src_a == 0 {
        return;
    }

    if src_a == 255 {
        data[idx] = color[0];
        data[idx + 1] = color[1];
        data[idx + 2] = color[2];
        data[idx + 3] = 255;
        return;
    }

    let inv_a = 255 - src_a;
    let dst_a = data[idx + 3] as u32;

    data[idx] = ((color[0] as u32 * src_a + data[idx] as u32 * inv_a) / 255) as u8;
    data[idx + 1] = ((color[1] as u32 * src_a + data[idx + 1] as u32 * inv_a) / 255) as u8;
    data[idx + 2] = ((color[2] as u32 * src_a + data[idx + 2] as u32 * inv_a) / 255) as u8;
    data[idx + 3] = (src_a + (dst_a * inv_a) / 255).min(255) as u8;
}

fn clear_pixmap(pixmap: &mut Pixmap, color: [u8; 4]) {
    for px in pixmap.data_mut().chunks_exact_mut(4) {
        px[0] = color[0];
        px[1] = color[1];
        px[2] = color[2];
        px[3] = color[3];
    }
}

fn fill_rect(pixmap: &mut Pixmap, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
    if w <= 0 || h <= 0 {
        return;
    }

    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(width);
    let y1 = (y + h).min(height);

    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let data = pixmap.data_mut();
    for py in y0..y1 {
        for px in x0..x1 {
            set_pixel_blended(data, width, height, px, py, color);
        }
    }
}

fn stroke_rect(
    pixmap: &mut Pixmap,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    thickness: i32,
    color: [u8; 4],
) {
    if w <= 0 || h <= 0 || thickness <= 0 {
        return;
    }

    fill_rect(pixmap, x, y, w, thickness, color);
    fill_rect(pixmap, x, y + h - thickness, w, thickness, color);
    fill_rect(pixmap, x, y, thickness, h, color);
    fill_rect(pixmap, x + w - thickness, y, thickness, h, color);
}

fn fill_circle(pixmap: &mut Pixmap, cx: i32, cy: i32, radius: i32, color: [u8; 4]) {
    if radius <= 0 {
        return;
    }

    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let r2 = radius * radius;
    let data = pixmap.data_mut();

    let y0 = (cy - radius).max(0);
    let y1 = (cy + radius).min(height - 1);
    let x0 = (cx - radius).max(0);
    let x1 = (cx + radius).min(width - 1);

    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= r2 {
                set_pixel_blended(data, width, height, x, y, color);
            }
        }
    }
}

fn draw_char(pixmap: &mut Pixmap, x: i32, y: i32, ch: char, scale: i32, color: [u8; 4]) {
    let Some(glyph) = BASIC_FONTS.get(ch).or_else(|| BASIC_FONTS.get('?')) else {
        return;
    };

    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let data = pixmap.data_mut();

    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8 {
            if ((bits >> col) & 1) == 0 {
                continue;
            }

            let px = x + (col as i32) * scale;
            let py = y + (row as i32) * scale;

            for sy in 0..scale {
                for sx in 0..scale {
                    set_pixel_blended(data, width, height, px + sx, py + sy, color);
                }
            }
        }
    }
}

fn draw_text(pixmap: &mut Pixmap, x: i32, y: i32, text: &str, scale: i32, color: [u8; 4]) {
    let mut cx = x;
    let mut cy = y;
    let step_x = 8 * scale + scale;
    let step_y = 10 * scale;

    for ch in text.chars() {
        if ch == '\n' {
            cx = x;
            cy += step_y;
            continue;
        }
        draw_char(pixmap, cx, cy, ch, scale, color);
        cx += step_x;
    }
}

fn draw_ui(pixmap: &mut Pixmap, ui: &UiState, fps: f32) {
    // Bugfix: 每帧先清为全透明，避免遮挡底层 Android 画面。
    clear_pixmap(pixmap, [0, 0, 0, 0]);

    match ui.mode {
        UiMode::Window => {
            let wx = ui.window.x.round() as i32;
            let wy = ui.window.y.round() as i32;
            let ww = ui.window.w.round() as i32;
            let wh = ui.window.h.round() as i32;
            let title_h = TITLE_BAR_H.round() as i32;

            fill_rect(pixmap, wx + 6, wy + 10, ww, wh, [0, 0, 0, 100]);
            fill_rect(pixmap, wx, wy, ww, wh, [30, 34, 46, 245]);
            stroke_rect(pixmap, wx, wy, ww, wh, 2, [110, 130, 160, 255]);
            fill_rect(pixmap, wx, wy, ww, title_h, [46, 64, 98, 255]);

            let (cx, cy, cw, ch) = ui.close_button_rect();
            fill_rect(
                pixmap,
                cx.round() as i32,
                cy.round() as i32,
                cw.round() as i32,
                ch.round() as i32,
                [220, 68, 68, 255],
            );
            draw_text(
                pixmap,
                cx.round() as i32 + 12,
                cy.round() as i32 + 10,
                "X",
                2,
                [255, 255, 255, 255],
            );

            let (bx, by, bw, bh) = ui.minimize_button_rect();
            fill_rect(
                pixmap,
                bx.round() as i32,
                by.round() as i32,
                bw.round() as i32,
                bh.round() as i32,
                [220, 90, 90, 255],
            );
            draw_text(
                pixmap,
                bx.round() as i32 + 10,
                by.round() as i32 + 8,
                "-",
                2,
                [255, 255, 255, 255],
            );

            let (rx, ry, rw, rh) = ui.resize_handle_rect();
            fill_rect(
                pixmap,
                rx.round() as i32,
                ry.round() as i32,
                rw.round() as i32,
                rh.round() as i32,
                [245, 190, 70, 255],
            );
            stroke_rect(
                pixmap,
                rx.round() as i32,
                ry.round() as i32,
                rw.round() as i32,
                rh.round() as i32,
                1,
                [255, 220, 120, 255],
            );

            draw_text(
                pixmap,
                wx + 16,
                wy + 14,
                "TOUCH UI TEST",
                2,
                [240, 246, 255, 255],
            );

            let info = format!(
                "FPS: {:.2}\nWIN X:{:.1} Y:{:.1}\nWIN W:{:.1} H:{:.1}\nTOUCH X:{:.1} Y:{:.1}\nTOUCH LAT(ms): {:.2}",
                fps,
                ui.window.x,
                ui.window.y,
                ui.window.w,
                ui.window.h,
                ui.last_touch_x,
                ui.last_touch_y,
                ui.last_latency_ms
            );
            draw_text(
                pixmap,
                wx + 16,
                wy + title_h + 18,
                &info,
                2,
                [220, 228, 238, 255],
            );
        }
        UiMode::Minimized => {
            let cx = ui.bubble.cx.round() as i32;
            let cy = ui.bubble.cy.round() as i32;
            let r = ui.bubble.r.round() as i32;

            fill_circle(pixmap, cx + 4, cy + 6, r, [0, 0, 0, 100]);
            fill_circle(pixmap, cx, cy, r, [48, 138, 240, 250]);
            fill_circle(pixmap, cx, cy, r - 6, [18, 56, 112, 255]);
            draw_text(pixmap, cx - 14, cy - 10, "UI", 2, [240, 248, 255, 255]);
        }
    }
}

fn drain_touch_events(rx: &Receiver<TouchMessage>, ui: &mut UiState, screen_w: f32, screen_h: f32) {
    while let Ok(msg) = rx.try_recv() {
        ui.on_touch(msg, screen_w, screen_h);
    }
}

fn main() {
    let cleanup_grabbed_fd = |grabbed_fd: &AtomicI32| {
        release_input_grab(grabbed_fd);
    };

    let args: Vec<String> = std::env::args().collect();
    let cfg = match parse_args(&args) {
        Ok(v) => v,
        Err(e) => {
            usage();
            eprintln!("touch_ui_error=parse_args_failed reason={}", e);
            std::process::exit(1);
        }
    };

    let mut client = match DsapiClient::connect(&cfg.socket) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "touch_ui_error=daemon_connect_failed socket={} err={}",
                cfg.socket, e
            );
            std::process::exit(2);
        }
    };

    let display = match client.get_display_info() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("touch_ui_error=display_get_failed err={}", e);
            std::process::exit(3);
        }
    };

    let screen_w = display.width;
    let screen_h = display.height;
    let rotation = cfg.rotation.unwrap_or(display.rotation as i32);

    let map_cfg = TouchMapConfig {
        max_x: cfg.max_x,
        max_y: cfg.max_y,
        width: i32::try_from(screen_w).unwrap_or(i32::MAX),
        height: i32::try_from(screen_h).unwrap_or(i32::MAX),
        rotation,
    };

    let mut pixmap = match Pixmap::new(screen_w, screen_h) {
        Some(v) => v,
        None => {
            eprintln!(
                "touch_ui_error=pixmap_create_failed width={} height={}",
                screen_w, screen_h
            );
            std::process::exit(4);
        }
    };

    let mut render_session = match client.create_render_session() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("touch_ui_error=create_render_session_failed err={}", e);
            std::process::exit(5);
        }
    };

    let mut ui = UiState::new(screen_w as f32, screen_h as f32);

    let grabbed_fd = Arc::new(AtomicI32::new(-1));
    let (touch_tx, touch_rx) = mpsc::channel::<TouchMessage>();
    spawn_touch_thread(cfg.clone(), map_cfg, touch_tx, Arc::clone(&grabbed_fd));

    if !cfg.quiet {
        eprintln!(
            "touch_ui_status=running display={}x{} hz={:.2} dpi={} rotation={} target_fps={}",
            display.width,
            display.height,
            display.refresh_hz,
            display.density_dpi,
            rotation,
            cfg.target_fps
        );
    }

    let target_frame = Duration::from_secs_f64(1.0 / cfg.target_fps as f64);
    let mut fps = 0.0f32;
    let mut fps_count = 0u32;
    let mut fps_window = Instant::now();
    let mut log_window = Instant::now();

    loop {
        let frame_start = Instant::now();

        // 先消费触摸事件，再绘制本帧，降低交互到画面的延迟。
        drain_touch_events(&touch_rx, &mut ui, screen_w as f32, screen_h as f32);
        if ui.should_exit() {
            cleanup_grabbed_fd(grabbed_fd.as_ref());
            std::process::exit(0);
        }
        draw_ui(&mut pixmap, &ui, fps);

        if let Err(e) = render_session.submit_frame(pixmap.data()) {
            eprintln!("touch_ui_error=submit_frame_failed err={}", e);
            cleanup_grabbed_fd(grabbed_fd.as_ref());
            std::process::exit(6);
        }

        if let Err(e) = client.render_present() {
            eprintln!("touch_ui_error=render_present_failed err={}", e);
            cleanup_grabbed_fd(grabbed_fd.as_ref());
            std::process::exit(7);
        }

        fps_count = fps_count.saturating_add(1);
        let elapsed = fps_window.elapsed();
        if elapsed >= Duration::from_millis(1000) {
            fps = fps_count as f32 / elapsed.as_secs_f32();
            fps_count = 0;
            fps_window = Instant::now();
        }

        if !cfg.quiet && log_window.elapsed() >= Duration::from_millis(1000) {
            log_window = Instant::now();
            eprintln!(
                "touch_ui_stats fps={:.2} mode={:?} touch=({:.1},{:.1}) latency_ms={:.2}",
                fps, ui.mode, ui.last_touch_x, ui.last_touch_y, ui.last_latency_ms
            );
        }

        let spent = frame_start.elapsed();
        if spent < target_frame {
            thread::sleep(target_frame - spent);
        }
    }
}
