use std::convert::TryInto;
use std::io::ErrorKind;
use std::path::PathBuf;

use super::core::SocketConnection;
use super::error::{DsapiError, Result};

const TOUCH_PACKET_BYTES: usize = 16;
const TOUCH_PACKET_DOWN: u8 = 1;
const TOUCH_PACKET_MOVE: u8 = 2;
const TOUCH_PACKET_UP: u8 = 3;
const TOUCH_PACKET_CLEAR: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchEvent {
    Down { id: u32, x: u32, y: u32 },
    Move { id: u32, x: u32, y: u32 },
    Up { id: u32 },
}

#[derive(Debug)]
pub struct TouchStream {
    route_name: String,
    connection: SocketConnection,
}

impl TouchStream {
    pub(crate) fn connect(socket_path: PathBuf, route_name: &str) -> Result<Self> {
        if route_name.trim().is_empty() {
            return Err(DsapiError::ParseError("route_name_empty".to_string()));
        }

        let mut connection = SocketConnection::connect(socket_path.as_path())?;
        let response = connection.send_line("STREAM_TOUCH_V1")?;
        if response != "OK STREAM_TOUCH_V1" {
            return Err(DsapiError::ProtocolError(format!(
                "touch_stream_handshake_bad:{}",
                response
            )));
        }

        Ok(Self {
            route_name: route_name.to_string(),
            connection,
        })
    }

    pub fn route_name(&self) -> &str {
        &self.route_name
    }

    pub fn next_event(&mut self) -> Result<TouchEvent> {
        // STREAM_TOUCH_V1 为固定 16 字节包，连续读取并解码。
        let mut packet = [0u8; TOUCH_PACKET_BYTES];
        self.connection.read_exact(&mut packet)?;
        decode_touch_packet(&packet)
    }

    pub fn send_event(&mut self, event: TouchEvent) -> Result<()> {
        let packet = encode_touch_packet(event)?;
        self.connection.write_all(&packet)?;
        self.connection.flush()?;
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        let mut packet = [0u8; TOUCH_PACKET_BYTES];
        packet[0] = TOUCH_PACKET_CLEAR;
        self.connection.write_all(&packet)?;
        self.connection.flush()?;
        Ok(())
    }
}

impl Iterator for TouchStream {
    type Item = Result<TouchEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_event() {
            Ok(event) => Some(Ok(event)),
            Err(DsapiError::IoError(err)) if err.kind() == ErrorKind::UnexpectedEof => None,
            Err(err) => Some(Err(err)),
        }
    }
}

fn decode_touch_packet(packet: &[u8; TOUCH_PACKET_BYTES]) -> Result<TouchEvent> {
    let kind = packet[0];
    let pointer_id = parse_pointer_id(&packet[4..8])?;
    let x = parse_coord(&packet[8..12], "x")?;
    let y = parse_coord(&packet[12..16], "y")?;

    match kind {
        TOUCH_PACKET_DOWN => Ok(TouchEvent::Down {
            id: pointer_id,
            x,
            y,
        }),
        TOUCH_PACKET_MOVE => Ok(TouchEvent::Move {
            id: pointer_id,
            x,
            y,
        }),
        TOUCH_PACKET_UP => Ok(TouchEvent::Up { id: pointer_id }),
        _ => Err(DsapiError::ParseError(format!(
            "unknown_touch_packet_kind:{}",
            kind
        ))),
    }
}

fn encode_touch_packet(event: TouchEvent) -> Result<[u8; TOUCH_PACKET_BYTES]> {
    let mut out = [0u8; TOUCH_PACKET_BYTES];
    match event {
        TouchEvent::Down { id, x, y } => {
            out[0] = TOUCH_PACKET_DOWN;
            write_pointer_id(&mut out[4..8], id)?;
            write_coord(&mut out[8..12], x);
            write_coord(&mut out[12..16], y);
        }
        TouchEvent::Move { id, x, y } => {
            out[0] = TOUCH_PACKET_MOVE;
            write_pointer_id(&mut out[4..8], id)?;
            write_coord(&mut out[8..12], x);
            write_coord(&mut out[12..16], y);
        }
        TouchEvent::Up { id } => {
            out[0] = TOUCH_PACKET_UP;
            write_pointer_id(&mut out[4..8], id)?;
        }
    }
    Ok(out)
}

fn parse_pointer_id(bytes: &[u8]) -> Result<u32> {
    let raw = i32::from_le_bytes(
        bytes
            .try_into()
            .map_err(|_| DsapiError::ParseError("pointer_id_len_mismatch".to_string()))?,
    );
    u32::try_from(raw).map_err(|_| DsapiError::ParseError(format!("negative_pointer_id:{}", raw)))
}

fn parse_coord(bytes: &[u8], field: &str) -> Result<u32> {
    let raw = f32::from_le_bytes(
        bytes
            .try_into()
            .map_err(|_| DsapiError::ParseError(format!("{}_len_mismatch", field)))?,
    );
    if !raw.is_finite() || raw < 0.0 || raw > u32::MAX as f32 {
        return Err(DsapiError::ParseError(format!(
            "invalid_touch_{}:{}",
            field, raw
        )));
    }
    Ok(raw.round() as u32)
}

fn write_pointer_id(dst: &mut [u8], id: u32) -> Result<()> {
    let raw = i32::try_from(id)
        .map_err(|_| DsapiError::ParseError(format!("pointer_id_out_of_range:{}", id)))?;
    dst.copy_from_slice(&raw.to_le_bytes());
    Ok(())
}

fn write_coord(dst: &mut [u8], value: u32) {
    dst.copy_from_slice(&(value as f32).to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::{decode_touch_packet, encode_touch_packet, TouchEvent, TOUCH_PACKET_BYTES};

    #[test]
    fn touch_packet_roundtrip_down() {
        let original = TouchEvent::Down {
            id: 7,
            x: 120,
            y: 340,
        };
        let raw = encode_touch_packet(original).expect("encode");
        let decoded = decode_touch_packet(&raw).expect("decode");
        assert_eq!(decoded, original);
    }

    #[test]
    fn touch_packet_roundtrip_up() {
        let original = TouchEvent::Up { id: 99 };
        let raw = encode_touch_packet(original).expect("encode");
        let decoded = decode_touch_packet(&raw).expect("decode");
        assert_eq!(decoded, original);
    }

    #[test]
    fn touch_packet_rejects_unknown_kind() {
        let mut raw = [0u8; TOUCH_PACKET_BYTES];
        raw[0] = 42;
        let err = decode_touch_packet(&raw).expect_err("must fail");
        assert!(err.to_string().contains("unknown_touch_packet_kind"));
    }
}
