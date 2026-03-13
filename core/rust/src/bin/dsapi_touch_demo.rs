#![allow(clippy::too_many_arguments)]
use std::fs;
use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use directscreen_core::api::{
    Status, KEYBOARD_EVENT_KIND_BACKSPACE, KEYBOARD_EVENT_KIND_CHAR, KEYBOARD_EVENT_KIND_DONE,
    KEYBOARD_EVENT_KIND_FOCUS_OFF, KEYBOARD_EVENT_KIND_FOCUS_ON,
};
use directscreen_core::client::{
    spawn_touch_router, TouchMessage, TouchPhase, TouchRouterConfig, TouchRouterHandle,
};
use directscreen_core::util::{
    ctl_wire, default_control_socket_path, derive_data_socket_path_text,
};
use evdev::{AbsoluteAxisType, Device};

const WINDOW_ARG_MIN_W: f32 = 120.0;
const WINDOW_ARG_MIN_H: f32 = 90.0;
const RESIZE_SNAP_PX: f32 = 4.0;
const BASE_TITLE_HEIGHT_DP: f32 = 44.0;
const BASE_RESIZE_HIT_DP: f32 = 42.0;
const BASE_WINDOW_MIN_W_DP: f32 = 240.0;
const BASE_WINDOW_MIN_H_DP: f32 = 170.0;
const BASE_CLOSE_W_DP: f32 = 40.0;
const BASE_CLOSE_H_DP: f32 = 32.0;
const BASE_CLOSE_MARGIN_DP: f32 = 8.0;
const BASE_CLOSE_TOP_DP: f32 = 6.0;
const BASE_ACTION_BUTTON_W_DP: f32 = 96.0;
const BASE_ACTION_BUTTON_H_DP: f32 = 34.0;
const BASE_INPUT_H_DP: f32 = 36.0;
const BASE_CONTENT_PADDING_DP: f32 = 12.0;
const BASE_IME_KEY_GAP_DP: f32 = 4.0;
const CURSOR_BLINK_MS: u64 = 420;
const WATCH_EVENT_TIMEOUT_MS: u32 = 120;

#[derive(Debug, Clone, Copy)]
struct DisplayInfo {
    width: u32,
    height: u32,
    refresh_hz: f32,
    density_dpi: u32,
    rotation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CoreKeyboardEvent {
    seq: u64,
    kind: u32,
    codepoint: u32,
}

#[derive(Debug)]
struct BinaryControlClient {
    stream: UnixStream,
    seq: u64,
}

impl BinaryControlClient {
    fn connect(socket_path: &str) -> io::Result<Self> {
        let stream = UnixStream::connect(socket_path)?;
        let timeout =
            directscreen_core::util::timeout_from_env("DSAPI_CLIENT_TIMEOUT_MS", 5000, 100);
        stream.set_read_timeout(timeout)?;
        stream.set_write_timeout(timeout)?;
        Ok(Self { stream, seq: 1 })
    }

    fn request(
        &mut self,
        cmd: &ctl_wire::BinaryCtlCommand,
    ) -> io::Result<ctl_wire::BinaryCtlResponse> {
        ctl_wire::write_request(&mut self.stream, self.seq, cmd)?;
        self.seq = self.seq.saturating_add(1);

        let Some(resp) = ctl_wire::read_response(&mut self.stream)? else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "control_socket_closed",
            ));
        };
        if resp.status != Status::Ok {
            return Err(io::Error::other(format!(
                "control_command_failed status={}",
                resp.status.as_str()
            )));
        }
        Ok(resp)
    }

    fn display_get(&mut self) -> io::Result<DisplayInfo> {
        let resp = self.request(&ctl_wire::BinaryCtlCommand::DisplayGet)?;
        Ok(DisplayInfo {
            width: resp.values[0] as u32,
            height: resp.values[1] as u32,
            refresh_hz: f32::from_bits(resp.values[2] as u32),
            density_dpi: resp.values[3] as u32,
            rotation: resp.values[4] as u32,
        })
    }

    fn display_wait_after(
        &mut self,
        last_seq: u64,
        timeout_ms: u32,
    ) -> io::Result<Option<(u64, DisplayInfo)>> {
        let resp = self.request(&ctl_wire::BinaryCtlCommand::DisplayWait {
            last_seq,
            timeout_ms,
        })?;
        if resp.values[6] == 0 {
            return Ok(None);
        }
        Ok(Some((
            resp.values[0],
            DisplayInfo {
                width: resp.values[1] as u32,
                height: resp.values[2] as u32,
                refresh_hz: f32::from_bits(resp.values[3] as u32),
                density_dpi: resp.values[4] as u32,
                rotation: resp.values[5] as u32,
            },
        )))
    }

    fn keyboard_inject(&mut self, kind: u32, codepoint: u32) -> io::Result<(u64, u32, u32)> {
        let resp = self.request(&ctl_wire::BinaryCtlCommand::KeyboardInject { kind, codepoint })?;
        Ok((resp.values[0], resp.values[1] as u32, resp.values[2] as u32))
    }

    fn keyboard_wait_after(
        &mut self,
        last_seq: u64,
        timeout_ms: u32,
    ) -> io::Result<Option<CoreKeyboardEvent>> {
        let resp = self.request(&ctl_wire::BinaryCtlCommand::KeyboardWait {
            last_seq,
            timeout_ms,
        })?;
        if resp.values[3] == 0 {
            return Ok(None);
        }
        Ok(Some(CoreKeyboardEvent {
            seq: resp.values[0],
            kind: resp.values[1] as u32,
            codepoint: resp.values[2] as u32,
        }))
    }
}

#[derive(Debug, Clone, Copy)]
struct UiMetrics {
    title_height: f32,
    resize_hit: f32,
    min_window_w: f32,
    min_window_h: f32,
    close_w: f32,
    close_h: f32,
    close_margin: f32,
    close_top: f32,
    action_button_w: f32,
    action_button_h: f32,
    input_h: f32,
    content_padding: f32,
    ime_key_gap: f32,
}

fn ui_metrics(display: DisplayInfo) -> UiMetrics {
    let density = if display.density_dpi > 0 {
        display.density_dpi as f32 / 160.0
    } else {
        1.0
    };
    let control_scale = (1.0 + (density - 1.0) * 0.18).clamp(1.0, 1.45);
    let min_scale = (1.0 + (density - 1.0) * 0.08).clamp(1.0, 1.20);
    let min_title_h = 20.0f32;

    UiMetrics {
        title_height: (BASE_TITLE_HEIGHT_DP * control_scale).max(min_title_h),
        resize_hit: BASE_RESIZE_HIT_DP * control_scale,
        min_window_w: BASE_WINDOW_MIN_W_DP * min_scale,
        min_window_h: BASE_WINDOW_MIN_H_DP * min_scale,
        close_w: BASE_CLOSE_W_DP * control_scale,
        close_h: BASE_CLOSE_H_DP * control_scale,
        close_margin: BASE_CLOSE_MARGIN_DP * control_scale,
        close_top: BASE_CLOSE_TOP_DP * control_scale,
        action_button_w: BASE_ACTION_BUTTON_W_DP * control_scale,
        action_button_h: BASE_ACTION_BUTTON_H_DP * control_scale,
        input_h: BASE_INPUT_H_DP * control_scale,
        content_padding: BASE_CONTENT_PADDING_DP * control_scale,
        ime_key_gap: BASE_IME_KEY_GAP_DP * control_scale,
    }
}

#[derive(Debug, Clone, Copy)]
struct RectF {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl RectF {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

#[derive(Debug, Clone)]
struct UiLayout {
    close_button: RectF,
    submit_button: RectF,
    input_box: RectF,
}

fn compute_ui_layout(window: WindowState, metrics: UiMetrics) -> UiLayout {
    let button_gap = metrics.ime_key_gap.max(2.0);
    let button_w = metrics.action_button_w.max(metrics.close_w).max(60.0);
    let button_h = metrics.action_button_h.max(metrics.close_h).max(24.0);

    let close_button = RectF {
        x: window.x + window.w - metrics.close_margin - button_w,
        y: window.y + metrics.close_top,
        w: button_w,
        h: button_h,
    };
    let submit_button = RectF {
        x: close_button.x - button_gap - button_w,
        y: close_button.y,
        w: button_w,
        h: button_h,
    };

    let content_left = window.x + metrics.content_padding;
    let content_right = window.x + window.w - metrics.content_padding;
    let content_w = (content_right - content_left).max(40.0);
    let input_top = window.y + metrics.title_height + metrics.content_padding;
    let input_box = RectF {
        x: content_left,
        y: input_top,
        w: content_w,
        h: metrics.input_h.max(22.0),
    };

    UiLayout {
        close_button,
        submit_button,
        input_box,
    }
}

#[derive(Debug, Clone)]
struct Args {
    control_socket: String,
    data_socket: String,
    state_pipe: String,
    presenter_pid: Option<i32>,
    device_path: Option<String>,
    route_name: String,
    fps: f32,
    run_seconds: Option<f32>,
    no_touch_router: bool,
    quiet: bool,
    window_x: Option<f32>,
    window_y: Option<f32>,
    window_w: f32,
    window_h: f32,
}

fn usage() {
    eprintln!("usage:");
    eprintln!("  dsapi_touch_demo [options]");
    eprintln!("options:");
    eprintln!("  --control-socket <path>   daemon control socket");
    eprintln!("  --data-socket <path>      daemon data socket");
    eprintln!("  --state-pipe <path>      write UI state line per frame to fifo/file");
    eprintln!("  --presenter-pid <pid>    presenter process pid for liveness guard");
    eprintln!("  --device <event_path>     touch input device path");
    eprintln!("  --route-name <name>       touch route name (default: demo-window)");
    eprintln!("  --fps <n>                 render fps cap, 0 means auto by refresh rate");
    eprintln!("  --run-seconds <n>         auto stop after n seconds");
    eprintln!("  --window-x <n>            initial window x");
    eprintln!("  --window-y <n>            initial window y");
    eprintln!("  --window-w <n>            initial window width (default: 620)");
    eprintln!("  --window-h <n>            initial window height (default: 440)");
    eprintln!("  --no-touch-router         render only, do not capture touch");
    eprintln!("  --quiet                   suppress runtime logs");
    eprintln!("  -h, --help                show this help");
}

fn parse_f32(name: &str, raw: &str) -> Result<f32, String> {
    raw.parse::<f32>()
        .map_err(|_| format!("invalid_{}_value:{}", name, raw))
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut control_socket = env_non_empty("DSAPI_CONTROL_SOCKET_PATH")
        .or_else(|| env_non_empty("DSAPI_SOCKET_PATH"))
        .unwrap_or_else(default_control_socket_path);
    let mut data_socket: Option<String> = env_non_empty("DSAPI_DATA_SOCKET_PATH");
    let mut state_pipe: Option<String> = env_non_empty("TOUCH_DEMO_STATE_PIPE");
    let mut presenter_pid: Option<i32> = env_non_empty("TOUCH_DEMO_PRESENTER_PID")
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|v| *v > 1);
    let mut device_path: Option<String> = None;
    let mut route_name = "demo-window".to_string();
    let mut fps = 0.0f32;
    let mut run_seconds: Option<f32> = None;
    let mut no_touch_router = false;
    let mut quiet = false;
    let mut window_x: Option<f32> = None;
    let mut window_y: Option<f32> = None;
    let mut window_w = 620.0f32;
    let mut window_h = 440.0f32;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--control-socket" => {
                if i + 1 >= args.len() {
                    return Err("missing_control_socket".to_string());
                }
                control_socket = args[i + 1].clone();
                i += 2;
            }
            "--data-socket" => {
                if i + 1 >= args.len() {
                    return Err("missing_data_socket".to_string());
                }
                data_socket = Some(args[i + 1].clone());
                i += 2;
            }
            "--state-pipe" => {
                if i + 1 >= args.len() {
                    return Err("missing_state_pipe".to_string());
                }
                state_pipe = Some(args[i + 1].clone());
                i += 2;
            }
            "--presenter-pid" => {
                if i + 1 >= args.len() {
                    return Err("missing_presenter_pid".to_string());
                }
                let pid = args[i + 1]
                    .parse::<i32>()
                    .map_err(|_| format!("invalid_presenter_pid:{}", args[i + 1]))?;
                if pid <= 1 {
                    return Err(format!("invalid_presenter_pid:{}", pid));
                }
                presenter_pid = Some(pid);
                i += 2;
            }
            "--device" => {
                if i + 1 >= args.len() {
                    return Err("missing_device_path".to_string());
                }
                device_path = Some(args[i + 1].clone());
                i += 2;
            }
            "--route-name" => {
                if i + 1 >= args.len() {
                    return Err("missing_route_name".to_string());
                }
                route_name = args[i + 1].clone();
                i += 2;
            }
            "--fps" => {
                if i + 1 >= args.len() {
                    return Err("missing_fps".to_string());
                }
                fps = parse_f32("fps", &args[i + 1])?;
                i += 2;
            }
            "--run-seconds" => {
                if i + 1 >= args.len() {
                    return Err("missing_run_seconds".to_string());
                }
                run_seconds = Some(parse_f32("run_seconds", &args[i + 1])?);
                i += 2;
            }
            "--window-x" => {
                if i + 1 >= args.len() {
                    return Err("missing_window_x".to_string());
                }
                window_x = Some(parse_f32("window_x", &args[i + 1])?);
                i += 2;
            }
            "--window-y" => {
                if i + 1 >= args.len() {
                    return Err("missing_window_y".to_string());
                }
                window_y = Some(parse_f32("window_y", &args[i + 1])?);
                i += 2;
            }
            "--window-w" => {
                if i + 1 >= args.len() {
                    return Err("missing_window_w".to_string());
                }
                window_w = parse_f32("window_w", &args[i + 1])?;
                i += 2;
            }
            "--window-h" => {
                if i + 1 >= args.len() {
                    return Err("missing_window_h".to_string());
                }
                window_h = parse_f32("window_h", &args[i + 1])?;
                i += 2;
            }
            "--no-touch-router" => {
                no_touch_router = true;
                i += 1;
            }
            "--quiet" => {
                quiet = true;
                i += 1;
            }
            "-h" | "--help" => {
                return Err("help".to_string());
            }
            other => {
                return Err(format!("unknown_arg:{}", other));
            }
        }
    }

    if route_name.trim().is_empty() {
        return Err("route_name_empty".to_string());
    }
    if !fps.is_finite() || fps < 0.0 || (fps > 0.0 && fps <= 1.0) {
        return Err("fps_must_be_zero_or_gt_1".to_string());
    }
    if let Some(v) = run_seconds {
        if !v.is_finite() || v <= 0.0 {
            return Err("run_seconds_must_be_gt_0".to_string());
        }
    }
    if !window_w.is_finite()
        || !window_h.is_finite()
        || window_w < WINDOW_ARG_MIN_W
        || window_h < WINDOW_ARG_MIN_H
    {
        return Err("window_size_invalid".to_string());
    }

    let state_pipe = state_pipe
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "state_pipe_required".to_string())?;

    Ok(Args {
        data_socket: data_socket.unwrap_or_else(|| derive_data_socket_path_text(&control_socket)),
        state_pipe,
        presenter_pid,
        control_socket,
        device_path,
        route_name,
        fps,
        run_seconds,
        no_touch_router,
        quiet,
        window_x,
        window_y,
        window_w,
        window_h,
    })
}

#[derive(Debug, Clone, Copy)]
struct WindowState {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    visible: bool,
}

impl WindowState {
    fn contains(&self, x: f32, y: f32) -> bool {
        self.visible && x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn clamp_to_display(&mut self, display: DisplayInfo, metrics: UiMetrics) {
        let max_w = display.width as f32;
        let max_h = display.height as f32;
        self.w = self
            .w
            .clamp(metrics.min_window_w, max_w.max(metrics.min_window_w));
        self.h = self
            .h
            .clamp(metrics.min_window_h, max_h.max(metrics.min_window_h));
        self.x = self.x.clamp(0.0, (max_w - self.w).max(0.0));
        self.y = self.y.clamp(0.0, (max_h - self.h).max(0.0));
    }

    fn title_hit(&self, x: f32, y: f32, metrics: UiMetrics) -> bool {
        self.contains(x, y) && y < self.y + metrics.title_height
    }

    fn resize_hit(&self, x: f32, y: f32, metrics: UiMetrics) -> bool {
        self.contains(x, y)
            && x >= self.x + self.w - metrics.resize_hit
            && y >= self.y + self.h - metrics.resize_hit
    }
}

fn snap_resize_edge(value: f32) -> f32 {
    if !value.is_finite() {
        return value;
    }
    let step = RESIZE_SNAP_PX.max(1.0);
    (value / step).round() * step
}

#[derive(Debug, Clone, Copy)]
struct RouteSnapshot {
    window: WindowState,
    scale_x: f32,
    scale_y: f32,
    exclusive_input: bool,
}

#[derive(Debug, Clone, Copy)]
enum Interaction {
    Idle,
    Press {
        pointer_id: i32,
        target: PressTarget,
        active: bool,
    },
    Drag {
        pointer_id: i32,
        start_x: f32,
        start_y: f32,
        origin_x: f32,
        origin_y: f32,
    },
    Resize {
        pointer_id: i32,
        start_x: f32,
        start_y: f32,
        origin_w: f32,
        origin_h: f32,
    },
}

impl Interaction {
    fn label(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Press { .. } => "press",
            Self::Drag { .. } => "drag",
            Self::Resize { .. } => "resize",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressTarget {
    CloseButton,
    SubmitButton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonAction {
    Close,
    Submit,
}

#[derive(Debug)]
struct DemoState {
    window: WindowState,
    interaction: Interaction,
    running: bool,
    ui_event_count: u64,
    last_latency_ms: f32,
    input_text: String,
    input_focused: bool,
    ime_visible: bool,
    cursor_visible: bool,
    cursor_last_toggle: Instant,
    submit_count: u64,
    button_callback_count: u64,
    last_submit_text: String,
    last_button_action: Option<ButtonAction>,
    close_flash_until: Option<Instant>,
    submit_flash_until: Option<Instant>,
    pending_focus_signal: Option<u32>,
}

impl DemoState {
    fn new(window: WindowState) -> Self {
        Self {
            window,
            interaction: Interaction::Idle,
            running: true,
            ui_event_count: 0,
            last_latency_ms: 0.0,
            input_text: String::new(),
            input_focused: false,
            ime_visible: false,
            cursor_visible: false,
            cursor_last_toggle: Instant::now(),
            submit_count: 0,
            button_callback_count: 0,
            last_submit_text: String::new(),
            last_button_action: None,
            close_flash_until: None,
            submit_flash_until: None,
            pending_focus_signal: None,
        }
    }

    fn tick(&mut self, now: Instant) {
        if self.input_focused {
            if now.duration_since(self.cursor_last_toggle).as_millis() as u64 >= CURSOR_BLINK_MS {
                self.cursor_last_toggle = now;
                self.cursor_visible = !self.cursor_visible;
            }
        } else {
            self.cursor_visible = false;
            self.cursor_last_toggle = now;
        }
    }

    fn input_exclusive(&self) -> bool {
        self.input_focused
    }

    fn flash_active(flash_until: Option<Instant>, now: Instant) -> bool {
        flash_until.map(|deadline| now < deadline).unwrap_or(false)
    }

    fn press_active(&self, target: PressTarget) -> bool {
        matches!(
            self.interaction,
            Interaction::Press {
                target: current,
                active: true,
                ..
            } if current == target
        )
    }

    fn hit_press_target(&self, x: f32, y: f32, layout: &UiLayout) -> Option<PressTarget> {
        if layout.close_button.contains(x, y) {
            return Some(PressTarget::CloseButton);
        }
        if layout.submit_button.contains(x, y) {
            return Some(PressTarget::SubmitButton);
        }
        None
    }

    fn set_focus(&mut self, focused: bool, now: Instant, emit_signal: bool) {
        let changed = self.input_focused != focused;
        self.input_focused = focused;
        self.ime_visible = focused;
        self.cursor_visible = focused;
        self.cursor_last_toggle = now;
        if emit_signal && changed {
            self.pending_focus_signal = Some(if focused {
                KEYBOARD_EVENT_KIND_FOCUS_ON
            } else {
                KEYBOARD_EVENT_KIND_FOCUS_OFF
            });
        }
    }

    fn take_pending_focus_signal(&mut self) -> Option<u32> {
        self.pending_focus_signal.take()
    }

    fn apply_core_keyboard_event(&mut self, event: CoreKeyboardEvent, now: Instant) {
        match event.kind {
            KEYBOARD_EVENT_KIND_CHAR => {
                if self.input_focused {
                    if let Some(ch) = char::from_u32(event.codepoint) {
                        if self.input_text.chars().count() < 64 {
                            self.input_text.push(ch);
                        }
                    }
                }
            }
            KEYBOARD_EVENT_KIND_BACKSPACE => {
                if self.input_focused {
                    self.input_text.pop();
                }
            }
            KEYBOARD_EVENT_KIND_DONE => {
                self.set_focus(false, now, false);
            }
            KEYBOARD_EVENT_KIND_FOCUS_ON => {
                self.set_focus(true, now, false);
            }
            KEYBOARD_EVENT_KIND_FOCUS_OFF => {
                self.set_focus(false, now, false);
            }
            _ => {}
        }
    }

    fn invoke_button_action(&mut self, action: ButtonAction, now: Instant) {
        self.button_callback_count = self.button_callback_count.saturating_add(1);
        self.last_button_action = Some(action);
        match action {
            ButtonAction::Close => {
                self.close_flash_until = Some(now + Duration::from_millis(140));
                self.running = false;
            }
            ButtonAction::Submit => {
                self.submit_flash_until = Some(now + Duration::from_millis(140));
                self.submit_count = self.submit_count.saturating_add(1);
                self.last_submit_text = self.input_text.clone();
            }
        }
    }

    fn apply_touch(&mut self, msg: TouchMessage, display: DisplayInfo) {
        let metrics = ui_metrics(display);
        let now = Instant::now();
        self.last_latency_ms = msg.latency_ms;
        let layout = compute_ui_layout(self.window, metrics);
        match msg.phase {
            TouchPhase::Clear => {
                self.interaction = Interaction::Idle;
            }
            TouchPhase::Down => {
                self.ui_event_count = self.ui_event_count.saturating_add(1);
                if let Some(target) = self.hit_press_target(msg.x, msg.y, &layout) {
                    // Close should respond immediately to avoid missed Up events on some devices.
                    if target == PressTarget::CloseButton {
                        self.interaction = Interaction::Idle;
                        self.invoke_button_action(ButtonAction::Close, now);
                        return;
                    }
                    self.interaction = Interaction::Press {
                        pointer_id: msg.pointer_id,
                        target,
                        active: true,
                    };
                    return;
                }

                if layout.input_box.contains(msg.x, msg.y) {
                    self.set_focus(true, now, true);
                    self.interaction = Interaction::Idle;
                    return;
                }

                if self.input_exclusive() && !self.window.contains(msg.x, msg.y) {
                    self.interaction = Interaction::Idle;
                    return;
                }

                if self.window.resize_hit(msg.x, msg.y, metrics) {
                    self.interaction = Interaction::Resize {
                        pointer_id: msg.pointer_id,
                        start_x: msg.x,
                        start_y: msg.y,
                        origin_w: self.window.w,
                        origin_h: self.window.h,
                    };
                    return;
                }

                if self.window.title_hit(msg.x, msg.y, metrics) {
                    self.interaction = Interaction::Drag {
                        pointer_id: msg.pointer_id,
                        start_x: msg.x,
                        start_y: msg.y,
                        origin_x: self.window.x,
                        origin_y: self.window.y,
                    };
                }
            }
            TouchPhase::Move => {
                self.ui_event_count = self.ui_event_count.saturating_add(1);
                match self.interaction {
                    Interaction::Press {
                        pointer_id,
                        target,
                        mut active,
                    } if pointer_id == msg.pointer_id => {
                        active = self.hit_press_target(msg.x, msg.y, &layout) == Some(target);
                        self.interaction = Interaction::Press {
                            pointer_id,
                            target,
                            active,
                        };
                    }
                    Interaction::Drag {
                        pointer_id,
                        start_x,
                        start_y,
                        origin_x,
                        origin_y,
                    } if pointer_id == msg.pointer_id => {
                        self.window.x = origin_x + (msg.x - start_x);
                        self.window.y = origin_y + (msg.y - start_y);
                        self.window.clamp_to_display(display, metrics);
                    }
                    Interaction::Resize {
                        pointer_id,
                        start_x,
                        start_y,
                        origin_w,
                        origin_h,
                    } if pointer_id == msg.pointer_id => {
                        self.window.w = snap_resize_edge(origin_w + (msg.x - start_x));
                        self.window.h = snap_resize_edge(origin_h + (msg.y - start_y));
                        self.window.clamp_to_display(display, metrics);
                    }
                    _ => {}
                }
            }
            TouchPhase::Up => {
                self.ui_event_count = self.ui_event_count.saturating_add(1);
                match self.interaction {
                    Interaction::Press {
                        pointer_id,
                        target,
                        active,
                    } if pointer_id == msg.pointer_id => {
                        let released_on_target =
                            active && self.hit_press_target(msg.x, msg.y, &layout) == Some(target);
                        self.interaction = Interaction::Idle;
                        if released_on_target {
                            match target {
                                PressTarget::CloseButton => {
                                    self.invoke_button_action(ButtonAction::Close, now);
                                }
                                PressTarget::SubmitButton => {
                                    self.invoke_button_action(ButtonAction::Submit, now);
                                }
                            }
                        }
                    }
                    Interaction::Drag { pointer_id, .. }
                    | Interaction::Resize { pointer_id, .. }
                        if pointer_id == msg.pointer_id =>
                    {
                        self.interaction = Interaction::Idle;
                    }
                    _ => {}
                }
            }
        }
    }
}

struct FpsCounter {
    start_at: Instant,
    frames: u32,
    value: f32,
}

impl FpsCounter {
    fn new() -> Self {
        Self {
            start_at: Instant::now(),
            frames: 0,
            value: 0.0,
        }
    }

    fn on_frame(&mut self) -> f32 {
        self.frames = self.frames.saturating_add(1);
        let elapsed = self.start_at.elapsed().as_secs_f32();
        if elapsed >= 0.5 {
            self.value = self.frames as f32 / elapsed.max(0.001);
            self.frames = 0;
            self.start_at = Instant::now();
        }
        self.value
    }
}

fn resolve_target_fps(user_fps: f32, refresh_hz: f32) -> f32 {
    if user_fps > 1.0 {
        return user_fps;
    }
    if refresh_hz.is_finite() && refresh_hz > 1.0 {
        return refresh_hz.clamp(30.0, 144.0);
    }
    60.0
}

fn frame_interval_from_fps(fps: f32) -> Duration {
    Duration::from_secs_f64((1.0f32 / fps.max(1.0)) as f64)
}

fn touch_scale(physical: DisplayInfo, render: DisplayInfo) -> (f32, f32) {
    let sx = render.width as f32 / (physical.width.max(1) as f32);
    let sy = render.height as f32 / (physical.height.max(1) as f32);
    (sx, sy)
}

fn rescale_window_for_render_change(
    window: &mut WindowState,
    old_render: DisplayInfo,
    new_render: DisplayInfo,
) {
    if old_render.width == 0 || old_render.height == 0 {
        return;
    }
    let sx = new_render.width as f32 / old_render.width as f32;
    let sy = new_render.height as f32 / old_render.height as f32;
    if !sx.is_finite() || !sy.is_finite() || sx <= 0.0 || sy <= 0.0 {
        return;
    }

    // 尺寸使用统一缩放，避免旋转后按 x/y 不同比例拉伸窗口。
    let uniform_scale = (sx * sy).sqrt();
    let old_cx = window.x + window.w * 0.5;
    let old_cy = window.y + window.h * 0.5;
    let new_cx = old_cx * sx;
    let new_cy = old_cy * sy;
    let new_w = window.w * uniform_scale;
    let new_h = window.h * uniform_scale;

    window.w = new_w;
    window.h = new_h;
    window.x = new_cx - new_w * 0.5;
    window.y = new_cy - new_h * 0.5;
}

#[derive(Debug)]
struct StatePipeWriter {
    path: String,
    out: fs::File,
}

impl StatePipeWriter {
    fn open(path: &str) -> io::Result<Self> {
        let out = fs::OpenOptions::new().write(true).open(path)?;
        Ok(Self {
            path: path.to_string(),
            out,
        })
    }

    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.out.write_all(line.as_bytes())?;
        self.out.write_all(b"\n")
    }
}

fn bool01(v: bool) -> u8 {
    if v {
        1
    } else {
        0
    }
}

fn encode_hex_text(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(2 + bytes.len().saturating_mul(2));
    out.push_str("0x");
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn encode_state_line(
    state: &DemoState,
    fps: f32,
    blocked: u64,
    passed: u64,
    now: Instant,
) -> String {
    let x = state.window.x.round() as i32;
    let y = state.window.y.round() as i32;
    let w = state.window.w.round().max(1.0) as i32;
    let h = state.window.h.round().max(1.0) as i32;
    let mode = state.interaction.label();
    let input_text = encode_hex_text(&state.input_text);
    let last_submit = encode_hex_text(&state.last_submit_text);
    let press_close = state.press_active(PressTarget::CloseButton);
    let press_submit = state.press_active(PressTarget::SubmitButton);
    let flash_close = DemoState::flash_active(state.close_flash_until, now);
    let flash_submit = DemoState::flash_active(state.submit_flash_until, now);

    format!(
        "x={} y={} w={} h={} focus={} ime={} cursor={} press_close={} press_submit={} flash_close={} flash_submit={} fps={:.2} blocked={} passed={} mode={} input_text={} last_submit={} ui_events={}",
        x,
        y,
        w,
        h,
        bool01(state.input_focused),
        bool01(state.ime_visible),
        bool01(state.cursor_visible),
        bool01(press_close),
        bool01(press_submit),
        bool01(flash_close),
        bool01(flash_submit),
        fps,
        blocked,
        passed,
        mode,
        input_text,
        last_submit,
        state.ui_event_count
    )
}

fn map_touch_to_render(
    msg: TouchMessage,
    physical: DisplayInfo,
    render: DisplayInfo,
) -> TouchMessage {
    let (sx, sy) = touch_scale(physical, render);
    let max_x = (render.width.saturating_sub(1)) as f32;
    let max_y = (render.height.saturating_sub(1)) as f32;
    TouchMessage {
        phase: msg.phase,
        pointer_id: msg.pointer_id,
        x: (msg.x * sx).clamp(0.0, max_x),
        y: (msg.y * sy).clamp(0.0, max_y),
        latency_ms: msg.latency_ms,
    }
}

fn spawn_display_watch(
    control_socket: String,
    quiet: bool,
    tx: mpsc::Sender<DisplayInfo>,
    running: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut client = match BinaryControlClient::connect(&control_socket) {
            Ok(v) => v,
            Err(e) => {
                if !quiet {
                    eprintln!(
                        "touch_demo_warn=display_wait_connect_failed socket={} err={}",
                        control_socket, e
                    );
                }
                return;
            }
        };

        let mut last_seq = 0u64;
        while running.load(Ordering::SeqCst) {
            match client.display_wait_after(last_seq, WATCH_EVENT_TIMEOUT_MS) {
                Ok(Some((next_seq, info))) => {
                    last_seq = next_seq.max(last_seq);
                    if tx.send(info).is_err() {
                        break;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    if !quiet {
                        eprintln!("touch_demo_warn=display_wait_failed err={}", e);
                    }
                    break;
                }
            }
        }
    })
}

fn spawn_keyboard_watch(
    control_socket: String,
    quiet: bool,
    tx: mpsc::Sender<CoreKeyboardEvent>,
    running: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut client = match BinaryControlClient::connect(&control_socket) {
            Ok(v) => v,
            Err(e) => {
                if !quiet {
                    eprintln!(
                        "touch_demo_warn=keyboard_wait_connect_failed socket={} err={}",
                        control_socket, e
                    );
                }
                return;
            }
        };

        let mut last_seq = 0u64;
        while running.load(Ordering::SeqCst) {
            match client.keyboard_wait_after(last_seq, WATCH_EVENT_TIMEOUT_MS) {
                Ok(Some(evt)) => {
                    last_seq = evt.seq.max(last_seq);
                    if tx.send(evt).is_err() {
                        break;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    if !quiet {
                        eprintln!("touch_demo_warn=keyboard_wait_failed err={}", e);
                    }
                    break;
                }
            }
        }
    })
}

fn read_pid_file(path: &str) -> Option<i32> {
    let raw = fs::read_to_string(path).ok()?;
    raw.trim().parse::<i32>().ok()
}

fn cmdline_contains_any(pid: i32, tokens: &[&str]) -> bool {
    if pid <= 1 {
        return false;
    }
    let path = format!("/proc/{}/cmdline", pid);
    let data = match fs::read(path) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if data.is_empty() {
        return false;
    }
    let mut merged = String::with_capacity(data.len());
    for b in data {
        if b == 0 {
            merged.push(' ');
        } else {
            merged.push(b as char);
        }
    }
    tokens.iter().any(|token| merged.contains(token))
}

fn pid_exists(pid: i32) -> bool {
    if pid <= 1 {
        return false;
    }
    fs::metadata(format!("/proc/{}/cmdline", pid)).is_ok()
}

fn stop_pid_quiet(pid: i32) {
    if pid <= 1 {
        return;
    }
    let _ = unsafe { libc::kill(pid, libc::SIGTERM) };
    thread::sleep(Duration::from_millis(20));
    let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
}

fn cleanup_touch_demo_module_state() {
    let Some(state_dir) = env_non_empty("DSAPI_MODULE_STATE_DIR") else {
        return;
    };
    let demo_pid_file = format!("{}/touch_demo.pid", state_dir);
    let presenter_pid_file = format!("{}/presenter.pid", state_dir);
    let presenter_pid = read_pid_file(&presenter_pid_file);
    let _ = fs::remove_file(&demo_pid_file);
    if let Some(pid) = presenter_pid {
        if cmdline_contains_any(
            pid,
            &["DSAPIPresenterDemo", "touch-ui-loop", "present-loop"],
        ) {
            stop_pid_quiet(pid);
        }
    }
    let _ = fs::remove_file(&presenter_pid_file);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg = match parse_args(&args) {
        Ok(v) => v,
        Err(e) if e == "help" => {
            usage();
            std::process::exit(0);
        }
        Err(e) => {
            usage();
            eprintln!("touch_demo_error=parse_args_failed reason={}", e);
            std::process::exit(2);
        }
    };

    if !cfg.quiet {
        eprintln!(
            "touch_demo_status=boot control_socket={} data_socket={} route={}",
            cfg.control_socket, cfg.data_socket, cfg.route_name
        );
    }

    let mut control = match BinaryControlClient::connect(&cfg.control_socket) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "touch_demo_error=control_connect_failed socket={} err={}",
                cfg.control_socket, e
            );
            std::process::exit(3);
        }
    };
    let mut display = match control.display_get() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("touch_demo_error=display_get_failed err={}", e);
            std::process::exit(4);
        }
    };
    if display.width == 0 || display.height == 0 {
        eprintln!(
            "touch_demo_error=display_invalid width={} height={}",
            display.width, display.height
        );
        std::process::exit(4);
    }
    let mut render_display = display;
    let (mut route_scale_x, mut route_scale_y) = touch_scale(display, render_display);
    let init_window_w = (cfg.window_w * route_scale_x).max(WINDOW_ARG_MIN_W * route_scale_x);
    let init_window_h = (cfg.window_h * route_scale_y).max(WINDOW_ARG_MIN_H * route_scale_y);

    let init_x = cfg
        .window_x
        .map(|v| v * route_scale_x)
        .unwrap_or_else(|| ((render_display.width as f32 - init_window_w) * 0.5).max(0.0));
    let init_y = cfg
        .window_y
        .map(|v| v * route_scale_y)
        .unwrap_or_else(|| ((render_display.height as f32 - init_window_h) * 0.35).max(0.0));
    let mut state = DemoState::new(WindowState {
        x: init_x,
        y: init_y,
        w: init_window_w,
        h: init_window_h,
        visible: true,
    });
    state
        .window
        .clamp_to_display(render_display, ui_metrics(render_display));

    let route_state = Arc::new(Mutex::new(RouteSnapshot {
        window: state.window,
        scale_x: route_scale_x,
        scale_y: route_scale_y,
        exclusive_input: state.input_exclusive(),
    }));
    let route_blocked_down = Arc::new(AtomicU64::new(0));
    let route_passed_down = Arc::new(AtomicU64::new(0));
    let (touch_tx, touch_rx) = mpsc::channel::<TouchMessage>();
    let (display_tx, display_rx) = mpsc::channel::<DisplayInfo>();
    let (keyboard_tx, keyboard_rx) = mpsc::channel::<CoreKeyboardEvent>();
    let display_watch_running = Arc::new(AtomicBool::new(true));
    let display_watch_handle = spawn_display_watch(
        cfg.control_socket.clone(),
        cfg.quiet,
        display_tx,
        Arc::clone(&display_watch_running),
    );
    let keyboard_watch_running = Arc::new(AtomicBool::new(true));
    let keyboard_watch_handle = spawn_keyboard_watch(
        cfg.control_socket.clone(),
        cfg.quiet,
        keyboard_tx,
        Arc::clone(&keyboard_watch_running),
    );

    let mut touch_handle: Option<TouchRouterHandle> = None;
    if !cfg.no_touch_router {
        let device_path = match cfg.device_path.clone() {
            Some(v) => v,
            None => match detect_touch_device() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("touch_demo_error=touch_device_not_found err={}", e);
                    if e.kind() == io::ErrorKind::PermissionDenied {
                        eprintln!(
                            "touch_demo_hint=permission_denied_reading_/dev/input_use_su_or_root"
                        );
                    }
                    eprintln!(
                        "touch_demo_hint=use_--device_/dev/input/eventX_or_--no-touch-router"
                    );
                    std::process::exit(5);
                }
            },
        };

        let route_blocked_ref = Arc::clone(&route_blocked_down);
        let route_passed_ref = Arc::clone(&route_passed_down);
        let router_cfg = TouchRouterConfig {
            control_socket_path: cfg.control_socket.clone(),
            data_socket_path: cfg.data_socket.clone(),
            route_name: cfg.route_name.clone(),
            device_path: device_path.clone(),
            screen_width: display.width,
            screen_height: display.height,
            rotation: display.rotation as i32,
            quiet: cfg.quiet,
        };

        let router = match spawn_touch_router(
            router_cfg,
            Arc::clone(&route_state),
            move |snapshot: RouteSnapshot, x: f32, y: f32| {
                let mapped_x = x * snapshot.scale_x;
                let mapped_y = y * snapshot.scale_y;
                // Keep system interaction available outside the demo window.
                // Input focus is handled by UI state, not by globally swallowing touches.
                let blocked = snapshot.window.contains(mapped_x, mapped_y);
                if blocked {
                    route_blocked_ref.fetch_add(1, Ordering::Relaxed);
                } else {
                    route_passed_ref.fetch_add(1, Ordering::Relaxed);
                }
                blocked
            },
            touch_tx,
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "touch_demo_error=touch_router_start_failed device={} err={}",
                    device_path, e
                );
                eprintln!("touch_demo_hint=root_permission_and_uinput_access_are_required");
                std::process::exit(6);
            }
        };

        if !cfg.quiet {
            eprintln!(
                "touch_demo_status=touch_router_started device={}",
                device_path
            );
        }
        touch_handle = Some(router);
    } else if !cfg.quiet {
        eprintln!("touch_demo_status=touch_router_disabled mode=render_only");
    }

    let mut state_pipe = match StatePipeWriter::open(&cfg.state_pipe) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "touch_demo_error=state_pipe_open_failed path={} err={}",
                cfg.state_pipe, e
            );
            std::process::exit(7);
        }
    };

    let mut target_fps = resolve_target_fps(cfg.fps, display.refresh_hz);
    if !cfg.quiet {
        let init_frame_w = state.window.w.round().max(1.0) as u32;
        let init_frame_h = state.window.h.round().max(1.0) as u32;
        eprintln!(
            "touch_demo_status=running display={}x{} frame={}x{} @ {:.2}Hz dpi={} rotation={} target_fps={:.1} output=state_pipe",
            display.width,
            display.height,
            init_frame_w,
            init_frame_h,
            display.refresh_hz,
            display.density_dpi,
            display.rotation,
            target_fps
        );
        eprintln!(
            "touch_demo_status=state_pipe_enabled path={}",
            state_pipe.path
        );
        if let Some(presenter_pid) = cfg.presenter_pid {
            eprintln!("touch_demo_status=presenter_guard_enabled pid={}", presenter_pid);
        }
        if cfg.no_touch_router {
            eprintln!(
                "touch_demo_hint=touch_router_disabled_window_not_interactive use_ctrl_c_or_--run-seconds"
            );
        } else {
            eprintln!("touch_demo_hint=drag_title resize_bottom_right tap_close_or_send tap_input_for_system_ime");
        }
    }

    let mut fps_counter = FpsCounter::new();
    let mut frame_interval = frame_interval_from_fps(target_fps);
    let started_at = Instant::now();
    let mut last_perf_log = Instant::now();
    let mut last_presenter_probe = Instant::now() - Duration::from_secs(1);

    while state.running {
        if let Some(limit_s) = cfg.run_seconds {
            if started_at.elapsed().as_secs_f32() >= limit_s {
                break;
            }
        }
        if let Some(presenter_pid) = cfg.presenter_pid {
            if last_presenter_probe.elapsed() >= Duration::from_millis(220) {
                let alive = pid_exists(presenter_pid)
                    && cmdline_contains_any(
                        presenter_pid,
                        &["DSAPIPresenterDemo", "touch-ui-loop", "app_process"],
                    );
                if !alive {
                    eprintln!(
                        "touch_demo_error=presenter_not_alive pid={} action=stop_demo_to_avoid_invisible_touch_block",
                        presenter_pid
                    );
                    break;
                }
                last_presenter_probe = Instant::now();
            }
        }
        let frame_started = Instant::now();

        while let Ok(msg) = touch_rx.try_recv() {
            let mapped = map_touch_to_render(msg, display, render_display);
            state.apply_touch(mapped, render_display);
        }
        while let Ok(evt) = keyboard_rx.try_recv() {
            state.apply_core_keyboard_event(evt, Instant::now());
        }
        while let Ok(next) = display_rx.try_recv() {
            if next.width == 0 || next.height == 0 {
                continue;
            }
            let layout_changed = next.width != display.width
                || next.height != display.height
                || next.density_dpi != display.density_dpi
                || next.rotation != display.rotation;
            let refresh_changed =
                (next.refresh_hz - display.refresh_hz).abs() >= 0.01f32 && next.refresh_hz > 1.0;
            if layout_changed || refresh_changed {
                let old_render = render_display;
                display = next;
                render_display = display;
                if layout_changed {
                    rescale_window_for_render_change(&mut state.window, old_render, render_display);
                    (route_scale_x, route_scale_y) = touch_scale(display, render_display);
                    state
                        .window
                        .clamp_to_display(render_display, ui_metrics(render_display));
                    if let Some(handle) = touch_handle.as_ref() {
                        let _ = handle.update_display_transform(
                            display.width,
                            display.height,
                            display.rotation as i32,
                        );
                    }
                }
                if cfg.fps <= 1.0 {
                    target_fps = resolve_target_fps(cfg.fps, display.refresh_hz);
                    frame_interval = frame_interval_from_fps(target_fps);
                }
                if !cfg.quiet {
                    eprintln!(
                        "touch_demo_status=display_changed size={}x{} refresh_hz={:.2} dpi={} rotation={} target_fps={:.1}",
                        display.width,
                        display.height,
                        display.refresh_hz,
                        display.density_dpi,
                        display.rotation,
                        target_fps
                    );
                }
            }
        }

        if !state.running {
            break;
        }

        if let Ok(mut route) = route_state.lock() {
            route.window = state.window;
            route.scale_x = route_scale_x;
            route.scale_y = route_scale_y;
            route.exclusive_input = state.input_exclusive();
        }
        if let Some(kind) = state.take_pending_focus_signal() {
            if let Err(e) = control.keyboard_inject(kind, 0) {
                if !cfg.quiet {
                    eprintln!(
                        "touch_demo_warn=keyboard_signal_failed kind={} err={}",
                        kind, e
                    );
                }
            }
        }

        let frame_now = Instant::now();
        state.tick(frame_now);
        let fps = fps_counter.on_frame();
        let blocked = route_blocked_down.load(Ordering::Relaxed);
        let passed = route_passed_down.load(Ordering::Relaxed);
        let line = encode_state_line(&state, fps, blocked, passed, frame_now);
        if let Err(e) = state_pipe.write_line(&line) {
            eprintln!(
                "touch_demo_error=state_pipe_write_failed path={} err={}",
                state_pipe.path, e
            );
            break;
        }
        let elapsed = frame_started.elapsed();
        if !cfg.quiet && last_perf_log.elapsed() >= Duration::from_secs(1) {
            let frame_ms = elapsed.as_secs_f64() * 1000.0;
            eprintln!(
                "touch_demo_perf fps={:.1} target_fps={:.1} frame_ms={:.2} blocked={} passed={}",
                fps, target_fps, frame_ms, blocked, passed
            );
            last_perf_log = Instant::now();
        }
        if elapsed < frame_interval {
            thread::sleep(frame_interval - elapsed);
        }
    }

    cleanup_touch_demo_module_state();

    if let Some(mut handle) = touch_handle {
        handle.shutdown_and_join();
    }

    display_watch_running.store(false, Ordering::SeqCst);
    let _ = display_watch_handle.join();
    keyboard_watch_running.store(false, Ordering::SeqCst);
    let _ = keyboard_watch_handle.join();

    if !cfg.quiet {
        eprintln!("touch_demo_status=stopped");
    }
}

fn detect_touch_device() -> io::Result<String> {
    let mut paths: Vec<String> = Vec::new();
    for entry in fs::read_dir("/dev/input")? {
        let entry = entry?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name.starts_with("event") {
            paths.push(entry.path().to_string_lossy().to_string());
        }
    }
    paths.sort();
    for path in paths {
        let device = match Device::open(&path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if is_touch_device(&device) {
            return Ok(path);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "no_multi_touch_device_with_abs_mt_axes",
    ))
}

fn is_touch_device(device: &Device) -> bool {
    let Some(abs) = device.supported_absolute_axes() else {
        return false;
    };
    abs.contains(AbsoluteAxisType::ABS_MT_POSITION_X)
        && abs.contains(AbsoluteAxisType::ABS_MT_POSITION_Y)
        && abs.contains(AbsoluteAxisType::ABS_MT_SLOT)
        && abs.contains(AbsoluteAxisType::ABS_MT_TRACKING_ID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescale_window_for_render_change_preserves_aspect_ratio_on_rotation() {
        let old_display = DisplayInfo {
            width: 1440,
            height: 3168,
            refresh_hz: 120.0,
            density_dpi: 640,
            rotation: 0,
        };
        let new_display = DisplayInfo {
            width: 3168,
            height: 1440,
            refresh_hz: 120.0,
            density_dpi: 640,
            rotation: 1,
        };
        let mut window = WindowState {
            x: 200.0,
            y: 300.0,
            w: 520.0,
            h: 360.0,
            visible: true,
        };
        let old_ratio = window.w / window.h;
        rescale_window_for_render_change(&mut window, old_display, new_display);
        let new_ratio = window.w / window.h;

        assert!((old_ratio - new_ratio).abs() < 0.0001);
        assert!(window.w.is_finite() && window.h.is_finite());
        assert!(window.w > 1.0 && window.h > 1.0);
    }
}
