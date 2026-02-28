use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use super::error::{DsapiError, Result};
use super::render::RenderSession;
use super::touch::TouchStream;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayInfo {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f32,
    pub density_dpi: u32,
    pub rotation: u32,
}

#[derive(Debug)]
pub(crate) struct SocketConnection {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

impl SocketConnection {
    pub(crate) fn connect(socket_path: &Path) -> Result<Self> {
        let writer = UnixStream::connect(socket_path)?;
        let reader = BufReader::new(writer.try_clone()?);
        Ok(Self { writer, reader })
    }

    pub(crate) fn send_line(&mut self, line: &str) -> Result<String> {
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        self.read_line()
    }

    pub(crate) fn read_line(&mut self) -> Result<String> {
        let mut response = String::new();
        let n = self.reader.read_line(&mut response)?;
        if n == 0 {
            return Err(DsapiError::ProtocolError("socket_closed".to_string()));
        }
        Ok(response.trim_end().to_string())
    }

    pub(crate) fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        Ok(())
    }

    pub(crate) fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }

    pub(crate) fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        self.reader.read_exact(buf)?;
        Ok(())
    }
}

pub(crate) fn parse_ok_tokens(line: &str) -> Result<Vec<&str>> {
    let mut tokens = line.split_whitespace();
    match tokens.next() {
        Some("OK") => Ok(tokens.collect()),
        Some("ERR") => Err(DsapiError::ProtocolError(line.to_string())),
        Some(other) => Err(DsapiError::ProtocolError(format!(
            "unexpected_response_prefix:{} line={}",
            other, line
        ))),
        None => Err(DsapiError::ProtocolError("empty_response".to_string())),
    }
}

fn parse_u32_token(token: &str, field: &str) -> Result<u32> {
    token
        .parse::<u32>()
        .map_err(|_| DsapiError::ParseError(format!("invalid_{}:{}", field, token)))
}

fn parse_f32_token(token: &str, field: &str) -> Result<f32> {
    token
        .parse::<f32>()
        .map_err(|_| DsapiError::ParseError(format!("invalid_{}:{}", field, token)))
}

#[derive(Debug)]
pub struct DsapiClient {
    socket_path: PathBuf,
    control: SocketConnection,
}

impl DsapiClient {
    pub fn connect<P: AsRef<Path>>(socket_path: P) -> Result<Self> {
        let socket_path = socket_path.as_ref().to_path_buf();
        let control = SocketConnection::connect(&socket_path)?;
        Ok(Self {
            socket_path,
            control,
        })
    }

    pub fn get_display_info(&mut self) -> Result<DisplayInfo> {
        let response = self.control.send_line("DISPLAY_GET")?;
        let tokens = parse_ok_tokens(&response)?;
        if tokens.len() != 5 {
            return Err(DsapiError::ProtocolError(format!(
                "DISPLAY_GET_bad_token_count:{} line={}",
                tokens.len(),
                response
            )));
        }

        Ok(DisplayInfo {
            width: parse_u32_token(tokens[0], "width")?,
            height: parse_u32_token(tokens[1], "height")?,
            refresh_hz: parse_f32_token(tokens[2], "refresh_hz")?,
            density_dpi: parse_u32_token(tokens[3], "density_dpi")?,
            rotation: parse_u32_token(tokens[4], "rotation")?,
        })
    }

    pub fn set_display(&mut self, width: u32, height: u32, refresh_hz: f32) -> Result<()> {
        if width == 0 || height == 0 || !refresh_hz.is_finite() || refresh_hz <= 0.0 {
            return Err(DsapiError::ParseError(
                "set_display_invalid_arguments".to_string(),
            ));
        }

        let current = self.get_display_info()?;
        self.set_display_full(
            width,
            height,
            refresh_hz,
            current.density_dpi,
            current.rotation,
        )
    }

    pub fn create_render_session(&mut self) -> Result<RenderSession> {
        // 渲染会话默认绑定当前显示分辨率，提交帧时无需重复传宽高。
        let display = self.get_display_info()?;
        RenderSession::connect(self.socket_path.clone(), display.width, display.height)
    }

    pub fn subscribe_touch(&self, route_name: &str) -> Result<TouchStream> {
        // route_name 作为上层业务标识保留，底层握手走 STREAM_TOUCH_V1。
        TouchStream::connect(self.socket_path.clone(), route_name)
    }

    pub fn render_present(&mut self) -> Result<()> {
        let response = self.control.send_line("RENDER_PRESENT")?;
        let tokens = parse_ok_tokens(&response)?;
        if tokens.is_empty() {
            return Err(DsapiError::ProtocolError(format!(
                "RENDER_PRESENT_unexpected_payload:{}",
                response
            )));
        }
        Ok(())
    }

    fn set_display_full(
        &mut self,
        width: u32,
        height: u32,
        refresh_hz: f32,
        density_dpi: u32,
        rotation: u32,
    ) -> Result<()> {
        let command = format!(
            "DISPLAY_SET {} {} {} {} {}",
            width, height, refresh_hz, density_dpi, rotation
        );
        let response = self.control.send_line(&command)?;
        let tokens = parse_ok_tokens(&response)?;
        if !tokens.is_empty() {
            return Err(DsapiError::ProtocolError(format!(
                "DISPLAY_SET_unexpected_payload:{}",
                response
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_ok_tokens;

    #[test]
    fn parse_ok_tokens_parses_payload() {
        let tokens = parse_ok_tokens("OK 1 2 3").expect("parse ok tokens");
        assert_eq!(tokens, vec!["1", "2", "3"]);
    }

    #[test]
    fn parse_ok_tokens_rejects_error_line() {
        let err = parse_ok_tokens("ERR INVALID_ARGUMENT").expect_err("should fail");
        assert_eq!(err.to_string(), "protocol_error: ERR INVALID_ARGUMENT");
    }
}
