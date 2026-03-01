use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use directscreen_core::api::Status;
use directscreen_core::engine::{
    execute_binary_command, parse_binary_header, BinaryOpcode, BinaryResponse, RuntimeEngine,
    BINARY_COMMAND_HEADER_BYTES, BINARY_PROTOCOL_MAGIC, BINARY_PROTOCOL_VERSION,
    BINARY_RESPONSE_PAYLOAD_BYTES, BINARY_RESPONSE_VALUE_COUNT,
};

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
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

fn encode_response(
    response: BinaryResponse,
) -> [u8; BINARY_COMMAND_HEADER_BYTES + BINARY_RESPONSE_PAYLOAD_BYTES] {
    let mut out = [0u8; BINARY_COMMAND_HEADER_BYTES + BINARY_RESPONSE_PAYLOAD_BYTES];

    out[0..4].copy_from_slice(&BINARY_PROTOCOL_MAGIC.to_le_bytes());
    out[4..6].copy_from_slice(&BINARY_PROTOCOL_VERSION.to_le_bytes());
    out[6..8].copy_from_slice(&response.opcode.to_le_bytes());
    out[8..12].copy_from_slice(&(BINARY_RESPONSE_PAYLOAD_BYTES as u32).to_le_bytes());
    out[12..20].copy_from_slice(&response.seq.to_le_bytes());

    let payload = &mut out[BINARY_COMMAND_HEADER_BYTES..];
    payload[0..4].copy_from_slice(&(response.status as i32).to_le_bytes());
    let mut cursor = 4usize;
    for v in response.values.iter().take(BINARY_RESPONSE_VALUE_COUNT) {
        let end = cursor + 8;
        payload[cursor..end].copy_from_slice(&v.to_le_bytes());
        cursor = end;
    }

    out
}

fn write_binary_response(stream: &mut UnixStream, response: BinaryResponse) -> std::io::Result<()> {
    let bytes = encode_response(response);
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

fn write_binary_error(stream: &mut UnixStream, status: Status) {
    let _ = write_binary_response(stream, BinaryResponse::with_status(0, 0, status));
}

pub(super) fn handle_control_client(
    mut stream: UnixStream,
    engine: Arc<RuntimeEngine>,
    shutdown: Arc<AtomicBool>,
    command_max_bytes: usize,
) {
    let max_payload = command_max_bytes.max(BINARY_COMMAND_HEADER_BYTES);
    let frame_capacity = BINARY_COMMAND_HEADER_BYTES.saturating_add(max_payload);
    let mut frame = vec![0u8; frame_capacity];

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

        let response = execute_binary_command(&engine, &frame[..frame_len]);
        if let Err(_e) = write_binary_response(&mut stream, response) {
            break;
        }

        let should_shutdown =
            response.status == Status::Ok && response.opcode == BinaryOpcode::Shutdown as u16;
        if should_shutdown {
            shutdown.store(true, Ordering::SeqCst);
            break;
        }
    }
}
