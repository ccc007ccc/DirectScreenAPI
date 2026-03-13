use std::ffi::CString;
use std::io::{IoSlice, IoSliceMut, Write};
use std::mem::size_of;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::ptr::NonNull;
use std::sync::Arc;

use directscreen_core::api::{RenderFrameInfo, Status, RENDER_MAX_FRAME_BYTES};
use directscreen_core::engine::RuntimeEngine;
use nix::cmsg_space;
use nix::sys::socket::{recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags};

use super::dsapid_parse::{DmabufFrameSubmitRequest, ShmFrameSubmitRequest};
use super::write_status_error;

const INITIAL_BOUND_FD_CAPACITY: usize = 512 * 1024;
const SHM_META_ALIGN: usize = 64;
const SHM_META_MAGIC: u32 = 0x4d48_5344; // "DSHM" (LE)
const SHM_META_VERSION: u16 = 1;
const PIXEL_FORMAT_RGBA8888: u32 = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SharedFrameMeta {
    pub magic: u32,
    pub version: u16,
    pub header_len: u16,
    pub frame_seq: u64,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub byte_len: u32,
    pub offset: u32,
    pub checksum_fnv1a32: u32,
    pub reserved0: u32,
    pub reserved1: u32,
}

#[derive(Debug)]
struct MappedSharedRegion {
    fd: i32,
    ptr: NonNull<u8>,
    len: usize,
}

impl MappedSharedRegion {
    fn create(name: &str, len: usize) -> std::io::Result<Self> {
        if len == 0 {
            return Err(std::io::Error::other("memfd_len_zero"));
        }

        let fd = create_memfd_sized(name, len)?;
        let mapped = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if mapped == libc::MAP_FAILED {
            let err = std::io::Error::last_os_error();
            close_fd(fd);
            return Err(err);
        }

        let ptr = NonNull::new(mapped as *mut u8)
            .ok_or_else(|| std::io::Error::other("mmap_null_pointer"))?;
        Ok(Self { fd, ptr, len })
    }

    fn fd(&self) -> i32 {
        self.fd
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl Drop for MappedSharedRegion {
    fn drop(&mut self) {
        let _ = unsafe { libc::munmap(self.ptr.as_ptr() as *mut libc::c_void, self.len) };
        close_fd(self.fd);
    }
}

#[derive(Debug)]
pub(super) struct BoundFrameFd {
    region: MappedSharedRegion,
    pub(super) capacity: usize,
    pub(super) data_offset: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BinaryFrameBindReply {
    pub(super) capacity: u64,
    pub(super) data_offset: u64,
    pub(super) fd: i32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BinaryFrameWaitReply {
    pub(super) frame_seq: u64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) byte_len: u32,
    pub(super) checksum_fnv1a32: u32,
    pub(super) offset: u32,
    pub(super) origin_x: i32,
    pub(super) origin_y: i32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BinaryFrameSubmitReply {
    pub(super) frame_seq: u64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) byte_len: u32,
    pub(super) checksum_fnv1a32: u32,
}

impl BoundFrameFd {
    fn fd(&self) -> i32 {
        self.region.fd()
    }
}

pub(super) fn close_fd(fd: i32) {
    if fd >= 0 {
        let _ = unsafe { libc::close(fd) };
    }
}

fn align_up(value: usize, align: usize) -> usize {
    if align <= 1 {
        return value;
    }
    let rem = value % align;
    if rem == 0 {
        value
    } else {
        value.saturating_add(align - rem)
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

fn create_bound_frame_shm(engine: &Arc<RuntimeEngine>) -> std::io::Result<BoundFrameFd> {
    let display_capacity = {
        let d = engine.display_state();
        (d.width as usize)
            .checked_mul(d.height as usize)
            .and_then(|v| v.checked_mul(4))
            .unwrap_or(0)
    };
    let last_frame_capacity = engine.render_frame_byte_len().unwrap_or(0);
    let frame_capacity = display_capacity.max(last_frame_capacity);
    let capacity = frame_capacity.clamp(INITIAL_BOUND_FD_CAPACITY, RENDER_MAX_FRAME_BYTES);
    let data_offset = align_up(size_of::<SharedFrameMeta>(), SHM_META_ALIGN);
    let total_len = data_offset
        .checked_add(capacity)
        .ok_or_else(|| std::io::Error::other("bound_shm_capacity_overflow"))?;
    let region = MappedSharedRegion::create("dsapi_frame_shm", total_len)?;
    Ok(BoundFrameFd {
        region,
        capacity,
        data_offset,
    })
}

fn ensure_bound_shm(
    engine: &Arc<RuntimeEngine>,
    bound_fd: &mut Option<BoundFrameFd>,
) -> std::io::Result<()> {
    if bound_fd.is_none() {
        *bound_fd = Some(create_bound_frame_shm(engine)?);
    }
    Ok(())
}

fn build_shared_meta(frame: RenderFrameInfo, data_offset: usize) -> SharedFrameMeta {
    SharedFrameMeta {
        magic: SHM_META_MAGIC,
        version: SHM_META_VERSION,
        header_len: size_of::<SharedFrameMeta>() as u16,
        frame_seq: frame.frame_seq,
        width: frame.width,
        height: frame.height,
        pixel_format: PIXEL_FORMAT_RGBA8888,
        byte_len: frame.byte_len,
        offset: data_offset as u32,
        checksum_fnv1a32: frame.checksum_fnv1a32,
        reserved0: 0,
        reserved1: 0,
    }
}

fn write_meta_to_slice(bytes: &mut [u8], meta: SharedFrameMeta) -> std::io::Result<()> {
    let header_len = size_of::<SharedFrameMeta>();
    if bytes.len() < header_len {
        return Err(std::io::Error::other("bound_shm_header_too_small"));
    }

    let mut cursor = 0usize;
    let mut write = |src: &[u8]| {
        let end = cursor + src.len();
        bytes[cursor..end].copy_from_slice(src);
        cursor = end;
    };

    write(&meta.magic.to_le_bytes());
    write(&meta.version.to_le_bytes());
    write(&meta.header_len.to_le_bytes());
    write(&meta.frame_seq.to_le_bytes());
    write(&meta.width.to_le_bytes());
    write(&meta.height.to_le_bytes());
    write(&meta.pixel_format.to_le_bytes());
    write(&meta.byte_len.to_le_bytes());
    write(&meta.offset.to_le_bytes());
    write(&meta.checksum_fnv1a32.to_le_bytes());
    write(&meta.reserved0.to_le_bytes());
    write(&meta.reserved1.to_le_bytes());
    Ok(())
}

fn write_frame_into_bound_shm(
    bound_fd: &mut BoundFrameFd,
    frame: RenderFrameInfo,
    pixels_rgba8: &[u8],
) -> std::io::Result<SharedFrameMeta> {
    let total = frame.byte_len as usize;
    if total != pixels_rgba8.len() {
        return Err(std::io::Error::other("frame_snapshot_len_mismatch"));
    }
    if total > bound_fd.capacity {
        return Err(std::io::Error::other("frame_len_out_of_bound_capacity"));
    }

    let end = bound_fd
        .data_offset
        .checked_add(total)
        .ok_or_else(|| std::io::Error::other("frame_offset_overflow"))?;
    let mapped = bound_fd.region.as_mut_slice();
    if end > mapped.len() {
        return Err(std::io::Error::other("frame_mapped_region_overflow"));
    }

    mapped[bound_fd.data_offset..end].copy_from_slice(pixels_rgba8);
    let meta = build_shared_meta(frame, bound_fd.data_offset);
    write_meta_to_slice(mapped, meta)?;
    Ok(meta)
}

pub(super) fn handle_frame_bind_shm_binary(
    engine: &Arc<RuntimeEngine>,
    bound_fd: &mut Option<BoundFrameFd>,
) -> std::io::Result<BinaryFrameBindReply> {
    ensure_bound_shm(engine, bound_fd)?;
    let Some(bound) = bound_fd.as_ref() else {
        return Err(std::io::Error::other("bound_shm_missing"));
    };
    Ok(BinaryFrameBindReply {
        capacity: bound.capacity as u64,
        data_offset: bound.data_offset as u64,
        fd: bound.fd(),
    })
}

pub(super) fn handle_frame_wait_shm_present_binary(
    engine: &Arc<RuntimeEngine>,
    bound_fd: &mut BoundFrameFd,
    last_seq: u64,
    timeout_ms: u32,
) -> Result<Option<BinaryFrameWaitReply>, Status> {
    let Some((frame, pixels_rgba8, origin_x, origin_y)) = engine
        .wait_for_frame_after_and_present(last_seq, timeout_ms)
        .map_err(|_| Status::InternalError)?
    else {
        return Ok(None);
    };

    if frame.byte_len as usize > bound_fd.capacity {
        return Err(Status::OutOfRange);
    }
    let meta = write_frame_into_bound_shm(bound_fd, frame, pixels_rgba8.as_ref())
        .map_err(|_| Status::InternalError)?;
    Ok(Some(BinaryFrameWaitReply {
        frame_seq: frame.frame_seq,
        width: frame.width,
        height: frame.height,
        byte_len: frame.byte_len,
        checksum_fnv1a32: frame.checksum_fnv1a32,
        offset: meta.offset,
        origin_x,
        origin_y,
    }))
}

pub(super) fn handle_frame_submit_shm_binary(
    engine: &Arc<RuntimeEngine>,
    bound_fd: &mut BoundFrameFd,
    req: ShmFrameSubmitRequest,
) -> Result<BinaryFrameSubmitReply, Status> {
    if req.byte_len > bound_fd.capacity {
        return Err(Status::OutOfRange);
    }
    let end = req
        .offset
        .checked_add(req.byte_len)
        .ok_or(Status::OutOfRange)?;
    let mapped = bound_fd.region.as_mut_slice();
    if end > mapped.len() {
        return Err(Status::OutOfRange);
    }
    let pixels = &mapped[req.offset..end];
    let frame = engine.submit_render_frame_rgba_at(
        req.width,
        req.height,
        pixels,
        req.origin_x,
        req.origin_y,
    )?;
    Ok(BinaryFrameSubmitReply {
        frame_seq: frame.frame_seq,
        width: frame.width,
        height: frame.height,
        byte_len: frame.byte_len,
        checksum_fnv1a32: frame.checksum_fnv1a32,
    })
}

pub(super) fn handle_frame_bind_shm(
    stream: &UnixStream,
    engine: &Arc<RuntimeEngine>,
    bound_fd: &mut Option<BoundFrameFd>,
) -> std::io::Result<()> {
    ensure_bound_shm(engine, bound_fd)?;
    let Some(bound) = bound_fd.as_ref() else {
        return Err(std::io::Error::other("bound_shm_missing"));
    };

    let line = format!("OK SHM_BOUND {} {}\n", bound.capacity, bound.data_offset);
    send_line_with_fd(stream, &line, bound.fd())
}

pub(super) fn handle_frame_wait_shm_present(
    stream: &mut UnixStream,
    engine: &Arc<RuntimeEngine>,
    bound_fd: &mut BoundFrameFd,
    last_seq: u64,
    timeout_ms: u32,
) -> std::io::Result<()> {
    let Some((frame, pixels_rgba8, origin_x, origin_y)) = engine
        .wait_for_frame_after_and_present(last_seq, timeout_ms)
        .map_err(|_| std::io::Error::other("frame_wait_failed"))?
    else {
        stream.write_all(b"OK TIMEOUT\n")?;
        stream.flush()?;
        return Ok(());
    };

    if frame.byte_len as usize > bound_fd.capacity {
        write_status_error(stream, Status::OutOfRange);
        return Ok(());
    }

    let meta = write_frame_into_bound_shm(bound_fd, frame, pixels_rgba8.as_ref())?;
    let line = format!(
        "OK {} {} {} RGBA8888 {} {} {} {} {}\n",
        frame.frame_seq,
        frame.width,
        frame.height,
        frame.byte_len,
        frame.checksum_fnv1a32,
        meta.offset,
        origin_x,
        origin_y
    );
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    Ok(())
}

pub(super) fn handle_frame_submit_shm(
    stream: &mut UnixStream,
    engine: &Arc<RuntimeEngine>,
    bound_fd: &mut BoundFrameFd,
    req: ShmFrameSubmitRequest,
) -> std::io::Result<()> {
    if req.byte_len > bound_fd.capacity {
        write_status_error(stream, Status::OutOfRange);
        return Ok(());
    }
    let end = req
        .offset
        .checked_add(req.byte_len)
        .ok_or_else(|| std::io::Error::other("submit_shm_offset_overflow"))?;

    let mapped = bound_fd.region.as_mut_slice();
    if end > mapped.len() {
        write_status_error(stream, Status::OutOfRange);
        return Ok(());
    }

    let pixels = &mapped[req.offset..end];
    let frame = match engine.submit_render_frame_rgba_at(
        req.width,
        req.height,
        pixels,
        req.origin_x,
        req.origin_y,
    ) {
        Ok(v) => v,
        Err(status) => {
            write_status_error(stream, status);
            return Ok(());
        }
    };

    let line = format!(
        "OK {} {} {} RGBA8888 {} {}\n",
        frame.frame_seq, frame.width, frame.height, frame.byte_len, frame.checksum_fnv1a32
    );
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    Ok(())
}

pub(super) fn recv_single_fd(stream: &UnixStream) -> std::io::Result<i32> {
    let mut marker = [0u8; 1];
    let mut iov = [IoSliceMut::new(&mut marker)];
    let mut cmsg_buf = cmsg_space!([i32; 1]);
    let msg = recvmsg::<()>(
        stream.as_raw_fd(),
        &mut iov,
        Some(&mut cmsg_buf),
        MsgFlags::empty(),
    )
    .map_err(std::io::Error::other)?;

    if msg.bytes == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "submit_dmabuf_fd_eof",
        ));
    }

    if let Ok(iter) = msg.cmsgs() {
        for cmsg in iter {
            if let ControlMessageOwned::ScmRights(fds) = cmsg {
                if let Some(fd) = fds.first() {
                    return Ok(*fd);
                }
            }
        }
    }

    Err(std::io::Error::other("submit_dmabuf_fd_missing"))
}

pub(super) fn handle_frame_submit_dmabuf_binary(
    read_stream: &UnixStream,
    engine: &Arc<RuntimeEngine>,
    req: DmabufFrameSubmitRequest,
) -> Result<BinaryFrameSubmitReply, Status> {
    let _usage = req.usage;
    let fd = recv_single_fd(read_stream).map_err(|_| Status::InvalidArgument)?;
    let frame = match submit_dmabuf_tight_rgba(engine, fd, req) {
        Ok(v) => {
            close_fd(fd);
            v
        }
        Err(status) => {
            close_fd(fd);
            return Err(status);
        }
    };

    Ok(BinaryFrameSubmitReply {
        frame_seq: frame.frame_seq,
        width: frame.width,
        height: frame.height,
        byte_len: frame.byte_len,
        checksum_fnv1a32: frame.checksum_fnv1a32,
    })
}

fn submit_dmabuf_tight_rgba(
    engine: &Arc<RuntimeEngine>,
    fd: i32,
    req: DmabufFrameSubmitRequest,
) -> Result<RenderFrameInfo, Status> {
    if req.format != PIXEL_FORMAT_RGBA8888 {
        return Err(Status::InvalidArgument);
    }

    let map_len = req.byte_len;
    let mapped = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            map_len,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    if mapped == libc::MAP_FAILED {
        return Err(Status::InvalidArgument);
    }

    let frame = (|| -> Result<RenderFrameInfo, Status> {
        let tight_len = (req.width as usize)
            .checked_mul(req.height as usize)
            .and_then(|v| v.checked_mul(4usize))
            .ok_or(Status::OutOfRange)?;
        let row_bytes = (req.stride as usize)
            .checked_mul(4usize)
            .ok_or(Status::OutOfRange)?;
        let crop_row_bytes = (req.width as usize)
            .checked_mul(4usize)
            .ok_or(Status::OutOfRange)?;

        let src = unsafe { std::slice::from_raw_parts(mapped as *const u8, map_len) };
        if req.byte_offset > src.len() {
            return Err(Status::InvalidArgument);
        }

        if req.stride == req.width {
            let end = req
                .byte_offset
                .checked_add(tight_len)
                .ok_or(Status::OutOfRange)?;
            if end > src.len() {
                return Err(Status::InvalidArgument);
            }

            return engine.submit_render_frame_rgba_at(
                req.width,
                req.height,
                &src[req.byte_offset..end],
                req.origin_x,
                req.origin_y,
            );
        }

        let mut tight = vec![0u8; tight_len];
        for y in 0..(req.height as usize) {
            let src_off = req
                .byte_offset
                .checked_add(y.checked_mul(row_bytes).ok_or(Status::OutOfRange)?)
                .ok_or(Status::OutOfRange)?;
            let src_end = src_off
                .checked_add(crop_row_bytes)
                .ok_or(Status::OutOfRange)?;
            if src_end > src.len() {
                return Err(Status::InvalidArgument);
            }
            let dst_off = y.checked_mul(crop_row_bytes).ok_or(Status::OutOfRange)?;
            let dst_end = dst_off
                .checked_add(crop_row_bytes)
                .ok_or(Status::OutOfRange)?;
            tight[dst_off..dst_end].copy_from_slice(&src[src_off..src_end]);
        }

        engine.submit_render_frame_rgba_at(
            req.width,
            req.height,
            tight.as_slice(),
            req.origin_x,
            req.origin_y,
        )
    })();

    let _ = unsafe { libc::munmap(mapped, map_len) };
    frame
}

pub(super) fn handle_frame_submit_dmabuf(
    stream: &mut UnixStream,
    read_stream: &UnixStream,
    engine: &Arc<RuntimeEngine>,
    req: DmabufFrameSubmitRequest,
) -> std::io::Result<()> {
    let _usage = req.usage;
    let fd = match recv_single_fd(read_stream) {
        Ok(v) => v,
        Err(e) => {
            write_status_error(stream, Status::InvalidArgument);
            return Err(e);
        }
    };

    let frame = match submit_dmabuf_tight_rgba(engine, fd, req) {
        Ok(v) => {
            close_fd(fd);
            v
        }
        Err(status) => {
            close_fd(fd);
            write_status_error(stream, status);
            return Ok(());
        }
    };

    let line = format!(
        "OK {} {} {} RGBA8888 {} {}\n",
        frame.frame_seq, frame.width, frame.height, frame.byte_len, frame.checksum_fnv1a32
    );
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    Ok(())
}
