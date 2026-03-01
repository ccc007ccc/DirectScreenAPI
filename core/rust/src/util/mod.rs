use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_CONTROL_SOCKET_PATH: &str = "artifacts/run/dsapi.sock";

pub fn normalize_rotation(rotation: u32) -> u32 {
    rotation % 4
}

pub fn default_control_socket_path() -> String {
    DEFAULT_CONTROL_SOCKET_PATH.to_string()
}

pub fn derive_data_socket_path_text(control_socket_path: &str) -> String {
    if let Some(prefix) = control_socket_path.strip_suffix(".sock") {
        return format!("{}.data.sock", prefix);
    }
    format!("{}.data", control_socket_path)
}

pub fn derive_data_socket_path(control_socket_path: &Path) -> PathBuf {
    let text = control_socket_path.to_string_lossy();
    PathBuf::from(derive_data_socket_path_text(&text))
}

pub fn timeout_from_env(env_key: &str, default_ms: u64, min_ms: u64) -> Option<Duration> {
    let raw = std::env::var(env_key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default_ms);
    if raw == 0 {
        return None;
    }
    Some(Duration::from_millis(raw.max(min_ms)))
}

pub mod ctl_wire;
