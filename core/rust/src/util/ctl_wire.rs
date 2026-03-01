use std::io::{Read, Write};

use crate::api::Status;
use crate::engine::{
    BinaryOpcode, BINARY_COMMAND_HEADER_BYTES, BINARY_PROTOCOL_MAGIC, BINARY_PROTOCOL_VERSION,
    BINARY_RESPONSE_PAYLOAD_BYTES, BINARY_RESPONSE_VALUE_COUNT,
};

#[derive(Debug, Clone)]
pub enum BinaryCtlCommand {
    Ping,
    Version,
    DisplayGet,
    DisplayWait {
        last_seq: u64,
        timeout_ms: u32,
    },
    DisplaySet {
        width: u32,
        height: u32,
        refresh_hz: f32,
        density_dpi: u32,
        rotation: u32,
    },
    TouchClear,
    TouchCount,
    TouchMove {
        pointer_id: i32,
        x: f32,
        y: f32,
    },
    RenderSubmit {
        draw_calls: u32,
        frost_passes: u32,
        text_calls: u32,
    },
    RenderGet,
    Shutdown,
}

#[derive(Debug, Clone, Copy)]
pub struct BinaryCtlResponse {
    pub seq: u64,
    pub opcode: u16,
    pub status: Status,
    pub values: [u64; BINARY_RESPONSE_VALUE_COUNT],
}

fn parse_u32(token: &str) -> Result<u32, String> {
    token
        .parse::<u32>()
        .map_err(|_| format!("invalid_u32:{}", token))
}

fn parse_i32(token: &str) -> Result<i32, String> {
    token
        .parse::<i32>()
        .map_err(|_| format!("invalid_i32:{}", token))
}

fn parse_u64(token: &str) -> Result<u64, String> {
    token
        .parse::<u64>()
        .map_err(|_| format!("invalid_u64:{}", token))
}

fn parse_f32(token: &str) -> Result<f32, String> {
    token
        .parse::<f32>()
        .map_err(|_| format!("invalid_f32:{}", token))
}

pub fn parse_command_tokens(tokens: &[&str]) -> Result<BinaryCtlCommand, String> {
    if tokens.is_empty() {
        return Err("empty_command".to_string());
    }

    let cmd = tokens[0];
    if cmd.eq_ignore_ascii_case("PING") {
        if tokens.len() != 1 {
            return Err("ping_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::Ping);
    }
    if cmd.eq_ignore_ascii_case("VERSION") {
        if tokens.len() != 1 {
            return Err("version_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::Version);
    }
    if cmd.eq_ignore_ascii_case("DISPLAY_GET") {
        if tokens.len() != 1 {
            return Err("display_get_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::DisplayGet);
    }
    if cmd.eq_ignore_ascii_case("DISPLAY_WAIT") {
        if tokens.len() != 3 {
            return Err("display_wait_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::DisplayWait {
            last_seq: parse_u64(tokens[1])?,
            timeout_ms: parse_u32(tokens[2])?,
        });
    }
    if cmd.eq_ignore_ascii_case("DISPLAY_SET") {
        if tokens.len() != 6 {
            return Err("display_set_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::DisplaySet {
            width: parse_u32(tokens[1])?,
            height: parse_u32(tokens[2])?,
            refresh_hz: parse_f32(tokens[3])?,
            density_dpi: parse_u32(tokens[4])?,
            rotation: parse_u32(tokens[5])?,
        });
    }
    if cmd.eq_ignore_ascii_case("TOUCH_CLEAR") {
        if tokens.len() != 1 {
            return Err("touch_clear_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::TouchClear);
    }
    if cmd.eq_ignore_ascii_case("TOUCH_COUNT") {
        if tokens.len() != 1 {
            return Err("touch_count_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::TouchCount);
    }
    if cmd.eq_ignore_ascii_case("TOUCH_MOVE") {
        if tokens.len() != 4 {
            return Err("touch_move_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::TouchMove {
            pointer_id: parse_i32(tokens[1])?,
            x: parse_f32(tokens[2])?,
            y: parse_f32(tokens[3])?,
        });
    }
    if cmd.eq_ignore_ascii_case("RENDER_SUBMIT") {
        if tokens.len() != 4 {
            return Err("render_submit_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::RenderSubmit {
            draw_calls: parse_u32(tokens[1])?,
            frost_passes: parse_u32(tokens[2])?,
            text_calls: parse_u32(tokens[3])?,
        });
    }
    if cmd.eq_ignore_ascii_case("RENDER_GET") {
        if tokens.len() != 1 {
            return Err("render_get_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::RenderGet);
    }
    if cmd.eq_ignore_ascii_case("SHUTDOWN") {
        if tokens.len() != 1 {
            return Err("shutdown_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::Shutdown);
    }

    Err(format!("unsupported_command:{}", tokens[0]))
}

pub fn parse_command_line(line: &str) -> Result<BinaryCtlCommand, String> {
    let mut tokens = line.split_ascii_whitespace();
    let Some(cmd) = tokens.next() else {
        return Err("empty_command".to_string());
    };

    if cmd.eq_ignore_ascii_case("PING") {
        if tokens.next().is_some() {
            return Err("ping_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::Ping);
    }
    if cmd.eq_ignore_ascii_case("VERSION") {
        if tokens.next().is_some() {
            return Err("version_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::Version);
    }
    if cmd.eq_ignore_ascii_case("DISPLAY_GET") {
        if tokens.next().is_some() {
            return Err("display_get_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::DisplayGet);
    }
    if cmd.eq_ignore_ascii_case("DISPLAY_WAIT") {
        let last_seq = tokens
            .next()
            .ok_or_else(|| "display_wait_args_invalid".to_string())
            .and_then(parse_u64)?;
        let timeout_ms = tokens
            .next()
            .ok_or_else(|| "display_wait_args_invalid".to_string())
            .and_then(parse_u32)?;
        if tokens.next().is_some() {
            return Err("display_wait_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::DisplayWait {
            last_seq,
            timeout_ms,
        });
    }
    if cmd.eq_ignore_ascii_case("DISPLAY_SET") {
        let width = tokens
            .next()
            .ok_or_else(|| "display_set_args_invalid".to_string())
            .and_then(parse_u32)?;
        let height = tokens
            .next()
            .ok_or_else(|| "display_set_args_invalid".to_string())
            .and_then(parse_u32)?;
        let refresh_hz = tokens
            .next()
            .ok_or_else(|| "display_set_args_invalid".to_string())
            .and_then(parse_f32)?;
        let density_dpi = tokens
            .next()
            .ok_or_else(|| "display_set_args_invalid".to_string())
            .and_then(parse_u32)?;
        let rotation = tokens
            .next()
            .ok_or_else(|| "display_set_args_invalid".to_string())
            .and_then(parse_u32)?;
        if tokens.next().is_some() {
            return Err("display_set_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::DisplaySet {
            width,
            height,
            refresh_hz,
            density_dpi,
            rotation,
        });
    }
    if cmd.eq_ignore_ascii_case("TOUCH_CLEAR") {
        if tokens.next().is_some() {
            return Err("touch_clear_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::TouchClear);
    }
    if cmd.eq_ignore_ascii_case("TOUCH_COUNT") {
        if tokens.next().is_some() {
            return Err("touch_count_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::TouchCount);
    }
    if cmd.eq_ignore_ascii_case("TOUCH_MOVE") {
        let pointer_id = tokens
            .next()
            .ok_or_else(|| "touch_move_args_invalid".to_string())
            .and_then(parse_i32)?;
        let x = tokens
            .next()
            .ok_or_else(|| "touch_move_args_invalid".to_string())
            .and_then(parse_f32)?;
        let y = tokens
            .next()
            .ok_or_else(|| "touch_move_args_invalid".to_string())
            .and_then(parse_f32)?;
        if tokens.next().is_some() {
            return Err("touch_move_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::TouchMove { pointer_id, x, y });
    }
    if cmd.eq_ignore_ascii_case("RENDER_SUBMIT") {
        let draw_calls = tokens
            .next()
            .ok_or_else(|| "render_submit_args_invalid".to_string())
            .and_then(parse_u32)?;
        let frost_passes = tokens
            .next()
            .ok_or_else(|| "render_submit_args_invalid".to_string())
            .and_then(parse_u32)?;
        let text_calls = tokens
            .next()
            .ok_or_else(|| "render_submit_args_invalid".to_string())
            .and_then(parse_u32)?;
        if tokens.next().is_some() {
            return Err("render_submit_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::RenderSubmit {
            draw_calls,
            frost_passes,
            text_calls,
        });
    }
    if cmd.eq_ignore_ascii_case("RENDER_GET") {
        if tokens.next().is_some() {
            return Err("render_get_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::RenderGet);
    }
    if cmd.eq_ignore_ascii_case("SHUTDOWN") {
        if tokens.next().is_some() {
            return Err("shutdown_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::Shutdown);
    }

    Err(format!("unsupported_command:{}", cmd))
}

fn opcode_of(cmd: &BinaryCtlCommand) -> BinaryOpcode {
    match cmd {
        BinaryCtlCommand::Ping => BinaryOpcode::Ping,
        BinaryCtlCommand::Version => BinaryOpcode::Version,
        BinaryCtlCommand::DisplayGet => BinaryOpcode::DisplayGet,
        BinaryCtlCommand::DisplayWait { .. } => BinaryOpcode::DisplayWait,
        BinaryCtlCommand::DisplaySet { .. } => BinaryOpcode::DisplaySet,
        BinaryCtlCommand::TouchClear => BinaryOpcode::TouchClear,
        BinaryCtlCommand::TouchCount => BinaryOpcode::TouchCount,
        BinaryCtlCommand::TouchMove { .. } => BinaryOpcode::TouchMove,
        BinaryCtlCommand::RenderSubmit { .. } => BinaryOpcode::RenderSubmit,
        BinaryCtlCommand::RenderGet => BinaryOpcode::RenderGet,
        BinaryCtlCommand::Shutdown => BinaryOpcode::Shutdown,
    }
}

fn payload_of(cmd: &BinaryCtlCommand) -> Vec<u8> {
    match cmd {
        BinaryCtlCommand::Ping
        | BinaryCtlCommand::Version
        | BinaryCtlCommand::DisplayGet
        | BinaryCtlCommand::TouchClear
        | BinaryCtlCommand::TouchCount
        | BinaryCtlCommand::RenderGet
        | BinaryCtlCommand::Shutdown => Vec::new(),
        BinaryCtlCommand::DisplayWait {
            last_seq,
            timeout_ms,
        } => {
            let mut out = Vec::with_capacity(12);
            out.extend_from_slice(&last_seq.to_le_bytes());
            out.extend_from_slice(&timeout_ms.to_le_bytes());
            out
        }
        BinaryCtlCommand::DisplaySet {
            width,
            height,
            refresh_hz,
            density_dpi,
            rotation,
        } => {
            let mut out = Vec::with_capacity(20);
            out.extend_from_slice(&width.to_le_bytes());
            out.extend_from_slice(&height.to_le_bytes());
            out.extend_from_slice(&refresh_hz.to_bits().to_le_bytes());
            out.extend_from_slice(&density_dpi.to_le_bytes());
            out.extend_from_slice(&rotation.to_le_bytes());
            out
        }
        BinaryCtlCommand::TouchMove { pointer_id, x, y } => {
            let mut out = Vec::with_capacity(12);
            out.extend_from_slice(&pointer_id.to_le_bytes());
            out.extend_from_slice(&x.to_bits().to_le_bytes());
            out.extend_from_slice(&y.to_bits().to_le_bytes());
            out
        }
        BinaryCtlCommand::RenderSubmit {
            draw_calls,
            frost_passes,
            text_calls,
        } => {
            let mut out = Vec::with_capacity(12);
            out.extend_from_slice(&draw_calls.to_le_bytes());
            out.extend_from_slice(&frost_passes.to_le_bytes());
            out.extend_from_slice(&text_calls.to_le_bytes());
            out
        }
    }
}

pub fn encode_request(seq: u64, cmd: &BinaryCtlCommand) -> Vec<u8> {
    let opcode = opcode_of(cmd) as u16;
    let payload = payload_of(cmd);
    let mut out = Vec::with_capacity(BINARY_COMMAND_HEADER_BYTES + payload.len());
    out.extend_from_slice(&BINARY_PROTOCOL_MAGIC.to_le_bytes());
    out.extend_from_slice(&BINARY_PROTOCOL_VERSION.to_le_bytes());
    out.extend_from_slice(&opcode.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

fn read_exact_or_eof<R: Read>(reader: &mut R, buf: &mut [u8]) -> std::io::Result<bool> {
    let mut offset = 0usize;
    while offset < buf.len() {
        match reader.read(&mut buf[offset..]) {
            Ok(0) => {
                if offset == 0 {
                    return Ok(false);
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "binary_response_truncated",
                ));
            }
            Ok(n) => offset += n,
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

fn read_u16_le(bytes: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([bytes[off], bytes[off + 1]])
}

fn read_u32_le(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
}

fn read_u64_le(bytes: &[u8], off: usize) -> u64 {
    u64::from_le_bytes([
        bytes[off],
        bytes[off + 1],
        bytes[off + 2],
        bytes[off + 3],
        bytes[off + 4],
        bytes[off + 5],
        bytes[off + 6],
        bytes[off + 7],
    ])
}

pub fn read_response<R: Read>(reader: &mut R) -> std::io::Result<Option<BinaryCtlResponse>> {
    let mut header = [0u8; BINARY_COMMAND_HEADER_BYTES];
    let has = read_exact_or_eof(reader, &mut header)?;
    if !has {
        return Ok(None);
    }

    let magic = read_u32_le(&header, 0);
    let version = read_u16_le(&header, 4);
    let opcode = read_u16_le(&header, 6);
    let payload_len = read_u32_le(&header, 8) as usize;
    let seq = read_u64_le(&header, 12);

    if magic != BINARY_PROTOCOL_MAGIC || version != BINARY_PROTOCOL_VERSION {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "binary_response_header_invalid",
        ));
    }
    if payload_len != BINARY_RESPONSE_PAYLOAD_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "binary_response_payload_len_invalid",
        ));
    }

    let mut payload = [0u8; BINARY_RESPONSE_PAYLOAD_BYTES];
    read_exact_or_eof(reader, &mut payload)?;

    let status_i32 = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let status = Status::from_i32(status_i32).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "binary_response_status_invalid",
        )
    })?;

    let mut values = [0u64; BINARY_RESPONSE_VALUE_COUNT];
    let mut cursor = 4usize;
    for slot in values.iter_mut() {
        *slot = read_u64_le(&payload, cursor);
        cursor += 8;
    }

    Ok(Some(BinaryCtlResponse {
        seq,
        opcode,
        status,
        values,
    }))
}

pub fn write_request<W: Write>(
    writer: &mut W,
    seq: u64,
    cmd: &BinaryCtlCommand,
) -> std::io::Result<()> {
    let frame = encode_request(seq, cmd);
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}

pub fn format_response(cmd: &BinaryCtlCommand, resp: &BinaryCtlResponse) -> String {
    if resp.status != Status::Ok {
        return format!("ERR {}", resp.status.as_str());
    }

    match cmd {
        BinaryCtlCommand::Ping => "OK PONG".to_string(),
        BinaryCtlCommand::Version => format!(
            "OK {}.{}.{}",
            resp.values[0], resp.values[1], resp.values[2]
        ),
        BinaryCtlCommand::DisplayGet => {
            let refresh = f32::from_bits(resp.values[2] as u32);
            format!(
                "OK {} {} {:.2} {} {}",
                resp.values[0], resp.values[1], refresh, resp.values[3], resp.values[4]
            )
        }
        BinaryCtlCommand::DisplayWait { .. } => {
            if resp.values[6] == 0 {
                "OK TIMEOUT".to_string()
            } else {
                let refresh = f32::from_bits(resp.values[3] as u32);
                format!(
                    "OK {} {} {} {:.2} {} {}",
                    resp.values[0],
                    resp.values[1],
                    resp.values[2],
                    refresh,
                    resp.values[4],
                    resp.values[5]
                )
            }
        }
        BinaryCtlCommand::DisplaySet { .. } => "OK".to_string(),
        BinaryCtlCommand::TouchClear => "OK".to_string(),
        BinaryCtlCommand::TouchCount => format!("OK {}", resp.values[0]),
        BinaryCtlCommand::TouchMove { .. } => {
            format!(
                "OK {} {}",
                resp.values[0] as i64 as i32, resp.values[1] as i64 as i32
            )
        }
        BinaryCtlCommand::RenderSubmit { .. } | BinaryCtlCommand::RenderGet => {
            format!(
                "OK {} {} {} {}",
                resp.values[0], resp.values[1], resp.values[2], resp.values[3]
            )
        }
        BinaryCtlCommand::Shutdown => "OK SHUTDOWN".to_string(),
    }
}
