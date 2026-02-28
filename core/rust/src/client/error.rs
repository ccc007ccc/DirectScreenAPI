use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum DsapiError {
    IoError(std::io::Error),
    ProtocolError(String),
    ParseError(String),
}

impl Display for DsapiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(err) => write!(f, "io_error: {}", err),
            Self::ProtocolError(msg) => write!(f, "protocol_error: {}", msg),
            Self::ParseError(msg) => write!(f, "parse_error: {}", msg),
        }
    }
}

impl Error for DsapiError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::IoError(err) => Some(err),
            Self::ProtocolError(_) | Self::ParseError(_) => None,
        }
    }
}

impl From<std::io::Error> for DsapiError {
    fn from(value: std::io::Error) -> Self {
        Self::IoError(value)
    }
}

pub type Result<T> = std::result::Result<T, DsapiError>;
