use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use directscreen_core::engine::{execute_command, RuntimeEngine};

struct SocketGuard {
    path: PathBuf,
}

impl SocketGuard {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn default_socket_path() -> String {
    "artifacts/run/dsapi.sock".to_string()
}

fn parse_socket_arg(args: &[String]) -> String {
    let mut i = 1usize;
    while i < args.len() {
        if args[i] == "--socket" && (i + 1) < args.len() {
            return args[i + 1].clone();
        }
        i += 1;
    }
    default_socket_path()
}

fn ensure_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let socket_path = parse_socket_arg(&args);
    let socket_path = PathBuf::from(socket_path);

    if let Err(e) = ensure_parent(&socket_path) {
        eprintln!("daemon_error=mkdir_failed err={}", e);
        std::process::exit(2);
    }

    let _ = fs::remove_file(&socket_path);

    let listener = match UnixListener::bind(&socket_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "daemon_error=bind_failed path={} err={}",
                socket_path.display(),
                e
            );
            std::process::exit(3);
        }
    };

    let _guard = SocketGuard::new(socket_path.clone());

    println!("daemon_status=started socket={}", socket_path.display());

    let mut engine = RuntimeEngine::default();
    let mut should_stop = false;

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("daemon_warn=accept_failed err={}", e);
                continue;
            }
        };

        let line = {
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = stream.write_all(b"ERR INVALID_ARGUMENT empty_stream\n");
                    continue;
                }
                Ok(_) => line,
                Err(e) => {
                    let _ = stream
                        .write_all(format!("ERR INTERNAL_ERROR read_failed:{}\n", e).as_bytes());
                    continue;
                }
            }
        };

        let outcome = execute_command(&mut engine, &line);
        let response = format!("{}\n", outcome.response_line);
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();

        if outcome.should_shutdown {
            should_stop = true;
            break;
        }
    }

    if should_stop {
        println!("daemon_status=stopped");
    } else {
        println!("daemon_status=exit");
    }
}
