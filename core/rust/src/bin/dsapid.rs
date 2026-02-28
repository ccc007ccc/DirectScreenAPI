use std::ffi::CString;
use std::fs;
use std::io::{BufRead, BufReader, IoSlice, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use directscreen_core::api::{RouteResult, Status, TouchEvent, RENDER_MAX_FRAME_BYTES};
use directscreen_core::engine::{execute_command, RuntimeEngine};
use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};

struct SocketGuard {
    path: PathBuf,
}

impl SocketGuard {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn default_socket_path() -> String {
    "artifacts/run/dsapi.sock".to_string()
}

#[derive(Debug, Clone)]
struct DaemonConfig {
    socket_path: String,
    render_output_dir: String,
    supervise_presenter_cmd: Option<String>,
    supervise_input_cmd: Option<String>,
    supervise_restart_ms: u64,
}

fn parse_u64_arg(token: &str) -> Result<u64, String> {
    token
        .parse::<u64>()
        .map_err(|_| format!("invalid_u64_value:{}", token))
}

fn parse_daemon_config(args: &[String]) -> Result<DaemonConfig, String> {
    let mut socket_path = default_socket_path();
    let mut render_output_dir =
        std::env::var("DSAPI_RENDER_OUTPUT_DIR").unwrap_or_else(|_| "artifacts/render".to_string());
    let mut supervise_presenter_cmd = std::env::var("DSAPI_SUPERVISE_PRESENTER_CMD")
        .ok()
        .filter(|v| !v.is_empty());
    let mut supervise_input_cmd = std::env::var("DSAPI_SUPERVISE_INPUT_CMD")
        .ok()
        .filter(|v| !v.is_empty());
    let mut supervise_restart_ms = std::env::var("DSAPI_SUPERVISE_RESTART_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(500);

    let mut i = 1usize;
    while i < args.len() {
        let key = args[i].as_str();
        match key {
            "--socket" => {
                if (i + 1) >= args.len() {
                    return Err("missing_socket_path".to_string());
                }
                socket_path = args[i + 1].clone();
                i += 2;
            }
            "--render-output-dir" => {
                if (i + 1) >= args.len() {
                    return Err("missing_render_output_dir".to_string());
                }
                render_output_dir = args[i + 1].clone();
                i += 2;
            }
            "--supervise-presenter" => {
                if (i + 1) >= args.len() {
                    return Err("missing_supervise_presenter_cmd".to_string());
                }
                supervise_presenter_cmd = Some(args[i + 1].clone());
                i += 2;
            }
            "--supervise-input" => {
                if (i + 1) >= args.len() {
                    return Err("missing_supervise_input_cmd".to_string());
                }
                supervise_input_cmd = Some(args[i + 1].clone());
                i += 2;
            }
            "--supervise-restart-ms" => {
                if (i + 1) >= args.len() {
                    return Err("missing_supervise_restart_ms".to_string());
                }
                supervise_restart_ms = parse_u64_arg(&args[i + 1])?;
                i += 2;
            }
            _ => return Err(format!("unknown_arg:{}", args[i])),
        }
    }

    if supervise_restart_ms < 50 {
        supervise_restart_ms = 50;
    }

    Ok(DaemonConfig {
        socket_path,
        render_output_dir,
        supervise_presenter_cmd,
        supervise_input_cmd,
        supervise_restart_ms,
    })
}

fn ensure_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawFrameSubmitRequest {
    width: u32,
    height: u32,
    byte_len: usize,
}

#[derive(Debug, Clone, Copy)]
struct BoundFrameFd {
    fd: i32,
    capacity: usize,
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Ok => "OK",
        Status::NullPointer => "NULL_POINTER",
        Status::InvalidArgument => "INVALID_ARGUMENT",
        Status::OutOfRange => "OUT_OF_RANGE",
        Status::InternalError => "INTERNAL_ERROR",
    }
}

fn parse_u32(token: &str) -> Result<u32, Status> {
    token.parse::<u32>().map_err(|_| Status::InvalidArgument)
}

fn parse_u64(token: &str) -> Result<u64, Status> {
    token.parse::<u64>().map_err(|_| Status::InvalidArgument)
}

fn parse_raw_frame_submit_request(line: &str) -> Result<Option<RawFrameSubmitRequest>, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(None);
    };

    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_SUBMIT_RGBA_RAW") {
        return Ok(None);
    }

    let width = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    let height = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    let byte_len_u32 = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    if width == 0 || height == 0 {
        return Err(Status::InvalidArgument);
    }

    let byte_len = usize::try_from(byte_len_u32).map_err(|_| Status::OutOfRange)?;
    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4usize))
        .ok_or(Status::OutOfRange)?;
    if expected_len != byte_len {
        return Err(Status::InvalidArgument);
    }
    if expected_len > RENDER_MAX_FRAME_BYTES {
        return Err(Status::OutOfRange);
    }

    Ok(Some(RawFrameSubmitRequest {
        width,
        height,
        byte_len,
    }))
}

fn parse_frame_get_raw_request(line: &str) -> Result<bool, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(false);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_GET_RAW") {
        return Ok(false);
    }
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(true)
}

fn parse_frame_get_fd_request(line: &str) -> Result<bool, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(false);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_GET_FD") {
        return Ok(false);
    }
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(true)
}

fn parse_frame_bind_fd_request(line: &str) -> Result<bool, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(false);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_BIND_FD") {
        return Ok(false);
    }
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(true)
}

fn parse_frame_get_bound_request(line: &str) -> Result<bool, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(false);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_GET_BOUND") {
        return Ok(false);
    }
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(true)
}

fn parse_frame_wait_bound_present_request(line: &str) -> Result<Option<(u64, u32)>, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(None);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_WAIT_BOUND_PRESENT") {
        return Ok(None);
    }
    let last_seq = parse_u64(tokens.next().ok_or(Status::InvalidArgument)?)?;
    let timeout_ms = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(Some((last_seq, timeout_ms)))
}

fn write_status_error(stream: &mut UnixStream, status: Status) {
    let _ = stream.write_all(format!("ERR {}\n", status_name(status)).as_bytes());
    let _ = stream.flush();
}

fn write_internal_error(stream: &mut UnixStream, msg: &str) {
    let _ = stream.write_all(format!("ERR INTERNAL_ERROR {}\n", msg).as_bytes());
    let _ = stream.flush();
}

fn handle_raw_frame_submit(
    stream: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    engine: &Arc<RuntimeEngine>,
    req: RawFrameSubmitRequest,
) -> std::io::Result<()> {
    stream.write_all(b"OK READY\n")?;
    stream.flush()?;

    let mut pixels_rgba8 = vec![0u8; req.byte_len];
    reader.read_exact(&mut pixels_rgba8)?;

    let response = match engine.submit_render_frame_rgba(req.width, req.height, pixels_rgba8) {
        Ok(frame) => format!(
            "OK {} {} {} RGBA8888 {} {}",
            frame.frame_seq, frame.width, frame.height, frame.byte_len, frame.checksum_fnv1a32
        ),
        Err(status) => format!("ERR {}", status_name(status)),
    };

    stream.write_all(response.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn handle_frame_get_raw(
    stream: &mut UnixStream,
    engine: &Arc<RuntimeEngine>,
) -> std::io::Result<()> {
    const RAW_CHUNK_BYTES: usize = 1024 * 1024;

    let (frame, pixels_rgba8) = {
        let Some(snapshot) = engine.render_frame_snapshot() else {
            write_status_error(stream, Status::OutOfRange);
            return Ok(());
        };
        snapshot
    };

    let header = format!(
        "OK {} {} {} {}\n",
        frame.frame_seq, frame.width, frame.height, frame.byte_len
    );
    stream.write_all(header.as_bytes())?;

    let total = frame.byte_len as usize;
    if total != pixels_rgba8.len() {
        return Err(std::io::Error::other("frame_snapshot_len_mismatch"));
    }

    let mut offset = 0usize;
    while offset < total {
        let end = offset.saturating_add(RAW_CHUNK_BYTES).min(total);
        let chunk = &pixels_rgba8[offset..end];
        if chunk.is_empty() {
            return Err(std::io::Error::other("frame_chunk_empty"));
        }
        stream.write_all(chunk)?;
        offset = end;
    }

    stream.flush()?;
    Ok(())
}

fn close_fd(fd: i32) {
    if fd >= 0 {
        let _ = unsafe { libc::close(fd) };
    }
}

fn create_memfd_sized(name: &str, capacity: usize) -> std::io::Result<i32> {
    let c_name = CString::new(name).map_err(|_| std::io::Error::other("memfd_name_invalid"))?;
    let fd = unsafe { libc::memfd_create(c_name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }

    if unsafe { libc::ftruncate(fd, capacity as libc::off_t) } < 0 {
        let err = std::io::Error::last_os_error();
        close_fd(fd);
        return Err(err);
    }
    Ok(fd)
}

fn write_all_to_fd_from_start(fd: i32, data: &[u8]) -> std::io::Result<()> {
    if unsafe { libc::lseek(fd, 0, libc::SEEK_SET) } < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let mut offset = 0usize;
    while offset < data.len() {
        let ptr = unsafe { data.as_ptr().add(offset) } as *const libc::c_void;
        let remain = data.len() - offset;
        let n = unsafe { libc::write(fd, ptr, remain) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if n == 0 {
            return Err(std::io::Error::other("fd_write_zero"));
        }
        offset += n as usize;
    }
    Ok(())
}

fn create_memfd_with_data(name: &str, data: &[u8]) -> std::io::Result<i32> {
    let fd = create_memfd_sized(name, data.len())?;
    if let Err(e) = write_all_to_fd_from_start(fd, data) {
        close_fd(fd);
        return Err(e);
    }
    if unsafe { libc::lseek(fd, 0, libc::SEEK_SET) } < 0 {
        let err = std::io::Error::last_os_error();
        close_fd(fd);
        return Err(err);
    }
    Ok(fd)
}

fn send_line_with_fd(stream: &UnixStream, line: &str, fd: i32) -> std::io::Result<()> {
    let payload = line.as_bytes();
    let iov = [IoSlice::new(payload)];
    let fds = [fd];
    let cmsgs = [ControlMessage::ScmRights(&fds)];
    let sent = sendmsg::<()>(stream.as_raw_fd(), &iov, &cmsgs, MsgFlags::empty(), None)
        .map_err(std::io::Error::other)?;
    if sent != payload.len() {
        return Err(std::io::Error::other("sendmsg_short_write"));
    }
    Ok(())
}

fn handle_frame_get_fd(
    stream: &mut UnixStream,
    engine: &Arc<RuntimeEngine>,
) -> std::io::Result<()> {
    let (frame, pixels_rgba8) = {
        let Some(snapshot) = engine.render_frame_snapshot() else {
            write_status_error(stream, Status::OutOfRange);
            return Ok(());
        };
        snapshot
    };

    let total = frame.byte_len as usize;
    if total != pixels_rgba8.len() {
        return Err(std::io::Error::other("frame_snapshot_len_mismatch"));
    }

    let fd = create_memfd_with_data("dsapi_frame", pixels_rgba8.as_ref())?;
    let header = format!(
        "OK {} {} {} {}\n",
        frame.frame_seq, frame.width, frame.height, frame.byte_len
    );
    let res = send_line_with_fd(stream, &header, fd);
    close_fd(fd);
    res
}

fn handle_frame_bind_fd(
    stream: &UnixStream,
    bound_fd: &mut Option<BoundFrameFd>,
) -> std::io::Result<()> {
    if bound_fd.is_none() {
        let capacity = RENDER_MAX_FRAME_BYTES;
        let fd = create_memfd_sized("dsapi_frame_bound", capacity)?;
        *bound_fd = Some(BoundFrameFd { fd, capacity });
    }
    let bound = bound_fd.expect("bound fd must exist");
    let line = format!("OK BOUND {}\n", bound.capacity);
    send_line_with_fd(stream, &line, bound.fd)
}

fn handle_frame_get_bound(
    stream: &mut UnixStream,
    engine: &Arc<RuntimeEngine>,
    bound_fd: &BoundFrameFd,
) -> std::io::Result<()> {
    let (frame, pixels_rgba8) = {
        let Some(snapshot) = engine.render_frame_snapshot() else {
            write_status_error(stream, Status::OutOfRange);
            return Ok(());
        };
        snapshot
    };

    let total = frame.byte_len as usize;
    if total != pixels_rgba8.len() {
        return Err(std::io::Error::other("frame_snapshot_len_mismatch"));
    }
    if total > bound_fd.capacity {
        write_status_error(stream, Status::OutOfRange);
        return Ok(());
    }

    write_all_to_fd_from_start(bound_fd.fd, pixels_rgba8.as_ref())?;
    let line = format!(
        "OK {} {} {} {}\n",
        frame.frame_seq, frame.width, frame.height, frame.byte_len
    );
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn handle_frame_wait_bound_present(
    stream: &mut UnixStream,
    engine: &Arc<RuntimeEngine>,
    bound_fd: &BoundFrameFd,
    last_seq: u64,
    timeout_ms: u32,
) -> std::io::Result<()> {
    let Some(_) = engine
        .wait_for_frame_after(last_seq, timeout_ms)
        .map_err(|_| std::io::Error::other("frame_wait_failed"))?
    else {
        stream.write_all(b"OK TIMEOUT\n")?;
        stream.flush()?;
        return Ok(());
    };

    let (frame, pixels_rgba8) = {
        let Some(snapshot) = engine.render_frame_snapshot() else {
            write_status_error(stream, Status::OutOfRange);
            return Ok(());
        };
        snapshot
    };

    let total = frame.byte_len as usize;
    if total != pixels_rgba8.len() {
        return Err(std::io::Error::other("frame_snapshot_len_mismatch"));
    }
    if total > bound_fd.capacity {
        write_status_error(stream, Status::OutOfRange);
        return Ok(());
    }

    write_all_to_fd_from_start(bound_fd.fd, pixels_rgba8.as_ref())?;
    engine
        .render_present()
        .map_err(|_| std::io::Error::other("frame_present_failed"))?;

    let line = format!(
        "OK {} {} {} RGBA8888 {} {}\n",
        frame.frame_seq, frame.width, frame.height, frame.byte_len, frame.checksum_fnv1a32
    );
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    Ok(())
}

const TOUCH_PACKET_BYTES: usize = 16;
const TOUCH_PACKET_DOWN: u8 = 1;
const TOUCH_PACKET_MOVE: u8 = 2;
const TOUCH_PACKET_UP: u8 = 3;
const TOUCH_PACKET_CANCEL: u8 = 4;
const TOUCH_PACKET_CLEAR: u8 = 5;

fn parse_i32_le(bytes: &[u8]) -> i32 {
    i32::from_le_bytes(bytes.try_into().expect("len=4"))
}

fn parse_f32_le(bytes: &[u8]) -> f32 {
    f32::from_le_bytes(bytes.try_into().expect("len=4"))
}

fn apply_touch_packet(engine: &RuntimeEngine, packet: &[u8; TOUCH_PACKET_BYTES]) {
    let kind = packet[0];
    let pointer_id = parse_i32_le(&packet[4..8]);
    let x = parse_f32_le(&packet[8..12]);
    let y = parse_f32_le(&packet[12..16]);

    let _ = match kind {
        TOUCH_PACKET_DOWN => engine.touch_down(TouchEvent { pointer_id, x, y }),
        TOUCH_PACKET_MOVE => engine.touch_move(TouchEvent { pointer_id, x, y }),
        TOUCH_PACKET_UP => engine.touch_up(TouchEvent { pointer_id, x, y }),
        TOUCH_PACKET_CANCEL => engine.touch_cancel(pointer_id),
        TOUCH_PACKET_CLEAR => {
            engine.clear_touches();
            Ok(RouteResult::passthrough())
        }
        _ => Err(Status::InvalidArgument),
    };
}

fn handle_touch_stream_v1(
    stream: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    engine: &Arc<RuntimeEngine>,
    shutdown: &Arc<AtomicBool>,
) -> std::io::Result<()> {
    stream.write_all(b"OK STREAM_TOUCH_V1\n")?;
    stream.flush()?;

    let mut packet = [0u8; TOUCH_PACKET_BYTES];
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        match reader.read_exact(&mut packet) {
            Ok(()) => apply_touch_packet(engine, &packet),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn handle_client(mut stream: UnixStream, engine: Arc<RuntimeEngine>, shutdown: Arc<AtomicBool>) {
    let reader_stream = match stream.try_clone() {
        Ok(v) => v,
        Err(e) => {
            let _ = stream.write_all(format!("ERR INTERNAL_ERROR clone_failed:{}\n", e).as_bytes());
            return;
        }
    };

    let mut reader = BufReader::new(reader_stream);
    let mut bound_frame_fd: Option<BoundFrameFd> = None;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let raw_get = match parse_frame_get_raw_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if raw_get {
                    if let Err(e) = handle_frame_get_raw(&mut stream, &engine) {
                        write_internal_error(&mut stream, &format!("raw_get_failed:{}", e));
                        break;
                    }
                    continue;
                }

                let bind_fd = match parse_frame_bind_fd_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if bind_fd {
                    if let Err(e) = handle_frame_bind_fd(&stream, &mut bound_frame_fd) {
                        write_internal_error(&mut stream, &format!("bind_fd_failed:{}", e));
                        break;
                    }
                    continue;
                }

                let get_bound = match parse_frame_get_bound_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if get_bound {
                    let Some(bound) = bound_frame_fd.as_ref() else {
                        write_status_error(&mut stream, Status::InvalidArgument);
                        continue;
                    };
                    if let Err(e) = handle_frame_get_bound(&mut stream, &engine, bound) {
                        write_internal_error(&mut stream, &format!("get_bound_failed:{}", e));
                        break;
                    }
                    continue;
                }

                let wait_bound_present = match parse_frame_wait_bound_present_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if let Some((last_seq, timeout_ms)) = wait_bound_present {
                    let Some(bound) = bound_frame_fd.as_ref() else {
                        write_status_error(&mut stream, Status::InvalidArgument);
                        continue;
                    };
                    if let Err(e) = handle_frame_wait_bound_present(
                        &mut stream,
                        &engine,
                        bound,
                        last_seq,
                        timeout_ms,
                    ) {
                        write_internal_error(
                            &mut stream,
                            &format!("wait_bound_present_failed:{}", e),
                        );
                        break;
                    }
                    continue;
                }

                let fd_get = match parse_frame_get_fd_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if fd_get {
                    if let Err(e) = handle_frame_get_fd(&mut stream, &engine) {
                        write_internal_error(&mut stream, &format!("fd_get_failed:{}", e));
                        break;
                    }
                    continue;
                }

                let raw_submit = match parse_raw_frame_submit_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };

                if let Some(req) = raw_submit {
                    if let Err(e) = handle_raw_frame_submit(&mut stream, &mut reader, &engine, req)
                    {
                        write_internal_error(&mut stream, &format!("raw_read_failed:{}", e));
                        break;
                    }
                    continue;
                }

                if line.trim().eq_ignore_ascii_case("STREAM_TOUCH_V1") {
                    if let Err(e) =
                        handle_touch_stream_v1(&mut stream, &mut reader, &engine, &shutdown)
                    {
                        write_internal_error(&mut stream, &format!("touch_stream_failed:{}", e));
                    }
                    break;
                }

                let outcome = execute_command(&engine, &line);

                let response = format!("{}\n", outcome.response_line);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();

                if outcome.should_shutdown {
                    shutdown.store(true, Ordering::SeqCst);
                    break;
                }
            }
            Err(e) => {
                let _ =
                    stream.write_all(format!("ERR INTERNAL_ERROR read_failed:{}\n", e).as_bytes());
                break;
            }
        }
    }

    if let Some(bound) = bound_frame_fd.take() {
        close_fd(bound.fd);
    }
}

fn kill_process_quiet(pid: i32, sig: i32) {
    if pid > 0 {
        let _ = unsafe { libc::kill(pid, sig) };
    }
}

fn supervise_worker(
    name: &'static str,
    cmd: String,
    restart_delay: Duration,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) {
            let mut child = match Command::new("sh").arg("-c").arg(&cmd).spawn() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("daemon_worker_error=name={} action=spawn err={}", name, e);
                    thread::sleep(restart_delay);
                    continue;
                }
            };

            let pid = child.id() as i32;
            println!(
                "daemon_worker_status=name={} state=started pid={}",
                name, pid
            );

            loop {
                if shutdown.load(Ordering::SeqCst) {
                    kill_process_quiet(pid, libc::SIGTERM);
                    thread::sleep(Duration::from_millis(120));
                    kill_process_quiet(pid, libc::SIGKILL);
                    let _ = child.wait();
                    println!("daemon_worker_status=name={} state=stopped", name);
                    return;
                }

                match child.try_wait() {
                    Ok(Some(status)) => {
                        eprintln!(
                            "daemon_worker_warn=name={} state=exited status={} restart_in_ms={}",
                            name,
                            status,
                            restart_delay.as_millis()
                        );
                        break;
                    }
                    Ok(None) => {
                        thread::sleep(Duration::from_millis(250));
                    }
                    Err(e) => {
                        eprintln!("daemon_worker_error=name={} action=wait err={}", name, e);
                        break;
                    }
                }
            }

            if !shutdown.load(Ordering::SeqCst) {
                thread::sleep(restart_delay);
            }
        }
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg = match parse_daemon_config(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("daemon_error=parse_args_failed reason={}", e);
            std::process::exit(1);
        }
    };
    let socket_path = PathBuf::from(cfg.socket_path.clone());

    if let Err(e) = ensure_parent(&socket_path) {
        eprintln!("daemon_error=mkdir_failed err={}", e);
        std::process::exit(2);
    }

    let _ = fs::remove_file(&socket_path);

    let listener = match UnixListener::bind(&socket_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "daemon_error=bind_failed path={} err={}",
                socket_path.display(),
                e
            );
            std::process::exit(3);
        }
    };

    if let Err(e) = listener.set_nonblocking(true) {
        eprintln!("daemon_error=set_nonblocking_failed err={}", e);
        std::process::exit(4);
    }

    let _guard = SocketGuard::new(socket_path.clone());

    println!("daemon_status=started socket={}", socket_path.display());

    let engine = Arc::new(RuntimeEngine::new_with_render_output_dir(
        cfg.render_output_dir.clone(),
    ));
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut worker_handles: Vec<thread::JoinHandle<()>> = Vec::new();

    println!("daemon_render_output_dir={}", cfg.render_output_dir);

    let restart_delay = Duration::from_millis(cfg.supervise_restart_ms);
    if let Some(cmd) = cfg.supervise_presenter_cmd.clone() {
        println!("daemon_worker_config=name=presenter cmd={}", cmd);
        worker_handles.push(supervise_worker(
            "presenter",
            cmd,
            restart_delay,
            Arc::clone(&shutdown),
        ));
    }
    if let Some(cmd) = cfg.supervise_input_cmd.clone() {
        println!("daemon_worker_config=name=input cmd={}", cmd);
        worker_handles.push(supervise_worker(
            "input",
            cmd,
            restart_delay,
            Arc::clone(&shutdown),
        ));
    }

    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let engine_ref = Arc::clone(&engine);
                let shutdown_ref = Arc::clone(&shutdown);
                thread::spawn(move || {
                    handle_client(stream, engine_ref, shutdown_ref);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                eprintln!("daemon_warn=accept_failed err={}", e);
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    shutdown.store(true, Ordering::SeqCst);
    for handle in worker_handles {
        let _ = handle.join();
    }

    println!("daemon_status=stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch_packet(kind: u8, pointer_id: i32, x: f32, y: f32) -> [u8; TOUCH_PACKET_BYTES] {
        let mut out = [0u8; TOUCH_PACKET_BYTES];
        out[0] = kind;
        out[4..8].copy_from_slice(&pointer_id.to_le_bytes());
        out[8..12].copy_from_slice(&x.to_le_bytes());
        out[12..16].copy_from_slice(&y.to_le_bytes());
        out
    }

    #[test]
    fn parse_daemon_config_parses_supervise_fields() {
        let args = vec![
            "dsapid".to_string(),
            "--socket".to_string(),
            "/tmp/a.sock".to_string(),
            "--render-output-dir".to_string(),
            "/tmp/render".to_string(),
            "--supervise-presenter".to_string(),
            "echo presenter".to_string(),
            "--supervise-input".to_string(),
            "echo input".to_string(),
            "--supervise-restart-ms".to_string(),
            "777".to_string(),
        ];
        let cfg = parse_daemon_config(&args).expect("parse daemon cfg");
        assert_eq!(cfg.socket_path, "/tmp/a.sock");
        assert_eq!(cfg.render_output_dir, "/tmp/render");
        assert_eq!(
            cfg.supervise_presenter_cmd.as_deref(),
            Some("echo presenter")
        );
        assert_eq!(cfg.supervise_input_cmd.as_deref(), Some("echo input"));
        assert_eq!(cfg.supervise_restart_ms, 777);
    }

    #[test]
    fn parse_daemon_config_rejects_unknown_arg() {
        let args = vec!["dsapid".to_string(), "--bad".to_string()];
        let err = parse_daemon_config(&args).expect_err("must fail");
        assert!(err.contains("unknown_arg"));
    }

    #[test]
    fn parse_raw_frame_submit_request_accepts_valid_header() {
        let req = parse_raw_frame_submit_request("RENDER_FRAME_SUBMIT_RGBA_RAW 2 1 8")
            .expect("parse ok")
            .expect("has request");
        assert_eq!(req.width, 2);
        assert_eq!(req.height, 1);
        assert_eq!(req.byte_len, 8);
    }

    #[test]
    fn parse_raw_frame_submit_request_rejects_mismatched_len() {
        let out = parse_raw_frame_submit_request("RENDER_FRAME_SUBMIT_RGBA_RAW 2 1 7")
            .expect_err("must reject");
        assert_eq!(out, Status::InvalidArgument);
    }

    #[test]
    fn parse_raw_frame_submit_request_ignores_other_commands() {
        let out = parse_raw_frame_submit_request("PING").expect("parse ping");
        assert_eq!(out, None);
    }

    #[test]
    fn parse_frame_get_raw_request_accepts_header() {
        let out = parse_frame_get_raw_request("RENDER_FRAME_GET_RAW").expect("parse raw get");
        assert!(out);
    }

    #[test]
    fn parse_frame_get_raw_request_rejects_extra_tokens() {
        let out = parse_frame_get_raw_request("RENDER_FRAME_GET_RAW extra").expect_err("reject");
        assert_eq!(out, Status::InvalidArgument);
    }

    #[test]
    fn parse_frame_get_fd_request_accepts_header() {
        let out = parse_frame_get_fd_request("RENDER_FRAME_GET_FD").expect("parse fd get");
        assert!(out);
    }

    #[test]
    fn parse_frame_get_fd_request_rejects_extra_tokens() {
        let out = parse_frame_get_fd_request("RENDER_FRAME_GET_FD extra").expect_err("reject");
        assert_eq!(out, Status::InvalidArgument);
    }

    #[test]
    fn parse_frame_bind_fd_request_accepts_header() {
        let out = parse_frame_bind_fd_request("RENDER_FRAME_BIND_FD").expect("parse bind");
        assert!(out);
    }

    #[test]
    fn parse_frame_bind_fd_request_rejects_extra_tokens() {
        let out = parse_frame_bind_fd_request("RENDER_FRAME_BIND_FD x").expect_err("reject");
        assert_eq!(out, Status::InvalidArgument);
    }

    #[test]
    fn parse_frame_get_bound_request_accepts_header() {
        let out = parse_frame_get_bound_request("RENDER_FRAME_GET_BOUND").expect("parse bound");
        assert!(out);
    }

    #[test]
    fn parse_frame_get_bound_request_rejects_extra_tokens() {
        let out = parse_frame_get_bound_request("RENDER_FRAME_GET_BOUND x").expect_err("reject");
        assert_eq!(out, Status::InvalidArgument);
    }

    #[test]
    fn parse_frame_wait_bound_present_request_accepts() {
        let out = parse_frame_wait_bound_present_request("RENDER_FRAME_WAIT_BOUND_PRESENT 7 16")
            .expect("parse")
            .expect("request");
        assert_eq!(out.0, 7);
        assert_eq!(out.1, 16);
    }

    #[test]
    fn parse_frame_wait_bound_present_request_rejects_extra_tokens() {
        let out = parse_frame_wait_bound_present_request("RENDER_FRAME_WAIT_BOUND_PRESENT 7 16 x")
            .expect_err("reject");
        assert_eq!(out, Status::InvalidArgument);
    }

    #[test]
    fn touch_stream_binary_packets_drive_touch_state() {
        let engine = RuntimeEngine::default();
        apply_touch_packet(&engine, &touch_packet(TOUCH_PACKET_DOWN, 7, 11.0, 22.0));
        assert_eq!(engine.active_touch_count(), 1);

        apply_touch_packet(&engine, &touch_packet(TOUCH_PACKET_MOVE, 7, 12.0, 23.0));
        assert_eq!(engine.active_touch_count(), 1);

        apply_touch_packet(&engine, &touch_packet(TOUCH_PACKET_UP, 7, 12.0, 23.0));
        assert_eq!(engine.active_touch_count(), 0);
    }

    #[test]
    fn touch_stream_clear_packet_resets_active_touches() {
        let engine = RuntimeEngine::default();
        apply_touch_packet(&engine, &touch_packet(TOUCH_PACKET_DOWN, 3, 1.0, 1.0));
        assert_eq!(engine.active_touch_count(), 1);

        apply_touch_packet(&engine, &touch_packet(TOUCH_PACKET_CLEAR, 0, 0.0, 0.0));
        assert_eq!(engine.active_touch_count(), 0);
    }
}
