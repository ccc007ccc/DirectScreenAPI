use crate::api::{DisplayState, RenderStats, RouteResult, Status, TouchEvent};
use crate::engine::RuntimeEngine;
use crate::DIRECTSCREEN_CORE_VERSION;

pub const BINARY_PROTOCOL_MAGIC: u32 = 0x5041_5344; // "DSAP" little-endian
pub const BINARY_PROTOCOL_VERSION: u16 = 1;
pub const BINARY_COMMAND_HEADER_BYTES: usize = 20;
pub const BINARY_RESPONSE_VALUE_COUNT: usize = 8;
pub const BINARY_RESPONSE_PAYLOAD_BYTES: usize = 4 + (BINARY_RESPONSE_VALUE_COUNT * 8);

#[derive(Debug, Clone, Copy)]
pub struct BinaryCommandHeader {
    pub magic: u32,
    pub version: u16,
    pub opcode: u16,
    pub payload_len: u32,
    pub seq: u64,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOpcode {
    Ping = 1,
    Version = 2,
    Shutdown = 3,
    DisplayGet = 4,
    DisplaySet = 5,
    TouchClear = 6,
    TouchCount = 7,
    TouchMove = 8,
    RenderSubmit = 9,
    RenderGet = 10,
}

impl BinaryOpcode {
    pub fn from_u16(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Ping),
            2 => Some(Self::Version),
            3 => Some(Self::Shutdown),
            4 => Some(Self::DisplayGet),
            5 => Some(Self::DisplaySet),
            6 => Some(Self::TouchClear),
            7 => Some(Self::TouchCount),
            8 => Some(Self::TouchMove),
            9 => Some(Self::RenderSubmit),
            10 => Some(Self::RenderGet),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BinaryTouchPayload {
    pub pointer_id: i32,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct BinaryDisplaySetPayload {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f32,
    pub density_dpi: u32,
    pub rotation: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct BinaryRenderSubmitPayload {
    pub draw_calls: u32,
    pub frost_passes: u32,
    pub text_calls: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum BinaryCommand {
    Ping {
        seq: u64,
    },
    Version {
        seq: u64,
    },
    Shutdown {
        seq: u64,
    },
    DisplayGet {
        seq: u64,
    },
    DisplaySet {
        seq: u64,
        payload: BinaryDisplaySetPayload,
    },
    TouchClear {
        seq: u64,
    },
    TouchCount {
        seq: u64,
    },
    TouchMove {
        seq: u64,
        payload: BinaryTouchPayload,
    },
    RenderSubmit {
        seq: u64,
        payload: BinaryRenderSubmitPayload,
    },
    RenderGet {
        seq: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryResponse {
    pub seq: u64,
    pub opcode: u16,
    pub status: Status,
    pub values: [u64; BINARY_RESPONSE_VALUE_COUNT],
}

impl BinaryResponse {
    pub fn with_status(seq: u64, opcode: u16, status: Status) -> Self {
        Self {
            seq,
            opcode,
            status,
            values: [0u64; BINARY_RESPONSE_VALUE_COUNT],
        }
    }
}

fn read_u16_le(bytes: &[u8], off: usize) -> Result<u16, Status> {
    let end = off.checked_add(2).ok_or(Status::OutOfRange)?;
    let part = bytes.get(off..end).ok_or(Status::InvalidArgument)?;
    Ok(u16::from_le_bytes([part[0], part[1]]))
}

fn read_u32_le(bytes: &[u8], off: usize) -> Result<u32, Status> {
    let end = off.checked_add(4).ok_or(Status::OutOfRange)?;
    let part = bytes.get(off..end).ok_or(Status::InvalidArgument)?;
    Ok(u32::from_le_bytes([part[0], part[1], part[2], part[3]]))
}

fn read_u64_le(bytes: &[u8], off: usize) -> Result<u64, Status> {
    let end = off.checked_add(8).ok_or(Status::OutOfRange)?;
    let part = bytes.get(off..end).ok_or(Status::InvalidArgument)?;
    Ok(u64::from_le_bytes([
        part[0], part[1], part[2], part[3], part[4], part[5], part[6], part[7],
    ]))
}

fn read_i32_le(bytes: &[u8], off: usize) -> Result<i32, Status> {
    Ok(read_u32_le(bytes, off)? as i32)
}

fn read_f32_le(bytes: &[u8], off: usize) -> Result<f32, Status> {
    Ok(f32::from_bits(read_u32_le(bytes, off)?))
}

pub fn parse_binary_header(frame: &[u8]) -> Result<BinaryCommandHeader, Status> {
    if frame.len() < BINARY_COMMAND_HEADER_BYTES {
        return Err(Status::InvalidArgument);
    }

    Ok(BinaryCommandHeader {
        magic: read_u32_le(frame, 0)?,
        version: read_u16_le(frame, 4)?,
        opcode: read_u16_le(frame, 6)?,
        payload_len: read_u32_le(frame, 8)?,
        seq: read_u64_le(frame, 12)?,
    })
}

pub fn parse_binary_command(frame: &[u8]) -> Result<BinaryCommand, Status> {
    let header = parse_binary_header(frame)?;

    if header.magic != BINARY_PROTOCOL_MAGIC {
        return Err(Status::InvalidArgument);
    }
    if header.version != BINARY_PROTOCOL_VERSION {
        return Err(Status::InvalidArgument);
    }

    let payload_len = header.payload_len as usize;
    let total_len = BINARY_COMMAND_HEADER_BYTES
        .checked_add(payload_len)
        .ok_or(Status::OutOfRange)?;
    if frame.len() != total_len {
        return Err(Status::InvalidArgument);
    }

    let payload = &frame[BINARY_COMMAND_HEADER_BYTES..];
    let opcode = BinaryOpcode::from_u16(header.opcode).ok_or(Status::InvalidArgument)?;

    match opcode {
        BinaryOpcode::Ping => {
            if !payload.is_empty() {
                return Err(Status::InvalidArgument);
            }
            Ok(BinaryCommand::Ping { seq: header.seq })
        }
        BinaryOpcode::Version => {
            if !payload.is_empty() {
                return Err(Status::InvalidArgument);
            }
            Ok(BinaryCommand::Version { seq: header.seq })
        }
        BinaryOpcode::Shutdown => {
            if !payload.is_empty() {
                return Err(Status::InvalidArgument);
            }
            Ok(BinaryCommand::Shutdown { seq: header.seq })
        }
        BinaryOpcode::DisplayGet => {
            if !payload.is_empty() {
                return Err(Status::InvalidArgument);
            }
            Ok(BinaryCommand::DisplayGet { seq: header.seq })
        }
        BinaryOpcode::DisplaySet => {
            if payload.len() != 20 {
                return Err(Status::InvalidArgument);
            }
            Ok(BinaryCommand::DisplaySet {
                seq: header.seq,
                payload: BinaryDisplaySetPayload {
                    width: read_u32_le(payload, 0)?,
                    height: read_u32_le(payload, 4)?,
                    refresh_hz: read_f32_le(payload, 8)?,
                    density_dpi: read_u32_le(payload, 12)?,
                    rotation: read_u32_le(payload, 16)?,
                },
            })
        }
        BinaryOpcode::TouchClear => {
            if !payload.is_empty() {
                return Err(Status::InvalidArgument);
            }
            Ok(BinaryCommand::TouchClear { seq: header.seq })
        }
        BinaryOpcode::TouchCount => {
            if !payload.is_empty() {
                return Err(Status::InvalidArgument);
            }
            Ok(BinaryCommand::TouchCount { seq: header.seq })
        }
        BinaryOpcode::TouchMove => {
            if payload.len() != 12 {
                return Err(Status::InvalidArgument);
            }
            Ok(BinaryCommand::TouchMove {
                seq: header.seq,
                payload: BinaryTouchPayload {
                    pointer_id: read_i32_le(payload, 0)?,
                    x: read_f32_le(payload, 4)?,
                    y: read_f32_le(payload, 8)?,
                },
            })
        }
        BinaryOpcode::RenderSubmit => {
            if payload.len() != 12 {
                return Err(Status::InvalidArgument);
            }
            Ok(BinaryCommand::RenderSubmit {
                seq: header.seq,
                payload: BinaryRenderSubmitPayload {
                    draw_calls: read_u32_le(payload, 0)?,
                    frost_passes: read_u32_le(payload, 4)?,
                    text_calls: read_u32_le(payload, 8)?,
                },
            })
        }
        BinaryOpcode::RenderGet => {
            if !payload.is_empty() {
                return Err(Status::InvalidArgument);
            }
            Ok(BinaryCommand::RenderGet { seq: header.seq })
        }
    }
}

fn parse_version_token_prefix(token: &str) -> u64 {
    let mut value = 0u64;
    for ch in token.bytes() {
        if !ch.is_ascii_digit() {
            break;
        }
        value = value.saturating_mul(10).saturating_add((ch - b'0') as u64);
    }
    value
}

fn parse_version_triplet() -> [u64; 3] {
    let mut out = [0u64; 3];
    let mut parts = DIRECTSCREEN_CORE_VERSION.split('.');
    for slot in &mut out {
        let Some(token) = parts.next() else {
            break;
        };
        *slot = parse_version_token_prefix(token);
    }
    out
}

pub fn execute_binary_command(engine: &RuntimeEngine, frame: &[u8]) -> BinaryResponse {
    let parsed = match parse_binary_command(frame) {
        Ok(v) => v,
        Err(status) => {
            return BinaryResponse::with_status(0, 0, status);
        }
    };

    match parsed {
        BinaryCommand::Ping { seq } => {
            let mut resp = BinaryResponse::with_status(seq, BinaryOpcode::Ping as u16, Status::Ok);
            resp.values[0] = 1;
            resp
        }
        BinaryCommand::Version { seq } => {
            let mut resp =
                BinaryResponse::with_status(seq, BinaryOpcode::Version as u16, Status::Ok);
            let v = parse_version_triplet();
            resp.values[0] = v[0];
            resp.values[1] = v[1];
            resp.values[2] = v[2];
            resp
        }
        BinaryCommand::Shutdown { seq } => {
            let mut resp =
                BinaryResponse::with_status(seq, BinaryOpcode::Shutdown as u16, Status::Ok);
            resp.values[0] = 1;
            resp
        }
        BinaryCommand::DisplayGet { seq } => {
            let d = engine.display_state();
            let mut resp =
                BinaryResponse::with_status(seq, BinaryOpcode::DisplayGet as u16, Status::Ok);
            resp.values[0] = d.width as u64;
            resp.values[1] = d.height as u64;
            resp.values[2] = d.refresh_hz.to_bits() as u64;
            resp.values[3] = d.density_dpi as u64;
            resp.values[4] = d.rotation as u64;
            resp
        }
        BinaryCommand::DisplaySet { seq, payload } => {
            let mut resp =
                BinaryResponse::with_status(seq, BinaryOpcode::DisplaySet as u16, Status::Ok);
            if let Err(status) = engine.set_display_state(DisplayState {
                width: payload.width,
                height: payload.height,
                refresh_hz: payload.refresh_hz,
                density_dpi: payload.density_dpi,
                rotation: payload.rotation,
            }) {
                resp.status = status;
            }
            resp
        }
        BinaryCommand::TouchClear { seq } => {
            engine.clear_touches();
            BinaryResponse::with_status(seq, BinaryOpcode::TouchClear as u16, Status::Ok)
        }
        BinaryCommand::TouchCount { seq } => {
            let mut resp =
                BinaryResponse::with_status(seq, BinaryOpcode::TouchCount as u16, Status::Ok);
            resp.values[0] = engine.active_touch_count() as u64;
            resp
        }
        BinaryCommand::TouchMove { seq, payload } => {
            let mut resp =
                BinaryResponse::with_status(seq, BinaryOpcode::TouchMove as u16, Status::Ok);
            let event = TouchEvent {
                pointer_id: payload.pointer_id,
                x: payload.x,
                y: payload.y,
            };
            match engine.touch_move(event) {
                Ok(RouteResult {
                    decision,
                    region_id,
                }) => {
                    resp.values[0] = decision as i32 as i64 as u64;
                    resp.values[1] = region_id as i64 as u64;
                }
                Err(status) => resp.status = status,
            }
            resp
        }
        BinaryCommand::RenderSubmit { seq, payload } => {
            let stats: RenderStats = engine.submit_render_stats(
                payload.draw_calls,
                payload.frost_passes,
                payload.text_calls,
            );
            let mut resp =
                BinaryResponse::with_status(seq, BinaryOpcode::RenderSubmit as u16, Status::Ok);
            resp.values[0] = stats.frame_seq;
            resp.values[1] = stats.draw_calls as u64;
            resp.values[2] = stats.frost_passes as u64;
            resp.values[3] = stats.text_calls as u64;
            resp
        }
        BinaryCommand::RenderGet { seq } => {
            let stats = engine.render_stats();
            let mut resp =
                BinaryResponse::with_status(seq, BinaryOpcode::RenderGet as u16, Status::Ok);
            resp.values[0] = stats.frame_seq;
            resp.values[1] = stats.draw_calls as u64;
            resp.values[2] = stats.frost_passes as u64;
            resp.values[3] = stats.text_calls as u64;
            resp
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Decision, RectRegion};

    fn build_frame(seq: u64, opcode: BinaryOpcode, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(BINARY_COMMAND_HEADER_BYTES + payload.len());
        out.extend_from_slice(&BINARY_PROTOCOL_MAGIC.to_le_bytes());
        out.extend_from_slice(&BINARY_PROTOCOL_VERSION.to_le_bytes());
        out.extend_from_slice(&(opcode as u16).to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&seq.to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn parse_binary_command_rejects_bad_magic() {
        let mut frame = build_frame(1, BinaryOpcode::Ping, &[]);
        frame[0] = 0;
        let err = parse_binary_command(&frame).expect_err("must reject");
        assert_eq!(err, Status::InvalidArgument);
    }

    #[test]
    fn ping_and_version_binary_commands() {
        let engine = RuntimeEngine::default();

        let ping_resp = execute_binary_command(&engine, &build_frame(7, BinaryOpcode::Ping, &[]));
        assert_eq!(ping_resp.status, Status::Ok);
        assert_eq!(ping_resp.seq, 7);
        assert_eq!(ping_resp.opcode, BinaryOpcode::Ping as u16);
        assert_eq!(ping_resp.values[0], 1);

        let version_resp =
            execute_binary_command(&engine, &build_frame(8, BinaryOpcode::Version, &[]));
        assert_eq!(version_resp.status, Status::Ok);
        assert_eq!(version_resp.seq, 8);
        assert_eq!(version_resp.opcode, BinaryOpcode::Version as u16);
    }

    #[test]
    fn display_set_then_get_binary_commands() {
        let engine = RuntimeEngine::default();

        let mut payload = Vec::with_capacity(20);
        payload.extend_from_slice(&1920u32.to_le_bytes());
        payload.extend_from_slice(&1080u32.to_le_bytes());
        payload.extend_from_slice(&120.0f32.to_bits().to_le_bytes());
        payload.extend_from_slice(&480u32.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());

        let set_resp = execute_binary_command(
            &engine,
            &build_frame(10, BinaryOpcode::DisplaySet, &payload),
        );
        assert_eq!(set_resp.status, Status::Ok);

        let get_resp =
            execute_binary_command(&engine, &build_frame(11, BinaryOpcode::DisplayGet, &[]));
        assert_eq!(get_resp.status, Status::Ok);
        assert_eq!(get_resp.values[0], 1920);
        assert_eq!(get_resp.values[1], 1080);
        assert_eq!(get_resp.values[2], 120.0f32.to_bits() as u64);
        assert_eq!(get_resp.values[3], 480);
        assert_eq!(get_resp.values[4], 1);
    }

    #[test]
    fn touch_move_binary_command_returns_route() {
        let engine = RuntimeEngine::default();
        engine
            .add_region_rect(RectRegion {
                region_id: 99,
                decision: Decision::Block,
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            })
            .expect("add region");
        engine
            .touch_down(TouchEvent {
                pointer_id: 1,
                x: 10.0,
                y: 20.0,
            })
            .expect("touch down");

        let mut payload = Vec::with_capacity(12);
        payload.extend_from_slice(&1i32.to_le_bytes());
        payload.extend_from_slice(&10.0f32.to_bits().to_le_bytes());
        payload.extend_from_slice(&20.0f32.to_bits().to_le_bytes());

        let resp =
            execute_binary_command(&engine, &build_frame(12, BinaryOpcode::TouchMove, &payload));
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(resp.values[0] as i64 as i32, Decision::Block as i32);
        assert_eq!(resp.values[1] as i64 as i32, 99);
    }

    #[test]
    fn render_submit_and_get_binary_commands() {
        let engine = RuntimeEngine::default();
        let payload = [8u32.to_le_bytes(), 2u32.to_le_bytes(), 3u32.to_le_bytes()].concat();

        let submit_resp = execute_binary_command(
            &engine,
            &build_frame(20, BinaryOpcode::RenderSubmit, &payload),
        );
        assert_eq!(submit_resp.status, Status::Ok);
        assert_eq!(submit_resp.values[0], 1);
        assert_eq!(submit_resp.values[1], 8);
        assert_eq!(submit_resp.values[2], 2);
        assert_eq!(submit_resp.values[3], 3);

        let get_resp =
            execute_binary_command(&engine, &build_frame(21, BinaryOpcode::RenderGet, &[]));
        assert_eq!(get_resp.status, Status::Ok);
        assert_eq!(get_resp.values[0], 1);
        assert_eq!(get_resp.values[1], 8);
        assert_eq!(get_resp.values[2], 2);
        assert_eq!(get_resp.values[3], 3);
    }
}
