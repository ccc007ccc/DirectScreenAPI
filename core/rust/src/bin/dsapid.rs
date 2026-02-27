use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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

fn parse_render_output_dir_arg(args: &[String]) -> String {
    let mut i = 1usize;
    while i < args.len() {
        if args[i] == "--render-output-dir" && (i + 1) < args.len() {
            return args[i + 1].clone();
        }
        i += 1;
    }
    std::env::var("DSAPI_RENDER_OUTPUT_DIR").unwrap_or_else(|_| "artifacts/render".to_string())
}

fn ensure_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn handle_client(
    mut stream: UnixStream,
    engine: Arc<Mutex<RuntimeEngine>>,
    shutdown: Arc<AtomicBool>,
) {
    let reader_stream = match stream.try_clone() {
        Ok(v) => v,
        Err(e) => {
            let _ = stream.write_all(format!("ERR INTERNAL_ERROR clone_failed:{}\n", e).as_bytes());
            return;
        }
    };

    let mut reader = BufReader::new(reader_stream);
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let outcome = {
                    match engine.lock() {
                        Ok(mut guard) => execute_command(&mut guard, &line),
                        Err(_) => {
                            let _ = stream.write_all(b"ERR INTERNAL_ERROR lock_poisoned\n");
                            break;
                        }
                    }
                };

                let response = format!("{}\n", outcome.response_line);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();

                if outcome.should_shutdown {
                    shutdown.store(true, Ordering::SeqCst);
                    break;
                }
            }
            Err(e) => {
                let _ =
                    stream.write_all(format!("ERR INTERNAL_ERROR read_failed:{}\n", e).as_bytes());
                break;
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let socket_path = parse_socket_arg(&args);
    let render_output_dir = parse_render_output_dir_arg(&args);
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

    if let Err(e) = listener.set_nonblocking(true) {
        eprintln!("daemon_error=set_nonblocking_failed err={}", e);
        std::process::exit(4);
    }

    let _guard = SocketGuard::new(socket_path.clone());

    println!("daemon_status=started socket={}", socket_path.display());

    let engine = Arc::new(Mutex::new(RuntimeEngine::new_with_render_output_dir(
        render_output_dir.clone(),
    )));
    let shutdown = Arc::new(AtomicBool::new(false));

    println!("daemon_render_output_dir={}", render_output_dir);

    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let engine_ref = Arc::clone(&engine);
                let shutdown_ref = Arc::clone(&shutdown);
                thread::spawn(move || {
                    handle_client(stream, engine_ref, shutdown_ref);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                eprintln!("daemon_warn=accept_failed err={}", e);
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    println!("daemon_status=stopped");
}
