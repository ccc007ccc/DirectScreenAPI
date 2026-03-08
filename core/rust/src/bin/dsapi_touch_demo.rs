#![allow(clippy::too_many_arguments)]
use std::fs;
use std::io::{self, IoSliceMut, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
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
use font8x8::{UnicodeFonts, BASIC_FONTS};
use nix::cmsg_space;
use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags};

const WINDOW_ARG_MIN_W: f32 = 120.0;
const WINDOW_ARG_MIN_H: f32 = 90.0;
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
const CLOSE_CLEAR_SETTLE_MS: u64 = 12;

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

#[derive(Debug)]
struct BoundMapping {
    ptr: *mut u8,
    len: usize,
    capacity: usize,
    data_offset: usize,
}

impl BoundMapping {
    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for BoundMapping {
    fn drop(&mut self) {
        if !self.ptr.is_null() && self.len > 0 {
            let _ = unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
            self.ptr = std::ptr::null_mut();
            self.len = 0;
        }
    }
}

#[derive(Debug)]
struct ShmSubmitClient {
    data_socket_path: String,
    stream: UnixStream,
    mapping: BoundMapping,
    submit_cmd_cache: Vec<u8>,
    submit_cmd_width: u32,
    submit_cmd_height: u32,
    submit_cmd_len: usize,
    submit_cmd_origin_x: i32,
    submit_cmd_origin_y: i32,
}

impl ShmSubmitClient {
    fn connect(data_socket_path: &str) -> io::Result<Self> {
        let stream = UnixStream::connect(data_socket_path)?;
        let timeout =
            directscreen_core::util::timeout_from_env("DSAPI_CLIENT_TIMEOUT_MS", 5000, 100);
        stream.set_read_timeout(timeout)?;
        stream.set_write_timeout(timeout)?;
        let mapping = bind_mapping(&stream)?;
        Ok(Self {
            data_socket_path: data_socket_path.to_string(),
            stream,
            mapping,
            submit_cmd_cache: Vec::new(),
            submit_cmd_width: 0,
            submit_cmd_height: 0,
            submit_cmd_len: 0,
            submit_cmd_origin_x: i32::MIN,
            submit_cmd_origin_y: i32::MIN,
        })
    }

    fn frame_buffer_mut(&mut self, width: u32, height: u32) -> io::Result<&mut [u8]> {
        let expected_len = frame_byte_len(width, height)?;
        self.ensure_mapping_capacity(expected_len)?;
        let begin = self.mapping.data_offset;
        let end = begin
            .checked_add(expected_len)
            .ok_or_else(|| io::Error::other("submit_offset_overflow"))?;
        if end > self.mapping.len {
            return Err(io::Error::other("submit_out_of_bound_mapping"));
        }
        Ok(&mut self.mapping.as_mut_slice()[begin..end])
    }

    fn submit_rendered_frame(
        &mut self,
        width: u32,
        height: u32,
        byte_len: usize,
        origin_x: i32,
        origin_y: i32,
    ) -> io::Result<()> {
        self.ensure_mapping_capacity(byte_len)?;
        self.ensure_submit_cmd(width, height, byte_len, origin_x, origin_y);
        self.stream.write_all(&self.submit_cmd_cache)?;
        let line = read_line_fast(&mut self.stream, 4096)?;
        if !line.starts_with("OK") {
            return Err(io::Error::other(format!("submit_failed response={}", line)));
        }
        Ok(())
    }

    fn ensure_mapping_capacity(&mut self, byte_len: usize) -> io::Result<()> {
        if byte_len > self.mapping.capacity {
            // 旧连接可能绑定了历史尺寸的 SHM，重连后重新绑定一次。
            self.reconnect()?;
        }
        if byte_len > self.mapping.capacity {
            return Err(io::Error::other(format!(
                "bound_shm_capacity_too_small capacity={} frame_len={} reconnect=1",
                self.mapping.capacity, byte_len
            )));
        }
        Ok(())
    }

    fn reconnect(&mut self) -> io::Result<()> {
        let new_stream = UnixStream::connect(&self.data_socket_path)?;
        let timeout =
            directscreen_core::util::timeout_from_env("DSAPI_CLIENT_TIMEOUT_MS", 5000, 100);
        new_stream.set_read_timeout(timeout)?;
        new_stream.set_write_timeout(timeout)?;
        let mapping = bind_mapping(&new_stream)?;
        self.stream = new_stream;
        self.mapping = mapping;
        self.submit_cmd_cache.clear();
        self.submit_cmd_width = 0;
        self.submit_cmd_height = 0;
        self.submit_cmd_len = 0;
        self.submit_cmd_origin_x = i32::MIN;
        self.submit_cmd_origin_y = i32::MIN;
        Ok(())
    }

    fn ensure_submit_cmd(
        &mut self,
        width: u32,
        height: u32,
        byte_len: usize,
        origin_x: i32,
        origin_y: i32,
    ) {
        if self.submit_cmd_width == width
            && self.submit_cmd_height == height
            && self.submit_cmd_len == byte_len
            && self.submit_cmd_origin_x == origin_x
            && self.submit_cmd_origin_y == origin_y
            && !self.submit_cmd_cache.is_empty()
        {
            return;
        }

        self.submit_cmd_cache = format!(
            "RENDER_FRAME_SUBMIT_SHM {} {} {} {} {} {}\n",
            width, height, byte_len, self.mapping.data_offset, origin_x, origin_y
        )
        .into_bytes();
        self.submit_cmd_width = width;
        self.submit_cmd_height = height;
        self.submit_cmd_len = byte_len;
        self.submit_cmd_origin_x = origin_x;
        self.submit_cmd_origin_y = origin_y;
    }
}

fn bind_mapping(stream: &UnixStream) -> io::Result<BoundMapping> {
    let mut stream = stream.try_clone()?;
    stream.write_all(b"RENDER_FRAME_BIND_SHM\n")?;
    stream.flush()?;

    let (line, fd_opt) = recv_line_with_optional_fd(&stream, true)?;
    let mut parts = line.split_whitespace();
    let p0 = parts.next();
    let p1 = parts.next();
    let p2 = parts.next();
    let p3 = parts.next();
    if p0 != Some("OK")
        || p1 != Some("SHM_BOUND")
        || p2.is_none()
        || p3.is_none()
        || parts.next().is_some()
    {
        return Err(io::Error::other(format!(
            "invalid_bind_response line={}",
            line
        )));
    }

    let capacity = p2
        .and_then(|v| v.parse::<usize>().ok())
        .ok_or_else(|| io::Error::other("invalid_bind_capacity"))?;
    let data_offset = p3
        .and_then(|v| v.parse::<usize>().ok())
        .ok_or_else(|| io::Error::other("invalid_bind_offset"))?;
    if capacity == 0 {
        return Err(io::Error::other("bind_capacity_zero"));
    }

    let map_len = data_offset
        .checked_add(capacity)
        .ok_or_else(|| io::Error::other("bind_map_len_overflow"))?;
    let fd = fd_opt.ok_or_else(|| io::Error::other("bind_missing_fd"))?;

    let mapped = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            map_len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    let _ = unsafe { libc::close(fd) };
    if mapped == libc::MAP_FAILED {
        return Err(io::Error::last_os_error());
    }

    Ok(BoundMapping {
        ptr: mapped as *mut u8,
        len: map_len,
        capacity,
        data_offset,
    })
}

fn recv_line_with_optional_fd(
    stream: &UnixStream,
    require_fd: bool,
) -> io::Result<(String, Option<RawFd>)> {
    let mut line: Vec<u8> = Vec::with_capacity(256);
    let mut received_fd: Option<RawFd> = None;

    loop {
        let mut buf = [0u8; 1024];
        let mut iov = [IoSliceMut::new(&mut buf)];
        let mut cmsg = cmsg_space!([RawFd; 4]);
        let bytes_read = {
            let msg = recvmsg::<()>(
                stream.as_raw_fd(),
                &mut iov,
                Some(&mut cmsg),
                MsgFlags::empty(),
            )
            .map_err(io::Error::other)?;
            if msg.bytes == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "data_socket_closed",
                ));
            }

            let cmsgs = msg.cmsgs().map_err(io::Error::other)?;
            for c in cmsgs {
                if let ControlMessageOwned::ScmRights(fds) = c {
                    for fd in fds {
                        if received_fd.is_none() {
                            received_fd = Some(fd);
                        } else {
                            let _ = unsafe { libc::close(fd) };
                        }
                    }
                }
            }
            msg.bytes
        };

        for b in &buf[..bytes_read] {
            if *b == b'\n' {
                let text = String::from_utf8_lossy(&line).trim_end().to_string();
                if require_fd && received_fd.is_none() {
                    return Err(io::Error::other("required_fd_missing"));
                }
                return Ok((text, received_fd));
            }
            if *b != b'\r' {
                line.push(*b);
            }
            if line.len() > 4096 {
                return Err(io::Error::other("response_line_too_long"));
            }
        }
    }
}

fn read_line_fast(stream: &mut UnixStream, max_len: usize) -> io::Result<String> {
    let mut out: Vec<u8> = Vec::with_capacity(128);
    let mut chunk = [0u8; 256];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            if out.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "socket_closed",
                ));
            }
            break;
        }
        for b in &chunk[..n] {
            if *b == b'\n' {
                return Ok(String::from_utf8_lossy(&out).trim().to_string());
            }
            if *b != b'\r' {
                out.push(*b);
            }
            if out.len() > max_len {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "line_too_long"));
            }
        }
    }
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

fn frame_byte_len(width: u32, height: u32) -> io::Result<usize> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "frame_size_overflow"))
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
    title_text_scale: i32,
    body_text_scale: i32,
    body_line_step: i32,
}

fn ui_metrics(display: DisplayInfo) -> UiMetrics {
    let density = if display.density_dpi > 0 {
        display.density_dpi as f32 / 160.0
    } else {
        1.0
    };
    let control_scale = (1.0 + (density - 1.0) * 0.18).clamp(1.0, 1.45);
    let min_scale = (1.0 + (density - 1.0) * 0.08).clamp(1.0, 1.20);
    let body_text_scale = if density >= 2.6 { 2 } else { 1 };
    let title_text_scale = body_text_scale;
    let body_line_step = (9 * body_text_scale + 5).max(14);
    let min_title_h = (10 * title_text_scale + 10) as f32;

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
        title_text_scale,
        body_text_scale,
        body_line_step,
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

    Ok(Args {
        data_socket: data_socket.unwrap_or_else(|| derive_data_socket_path_text(&control_socket)),
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

impl ButtonAction {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Submit => "submit",
        }
    }
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
                        self.window.w = origin_w + (msg.x - start_x);
                        self.window.h = origin_h + (msg.y - start_y);
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

#[allow(dead_code)]
struct BlurScratch {
    src: Vec<u8>,
    tmp: Vec<u8>,
    kernel: Vec<f32>,
    radius: i32,
    cached_patch: Vec<u8>,
    cached_x: i32,
    cached_y: i32,
    cached_w: usize,
    cached_h: usize,
    cache_valid: bool,
}

#[allow(dead_code)]
impl BlurScratch {
    fn new(radius: u32, sigma: f32) -> Self {
        Self {
            src: Vec::new(),
            tmp: Vec::new(),
            kernel: build_gaussian_kernel(radius as usize, sigma),
            radius: radius as i32,
            cached_patch: Vec::new(),
            cached_x: 0,
            cached_y: 0,
            cached_w: 0,
            cached_h: 0,
            cache_valid: false,
        }
    }

    fn ensure_len(&mut self, len: usize) {
        if self.src.len() != len {
            self.src.resize(len, 0);
        }
        if self.tmp.len() != len {
            self.tmp.resize(len, 0);
        }
    }

    fn cache_matches(&self, x: i32, y: i32, w: usize, h: usize) -> bool {
        self.cache_valid
            && self.cached_x == x
            && self.cached_y == y
            && self.cached_w == w
            && self.cached_h == h
            && self.cached_patch.len() == w.saturating_mul(h).saturating_mul(4)
    }

    fn write_cached(&self, frame: &mut [u8], fb_width: u32) {
        if !self.cache_valid || self.cached_w == 0 || self.cached_h == 0 {
            return;
        }
        for ry in 0..self.cached_h {
            let dst_row =
                ((self.cached_y as usize + ry) * fb_width as usize + self.cached_x as usize) * 4;
            let src_row = ry * self.cached_w * 4;
            frame[dst_row..dst_row + self.cached_w * 4]
                .copy_from_slice(&self.cached_patch[src_row..src_row + self.cached_w * 4]);
        }
    }

    fn update_cache_from_frame(
        &mut self,
        frame: &[u8],
        fb_width: u32,
        x: i32,
        y: i32,
        w: usize,
        h: usize,
    ) {
        let len = w.saturating_mul(h).saturating_mul(4);
        if self.cached_patch.len() != len {
            self.cached_patch.resize(len, 0);
        }
        for ry in 0..h {
            let src_row = ((y as usize + ry) * fb_width as usize + x as usize) * 4;
            let dst_row = ry * w * 4;
            self.cached_patch[dst_row..dst_row + w * 4]
                .copy_from_slice(&frame[src_row..src_row + w * 4]);
        }
        self.cached_x = x;
        self.cached_y = y;
        self.cached_w = w;
        self.cached_h = h;
        self.cache_valid = true;
    }

    fn invalidate_cache(&mut self) {
        self.cache_valid = false;
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

fn submit_clear_frame(submitter: &mut ShmSubmitClient) -> io::Result<()> {
    let width = 1u32;
    let height = 1u32;
    let frame_len = frame_byte_len(width, height)?;
    let frame_buf = submitter.frame_buffer_mut(width, height)?;
    frame_buf.fill(0);
    submitter.submit_rendered_frame(width, height, frame_len, 0, 0)
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
        if cmdline_contains_any(pid, &["DSAPIPresenterDemo", "present-loop"]) {
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

    let mut submitter = match ShmSubmitClient::connect(&cfg.data_socket) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "touch_demo_error=data_connect_failed socket={} err={}",
                cfg.data_socket, e
            );
            std::process::exit(7);
        }
    };

    let mut target_fps = resolve_target_fps(cfg.fps, display.refresh_hz);
    if !cfg.quiet {
        let init_frame_w = state.window.w.round().max(1.0) as u32;
        let init_frame_h = state.window.h.round().max(1.0) as u32;
        eprintln!(
            "touch_demo_status=running display={}x{} frame={}x{} @ {:.2}Hz dpi={} rotation={} target_fps={:.1}",
            display.width,
            display.height,
            init_frame_w,
            init_frame_h,
            display.refresh_hz,
            display.density_dpi,
            display.rotation,
            target_fps
        );
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

    while state.running {
        if let Some(limit_s) = cfg.run_seconds {
            if started_at.elapsed().as_secs_f32() >= limit_s {
                break;
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

        state.tick(Instant::now());
        let fps = fps_counter.on_frame();
        let blocked = route_blocked_down.load(Ordering::Relaxed);
        let passed = route_passed_down.load(Ordering::Relaxed);

        let frame_w = state
            .window
            .w
            .round()
            .max(1.0)
            .min(render_display.width.max(1) as f32) as u32;
        let frame_h = state
            .window
            .h
            .round()
            .max(1.0)
            .min(render_display.height.max(1) as f32) as u32;
        let max_origin_x = render_display.width.saturating_sub(frame_w) as i32;
        let max_origin_y = render_display.height.saturating_sub(frame_h) as i32;
        let frame_origin_x = (state.window.x.round() as i32).clamp(0, max_origin_x);
        let frame_origin_y = (state.window.y.round() as i32).clamp(0, max_origin_y);

        let frame_len = match frame_byte_len(frame_w, frame_h) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("touch_demo_error=frame_size_invalid err={}", e);
                break;
            }
        };
        let frame_buf = match submitter.frame_buffer_mut(frame_w, frame_h) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("touch_demo_error=frame_buffer_failed err={}", e);
                break;
            }
        };
        let frame_display = DisplayInfo {
            width: frame_w,
            height: frame_h,
            refresh_hz: display.refresh_hz,
            density_dpi: display.density_dpi,
            rotation: display.rotation,
        };
        render_scene(frame_buf, frame_display, &state, fps, blocked, passed);

        if let Err(e) = submitter.submit_rendered_frame(
            frame_w,
            frame_h,
            frame_len,
            frame_origin_x,
            frame_origin_y,
        ) {
            eprintln!("touch_demo_error=submit_frame_failed err={}", e);
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

    if let Err(e) = submit_clear_frame(&mut submitter) {
        if !cfg.quiet {
            eprintln!("touch_demo_warn=submit_clear_frame_failed err={}", e);
        }
    } else {
        // 给 presenter 最小处理窗口，确保 clear frame 先落地。
        thread::sleep(Duration::from_millis(CLOSE_CLEAR_SETTLE_MS));
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

fn render_scene(
    frame: &mut [u8],
    display: DisplayInfo,
    state: &DemoState,
    fps: f32,
    blocked_down: u64,
    passed_down: u64,
) {
    frame.fill(0);
    let metrics = ui_metrics(display);
    let now = Instant::now();

    // 窗口内容帧采用局部坐标系：提交 origin 后由 presenter 负责层位置。
    let wx = 0i32;
    let wy = 0i32;
    let ww = display.width.max(1) as i32;
    let wh = display.height.max(1) as i32;
    let title_h = metrics.title_height.round().max(1.0) as i32;
    let local_window = WindowState {
        x: wx as f32,
        y: wy as f32,
        w: ww as f32,
        h: wh as f32,
        visible: true,
    };
    let layout = compute_ui_layout(local_window, metrics);

    fill_rect(
        frame,
        display.width,
        display.height,
        wx,
        wy,
        ww,
        wh,
        [26, 34, 48, 138],
    );

    fill_rect(
        frame,
        display.width,
        display.height,
        wx,
        wy,
        ww,
        title_h,
        [34, 49, 72, 224],
    );
    draw_rect_border(
        frame,
        display.width,
        display.height,
        wx,
        wy,
        ww,
        wh,
        [130, 175, 240, 250],
    );

    draw_button(
        frame,
        display.width,
        display.height,
        layout.close_button,
        "CLOSE",
        metrics.body_text_scale.max(1),
        [198, 76, 74, 244],
        [255, 220, 220, 255],
        [255, 248, 248, 255],
        state.press_active(PressTarget::CloseButton),
        DemoState::flash_active(state.close_flash_until, now),
    );

    draw_button(
        frame,
        display.width,
        display.height,
        layout.submit_button,
        "SEND",
        metrics.body_text_scale.max(1),
        [63, 128, 220, 244],
        [205, 234, 255, 255],
        [243, 251, 255, 255],
        state.press_active(PressTarget::SubmitButton),
        DemoState::flash_active(state.submit_flash_until, now),
    );

    let input_x = layout.input_box.x.round() as i32;
    let input_y = layout.input_box.y.round() as i32;
    let input_w = layout.input_box.w.round().max(20.0) as i32;
    let input_h = layout.input_box.h.round().max(16.0) as i32;
    let input_fill = if state.input_focused {
        [30, 48, 72, 234]
    } else {
        [24, 38, 57, 224]
    };
    let input_border = if state.input_focused {
        [163, 210, 255, 255]
    } else {
        [108, 152, 200, 255]
    };
    fill_rect(
        frame,
        display.width,
        display.height,
        input_x,
        input_y,
        input_w,
        input_h,
        input_fill,
    );
    draw_rect_border(
        frame,
        display.width,
        display.height,
        input_x,
        input_y,
        input_w,
        input_h,
        input_border,
    );

    let input_scale = metrics.body_text_scale.max(1);
    let char_step = 8 * input_scale + input_scale;
    let text_left = input_x + 6;
    let text_baseline = input_y + ((input_h - 8 * input_scale) / 2).max(2);
    let max_chars = ((input_w - 12).max(8) / char_step.max(1)) as usize;
    let input_text = tail_chars(&state.input_text, max_chars.max(1));
    let input_color = if state.input_focused {
        [239, 248, 255, 255]
    } else {
        [203, 219, 236, 255]
    };
    draw_text(
        frame,
        display.width,
        display.height,
        text_left,
        text_baseline,
        if input_text.is_empty() && !state.input_focused {
            "tap to type"
        } else {
            input_text.as_str()
        },
        input_color,
        input_scale,
    );
    if state.input_focused && state.cursor_visible {
        let caret_x = text_left + text_pixel_width(input_text.as_str(), input_scale) + input_scale;
        let caret_y0 = input_y + 5;
        let caret_y1 = input_y + input_h - 6;
        draw_line(
            frame,
            display.width,
            display.height,
            caret_x,
            caret_y0,
            caret_x,
            caret_y1,
            [239, 248, 255, 255],
        );
    }

    let resize_size = metrics.resize_hit.round().max(16.0) as i32;
    let rx = wx + ww - resize_size;
    let ry = wy + wh - resize_size;
    let step = (resize_size / 4).max(4);
    let resize_end = resize_size - step;
    for i in 0..3 {
        let delta = i * step;
        draw_line(
            frame,
            display.width,
            display.height,
            rx + delta,
            ry + resize_end,
            rx + resize_end,
            ry + delta,
            [158, 198, 255, 255],
        );
    }

    let mode = state.interaction.label();
    let title = "DirectScreenAPI Touch Demo";
    let title_text_h = 8 * metrics.title_text_scale;
    let title_y = wy + ((title_h - title_text_h) / 2).max(2);
    draw_text(
        frame,
        display.width,
        display.height,
        wx + 12,
        title_y,
        title,
        [238, 245, 255, 255],
        metrics.title_text_scale,
    );

    let ime_state = if state.ime_visible { "shown" } else { "hidden" };
    let focus_state = if state.input_focused {
        "focused"
    } else {
        "unfocused"
    };
    let last_action = state
        .last_button_action
        .map(|v| v.as_str())
        .unwrap_or("none");
    let lines = [
        format!("FPS: {:>5.1}", fps),
        format!("Mode: {}", mode),
        format!(
            "Input: {} chars={} ime={}",
            focus_state,
            state.input_text.chars().count(),
            ime_state
        ),
        format!(
            "Window: x={:.0} y={:.0} w={:.0} h={:.0}",
            state.window.x, state.window.y, state.window.w, state.window.h
        ),
        format!(
            "Frame: {}x{} dpi={}",
            display.width, display.height, display.density_dpi
        ),
        format!(
            "Buttons: callbacks={} submit_count={} last_action={}",
            state.button_callback_count, state.submit_count, last_action
        ),
        format!("Touch block downs: {}", blocked_down),
        format!("Touch passthrough downs: {}", passed_down),
        format!(
            "UI events: {} latency_ms={:.2}",
            state.ui_event_count, state.last_latency_ms
        ),
        format!("Last submit: {}", state.last_submit_text),
        "Drag title, resize corner, tap CLOSE/SEND, tap input for system IME.".to_string(),
    ];

    let mut text_y = input_y + input_h + 10;
    let text_limit = wy + wh - 14;
    for line in lines.iter() {
        if text_y + metrics.body_line_step > text_limit {
            break;
        }
        draw_text(
            frame,
            display.width,
            display.height,
            wx + 12,
            text_y,
            line,
            [208, 223, 238, 255],
            metrics.body_text_scale,
        );
        text_y += metrics.body_line_step;
    }

}

fn tail_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    text.chars().skip(total - max_chars).collect()
}

fn text_pixel_width(text: &str, scale: i32) -> i32 {
    if text.is_empty() {
        return 0;
    }
    let step = 8 * scale + scale;
    step * i32::try_from(text.chars().count()).unwrap_or(i32::MAX) - scale
}

fn adjust_color(color: [u8; 4], mul: f32, add: i16) -> [u8; 4] {
    let mut out = color;
    for item in out.iter_mut().take(3) {
        let base = ((*item as f32) * mul).round() as i16 + add;
        *item = base.clamp(0, 255) as u8;
    }
    out
}

fn draw_button(
    frame: &mut [u8],
    fb_width: u32,
    fb_height: u32,
    rect: RectF,
    label: &str,
    text_scale: i32,
    base_color: [u8; 4],
    border_color: [u8; 4],
    text_color: [u8; 4],
    pressed: bool,
    flash: bool,
) {
    let x = rect.x.round() as i32;
    let y = rect.y.round() as i32;
    let w = rect.w.round().max(16.0) as i32;
    let h = rect.h.round().max(12.0) as i32;
    let mut fill = base_color;
    if pressed {
        fill = adjust_color(fill, 0.72, -12);
    } else if flash {
        fill = adjust_color(fill, 1.0, 24);
    }
    fill_rect(frame, fb_width, fb_height, x, y, w, h, fill);
    draw_rect_border(frame, fb_width, fb_height, x, y, w, h, border_color);
    if !label.is_empty() {
        let text_w = text_pixel_width(label, text_scale.max(1));
        let text_h = 8 * text_scale.max(1);
        let text_x = x + ((w - text_w) / 2).max(2);
        let text_y = y + ((h - text_h) / 2).max(1);
        draw_text(
            frame,
            fb_width,
            fb_height,
            text_x,
            text_y,
            label,
            text_color,
            text_scale.max(1),
        );
    }
}

fn fill_rect(
    frame: &mut [u8],
    fb_width: u32,
    fb_height: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: [u8; 4],
) {
    if w <= 0 || h <= 0 {
        return;
    }
    let start_x = x.max(0);
    let start_y = y.max(0);
    let end_x = (x + w).min(fb_width as i32);
    let end_y = (y + h).min(fb_height as i32);
    if start_x >= end_x || start_y >= end_y {
        return;
    }

    let fb_w = fb_width as usize;
    for py in start_y..end_y {
        let row = py as usize * fb_w * 4;
        for px in start_x..end_x {
            let idx = row + px as usize * 4;
            frame[idx] = color[0];
            frame[idx + 1] = color[1];
            frame[idx + 2] = color[2];
            frame[idx + 3] = color[3];
        }
    }
}

#[allow(dead_code)]
fn blend_rect(
    frame: &mut [u8],
    fb_width: u32,
    fb_height: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: [u8; 4],
    alpha: u8,
) {
    if w <= 0 || h <= 0 || alpha == 0 {
        return;
    }
    let start_x = x.max(0);
    let start_y = y.max(0);
    let end_x = (x + w).min(fb_width as i32);
    let end_y = (y + h).min(fb_height as i32);
    if start_x >= end_x || start_y >= end_y {
        return;
    }

    let a = alpha as u16;
    let inv_a = 255u16.saturating_sub(a);
    let fb_w = fb_width as usize;
    for py in start_y..end_y {
        let row = py as usize * fb_w * 4;
        for px in start_x..end_x {
            let idx = row + px as usize * 4;
            for c in 0..3 {
                let dst = frame[idx + c] as u16;
                let src = color[c] as u16;
                frame[idx + c] = (((dst * inv_a) + (src * a)) / 255) as u8;
            }
            frame[idx + 3] = 255;
        }
    }
}

#[allow(dead_code)]
fn draw_backdrop_pattern(
    frame: &mut [u8],
    fb_width: u32,
    fb_height: u32,
    wx: i32,
    wy: i32,
    ww: i32,
    wh: i32,
) {
    let pad = 96i32;
    let start_x = (wx - pad).max(0);
    let start_y = (wy - pad).max(0);
    let end_x = (wx + ww + pad).min(fb_width as i32);
    let end_y = (wy + wh + pad).min(fb_height as i32);
    if start_x >= end_x || start_y >= end_y {
        return;
    }

    let phase = 0i32;
    let cell = 28i32;
    let fb_w = fb_width as usize;
    for py in start_y..end_y {
        let row = py as usize * fb_w * 4;
        for px in start_x..end_x {
            let cx = (px + phase) / cell;
            let cy = (py - phase) / cell;
            let checker = (cx + cy) & 1;
            let idx = row + px as usize * 4;
            if checker == 0 {
                frame[idx] = 44;
                frame[idx + 1] = 72;
                frame[idx + 2] = 116;
            } else {
                frame[idx] = 26;
                frame[idx + 1] = 46;
                frame[idx + 2] = 82;
            }
            frame[idx + 3] = 255;
        }
    }
}

#[allow(dead_code)]
fn blur_region_gaussian(
    frame: &mut [u8],
    fb_width: u32,
    fb_height: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scratch: &mut BlurScratch,
) {
    if w <= 0 || h <= 0 || scratch.radius <= 0 || scratch.kernel.is_empty() {
        return;
    }
    let sx = x.max(0);
    let sy = y.max(0);
    let ex = (x + w).min(fb_width as i32);
    let ey = (y + h).min(fb_height as i32);
    if sx >= ex || sy >= ey {
        return;
    }
    let rw = (ex - sx) as usize;
    let rh = (ey - sy) as usize;
    if scratch.cache_matches(sx, sy, rw, rh) {
        scratch.write_cached(frame, fb_width);
        return;
    }
    let len = rw
        .checked_mul(rh)
        .and_then(|v| v.checked_mul(4))
        .unwrap_or(0usize);
    if len == 0 {
        return;
    }
    scratch.ensure_len(len);

    for ry in 0..rh {
        let src_row = ((sy as usize + ry) * fb_width as usize + sx as usize) * 4;
        let dst_row = ry * rw * 4;
        scratch.src[dst_row..dst_row + rw * 4].copy_from_slice(&frame[src_row..src_row + rw * 4]);
    }

    let radius = scratch.radius;
    let kernel = &scratch.kernel;

    for ry in 0..rh {
        for rx in 0..rw {
            let mut acc = [0.0f32; 4];
            let mut wsum = 0.0f32;
            for k in -radius..=radius {
                let sx2 = (rx as i32 + k).clamp(0, rw as i32 - 1) as usize;
                let weight = kernel[(k + radius) as usize];
                wsum += weight;
                let idx = (ry * rw + sx2) * 4;
                for (c, acc_slot) in acc.iter_mut().enumerate() {
                    *acc_slot += scratch.src[idx + c] as f32 * weight;
                }
            }
            let out_idx = (ry * rw + rx) * 4;
            for (c, acc_value) in acc.iter().enumerate() {
                scratch.tmp[out_idx + c] =
                    (acc_value / wsum.max(0.0001)).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    for ry in 0..rh {
        for rx in 0..rw {
            let mut acc = [0.0f32; 4];
            let mut wsum = 0.0f32;
            for k in -radius..=radius {
                let sy2 = (ry as i32 + k).clamp(0, rh as i32 - 1) as usize;
                let weight = kernel[(k + radius) as usize];
                wsum += weight;
                let idx = (sy2 * rw + rx) * 4;
                for (c, acc_slot) in acc.iter_mut().enumerate() {
                    *acc_slot += scratch.tmp[idx + c] as f32 * weight;
                }
            }
            let out_idx = ((sy as usize + ry) * fb_width as usize + (sx as usize + rx)) * 4;
            for c in 0..3usize {
                frame[out_idx + c] = (acc[c] / wsum.max(0.0001)).round().clamp(0.0, 255.0) as u8;
            }
            frame[out_idx + 3] = 255;
        }
    }
    scratch.update_cache_from_frame(frame, fb_width, sx, sy, rw, rh);
}

fn build_gaussian_kernel(radius: usize, sigma: f32) -> Vec<f32> {
    if radius == 0 {
        return vec![1.0];
    }
    let sigma = if sigma.is_finite() && sigma > 0.01 {
        sigma
    } else {
        (radius as f32) * 0.45
    };
    let mut kernel = Vec::with_capacity(radius * 2 + 1);
    let denom = 2.0 * sigma * sigma;
    let mut sum = 0.0f32;
    for i in 0..=(radius * 2) {
        let x = i as i32 - radius as i32;
        let v = (-(x * x) as f32 / denom).exp();
        kernel.push(v);
        sum += v;
    }
    if sum > 0.0 {
        for v in &mut kernel {
            *v /= sum;
        }
    }
    kernel
}

fn draw_rect_border(
    frame: &mut [u8],
    fb_width: u32,
    fb_height: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: [u8; 4],
) {
    draw_line(frame, fb_width, fb_height, x, y, x + w - 1, y, color);
    draw_line(
        frame,
        fb_width,
        fb_height,
        x,
        y + h - 1,
        x + w - 1,
        y + h - 1,
        color,
    );
    draw_line(frame, fb_width, fb_height, x, y, x, y + h - 1, color);
    draw_line(
        frame,
        fb_width,
        fb_height,
        x + w - 1,
        y,
        x + w - 1,
        y + h - 1,
        color,
    );
}

fn draw_line(
    frame: &mut [u8],
    fb_width: u32,
    fb_height: u32,
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    color: [u8; 4],
) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        put_pixel(frame, fb_width, fb_height, x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn put_pixel(frame: &mut [u8], fb_width: u32, fb_height: u32, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= fb_width as i32 || y >= fb_height as i32 {
        return;
    }
    let idx = ((y as usize * fb_width as usize) + x as usize) * 4;
    if idx + 3 >= frame.len() {
        return;
    }
    frame[idx] = color[0];
    frame[idx + 1] = color[1];
    frame[idx + 2] = color[2];
    frame[idx + 3] = color[3];
}

fn draw_text(
    frame: &mut [u8],
    fb_width: u32,
    fb_height: u32,
    mut x: i32,
    y: i32,
    text: &str,
    color: [u8; 4],
    scale: i32,
) {
    for ch in text.chars() {
        if ch == '\n' {
            continue;
        }
        draw_char(frame, fb_width, fb_height, x, y, ch, color, scale);
        x += 8 * scale + scale;
    }
}

fn draw_char(
    frame: &mut [u8],
    fb_width: u32,
    fb_height: u32,
    x: i32,
    y: i32,
    ch: char,
    color: [u8; 4],
    scale: i32,
) {
    let Some(glyph) = BASIC_FONTS.get(ch) else {
        return;
    };

    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8 {
            if (bits >> col) & 1 == 1 {
                fill_rect(
                    frame,
                    fb_width,
                    fb_height,
                    x + col * scale,
                    y + row as i32 * scale,
                    scale,
                    scale,
                    color,
                );
            }
        }
    }
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
