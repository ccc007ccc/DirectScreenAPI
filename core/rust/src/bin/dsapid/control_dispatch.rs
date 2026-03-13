use std::io::IoSlice;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use directscreen_core::api::Status;
use directscreen_core::engine::{
    execute_binary_command, parse_binary_command, parse_binary_header, BinaryCommand, BinaryOpcode,
    BinaryResponse, RuntimeEngine, BINARY_COMMAND_HEADER_BYTES, BINARY_PROTOCOL_MAGIC,
    BINARY_PROTOCOL_VERSION, BINARY_RESPONSE_PAYLOAD_BYTES, BINARY_RESPONSE_VALUE_COUNT,
};
use mio::Waker;
use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};

use super::dsapid_frame_fd::{
    handle_frame_bind_shm_binary, handle_frame_submit_dmabuf_binary,
    handle_frame_submit_shm_binary, handle_frame_wait_shm_present_binary, BoundFrameFd,
};
use super::dsapid_parse::{DmabufFrameSubmitRequest, ShmFrameSubmitRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub(super) enum DaemonStartupState {
    Starting = 0,
    Ready = 1,
    Stopping = 2,
}

pub(super) struct DaemonLifecycle {
    state: AtomicU8,
    boot_id: u64,
    started_at: Instant,
    pid: u32,
}

impl DaemonLifecycle {
    pub(super) fn new() -> Self {
        let now_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let pid = std::process::id();
        let boot_id = now_nanos ^ (pid as u64);
        Self {
            state: AtomicU8::new(DaemonStartupState::Starting as u8),
            boot_id,
            started_at: Instant::now(),
            pid,
        }
    }

    pub(super) fn transition(&self, state: DaemonStartupState) {
        self.state.store(state as u8, Ordering::SeqCst);
    }

    fn build_ready_response(&self, seq: u64) -> BinaryResponse {
        let state = self.state.load(Ordering::SeqCst) as u64;
        let uptime_ms = self.started_at.elapsed().as_millis() as u64;
        let mut resp = BinaryResponse::with_status(seq, BinaryOpcode::ReadyGet as u16, Status::Ok);
        resp.values[0] = state;
        resp.values[1] = self.boot_id;
        resp.values[2] = uptime_ms;
        resp.values[3] = self.pid as u64;
        resp
    }
}

fn read_exact_or_disconnect(stream: &mut UnixStream, buf: &mut [u8]) -> std::io::Result<bool> {
    let mut offset = 0usize;
    while offset < buf.len() {
        match stream.read(&mut buf[offset..]) {
            Ok(0) => {
                if offset == 0 {
                    return Ok(false);
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "binary_frame_truncated",
                ));
            }
            Ok(n) => offset += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

fn encode_response(response: BinaryResponse, body: &[u8]) -> Vec<u8> {
    let payload_len = BINARY_RESPONSE_PAYLOAD_BYTES.saturating_add(body.len());
    let mut out = vec![0u8; BINARY_COMMAND_HEADER_BYTES + payload_len];

    out[0..4].copy_from_slice(&BINARY_PROTOCOL_MAGIC.to_le_bytes());
    out[4..6].copy_from_slice(&BINARY_PROTOCOL_VERSION.to_le_bytes());
    out[6..8].copy_from_slice(&response.opcode.to_le_bytes());
    out[8..12].copy_from_slice(&(payload_len as u32).to_le_bytes());
    out[12..20].copy_from_slice(&response.seq.to_le_bytes());

    let fixed_payload_end = BINARY_COMMAND_HEADER_BYTES + BINARY_RESPONSE_PAYLOAD_BYTES;
    let payload = &mut out[BINARY_COMMAND_HEADER_BYTES..fixed_payload_end];
    payload[0..4].copy_from_slice(&(response.status as i32).to_le_bytes());
    let mut cursor = 4usize;
    for v in response.values.iter().take(BINARY_RESPONSE_VALUE_COUNT) {
        let end = cursor + 8;
        payload[cursor..end].copy_from_slice(&v.to_le_bytes());
        cursor = end;
    }
    if !body.is_empty() {
        out[fixed_payload_end..].copy_from_slice(body);
    }

    out
}

fn write_binary_response(
    stream: &mut UnixStream,
    response: BinaryResponse,
    body: &[u8],
) -> std::io::Result<()> {
    let bytes = encode_response(response, body);
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

fn write_binary_response_with_fd(
    stream: &UnixStream,
    response: BinaryResponse,
    fd: i32,
) -> std::io::Result<()> {
    let bytes = encode_response(response, &[]);
    let iov = [IoSlice::new(&bytes)];
    let fds = [fd];
    let cmsgs = [ControlMessage::ScmRights(&fds)];
    let sent = sendmsg::<()>(stream.as_raw_fd(), &iov, &cmsgs, MsgFlags::empty(), None)
        .map_err(std::io::Error::other)?;
    if sent != bytes.len() {
        return Err(std::io::Error::other("sendmsg_short_write"));
    }
    Ok(())
}

fn shm_submit_request_from_payload(
    payload: directscreen_core::engine::BinaryRenderFrameSubmitShmPayload,
) -> Result<ShmFrameSubmitRequest, Status> {
    Ok(ShmFrameSubmitRequest {
        width: payload.width,
        height: payload.height,
        byte_len: usize::try_from(payload.byte_len).map_err(|_| Status::OutOfRange)?,
        offset: usize::try_from(payload.offset).map_err(|_| Status::OutOfRange)?,
        origin_x: payload.origin_x,
        origin_y: payload.origin_y,
    })
}

fn dmabuf_submit_request_from_payload(
    payload: directscreen_core::engine::BinaryRenderFrameSubmitDmabufPayload,
) -> Result<DmabufFrameSubmitRequest, Status> {
    Ok(DmabufFrameSubmitRequest {
        width: payload.width,
        height: payload.height,
        stride: payload.stride,
        format: payload.format,
        usage: payload.usage,
        byte_len: usize::try_from(payload.byte_len).map_err(|_| Status::OutOfRange)?,
        byte_offset: usize::try_from(payload.byte_offset).map_err(|_| Status::OutOfRange)?,
        origin_x: payload.origin_x,
        origin_y: payload.origin_y,
    })
}

fn write_binary_error(stream: &mut UnixStream, status: Status) {
    let _ = write_binary_response(stream, BinaryResponse::with_status(0, 0, status), &[]);
}

pub(super) fn handle_control_client(
    mut stream: UnixStream,
    peer_uid: u32,
    engine: Arc<RuntimeEngine>,
    shutdown: Arc<AtomicBool>,
    shutdown_waker: Arc<Waker>,
    lifecycle: Arc<DaemonLifecycle>,
    command_max_bytes: usize,
) {
    let max_payload = command_max_bytes.max(BINARY_COMMAND_HEADER_BYTES);
    let frame_capacity = BINARY_COMMAND_HEADER_BYTES.saturating_add(max_payload);
    let mut frame = vec![0u8; frame_capacity];
    let mut bound_frame_fd: Option<BoundFrameFd> = None;

    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let header_bytes = &mut frame[..BINARY_COMMAND_HEADER_BYTES];
        match read_exact_or_disconnect(&mut stream, header_bytes) {
            Ok(false) => break,
            Ok(true) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                write_binary_error(&mut stream, Status::InternalError);
                break;
            }
            Err(_e) if _e.kind() == std::io::ErrorKind::InvalidData => {
                write_binary_error(&mut stream, Status::InvalidArgument);
                continue;
            }
            Err(_e) => {
                write_binary_error(&mut stream, Status::InternalError);
                break;
            }
        }

        let header = match parse_binary_header(header_bytes) {
            Ok(v) => v,
            Err(status) => {
                write_binary_error(&mut stream, status);
                continue;
            }
        };

        let payload_len = header.payload_len as usize;
        if payload_len > max_payload {
            write_binary_error(&mut stream, Status::OutOfRange);
            break;
        }

        let frame_len = BINARY_COMMAND_HEADER_BYTES.saturating_add(payload_len);
        if payload_len > 0 {
            let payload = &mut frame[BINARY_COMMAND_HEADER_BYTES..frame_len];
            match read_exact_or_disconnect(&mut stream, payload) {
                Ok(true) => {}
                Ok(false) => break,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    write_binary_error(&mut stream, Status::InternalError);
                    break;
                }
                Err(_) => {
                    write_binary_error(&mut stream, Status::InvalidArgument);
                    break;
                }
            }
        }

        let parsed = match parse_binary_command(&frame[..frame_len]) {
            Ok(v) => v,
            Err(status) => {
                write_binary_error(&mut stream, status);
                continue;
            }
        };

        let mut response_fd: Option<i32> = None;
        let mut response_body: Vec<u8> = Vec::new();
        let mut response = match parsed {
            BinaryCommand::RenderFrameBindShm { seq } => {
                let mut resp = BinaryResponse::with_status(
                    seq,
                    BinaryOpcode::RenderFrameBindShm as u16,
                    Status::Ok,
                );
                match handle_frame_bind_shm_binary(&engine, &mut bound_frame_fd) {
                    Ok(bound) => {
                        resp.values[0] = bound.capacity;
                        resp.values[1] = bound.data_offset;
                        resp.values[2] = 1;
                        response_fd = Some(bound.fd);
                    }
                    Err(_) => {
                        resp.status = Status::InternalError;
                    }
                }
                resp
            }
            BinaryCommand::RenderFrameWaitShmPresent { seq, payload } => {
                let mut resp = BinaryResponse::with_status(
                    seq,
                    BinaryOpcode::RenderFrameWaitShmPresent as u16,
                    Status::Ok,
                );
                if let Some(bound) = bound_frame_fd.as_mut() {
                    match handle_frame_wait_shm_present_binary(
                        &engine,
                        bound,
                        payload.last_seq,
                        payload.timeout_ms,
                    ) {
                        Ok(Some(frame)) => {
                            resp.values[0] = frame.frame_seq;
                            resp.values[1] = frame.width as u64;
                            resp.values[2] = frame.height as u64;
                            resp.values[3] = frame.byte_len as u64;
                            resp.values[4] = frame.checksum_fnv1a32 as u64;
                            resp.values[5] = frame.offset as u64;
                            resp.values[6] = frame.origin_x as i64 as u64;
                            resp.values[7] = frame.origin_y as i64 as u64;
                        }
                        Ok(None) => {
                            resp.values[0] = payload.last_seq;
                            resp.values[3] = 0;
                        }
                        Err(status) => {
                            resp.status = status;
                        }
                    }
                } else {
                    resp.status = Status::InvalidArgument;
                }
                resp
            }
            BinaryCommand::RenderFrameSubmitShm { seq, payload } => {
                let mut resp = BinaryResponse::with_status(
                    seq,
                    BinaryOpcode::RenderFrameSubmitShm as u16,
                    Status::Ok,
                );
                if let Some(bound) = bound_frame_fd.as_mut() {
                    match shm_submit_request_from_payload(payload) {
                        Ok(req) => match handle_frame_submit_shm_binary(&engine, bound, req) {
                            Ok(frame) => {
                                resp.values[0] = frame.frame_seq;
                                resp.values[1] = frame.width as u64;
                                resp.values[2] = frame.height as u64;
                                resp.values[3] = frame.byte_len as u64;
                                resp.values[4] = frame.checksum_fnv1a32 as u64;
                            }
                            Err(status) => {
                                resp.status = status;
                            }
                        },
                        Err(status) => {
                            resp.status = status;
                        }
                    }
                } else {
                    resp.status = Status::InvalidArgument;
                }
                resp
            }
            BinaryCommand::RenderFrameSubmitDmabuf { seq, payload } => {
                let mut resp = BinaryResponse::with_status(
                    seq,
                    BinaryOpcode::RenderFrameSubmitDmabuf as u16,
                    Status::Ok,
                );
                match dmabuf_submit_request_from_payload(payload) {
                    Ok(req) => match handle_frame_submit_dmabuf_binary(&stream, &engine, req) {
                        Ok(frame) => {
                            resp.values[0] = frame.frame_seq;
                            resp.values[1] = frame.width as u64;
                            resp.values[2] = frame.height as u64;
                            resp.values[3] = frame.byte_len as u64;
                            resp.values[4] = frame.checksum_fnv1a32 as u64;
                        }
                        Err(status) => {
                            resp.status = status;
                        }
                    },
                    Err(status) => {
                        resp.status = status;
                    }
                }
                resp
            }
            BinaryCommand::ModuleRpc { seq, payload } => {
                let mut resp =
                    BinaryResponse::with_status(seq, BinaryOpcode::ModuleRpc as u16, Status::Ok);
                match engine.module_rpc(&payload, peer_uid) {
                    Ok(body) => {
                        if !body.is_empty() {
                            response_body = body.into_bytes();
                        }
                    }
                    Err(err) => {
                        resp.status = err.status;
                        if !err.body.is_empty() {
                            response_body = err.body.into_bytes();
                        }
                    }
                }
                resp
            }
            _ => execute_binary_command(&engine, &frame[..frame_len]),
        };
        if response.status == Status::Ok && response.opcode == BinaryOpcode::ReadyGet as u16 {
            response = lifecycle.build_ready_response(response.seq);
            if let Ok(module_count) = engine.module_registry_count() {
                response.values[4] = module_count as u64;
            }
            if let Ok(module_event_seq) = engine.module_registry_event_seq() {
                response.values[5] = module_event_seq;
            }
            if let Ok(last_error) = engine.module_registry_last_error() {
                response.values[6] = if last_error.is_some() { 1 } else { 0 };
            }
        }
        let write_result = if let Some(fd) = response_fd {
            write_binary_response_with_fd(&stream, response, fd)
        } else {
            write_binary_response(&mut stream, response, &response_body)
        };
        if let Err(_e) = write_result {
            break;
        }

        let should_shutdown =
            response.status == Status::Ok && response.opcode == BinaryOpcode::Shutdown as u16;
        if should_shutdown {
            lifecycle.transition(DaemonStartupState::Stopping);
            shutdown.store(true, Ordering::SeqCst);
            let _ = shutdown_waker.wake();
            break;
        }
    }
}
