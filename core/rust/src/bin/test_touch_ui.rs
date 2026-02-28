use directscreen_core::client::{
    spawn_touch_router, DisplayEvent, DsapiClient, TouchMessage, TouchPhase, TouchRouterConfig,
    TouchRouterHandle,
};
use font8x8::{UnicodeFonts, BASIC_FONTS};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tiny_skia::{FillRule, Mask, Paint, PathBuilder, Pixmap, Rect, Transform};

const TITLE_BAR_H: f32 = 80.0;
const MIN_BTN_SIZE: f32 = 52.0;
const CLOSE_BTN_SIZE: f32 = 56.0;
const RESIZE_HANDLE_SIZE: f32 = 100.0;
const RESIZE_HANDLE_HIT_SIZE: f32 = 120.0;
const BUBBLE_RADIUS: f32 = 50.0;
const CLICK_MOVE_THRESHOLD: f32 = 10.0;
const MIN_WINDOW_W: f32 = 400.0;
const MIN_WINDOW_H: f32 = 400.0;

#[derive(Debug, Clone)]
struct Args {
    control_socket: String,
    data_socket: String,
    device: String,
    rotation: Option<i32>,
    target_fps: u32,
    quiet: bool,
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
struct UiRouteSnapshot {
    mode: UiMode,
    window: WindowRect,
    bubble: BubbleState,
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

impl UiState {
    fn new(screen_w: f32, screen_h: f32) -> Self {
        let max_w = (screen_w - 40.0).max(200.0);
        let max_h = (screen_h - 40.0).max(200.0);
        let min_w = MIN_WINDOW_W.min(max_w);
        let min_h = MIN_WINDOW_H.min(max_h);

        let base_w = (screen_w * 0.72).clamp(min_w, max_w);
        let base_h = (screen_h * 0.60).clamp(min_h, max_h);
        let window = WindowRect {
            x: ((screen_w - base_w) * 0.5).max(0.0),
            y: ((screen_h - base_h) * 0.5).max(0.0),
            w: base_w,
            h: base_h,
        };

        let bubble = BubbleState {
            cx: (screen_w - 100.0).max(BUBBLE_RADIUS + 6.0),
            cy: (screen_h * 0.35).max(BUBBLE_RADIUS + 6.0),
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
            TouchPhase::Down => self.on_touch_down(msg),
            TouchPhase::Move => self.on_touch_move(msg, screen_w, screen_h),
            TouchPhase::Up => self.on_touch_up(msg, screen_w, screen_h),
            TouchPhase::Clear => {
                self.active = None;
            }
        }
    }

    fn on_touch_down(&mut self, msg: TouchMessage) {
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
                    let min_w = MIN_WINDOW_W.min(screen_w.max(1.0));
                    let min_h = MIN_WINDOW_H.min(screen_h.max(1.0));
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
        let close_gap = 12.0;
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
        let min_w = MIN_WINDOW_W.min(screen_w.max(1.0));
        let min_h = MIN_WINDOW_H.min(screen_h.max(1.0));

        self.window.w = self.window.w.max(min_w).min(screen_w.max(min_w));
        self.window.h = self.window.h.max(min_h).min(screen_h.max(min_h));

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

    fn on_screen_resize(&mut self, screen_w: f32, screen_h: f32) {
        self.clamp_window(screen_w, screen_h);
        self.clamp_bubble(screen_w, screen_h);
    }

    fn should_exit(&self) -> bool {
        self.exit_requested
    }

    fn route_snapshot(&self) -> UiRouteSnapshot {
        UiRouteSnapshot {
            mode: self.mode,
            window: self.window,
            bubble: self.bubble,
        }
    }
}

fn default_control_socket_path() -> String {
    "artifacts/run/dsapi.sock".to_string()
}

fn derive_data_socket_path(control_socket_path: &str) -> String {
    if let Some(prefix) = control_socket_path.strip_suffix(".sock") {
        return format!("{}.data.sock", prefix);
    }
    format!("{}.data", control_socket_path)
}

fn usage() {
    eprintln!("usage:");
    eprintln!(
        "  test_touch_ui --device <event_path> [--control-socket <path>] [--data-socket <path>] [--rotation <0-3>] [--target-fps <n>] [--quiet]"
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
    let mut control_socket = default_control_socket_path();
    let mut data_socket = None::<String>;
    let mut device = String::new();
    let mut rotation = None::<i32>;
    let mut target_fps = 60u32;
    let mut quiet = false;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--control-socket" => {
                if i + 1 >= args.len() {
                    return Err("missing_control_socket_path".to_string());
                }
                control_socket = args[i + 1].clone();
                i += 2;
            }
            "--data-socket" => {
                if i + 1 >= args.len() {
                    return Err("missing_data_socket_path".to_string());
                }
                data_socket = Some(args[i + 1].clone());
                i += 2;
            }
            "--device" => {
                if i + 1 >= args.len() {
                    return Err("missing_device_path".to_string());
                }
                device = args[i + 1].clone();
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

    if let Some(r) = rotation {
        if !(0..=3).contains(&r) {
            return Err("rotation_must_be_0_3".to_string());
        }
    }

    if target_fps == 0 {
        return Err("target_fps_must_be_positive".to_string());
    }

    let data_socket = data_socket
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| derive_data_socket_path(&control_socket));
    if control_socket == data_socket {
        return Err("control_socket_and_data_socket_must_differ".to_string());
    }

    Ok(Args {
        control_socket,
        data_socket,
        device,
        rotation,
        target_fps,
        quiet,
    })
}

fn point_in_rect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32) -> bool {
    px >= x && px <= (x + w) && py >= y && py <= (y + h)
}

fn point_in_circle(px: f32, py: f32, cx: f32, cy: f32, r: f32) -> bool {
    let dx = px - cx;
    let dy = py - cy;
    (dx * dx + dy * dy) <= r * r
}

fn hit_ui_region(snapshot: UiRouteSnapshot, x: f32, y: f32) -> bool {
    match snapshot.mode {
        UiMode::Window => point_in_rect(
            x,
            y,
            snapshot.window.x,
            snapshot.window.y,
            snapshot.window.w,
            snapshot.window.h,
        ),
        UiMode::Minimized => point_in_circle(
            x,
            y,
            snapshot.bubble.cx,
            snapshot.bubble.cy,
            snapshot.bubble.r,
        ),
    }
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

fn fill_rect_masked(
    pixmap: &mut Pixmap,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: [u8; 4],
    mask: Option<&Mask>,
) {
    if w <= 0 || h <= 0 {
        return;
    }

    let Some(rect) = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = false;

    pixmap
        .as_mut()
        .fill_rect(rect, &paint, Transform::identity(), mask);
}

fn draw_char(
    pixmap: &mut Pixmap,
    x: i32,
    y: i32,
    ch: char,
    scale: i32,
    color: [u8; 4],
    clip_mask: Option<&Mask>,
) {
    let Some(glyph) = BASIC_FONTS.get(ch).or_else(|| BASIC_FONTS.get('?')) else {
        return;
    };

    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8 {
            if ((bits >> col) & 1) == 0 {
                continue;
            }

            let px = x + col * scale;
            let py = y + (row as i32) * scale;
            fill_rect_masked(pixmap, px, py, scale, scale, color, clip_mask);
        }
    }
}

fn draw_text(
    pixmap: &mut Pixmap,
    x: i32,
    y: i32,
    text: &str,
    scale: i32,
    color: [u8; 4],
    clip_mask: Option<&Mask>,
) {
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

        draw_char(pixmap, cx, cy, ch, scale, color, clip_mask);
        cx += step_x;
    }
}

fn make_rect_clip_mask(pixmap: &Pixmap, x: i32, y: i32, w: i32, h: i32) -> Option<Mask> {
    if w <= 0 || h <= 0 {
        return None;
    }

    let mut mask = Mask::new(pixmap.width(), pixmap.height())?;
    let rect = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32)?;
    let path = PathBuilder::from_rect(rect);
    mask.fill_path(&path, FillRule::Winding, false, Transform::identity());
    Some(mask)
}

fn draw_ui(pixmap: &mut Pixmap, ui: &UiState, fps: f32) {
    // Bugfix: 每帧先清为全透明，只绘制悬浮窗内容。
    clear_pixmap(pixmap, [0, 0, 0, 0]);

    match ui.mode {
        UiMode::Window => {
            let wx = ui.window.x.round() as i32;
            let wy = ui.window.y.round() as i32;
            let ww = ui.window.w.round() as i32;
            let wh = ui.window.h.round() as i32;
            let title_h = TITLE_BAR_H.round() as i32;

            fill_rect(pixmap, wx + 8, wy + 12, ww, wh, [0, 0, 0, 100]);
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
                cx.round() as i32 + 16,
                cy.round() as i32 + 14,
                "X",
                2,
                [255, 255, 255, 255],
                None,
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
                bx.round() as i32 + 16,
                by.round() as i32 + 12,
                "-",
                2,
                [255, 255, 255, 255],
                None,
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
                wy + 22,
                "TOUCH UI TEST",
                2,
                [240, 246, 255, 255],
                None,
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

            // Bugfix: 用 tiny-skia mask 裁剪正文文字，防止越界绘制到窗口外。
            let clip_x = wx + 14;
            let clip_y = wy + title_h + 8;
            let clip_w = (ww - 28).max(1);
            let clip_h = (wh - title_h - 16).max(1);
            let content_mask = make_rect_clip_mask(pixmap, clip_x, clip_y, clip_w, clip_h);

            draw_text(
                pixmap,
                wx + 16,
                wy + title_h + 18,
                &info,
                2,
                [220, 228, 238, 255],
                content_mask.as_ref(),
            );
        }
        UiMode::Minimized => {
            let cx = ui.bubble.cx.round() as i32;
            let cy = ui.bubble.cy.round() as i32;
            let r = ui.bubble.r.round() as i32;

            fill_circle(pixmap, cx + 4, cy + 6, r, [0, 0, 0, 100]);
            fill_circle(pixmap, cx, cy, r, [48, 138, 240, 250]);
            fill_circle(pixmap, cx, cy, r - 6, [18, 56, 112, 255]);
            draw_text(
                pixmap,
                cx - 14,
                cy - 10,
                "UI",
                2,
                [240, 248, 255, 255],
                None,
            );
        }
    }
}

fn drain_touch_events(rx: &Receiver<TouchMessage>, ui: &mut UiState, screen_w: f32, screen_h: f32) {
    while let Ok(msg) = rx.try_recv() {
        ui.on_touch(msg, screen_w, screen_h);
    }
}

fn publish_route_snapshot(route_state: &Arc<Mutex<UiRouteSnapshot>>, ui: &UiState) {
    if let Ok(mut snapshot) = route_state.lock() {
        *snapshot = ui.route_snapshot();
    }
}

fn render_transparent_frame_and_present(
    client: &mut DsapiClient,
    render_session: &mut directscreen_core::client::RenderSession,
    pixmap: &mut Pixmap,
) {
    clear_pixmap(pixmap, [0, 0, 0, 0]);
    let _ = render_session.submit_frame(pixmap.data());
    let _ = client.render_present();
}

fn spawn_display_forwarder(
    control_socket: String,
    data_socket: String,
    quiet: bool,
    display_tx: mpsc::Sender<DisplayEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || loop {
        let client = match DsapiClient::connect_with_data_socket(&control_socket, &data_socket) {
            Ok(v) => v,
            Err(e) => {
                if !quiet {
                    eprintln!("touch_ui_warn=display_client_connect_failed err={}", e);
                }
                thread::sleep(Duration::from_millis(300));
                continue;
            }
        };

        let mut display_stream = match client.subscribe_display() {
            Ok(v) => v,
            Err(e) => {
                if !quiet {
                    eprintln!("touch_ui_warn=display_subscribe_failed err={}", e);
                }
                thread::sleep(Duration::from_millis(300));
                continue;
            }
        };

        loop {
            match display_stream.next_event() {
                Ok(event) => {
                    if display_tx.send(event).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    if !quiet {
                        eprintln!("touch_ui_warn=display_stream_closed err={}", e);
                    }
                    break;
                }
            }
        }

        thread::sleep(Duration::from_millis(300));
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg = match parse_args(&args) {
        Ok(v) => v,
        Err(e) => {
            usage();
            eprintln!("touch_ui_error=parse_args_failed reason={}", e);
            std::process::exit(1);
        }
    };

    let mut client =
        match DsapiClient::connect_with_data_socket(&cfg.control_socket, &cfg.data_socket) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "touch_ui_error=daemon_connect_failed control_socket={} data_socket={} err={}",
                    cfg.control_socket, cfg.data_socket, e
                );
                std::process::exit(2);
            }
        };

    let mut display = match client.get_display_info() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("touch_ui_error=display_get_failed err={}", e);
            std::process::exit(3);
        }
    };

    let mut render_session = match client.create_render_session() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("touch_ui_error=create_render_session_failed err={}", e);
            std::process::exit(5);
        }
    };
    let mut rotation = cfg.rotation.unwrap_or(display.rotation as i32);
    // Android presenter 已经以当前朝向尺寸提交画面，这里禁用客户端二次旋转。
    if let Err(e) = render_session.set_rotation(0) {
        eprintln!("touch_ui_error=render_set_rotation_failed err={}", e);
        std::process::exit(9);
    }
    if let Err(e) = render_session.set_fps_cap(Some(cfg.target_fps as f32)) {
        eprintln!("touch_ui_error=render_set_fps_cap_failed err={}", e);
        std::process::exit(10);
    }

    let (mut screen_w, mut screen_h) = render_session.frame_size();
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

    let mut ui = UiState::new(screen_w as f32, screen_h as f32);
    let route_state = Arc::new(Mutex::new(ui.route_snapshot()));

    let (display_tx, display_rx) = mpsc::channel::<DisplayEvent>();
    let _display_thread = spawn_display_forwarder(
        cfg.control_socket.clone(),
        cfg.data_socket.clone(),
        cfg.quiet,
        display_tx,
    );

    let (touch_tx, touch_rx) = mpsc::channel::<TouchMessage>();
    let touch_cfg = TouchRouterConfig {
        control_socket_path: cfg.control_socket.clone(),
        data_socket_path: cfg.data_socket.clone(),
        route_name: "test_touch_ui".to_string(),
        device_path: cfg.device.clone(),
        screen_width: screen_w,
        screen_height: screen_h,
        rotation,
        quiet: cfg.quiet,
    };
    let mut touch_router: TouchRouterHandle =
        match spawn_touch_router(touch_cfg, Arc::clone(&route_state), hit_ui_region, touch_tx) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("touch_ui_error=spawn_touch_router_failed err={}", e);
                std::process::exit(8);
            }
        };

    if !cfg.quiet {
        eprintln!(
            "touch_ui_status=running logical={}x{} display={}x{} hz={:.2} dpi={} rotation={} target_fps={}",
            screen_w,
            screen_h,
            display.width,
            display.height,
            display.refresh_hz,
            display.density_dpi,
            rotation,
            cfg.target_fps
        );
    }

    let mut fps = 0.0f32;
    let mut fps_count = 0u32;
    let mut fps_window = Instant::now();
    let mut log_window = Instant::now();

    loop {
        drain_touch_events(&touch_rx, &mut ui, screen_w as f32, screen_h as f32);
        publish_route_snapshot(&route_state, &ui);

        while let Ok(display_event) = display_rx.try_recv() {
            let latest = display_event.info;
            let latest_rotation = cfg.rotation.unwrap_or(latest.rotation as i32);
            let changed = latest.width != display.width
                || latest.height != display.height
                || latest_rotation != rotation
                || (latest.refresh_hz - display.refresh_hz).abs() > 0.01;

            if !changed {
                continue;
            }

            if let Err(e) = render_session.reconfigure_display(
                latest.width,
                latest.height,
                0,
                latest.refresh_hz,
            ) {
                eprintln!("touch_ui_error=render_reconfigure_failed err={}", e);
                touch_router.shutdown_and_join();
                std::process::exit(11);
            }
            if let Err(e) = render_session.set_fps_cap(Some(cfg.target_fps as f32)) {
                eprintln!("touch_ui_error=render_set_fps_cap_failed err={}", e);
                touch_router.shutdown_and_join();
                std::process::exit(10);
            }

            let (new_w, new_h) = render_session.frame_size();
            if new_w != screen_w || new_h != screen_h {
                screen_w = new_w;
                screen_h = new_h;
                pixmap = match Pixmap::new(screen_w, screen_h) {
                    Some(v) => v,
                    None => {
                        eprintln!(
                            "touch_ui_error=pixmap_recreate_failed width={} height={}",
                            screen_w, screen_h
                        );
                        touch_router.shutdown_and_join();
                        std::process::exit(12);
                    }
                };
            }

            ui.on_screen_resize(screen_w as f32, screen_h as f32);
            if let Err(e) =
                touch_router.update_display_transform(screen_w, screen_h, latest_rotation)
            {
                eprintln!(
                    "touch_ui_error=touch_router_update_transform_failed err={}",
                    e
                );
                touch_router.shutdown_and_join();
                std::process::exit(13);
            }

            rotation = latest_rotation;
            display = latest;

            if !cfg.quiet {
                eprintln!(
                    "touch_ui_status=display_reconfigured seq={} logical={}x{} display={}x{} hz={:.2} rotation={}",
                    display_event.seq,
                    screen_w,
                    screen_h,
                    display.width,
                    display.height,
                    display.refresh_hz,
                    rotation
                );
            }
        }

        if ui.should_exit() {
            // 无痕退出顺序：透明帧 -> ungrab(线程内) -> sleep -> exit。
            render_transparent_frame_and_present(&mut client, &mut render_session, &mut pixmap);
            touch_router.shutdown_and_join();
            thread::sleep(Duration::from_millis(100));
            std::process::exit(0);
        }

        draw_ui(&mut pixmap, &ui, fps);

        if let Err(e) = render_session.submit_frame(pixmap.data()) {
            eprintln!("touch_ui_error=submit_frame_failed err={}", e);
            touch_router.shutdown_and_join();
            std::process::exit(6);
        }

        if let Err(e) = client.render_present() {
            eprintln!("touch_ui_error=render_present_failed err={}", e);
            touch_router.shutdown_and_join();
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
    }
}
