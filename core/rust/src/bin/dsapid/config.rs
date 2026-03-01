use std::collections::HashSet;
use std::path::Path;

use directscreen_core::util::{default_control_socket_path, derive_data_socket_path_text};

#[derive(Debug, Clone)]
pub(super) struct DaemonConfig {
    pub(super) control_socket_path: String,
    pub(super) data_socket_path: String,
    pub(super) render_output_dir: String,
    pub(super) supervise_presenter_cmd: Option<String>,
    pub(super) supervise_input_cmd: Option<String>,
    pub(super) supervise_restart_ms: u64,
    pub(super) max_connections: usize,
    pub(super) dispatch_workers: usize,
    pub(super) socket_rw_timeout_ms: u64,
    pub(super) command_max_bytes: usize,
    pub(super) allowed_uids: HashSet<u32>,
    pub(super) auth_permissive: bool,
}

fn parse_u64_arg(token: &str) -> Result<u64, String> {
    token
        .parse::<u64>()
        .map_err(|_| format!("invalid_u64_value:{}", token))
}

fn parse_usize_arg(token: &str) -> Result<usize, String> {
    token
        .parse::<usize>()
        .map_err(|_| format!("invalid_usize_value:{}", token))
}

pub(super) fn parse_daemon_config(args: &[String]) -> Result<DaemonConfig, String> {
    let mut control_socket_path = std::env::var("DSAPI_CONTROL_SOCKET_PATH")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            std::env::var("DSAPI_SOCKET_PATH")
                .ok()
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_else(default_control_socket_path);
    let mut data_socket_path = std::env::var("DSAPI_DATA_SOCKET_PATH")
        .ok()
        .filter(|v| !v.is_empty());
    let mut render_output_dir =
        std::env::var("DSAPI_RENDER_OUTPUT_DIR").unwrap_or_else(|_| "artifacts/render".to_string());
    let mut supervise_presenter_cmd = std::env::var("DSAPI_SUPERVISE_PRESENTER_CMD")
        .ok()
        .filter(|v| !v.is_empty());
    let mut supervise_input_cmd = std::env::var("DSAPI_SUPERVISE_INPUT_CMD")
        .ok()
        .filter(|v| !v.is_empty());
    let mut supervise_restart_ms = std::env::var("DSAPI_SUPERVISE_RESTART_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(500);
    let mut max_connections = std::env::var("DSAPI_MAX_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(64usize);
    let mut dispatch_workers = std::env::var("DSAPI_DISPATCH_WORKERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|v| v.get().min(8).max(4))
                .unwrap_or(4)
        });
    let mut socket_rw_timeout_ms = std::env::var("DSAPI_SOCKET_RW_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(30000);
    let mut command_max_bytes = std::env::var("DSAPI_COMMAND_MAX_BYTES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4096usize);
    let auth_permissive = parse_auth_permissive_from_env();

    let mut i = 1usize;
    while i < args.len() {
        let key = args[i].as_str();
        match key {
            "--socket" => {
                if (i + 1) >= args.len() {
                    return Err("missing_socket_path".to_string());
                }
                control_socket_path = args[i + 1].clone();
                i += 2;
            }
            "--control-socket" => {
                if (i + 1) >= args.len() {
                    return Err("missing_control_socket_path".to_string());
                }
                control_socket_path = args[i + 1].clone();
                i += 2;
            }
            "--data-socket" => {
                if (i + 1) >= args.len() {
                    return Err("missing_data_socket_path".to_string());
                }
                data_socket_path = Some(args[i + 1].clone());
                i += 2;
            }
            "--render-output-dir" => {
                if (i + 1) >= args.len() {
                    return Err("missing_render_output_dir".to_string());
                }
                render_output_dir = args[i + 1].clone();
                i += 2;
            }
            "--supervise-presenter" => {
                if (i + 1) >= args.len() {
                    return Err("missing_supervise_presenter_cmd".to_string());
                }
                supervise_presenter_cmd = Some(args[i + 1].clone());
                i += 2;
            }
            "--supervise-input" => {
                if (i + 1) >= args.len() {
                    return Err("missing_supervise_input_cmd".to_string());
                }
                supervise_input_cmd = Some(args[i + 1].clone());
                i += 2;
            }
            "--supervise-restart-ms" => {
                if (i + 1) >= args.len() {
                    return Err("missing_supervise_restart_ms".to_string());
                }
                supervise_restart_ms = parse_u64_arg(&args[i + 1])?;
                i += 2;
            }
            "--max-connections" => {
                if (i + 1) >= args.len() {
                    return Err("missing_max_connections".to_string());
                }
                max_connections = parse_usize_arg(&args[i + 1])?;
                i += 2;
            }
            "--dispatch-workers" => {
                if (i + 1) >= args.len() {
                    return Err("missing_dispatch_workers".to_string());
                }
                dispatch_workers = parse_usize_arg(&args[i + 1])?;
                i += 2;
            }
            "--socket-rw-timeout-ms" => {
                if (i + 1) >= args.len() {
                    return Err("missing_socket_rw_timeout_ms".to_string());
                }
                socket_rw_timeout_ms = parse_u64_arg(&args[i + 1])?;
                i += 2;
            }
            "--command-max-bytes" => {
                if (i + 1) >= args.len() {
                    return Err("missing_command_max_bytes".to_string());
                }
                command_max_bytes = parse_usize_arg(&args[i + 1])?;
                i += 2;
            }
            _ => return Err(format!("unknown_arg:{}", args[i])),
        }
    }

    if supervise_restart_ms < 50 {
        supervise_restart_ms = 50;
    }
    if max_connections == 0 {
        max_connections = 1;
    }
    if dispatch_workers == 0 {
        dispatch_workers = 1;
    }
    if dispatch_workers > max_connections {
        dispatch_workers = max_connections;
    }
    if socket_rw_timeout_ms > 0 && socket_rw_timeout_ms < 100 {
        socket_rw_timeout_ms = 100;
    }
    if command_max_bytes < 128 {
        command_max_bytes = 128;
    }

    let data_socket_path = data_socket_path
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| derive_data_socket_path_text(&control_socket_path));
    if control_socket_path == data_socket_path {
        return Err("control_socket_and_data_socket_must_differ".to_string());
    }

    let allowed_uids = parse_allowed_uids_from_env()?;

    Ok(DaemonConfig {
        control_socket_path,
        data_socket_path,
        render_output_dir,
        supervise_presenter_cmd,
        supervise_input_cmd,
        supervise_restart_ms,
        max_connections,
        dispatch_workers,
        socket_rw_timeout_ms,
        command_max_bytes,
        allowed_uids,
        auth_permissive,
    })
}

fn parse_allowed_uids_from_env() -> Result<HashSet<u32>, String> {
    let default_uid = unsafe { libc::geteuid() as u32 };
    let raw = match std::env::var("DSAPI_ALLOWED_UIDS") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => {
            let mut out = HashSet::new();
            out.insert(default_uid);
            #[cfg(target_os = "android")]
            {
                // Android 常见部署路径里，daemon 与客户端经常跨 uid
                // (Termux 用户 <-> root)。默认放通 root 与 tsu/su 来源 uid，
                // 避免因为脚本/会话切换导致渲染与输入链路被鉴权拦截。
                out.insert(0);
                for key in ["TSU_UID", "SUDO_UID", "KSU_UID", "MAGISKSU_UID"] {
                    if let Some(uid) = parse_uid_from_env(key) {
                        out.insert(uid);
                    }
                }
            }
            return Ok(out);
        }
    };

    let mut out = HashSet::new();
    for token in raw.split(',') {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }
        let uid = t
            .parse::<u32>()
            .map_err(|_| format!("invalid_allowed_uid:{}", t))?;
        out.insert(uid);
    }

    if out.is_empty() {
        return Err("allowed_uids_empty".to_string());
    }
    Ok(out)
}

fn parse_uid_from_env(key: &str) -> Option<u32> {
    let raw = std::env::var(key).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u32>().ok()
}

fn parse_auth_permissive_from_env() -> bool {
    if let Ok(raw) = std::env::var("DSAPI_AUTH_PERMISSIVE") {
        let v = raw.trim();
        if v.eq_ignore_ascii_case("1")
            || v.eq_ignore_ascii_case("true")
            || v.eq_ignore_ascii_case("yes")
            || v.eq_ignore_ascii_case("on")
        {
            return true;
        }
        if v.eq_ignore_ascii_case("0")
            || v.eq_ignore_ascii_case("false")
            || v.eq_ignore_ascii_case("no")
            || v.eq_ignore_ascii_case("off")
        {
            return false;
        }
    }

    #[cfg(target_os = "android")]
    {
        // Android 本地调试阶段默认放开 peer uid 鉴权，避免 root/tsu/termux
        // 多 uid 组合导致控制面或数据面被拒绝，影响联调效率。
        true
    }

    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

pub(super) fn ensure_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        match std::fs::symlink_metadata(parent) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(std::io::Error::other("parent_is_symlink"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir_all(parent)?;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
