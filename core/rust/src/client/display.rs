use std::path::PathBuf;

use super::core::DisplayInfo;
use super::error::{DsapiError, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayEvent {
    pub seq: u64,
    pub info: DisplayInfo,
}

#[derive(Debug)]
pub struct DisplayStream {
    _private: (),
}

impl DisplayStream {
    pub(crate) fn connect(_socket_path: PathBuf) -> Result<Self> {
        Err(DsapiError::ProtocolError(
            "display_stream_removed_use_binary_display_get_polling".to_string(),
        ))
    }

    pub fn next_event(&mut self) -> Result<DisplayEvent> {
        Err(DsapiError::ProtocolError(
            "display_stream_removed_use_binary_display_get_polling".to_string(),
        ))
    }
}

impl Iterator for DisplayStream {
    type Item = Result<DisplayEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.next_event())
    }
}
