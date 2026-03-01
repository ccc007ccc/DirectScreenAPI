use std::ffi::CString;
use std::io::{IoSlice, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use directscreen_core::api::{Status, RENDER_MAX_FRAME_BYTES};
use directscreen_core::engine::RuntimeEngine;
use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};

use super::write_status_error;

const INITIAL_BOUND_FD_CAPACITY: usize = 512 * 1024;

#[derive(Debug, Clone, Copy)]
pub(super) struct BoundFrameFd {
    pub(super) fd: i32,
    pub(super) capacity: usize,
}

pub(super) fn close_fd(fd: i32) {
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

pub(super) fn handle_frame_get_fd(
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

pub(super) fn handle_frame_bind_fd(
    stream: &UnixStream,
    engine: &Arc<RuntimeEngine>,
    bound_fd: &mut Option<BoundFrameFd>,
) -> std::io::Result<()> {
    if bound_fd.is_none() {
        let frame_capacity = engine.render_frame_byte_len().unwrap_or(0);
        let capacity = frame_capacity.clamp(INITIAL_BOUND_FD_CAPACITY, RENDER_MAX_FRAME_BYTES);
        let fd = create_memfd_sized("dsapi_frame_bound", capacity)?;
        *bound_fd = Some(BoundFrameFd { fd, capacity });
    }
    let Some(bound) = bound_fd.as_ref() else {
        return Err(std::io::Error::other("bound_fd_missing"));
    };
    let line = format!("OK BOUND {}\n", bound.capacity);
    send_line_with_fd(stream, &line, bound.fd)
}

pub(super) fn handle_frame_get_bound(
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

pub(super) fn handle_frame_wait_bound_present(
    stream: &mut UnixStream,
    engine: &Arc<RuntimeEngine>,
    bound_fd: &BoundFrameFd,
    last_seq: u64,
    timeout_ms: u32,
) -> std::io::Result<()> {
    let Some((frame, pixels_rgba8)) = engine
        .wait_for_frame_after_and_present(last_seq, timeout_ms)
        .map_err(|_| std::io::Error::other("frame_wait_failed"))?
    else {
        stream.write_all(b"OK TIMEOUT\n")?;
        stream.flush()?;
        return Ok(());
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
        "OK {} {} {} RGBA8888 {} {}\n",
        frame.frame_seq, frame.width, frame.height, frame.byte_len, frame.checksum_fnv1a32
    );
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    Ok(())
}
