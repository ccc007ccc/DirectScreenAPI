use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use directscreen_core::api::{Status, RENDER_MAX_FRAME_BYTES};
use directscreen_core::engine::{execute_command, RuntimeEngine};

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

fn parse_socket_arg(args: &[String]) -> String {
    let mut i = 1usize;
    while i < args.len() {
        if args[i] == "--socket" && (i + 1) < args.len() {
            return args[i + 1].clone();
        }
        i += 1;
    }
    default_socket_path()
}

fn parse_render_output_dir_arg(args: &[String]) -> String {
    let mut i = 1usize;
    while i < args.len() {
        if args[i] == "--render-output-dir" && (i + 1) < args.len() {
            return args[i + 1].clone();
        }
        i += 1;
    }
    std::env::var("DSAPI_RENDER_OUTPUT_DIR").unwrap_or_else(|_| "artifacts/render".to_string())
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
    engine: &Arc<Mutex<RuntimeEngine>>,
    req: RawFrameSubmitRequest,
) -> std::io::Result<()> {
    stream.write_all(b"OK READY\n")?;
    stream.flush()?;

    let mut pixels_rgba8 = vec![0u8; req.byte_len];
    reader.read_exact(&mut pixels_rgba8)?;

    let response = match engine.lock() {
        Ok(mut guard) => {
            match guard.submit_render_frame_rgba(req.width, req.height, pixels_rgba8) {
                Ok(frame) => format!(
                    "OK {} {} {} RGBA8888 {} {}",
                    frame.frame_seq,
                    frame.width,
                    frame.height,
                    frame.byte_len,
                    frame.checksum_fnv1a32
                ),
                Err(status) => format!("ERR {}", status_name(status)),
            }
        }
        Err(_) => {
            return Err(std::io::Error::other("lock_poisoned"));
        }
    };

    stream.write_all(response.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn handle_frame_get_raw(
    stream: &mut UnixStream,
    engine: &Arc<Mutex<RuntimeEngine>>,
) -> std::io::Result<()> {
    const RAW_CHUNK_BYTES: usize = 1024 * 1024;

    let guard = engine
        .lock()
        .map_err(|_| std::io::Error::other("lock_poisoned"))?;
    let Some(frame) = guard.render_frame_info() else {
        write_status_error(stream, Status::OutOfRange);
        return Ok(());
    };

    let header = format!(
        "OK {} {} {} {}\n",
        frame.frame_seq, frame.width, frame.height, frame.byte_len
    );
    stream.write_all(header.as_bytes())?;

    let total = frame.byte_len as usize;
    let mut offset = 0usize;
    while offset < total {
        let ask = (total - offset).min(RAW_CHUNK_BYTES);
        let chunk = guard
            .render_frame_read_chunk(offset, ask)
            .map_err(|_| std::io::Error::other("frame_chunk_read_failed"))?;
        if chunk.chunk_bytes.is_empty() {
            return Err(std::io::Error::other("frame_chunk_empty"));
        }
        stream.write_all(&chunk.chunk_bytes)?;
        offset += chunk.chunk_bytes.len();
    }

    stream.flush()?;
    Ok(())
}

fn handle_client(
    mut stream: UnixStream,
    engine: Arc<Mutex<RuntimeEngine>>,
    shutdown: Arc<AtomicBool>,
) {
    let reader_stream = match stream.try_clone() {
        Ok(v) => v,
        Err(e) => {
            let _ = stream.write_all(format!("ERR INTERNAL_ERROR clone_failed:{}\n", e).as_bytes());
            return;
        }
    };

    let mut reader = BufReader::new(reader_stream);
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

                let outcome = {
                    match engine.lock() {
                        Ok(mut guard) => execute_command(&mut guard, &line),
                        Err(_) => {
                            let _ = stream.write_all(b"ERR INTERNAL_ERROR lock_poisoned\n");
                            break;
                        }
                    }
                };

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
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let socket_path = parse_socket_arg(&args);
    let render_output_dir = parse_render_output_dir_arg(&args);
    let socket_path = PathBuf::from(socket_path);

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

    let engine = Arc::new(Mutex::new(RuntimeEngine::new_with_render_output_dir(
        render_output_dir.clone(),
    )));
    let shutdown = Arc::new(AtomicBool::new(false));

    println!("daemon_render_output_dir={}", render_output_dir);

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

    println!("daemon_status=stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
