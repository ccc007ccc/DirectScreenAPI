use std::io::ErrorKind;
use std::path::PathBuf;

use super::core::{DisplayInfo, SocketConnection};
use super::error::{DsapiError, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayEvent {
    pub seq: u64,
    pub info: DisplayInfo,
}

#[derive(Debug)]
pub struct DisplayStream {
    connection: SocketConnection,
}

impl DisplayStream {
    pub(crate) fn connect(socket_path: PathBuf) -> Result<Self> {
        let mut connection = SocketConnection::connect(socket_path.as_path())?;
        let response = connection.send_line("STREAM_DISPLAY_V1")?;
        if response != "OK STREAM_DISPLAY_V1" {
            return Err(DsapiError::ProtocolError(format!(
                "display_stream_handshake_bad:{}",
                response
            )));
        }

        Ok(Self { connection })
    }

    pub fn next_event(&mut self) -> Result<DisplayEvent> {
        let line = self.connection.read_line()?;
        parse_display_event_line(&line)
    }
}

impl Iterator for DisplayStream {
    type Item = Result<DisplayEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_event() {
            Ok(event) => Some(Ok(event)),
            Err(DsapiError::IoError(err)) if err.kind() == ErrorKind::UnexpectedEof => None,
            Err(err) => Some(Err(err)),
        }
    }
}

fn parse_display_event_line(line: &str) -> Result<DisplayEvent> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.is_empty() {
        return Err(DsapiError::ProtocolError("display_event_empty".to_string()));
    }
    if tokens[0] == "ERR" {
        return Err(DsapiError::ProtocolError(line.to_string()));
    }
    if tokens[0] != "EVENT" {
        return Err(DsapiError::ProtocolError(format!(
            "display_event_bad_prefix:{} line={}",
            tokens[0], line
        )));
    }
    if tokens.len() != 7 {
        return Err(DsapiError::ProtocolError(format!(
            "display_event_bad_token_count:{} line={}",
            tokens.len(),
            line
        )));
    }

    let seq = parse_u64_token(tokens[1], "seq")?;
    let width = parse_u32_token(tokens[2], "width")?;
    let height = parse_u32_token(tokens[3], "height")?;
    let refresh_hz = parse_f32_token(tokens[4], "refresh_hz")?;
    let density_dpi = parse_u32_token(tokens[5], "density_dpi")?;
    let rotation = parse_u32_token(tokens[6], "rotation")?;

    if width == 0 || height == 0 {
        return Err(DsapiError::ParseError(
            "display_event_invalid_size".to_string(),
        ));
    }
    if !refresh_hz.is_finite() || refresh_hz <= 0.0 {
        return Err(DsapiError::ParseError(
            "display_event_invalid_refresh_hz".to_string(),
        ));
    }
    if density_dpi == 0 {
        return Err(DsapiError::ParseError(
            "display_event_invalid_density_dpi".to_string(),
        ));
    }
    if rotation > 3 {
        return Err(DsapiError::ParseError(
            "display_event_invalid_rotation".to_string(),
        ));
    }

    Ok(DisplayEvent {
        seq,
        info: DisplayInfo {
            width,
            height,
            refresh_hz,
            density_dpi,
            rotation,
        },
    })
}

fn parse_u32_token(token: &str, field: &str) -> Result<u32> {
    token
        .parse::<u32>()
        .map_err(|_| DsapiError::ParseError(format!("invalid_{}:{}", field, token)))
}

fn parse_u64_token(token: &str, field: &str) -> Result<u64> {
    token
        .parse::<u64>()
        .map_err(|_| DsapiError::ParseError(format!("invalid_{}:{}", field, token)))
}

fn parse_f32_token(token: &str, field: &str) -> Result<f32> {
    token
        .parse::<f32>()
        .map_err(|_| DsapiError::ParseError(format!("invalid_{}:{}", field, token)))
}

#[cfg(test)]
mod tests {
    use super::parse_display_event_line;

    #[test]
    fn parse_display_event_line_parses_valid_event() {
        let event = parse_display_event_line("EVENT 7 1080 2400 120 420 1").expect("parse");
        assert_eq!(event.seq, 7);
        assert_eq!(event.info.width, 1080);
        assert_eq!(event.info.height, 2400);
        assert_eq!(event.info.rotation, 1);
    }

    #[test]
    fn parse_display_event_line_rejects_bad_prefix() {
        let err = parse_display_event_line("OK 7 1080 2400 120 420 1").expect_err("reject");
        assert!(err.to_string().contains("display_event_bad_prefix"));
    }
}
