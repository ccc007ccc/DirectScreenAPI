use std::fs::File;
use std::io::{self, IoSliceMut, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use nix::cmsg_space;
use nix::errno::Errno;
use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags};

const DEFAULT_SOCKET_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_WAIT_TIMEOUT_MS: u32 = 250;
const MAX_HEADER_LEN: usize = 4096;

#[derive(Debug)]
struct AppError {
    code: i32,
    message: String,
}

impl AppError {
    fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug)]
struct MappedReadOnly {
    ptr: *mut libc::c_void,
    len: usize,
}

impl MappedReadOnly {
    fn new(fd: RawFd, len: usize) -> Result<Self, AppError> {
        if len == 0 {
            return Err(AppError::new(6, "frame_pull_error=invalid_bind_layout"));
        }
        let mapped = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if mapped == libc::MAP_FAILED {
            return Err(AppError::new(
                6,
                format!(
                    "frame_pull_error=mmap_failed err={}",
                    io::Error::last_os_error()
                ),
            ));
        }
        Ok(Self { ptr: mapped, len })
    }

    fn slice(&self, offset: usize, byte_len: usize) -> Result<&[u8], AppError> {
        let end = offset
            .checked_add(byte_len)
            .ok_or_else(|| AppError::new(6, "frame_pull_error=invalid_offset_or_length"))?;
        if end > self.len {
            return Err(AppError::new(
                6,
                "frame_pull_error=invalid_offset_or_length",
            ));
        }
        let start_ptr = unsafe { (self.ptr as *const u8).add(offset) };
        Ok(unsafe { std::slice::from_raw_parts(start_ptr, byte_len) })
    }
}

impl Drop for MappedReadOnly {
    fn drop(&mut self) {
        if !self.ptr.is_null() && self.len > 0 {
            let _ = unsafe { libc::munmap(self.ptr, self.len) };
            self.ptr = std::ptr::null_mut();
            self.len = 0;
        }
    }
}

fn usage() {
    eprintln!("usage:");
    eprintln!("  dsapiframepull <data_socket_path> <out_rgba_path>");
}

fn parse_env_u64(name: &str, default_value: u64) -> u64 {
    let raw = std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default_value);
    if raw == 0 {
        default_value
    } else {
        raw
    }
}

fn parse_env_u32(name: &str, default_value: u32) -> u32 {
    let raw = std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(default_value);
    if raw == 0 {
        default_value
    } else {
        raw
    }
}

fn connect_socket(path: &str, timeout_ms: u64) -> Result<UnixStream, AppError> {
    let stream = UnixStream::connect(path)
        .map_err(|e| AppError::new(4, format!("frame_pull_error=connect_failed err={}", e)))?;
    let timeout = Some(Duration::from_millis(timeout_ms.max(1)));
    stream.set_read_timeout(timeout).map_err(|e| {
        AppError::new(
            4,
            format!("frame_pull_error=set_read_timeout_failed err={}", e),
        )
    })?;
    stream.set_write_timeout(timeout).map_err(|e| {
        AppError::new(
            4,
            format!("frame_pull_error=set_write_timeout_failed err={}", e),
        )
    })?;
    Ok(stream)
}

fn write_line(stream: &mut UnixStream, line: &str) -> Result<(), AppError> {
    stream
        .write_all(line.as_bytes())
        .map_err(|e| AppError::new(4, format!("frame_pull_error=socket_write_failed err={}", e)))?;
    stream
        .write_all(b"\n")
        .map_err(|e| AppError::new(4, format!("frame_pull_error=socket_write_failed err={}", e)))?;
    stream
        .flush()
        .map_err(|e| AppError::new(4, format!("frame_pull_error=socket_write_failed err={}", e)))
}

fn recv_line_with_optional_fd(
    stream: &UnixStream,
    require_fd: bool,
) -> Result<(String, Option<RawFd>), AppError> {
    let mut line: Vec<u8> = Vec::with_capacity(256);
    let mut line_done = false;
    let mut line_text = String::new();
    let mut received_fd: Option<RawFd> = None;

    loop {
        let mut buf = [0u8; 1024];
        let mut iov = [IoSliceMut::new(&mut buf)];
        let mut cmsg = cmsg_space!([RawFd; 4]);
        let bytes_read = {
            let msg = match recvmsg::<()>(
                stream.as_raw_fd(),
                &mut iov,
                Some(&mut cmsg),
                MsgFlags::empty(),
            ) {
                Ok(v) => v,
                Err(errno) if errno == Errno::EAGAIN || errno == Errno::EWOULDBLOCK => {
                    return Err(AppError::new(4, "frame_pull_error=socket_timeout"));
                }
                Err(errno) => {
                    return Err(AppError::new(
                        4,
                        format!("frame_pull_error=socket_read_failed err={}", errno),
                    ));
                }
            };

            if msg.bytes == 0 {
                if line_done {
                    return Err(AppError::new(
                        5,
                        format!("frame_pull_error=bind_missing_fd line='{}'", line_text),
                    ));
                }
                return Err(AppError::new(4, "frame_pull_error=header_eof"));
            }

            let cmsgs = msg.cmsgs().map_err(|e| {
                AppError::new(4, format!("frame_pull_error=socket_read_failed err={}", e))
            })?;
            for cmsg in cmsgs {
                if let ControlMessageOwned::ScmRights(fds) = cmsg {
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
            if line_done {
                continue;
            }
            if *b == b'\n' {
                line_text = String::from_utf8_lossy(&line).trim_end().to_string();
                line_done = true;
                continue;
            }
            if *b != b'\r' {
                line.push(*b);
            }
            if line.len() > MAX_HEADER_LEN {
                return Err(AppError::new(4, "frame_pull_error=header_too_long"));
            }
        }

        if line_done && (!require_fd || received_fd.is_some()) {
            if !require_fd {
                if let Some(fd) = received_fd.take() {
                    let _ = unsafe { libc::close(fd) };
                }
            }
            return Ok((line_text, received_fd));
        }
    }
}

fn parse_bind_response(line: &str) -> Result<(usize, usize), AppError> {
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
        return Err(AppError::new(
            5,
            format!("frame_pull_error=bad_bind_header line='{}'", line),
        ));
    }

    let capacity = p2
        .and_then(|v| v.parse::<usize>().ok())
        .ok_or_else(|| AppError::new(6, "frame_pull_error=invalid_bind_layout"))?;
    let offset = p3
        .and_then(|v| v.parse::<usize>().ok())
        .ok_or_else(|| AppError::new(6, "frame_pull_error=invalid_bind_layout"))?;
    if capacity == 0 {
        return Err(AppError::new(6, "frame_pull_error=invalid_bind_layout"));
    }
    Ok((capacity, offset))
}

#[derive(Debug)]
struct WaitFrame {
    frame_seq: String,
    width: String,
    height: String,
    byte_len: usize,
    offset: usize,
}

fn parse_wait_response(line: &str) -> Result<Option<WaitFrame>, AppError> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() == 2 && parts[0] == "OK" && parts[1] == "TIMEOUT" {
        return Ok(None);
    }
    if parts.len() != 8 || parts[0] != "OK" {
        return Err(AppError::new(
            5,
            format!("frame_pull_error=bad_wait_header line='{}'", line),
        ));
    }
    if parts[4] != "RGBA8888" {
        return Err(AppError::new(
            6,
            format!("frame_pull_error=bad_pixel_format:{}", parts[4]),
        ));
    }
    let byte_len = parts[5]
        .parse::<usize>()
        .map_err(|_| AppError::new(6, "frame_pull_error=invalid_byte_len"))?;
    let offset = parts[7]
        .parse::<usize>()
        .map_err(|_| AppError::new(6, "frame_pull_error=invalid_offset_or_length"))?;
    if byte_len == 0 {
        return Err(AppError::new(6, "frame_pull_error=invalid_byte_len"));
    }
    Ok(Some(WaitFrame {
        frame_seq: parts[1].to_string(),
        width: parts[2].to_string(),
        height: parts[3].to_string(),
        byte_len,
        offset,
    }))
}

fn run(data_socket_path: &str, out_path: &str) -> Result<(), AppError> {
    let socket_timeout_ms = parse_env_u64("DSAPI_FRAME_PULL_TIMEOUT_MS", DEFAULT_SOCKET_TIMEOUT_MS);
    let wait_timeout_ms = parse_env_u32("DSAPI_FRAME_WAIT_TIMEOUT_MS", DEFAULT_WAIT_TIMEOUT_MS);

    let mut stream = connect_socket(data_socket_path, socket_timeout_ms)?;

    write_line(&mut stream, "RENDER_FRAME_BIND_SHM")?;
    let (bind_line, fd_opt) = recv_line_with_optional_fd(&stream, true)?;
    let (capacity, bind_offset) = match parse_bind_response(&bind_line) {
        Ok(v) => v,
        Err(e) => {
            if let Some(fd) = fd_opt {
                let _ = unsafe { libc::close(fd) };
            }
            return Err(e);
        }
    };

    let map_len = capacity
        .checked_add(bind_offset)
        .ok_or_else(|| AppError::new(6, "frame_pull_error=invalid_bind_layout"))?;
    let fd = fd_opt.ok_or_else(|| {
        AppError::new(
            5,
            format!("frame_pull_error=bind_missing_fd line='{}'", bind_line),
        )
    })?;
    let mapping = MappedReadOnly::new(fd, map_len);
    let _ = unsafe { libc::close(fd) };
    let mapping = mapping?;

    write_line(
        &mut stream,
        &format!("RENDER_FRAME_WAIT_SHM_PRESENT 0 {}", wait_timeout_ms),
    )?;
    let (wait_line, _) = recv_line_with_optional_fd(&stream, false)?;
    let frame = match parse_wait_response(&wait_line)? {
        Some(v) => v,
        None => return Err(AppError::new(7, "frame_pull_error=frame_wait_timeout")),
    };

    let rgba = mapping.slice(frame.offset, frame.byte_len)?;
    let mut out = File::create(out_path)
        .map_err(|e| AppError::new(8, format!("frame_pull_error=open_out_failed err={}", e)))?;
    out.write_all(rgba)
        .map_err(|e| AppError::new(8, format!("frame_pull_error=write_out_failed err={}", e)))?;
    out.flush()
        .map_err(|e| AppError::new(8, format!("frame_pull_error=flush_out_failed err={}", e)))?;

    let actual = std::fs::metadata(out_path)
        .map_err(|e| AppError::new(8, format!("frame_pull_error=stat_out_failed err={}", e)))?
        .len() as usize;
    if actual != frame.byte_len {
        return Err(AppError::new(
            8,
            format!(
                "frame_pull_error=byte_count_mismatch expected={} actual={}",
                frame.byte_len, actual
            ),
        ));
    }

    println!(
        "frame_pull=ok frame_seq={} size={}x{} bytes={}",
        frame.frame_seq, frame.width, frame.height, frame.byte_len
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        usage();
        std::process::exit(1);
    }

    let data_socket_path = args[1].trim();
    let out_path = args[2].trim();
    if data_socket_path.is_empty() || out_path.is_empty() {
        usage();
        std::process::exit(1);
    }

    match run(data_socket_path, out_path) {
        Ok(()) => {}
        Err(e) => {
            println!("{}", e.message);
            std::process::exit(e.code);
        }
    }
}
