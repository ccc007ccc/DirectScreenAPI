use std::io::{Read, Write};

use crate::api::Status;
use crate::engine::{
    BinaryOpcode, BINARY_COMMAND_HEADER_BYTES, BINARY_PROTOCOL_MAGIC, BINARY_PROTOCOL_VERSION,
    BINARY_RESPONSE_VALUE_COUNT,
};

#[derive(Debug, Clone)]
pub enum BinaryCtlCommand {
    Ping,
    Version,
    ReadyGet,
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
    FilterChainSet {
        passes: Vec<BinaryCtlGaussianPass>,
    },
    FilterClear,
    FilterGet,
    KeyboardInject {
        kind: u32,
        codepoint: u32,
    },
    KeyboardWait {
        last_seq: u64,
        timeout_ms: u32,
    },
    RenderFrameBindShm,
    RenderFrameWaitShmPresent {
        last_seq: u64,
        timeout_ms: u32,
    },
    RenderFrameSubmitShm {
        width: u32,
        height: u32,
        byte_len: u32,
        offset: u32,
        origin_x: i32,
        origin_y: i32,
    },
    ModuleRpc {
        command: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BinaryCtlGaussianPass {
    pub radius: u32,
    pub sigma: f32,
}

#[derive(Debug, Clone)]
pub struct BinaryCtlResponse {
    pub seq: u64,
    pub opcode: u16,
    pub status: Status,
    pub values: [u64; BINARY_RESPONSE_VALUE_COUNT],
    pub body: Vec<u8>,
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

fn parse_keyboard_codepoint(token: &str) -> Result<u32, String> {
    if let Ok(v) = parse_u32(token) {
        return Ok(v);
    }
    let mut chars = token.chars();
    let Some(ch) = chars.next() else {
        return Err("keyboard_codepoint_empty".to_string());
    };
    if chars.next().is_some() {
        return Err(format!("invalid_keyboard_codepoint:{}", token));
    }
    Ok(ch as u32)
}

fn parse_filter_chain_passes(
    tokens: &[&str],
    err: &str,
) -> Result<Vec<BinaryCtlGaussianPass>, String> {
    if tokens.len() < 2 {
        return Err(err.to_string());
    }
    let count = parse_u32(tokens[1])? as usize;
    let expected = 2usize
        .checked_add(count.saturating_mul(2usize))
        .ok_or_else(|| err.to_string())?;
    if tokens.len() != expected {
        return Err(err.to_string());
    }

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let radius = parse_u32(tokens[2 + i * 2])?;
        let sigma = parse_f32(tokens[3 + i * 2])?;
        out.push(BinaryCtlGaussianPass { radius, sigma });
    }
    Ok(out)
}

fn format_filter_info(values: &[u64; BINARY_RESPONSE_VALUE_COUNT]) -> String {
    let backend = match values[0] {
        1 => "vulkan",
        _ => "cpu",
    };
    let gpu_active = values[1];
    let pass_count = values[2];
    let first_radius = values[3];
    let first_sigma = f32::from_bits(values[4] as u32);
    let second_radius = values[5];
    let second_sigma = f32::from_bits(values[6] as u32);
    format!(
        "backend={} gpu_active={} pass_count={} first_gaussian={}:{:.3} second_gaussian={}:{:.3}",
        backend, gpu_active, pass_count, first_radius, first_sigma, second_radius, second_sigma
    )
}

fn format_ready_state(state_code: u64) -> &'static str {
    match state_code {
        0 => "starting",
        1 => "ready",
        2 => "stopping",
        _ => "unknown",
    }
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
    if cmd.eq_ignore_ascii_case("READY") || cmd.eq_ignore_ascii_case("READY_GET") {
        if tokens.len() != 1 {
            return Err("ready_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::ReadyGet);
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
    if cmd.eq_ignore_ascii_case("FILTER_SET_GAUSSIAN") {
        if tokens.len() != 3 {
            return Err("filter_set_gaussian_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::FilterChainSet {
            passes: vec![BinaryCtlGaussianPass {
                radius: parse_u32(tokens[1])?,
                sigma: parse_f32(tokens[2])?,
            }],
        });
    }
    if cmd.eq_ignore_ascii_case("FILTER_CHAIN_SET") {
        return Ok(BinaryCtlCommand::FilterChainSet {
            passes: parse_filter_chain_passes(tokens, "filter_chain_set_args_invalid")?,
        });
    }
    if cmd.eq_ignore_ascii_case("FILTER_CLEAR") || cmd.eq_ignore_ascii_case("FILTER_CHAIN_CLEAR") {
        if tokens.len() != 1 {
            return Err("filter_clear_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::FilterClear);
    }
    if cmd.eq_ignore_ascii_case("FILTER_GET") {
        if tokens.len() != 1 {
            return Err("filter_get_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::FilterGet);
    }
    if cmd.eq_ignore_ascii_case("KEYBOARD_CHAR") {
        if tokens.len() != 2 {
            return Err("keyboard_char_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::KeyboardInject {
            kind: crate::api::KEYBOARD_EVENT_KIND_CHAR,
            codepoint: parse_keyboard_codepoint(tokens[1])?,
        });
    }
    if cmd.eq_ignore_ascii_case("KEYBOARD_BACKSPACE") {
        if tokens.len() != 1 {
            return Err("keyboard_backspace_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::KeyboardInject {
            kind: crate::api::KEYBOARD_EVENT_KIND_BACKSPACE,
            codepoint: 0,
        });
    }
    if cmd.eq_ignore_ascii_case("KEYBOARD_DONE") {
        if tokens.len() != 1 {
            return Err("keyboard_done_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::KeyboardInject {
            kind: crate::api::KEYBOARD_EVENT_KIND_DONE,
            codepoint: 0,
        });
    }
    if cmd.eq_ignore_ascii_case("KEYBOARD_FOCUS_ON") {
        if tokens.len() != 1 {
            return Err("keyboard_focus_on_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::KeyboardInject {
            kind: crate::api::KEYBOARD_EVENT_KIND_FOCUS_ON,
            codepoint: 0,
        });
    }
    if cmd.eq_ignore_ascii_case("KEYBOARD_FOCUS_OFF") {
        if tokens.len() != 1 {
            return Err("keyboard_focus_off_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::KeyboardInject {
            kind: crate::api::KEYBOARD_EVENT_KIND_FOCUS_OFF,
            codepoint: 0,
        });
    }
    if cmd.eq_ignore_ascii_case("KEYBOARD_INJECT") {
        if tokens.len() != 3 {
            return Err("keyboard_inject_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::KeyboardInject {
            kind: parse_u32(tokens[1])?,
            codepoint: parse_u32(tokens[2])?,
        });
    }
    if cmd.eq_ignore_ascii_case("KEYBOARD_WAIT") {
        if tokens.len() != 3 {
            return Err("keyboard_wait_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::KeyboardWait {
            last_seq: parse_u64(tokens[1])?,
            timeout_ms: parse_u32(tokens[2])?,
        });
    }
    if cmd.eq_ignore_ascii_case("RENDER_FRAME_BIND_SHM") {
        if tokens.len() != 1 {
            return Err("render_frame_bind_shm_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::RenderFrameBindShm);
    }
    if cmd.eq_ignore_ascii_case("RENDER_FRAME_WAIT_SHM_PRESENT") {
        if tokens.len() != 3 {
            return Err("render_frame_wait_shm_present_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::RenderFrameWaitShmPresent {
            last_seq: parse_u64(tokens[1])?,
            timeout_ms: parse_u32(tokens[2])?,
        });
    }
    if cmd.eq_ignore_ascii_case("RENDER_FRAME_SUBMIT_SHM") {
        if tokens.len() != 5 && tokens.len() != 7 {
            return Err("render_frame_submit_shm_args_invalid".to_string());
        }
        let width = parse_u32(tokens[1])?;
        let height = parse_u32(tokens[2])?;
        let byte_len = parse_u32(tokens[3])?;
        let offset = parse_u32(tokens[4])?;
        let origin_x = if tokens.len() == 7 {
            parse_i32(tokens[5])?
        } else {
            0
        };
        let origin_y = if tokens.len() == 7 {
            parse_i32(tokens[6])?
        } else {
            0
        };
        return Ok(BinaryCtlCommand::RenderFrameSubmitShm {
            width,
            height,
            byte_len,
            offset,
            origin_x,
            origin_y,
        });
    }
    if cmd.eq_ignore_ascii_case("SHUTDOWN") {
        if tokens.len() != 1 {
            return Err("shutdown_args_invalid".to_string());
        }
        return Ok(BinaryCtlCommand::Shutdown);
    }
    if cmd.starts_with("MODULE_") {
        return Ok(BinaryCtlCommand::ModuleRpc {
            command: tokens.join(" "),
        });
    }

    Err(format!("unsupported_command:{}", tokens[0]))
}

pub fn parse_command_line(line: &str) -> Result<BinaryCtlCommand, String> {
    let tokens: Vec<&str> = line.split_ascii_whitespace().collect();
    parse_command_tokens(&tokens)
}

fn opcode_of(cmd: &BinaryCtlCommand) -> BinaryOpcode {
    match cmd {
        BinaryCtlCommand::Ping => BinaryOpcode::Ping,
        BinaryCtlCommand::Version => BinaryOpcode::Version,
        BinaryCtlCommand::ReadyGet => BinaryOpcode::ReadyGet,
        BinaryCtlCommand::DisplayGet => BinaryOpcode::DisplayGet,
        BinaryCtlCommand::DisplayWait { .. } => BinaryOpcode::DisplayWait,
        BinaryCtlCommand::DisplaySet { .. } => BinaryOpcode::DisplaySet,
        BinaryCtlCommand::TouchClear => BinaryOpcode::TouchClear,
        BinaryCtlCommand::TouchCount => BinaryOpcode::TouchCount,
        BinaryCtlCommand::TouchMove { .. } => BinaryOpcode::TouchMove,
        BinaryCtlCommand::RenderSubmit { .. } => BinaryOpcode::RenderSubmit,
        BinaryCtlCommand::RenderGet => BinaryOpcode::RenderGet,
        BinaryCtlCommand::FilterChainSet { .. } => BinaryOpcode::FilterChainSet,
        BinaryCtlCommand::FilterClear => BinaryOpcode::FilterClear,
        BinaryCtlCommand::FilterGet => BinaryOpcode::FilterGet,
        BinaryCtlCommand::KeyboardInject { .. } => BinaryOpcode::KeyboardInject,
        BinaryCtlCommand::KeyboardWait { .. } => BinaryOpcode::KeyboardWait,
        BinaryCtlCommand::RenderFrameBindShm => BinaryOpcode::RenderFrameBindShm,
        BinaryCtlCommand::RenderFrameWaitShmPresent { .. } => {
            BinaryOpcode::RenderFrameWaitShmPresent
        }
        BinaryCtlCommand::RenderFrameSubmitShm { .. } => BinaryOpcode::RenderFrameSubmitShm,
        BinaryCtlCommand::ModuleRpc { .. } => BinaryOpcode::ModuleRpc,
        BinaryCtlCommand::Shutdown => BinaryOpcode::Shutdown,
    }
}

fn payload_of(cmd: &BinaryCtlCommand) -> Vec<u8> {
    match cmd {
        BinaryCtlCommand::Ping
        | BinaryCtlCommand::Version
        | BinaryCtlCommand::ReadyGet
        | BinaryCtlCommand::DisplayGet
        | BinaryCtlCommand::TouchClear
        | BinaryCtlCommand::TouchCount
        | BinaryCtlCommand::RenderGet
        | BinaryCtlCommand::FilterClear
        | BinaryCtlCommand::FilterGet
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

        BinaryCtlCommand::FilterChainSet { passes } => {
            let mut out = Vec::with_capacity(4 + passes.len() * 12);
            out.extend_from_slice(&(passes.len() as u32).to_le_bytes());
            for pass in passes {
                out.extend_from_slice(&1u32.to_le_bytes());
                out.extend_from_slice(&pass.radius.to_le_bytes());
                out.extend_from_slice(&pass.sigma.to_bits().to_le_bytes());
            }
            out
        }
        BinaryCtlCommand::KeyboardInject { kind, codepoint } => {
            let mut out = Vec::with_capacity(8);
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&codepoint.to_le_bytes());
            out
        }
        BinaryCtlCommand::KeyboardWait {
            last_seq,
            timeout_ms,
        } => {
            let mut out = Vec::with_capacity(12);
            out.extend_from_slice(&last_seq.to_le_bytes());
            out.extend_from_slice(&timeout_ms.to_le_bytes());
            out
        }
        BinaryCtlCommand::RenderFrameBindShm => Vec::new(),
        BinaryCtlCommand::RenderFrameWaitShmPresent {
            last_seq,
            timeout_ms,
        } => {
            let mut out = Vec::with_capacity(12);
            out.extend_from_slice(&last_seq.to_le_bytes());
            out.extend_from_slice(&timeout_ms.to_le_bytes());
            out
        }
        BinaryCtlCommand::RenderFrameSubmitShm {
            width,
            height,
            byte_len,
            offset,
            origin_x,
            origin_y,
        } => {
            let mut out = Vec::with_capacity(24);
            out.extend_from_slice(&width.to_le_bytes());
            out.extend_from_slice(&height.to_le_bytes());
            out.extend_from_slice(&byte_len.to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&origin_x.to_le_bytes());
            out.extend_from_slice(&origin_y.to_le_bytes());
            out
        }
        BinaryCtlCommand::ModuleRpc { command } => command.as_bytes().to_vec(),
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
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
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
    if payload_len < 4 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "binary_response_payload_len_invalid",
        ));
    }

    let mut payload = vec![0u8; payload_len];
    let has_payload = read_exact_or_eof(reader, &mut payload)?;
    if !has_payload {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "binary_response_payload_missing",
        ));
    }

    let status_i32 = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let status = Status::from_i32(status_i32).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "binary_response_status_invalid",
        )
    })?;

    let mut values = [0u64; BINARY_RESPONSE_VALUE_COUNT];
    let max_value_bytes = BINARY_RESPONSE_VALUE_COUNT.saturating_mul(8usize);
    let value_bytes = payload.len().saturating_sub(4).min(max_value_bytes);
    let value_slots = value_bytes / 8;
    let mut cursor = 4usize;
    for idx in 0..value_slots {
        values[idx] = read_u64_le(&payload, cursor);
        cursor += 8;
    }
    let body_offset = 4usize.saturating_add(max_value_bytes);
    let body = if payload.len() > body_offset {
        payload[body_offset..].to_vec()
    } else {
        Vec::new()
    };

    Ok(Some(BinaryCtlResponse {
        seq,
        opcode,
        status,
        values,
        body,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, ErrorKind};

    fn encode_test_response_frame(
        status: Status,
        values: [u64; BINARY_RESPONSE_VALUE_COUNT],
    ) -> Vec<u8> {
        let payload_bytes = 4usize + (BINARY_RESPONSE_VALUE_COUNT * 8usize);
        let mut out = Vec::with_capacity(BINARY_COMMAND_HEADER_BYTES + payload_bytes);
        out.extend_from_slice(&BINARY_PROTOCOL_MAGIC.to_le_bytes());
        out.extend_from_slice(&BINARY_PROTOCOL_VERSION.to_le_bytes());
        out.extend_from_slice(&(BinaryOpcode::Ping as u16).to_le_bytes());
        out.extend_from_slice(&(payload_bytes as u32).to_le_bytes());
        out.extend_from_slice(&42u64.to_le_bytes());
        out.extend_from_slice(&(status as i32).to_le_bytes());
        for value in values {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    #[test]
    fn read_response_returns_none_on_clean_eof() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let got = read_response(&mut cursor).expect("eof should not fail");
        assert!(got.is_none());
    }

    #[test]
    fn read_response_payload_missing_returns_unexpected_eof() {
        let frame = encode_test_response_frame(Status::Ok, [0u64; BINARY_RESPONSE_VALUE_COUNT]);
        let truncated = frame[..BINARY_COMMAND_HEADER_BYTES].to_vec();
        let mut cursor = Cursor::new(truncated);
        let err = read_response(&mut cursor).expect_err("payload eof must fail");
        assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
    }
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
        if !resp.body.is_empty() {
            let body = String::from_utf8_lossy(&resp.body).trim().to_string();
            if !body.is_empty() {
                return format!("ERR {} {}", resp.status.as_str(), body);
            }
        }
        return format!("ERR {}", resp.status.as_str());
    }

    match cmd {
        BinaryCtlCommand::Ping => "OK PONG".to_string(),
        BinaryCtlCommand::Version => format!(
            "OK {}.{}.{}",
            resp.values[0], resp.values[1], resp.values[2]
        ),
        BinaryCtlCommand::ReadyGet => {
            let state_code = resp.values[0];
            let state = format_ready_state(state_code);
            format!(
                "OK state={} state_code={} boot_id={} uptime_ms={} pid={} module_count={} module_event_seq={} module_error={}",
                state,
                state_code,
                resp.values[1],
                resp.values[2],
                resp.values[3],
                resp.values[4],
                resp.values[5],
                resp.values[6]
            )
        }
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
        BinaryCtlCommand::FilterChainSet { .. }
        | BinaryCtlCommand::FilterClear
        | BinaryCtlCommand::FilterGet => format!("OK {}", format_filter_info(&resp.values)),
        BinaryCtlCommand::KeyboardInject { .. } => format!(
            "OK seq={} kind={} codepoint={}",
            resp.values[0], resp.values[1], resp.values[2]
        ),
        BinaryCtlCommand::KeyboardWait { .. } => {
            if resp.values[3] == 0 {
                "OK TIMEOUT".to_string()
            } else {
                format!(
                    "OK seq={} kind={} codepoint={}",
                    resp.values[0], resp.values[1], resp.values[2]
                )
            }
        }
        BinaryCtlCommand::RenderFrameBindShm => {
            format!(
                "OK SHM_BOUND {} {} fd_attached={}",
                resp.values[0], resp.values[1], resp.values[2]
            )
        }
        BinaryCtlCommand::RenderFrameWaitShmPresent { .. } => {
            if resp.values[3] == 0 {
                "OK TIMEOUT".to_string()
            } else {
                format!(
                    "OK {} {} {} RGBA8888 {} {} {} {} {}",
                    resp.values[0],
                    resp.values[1],
                    resp.values[2],
                    resp.values[3],
                    resp.values[4],
                    resp.values[5],
                    resp.values[6] as i64 as i32,
                    resp.values[7] as i64 as i32
                )
            }
        }
        BinaryCtlCommand::RenderFrameSubmitShm { .. } => {
            format!(
                "OK {} {} {} RGBA8888 {} {}",
                resp.values[0], resp.values[1], resp.values[2], resp.values[3], resp.values[4]
            )
        }
        BinaryCtlCommand::ModuleRpc { .. } => {
            let body = String::from_utf8_lossy(&resp.body).to_string();
            if body.trim().is_empty() {
                "OK".to_string()
            } else {
                body.trim_end().to_string()
            }
        }
        BinaryCtlCommand::Shutdown => "OK SHUTDOWN".to_string(),
    }
}
