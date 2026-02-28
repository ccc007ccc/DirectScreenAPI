use std::path::PathBuf;

use super::core::{parse_ok_tokens, SocketConnection};
use super::error::{DsapiError, Result};

#[derive(Debug)]
pub struct RenderSession {
    connection: SocketConnection,
    width: u32,
    height: u32,
    last_frame_seq: u64,
}

impl RenderSession {
    pub(crate) fn connect(socket_path: PathBuf, width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(DsapiError::ParseError(
                "render_session_invalid_display_size".to_string(),
            ));
        }
        let connection = SocketConnection::connect(socket_path.as_path())?;
        Ok(Self {
            connection,
            width,
            height,
            last_frame_seq: 0,
        })
    }

    pub fn submit_frame(&mut self, rgba_buffer: &[u8]) -> Result<()> {
        let expected_len = expected_rgba_len(self.width, self.height)?;
        if rgba_buffer.len() != expected_len {
            return Err(DsapiError::ParseError(format!(
                "rgba_len_mismatch expected={} actual={}",
                expected_len,
                rgba_buffer.len()
            )));
        }

        let command = format!(
            "RENDER_FRAME_SUBMIT_RGBA_RAW {} {} {}",
            self.width,
            self.height,
            rgba_buffer.len()
        );
        let ready = self.connection.send_line(&command)?;
        if ready != "OK READY" {
            return Err(DsapiError::ProtocolError(format!(
                "RENDER_FRAME_SUBMIT_RGBA_RAW_bad_ready:{}",
                ready
            )));
        }

        // 严格按协议发送定长 RGBA 原始字节流。
        self.connection.write_all(rgba_buffer)?;
        self.connection.flush()?;

        let response = self.connection.read_line()?;
        let tokens = parse_ok_tokens(&response)?;
        if tokens.len() != 6 {
            return Err(DsapiError::ProtocolError(format!(
                "RENDER_FRAME_SUBMIT_RGBA_RAW_bad_token_count:{} line={}",
                tokens.len(),
                response
            )));
        }
        if tokens[3] != "RGBA8888" {
            return Err(DsapiError::ProtocolError(format!(
                "RENDER_FRAME_SUBMIT_RGBA_RAW_bad_pixel_format:{}",
                response
            )));
        }

        let frame_seq = parse_u64(tokens[0], "frame_seq")?;
        let width = parse_u32(tokens[1], "width")?;
        let height = parse_u32(tokens[2], "height")?;
        let byte_len = parse_u32(tokens[4], "byte_len")?;
        let _checksum = parse_u32(tokens[5], "checksum_fnv1a32")?;

        if width != self.width || height != self.height {
            return Err(DsapiError::ProtocolError(format!(
                "submit_frame_response_size_mismatch line={}",
                response
            )));
        }
        if byte_len as usize != rgba_buffer.len() {
            return Err(DsapiError::ProtocolError(format!(
                "submit_frame_response_len_mismatch line={}",
                response
            )));
        }

        self.last_frame_seq = frame_seq;
        Ok(())
    }

    pub fn frame_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn last_frame_seq(&self) -> u64 {
        self.last_frame_seq
    }
}

fn expected_rgba_len(width: u32, height: u32) -> Result<usize> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4))
        .ok_or_else(|| DsapiError::ParseError("frame_size_overflow".to_string()))
}

fn parse_u32(token: &str, field: &str) -> Result<u32> {
    token
        .parse::<u32>()
        .map_err(|_| DsapiError::ParseError(format!("invalid_{}:{}", field, token)))
}

fn parse_u64(token: &str, field: &str) -> Result<u64> {
    token
        .parse::<u64>()
        .map_err(|_| DsapiError::ParseError(format!("invalid_{}:{}", field, token)))
}

#[cfg(test)]
mod tests {
    use super::expected_rgba_len;

    #[test]
    fn expected_rgba_len_computes_bytes() {
        let len = expected_rgba_len(2, 3).expect("len");
        assert_eq!(len, 24);
    }

    #[test]
    fn expected_rgba_len_rejects_overflow() {
        let err = expected_rgba_len(u32::MAX, u32::MAX).expect_err("must overflow");
        assert_eq!(err.to_string(), "parse_error: frame_size_overflow");
    }
}
