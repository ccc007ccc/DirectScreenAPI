use std::fs;
use std::io::{BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use directscreen_core::api::{RouteResult, Status, TouchEvent};
use directscreen_core::engine::RuntimeEngine;
use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token};
use nix::sys::socket::{getsockopt, sockopt};

#[path = "dsapid/config.rs"]
mod dsapid_config;
#[path = "dsapid/control_dispatch.rs"]
mod dsapid_control_dispatch;
#[path = "dsapid/data_dispatch.rs"]
mod dsapid_data_dispatch;
#[path = "dsapid/frame_fd.rs"]
mod dsapid_frame_fd;
#[path = "dsapid/parse.rs"]
mod dsapid_parse;
use dsapid_config::{ensure_parent, parse_daemon_config};
use dsapid_control_dispatch::handle_control_client;
use dsapid_data_dispatch::handle_data_client;
#[cfg(test)]
use dsapid_parse::{parse_frame_bind_shm_request, parse_frame_wait_shm_present_request};

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
        let _ = remove_stale_socket(&self.path);
    }
}

#[derive(Debug, Clone)]
struct WorkerCommand {
    program: String,
    args: Vec<String>,
}

fn socket_peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
    let creds = getsockopt(stream, sockopt::PeerCredentials).map_err(std::io::Error::other)?;
    Ok(creds.uid())
}

fn write_status_error(stream: &mut UnixStream, status: Status) {
    let _ = stream.write_all(format!("ERR {}\n", status.as_str()).as_bytes());
    let _ = stream.flush();
}

fn write_internal_error(stream: &mut UnixStream, msg: &str) {
    let _ = stream.write_all(format!("ERR INTERNAL_ERROR {}\n", msg).as_bytes());
    let _ = stream.flush();
}

fn write_busy(stream: &mut UnixStream) {
    let _ = stream.write_all(b"ERR BUSY\n");
    let _ = stream.flush();
}

fn read_command_line(
    reader: &mut BufReader<UnixStream>,
    max_bytes: usize,
) -> std::io::Result<Option<String>> {
    let mut out: Vec<u8> = Vec::with_capacity(128);
    let mut byte = [0u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => {
                if out.is_empty() {
                    return Ok(None);
                }
                break;
            }
            Ok(_) => {
                if out.len() >= max_bytes {
                    // 超长命令直接丢弃本行剩余数据，避免单连接持续占用解析内存。
                    loop {
                        match reader.read(&mut byte) {
                            Ok(0) => break,
                            Ok(_) if byte[0] == b'\n' => break,
                            Ok(_) => {}
                            Err(e) => return Err(e),
                        }
                    }
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "command_too_long",
                    ));
                }
                out.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(e) => return Err(e),
        }
    }
    String::from_utf8(out)
        .map(Some)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "command_not_utf8"))
}

fn contains_shell_meta(raw: &str) -> bool {
    raw.chars().any(|c| {
        matches!(
            c,
            ';' | '|' | '&' | '<' | '>' | '$' | '`' | '\n' | '\r' | '\'' | '"' | '\\'
        )
    })
}

fn parse_worker_command(raw: &str) -> Result<WorkerCommand, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("worker_cmd_empty".to_string());
    }
    if contains_shell_meta(trimmed) {
        return Err("worker_cmd_contains_shell_meta".to_string());
    }
    let tokens: Vec<String> = trimmed
        .split_whitespace()
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .collect();
    if tokens.is_empty() {
        return Err("worker_cmd_empty".to_string());
    }
    if tokens[0].starts_with('-') {
        return Err("worker_program_invalid".to_string());
    }
    Ok(WorkerCommand {
        program: tokens[0].clone(),
        args: tokens[1..].to_vec(),
    })
}

fn backoff_delay(base: Duration, failures: u32) -> Duration {
    let max_delay = Duration::from_secs(30);
    let shift = failures.min(6);
    let factor = 1u128 << shift;
    let ms = base.as_millis().saturating_mul(factor);
    let capped = ms.min(max_delay.as_millis());
    Duration::from_millis(capped as u64)
}

fn remove_stale_socket(path: &std::path::Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            let ft = meta.file_type();
            if ft.is_socket() {
                fs::remove_file(path)
            } else {
                Err(std::io::Error::other("refuse_remove_non_socket"))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn secure_socket_permissions(path: &std::path::Path) -> std::io::Result<()> {
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
}

const TOUCH_PACKET_BYTES: usize = 16;
const TOUCH_PACKET_DOWN: u8 = 1;
const TOUCH_PACKET_MOVE: u8 = 2;
const TOUCH_PACKET_UP: u8 = 3;
const TOUCH_PACKET_CANCEL: u8 = 4;
const TOUCH_PACKET_CLEAR: u8 = 5;

fn parse_i32_le(bytes: &[u8]) -> i32 {
    i32::from_le_bytes(bytes.try_into().expect("len=4"))
}

fn parse_f32_le(bytes: &[u8]) -> f32 {
    f32::from_le_bytes(bytes.try_into().expect("len=4"))
}

fn apply_touch_packet(engine: &RuntimeEngine, packet: &[u8; TOUCH_PACKET_BYTES]) {
    let kind = packet[0];
    let pointer_id = parse_i32_le(&packet[4..8]);
    let x = parse_f32_le(&packet[8..12]);
    let y = parse_f32_le(&packet[12..16]);

    let _ = match kind {
        TOUCH_PACKET_DOWN => engine.touch_down(TouchEvent { pointer_id, x, y }),
        TOUCH_PACKET_MOVE => engine.touch_move(TouchEvent { pointer_id, x, y }),
        TOUCH_PACKET_UP => engine.touch_up(TouchEvent { pointer_id, x, y }),
        TOUCH_PACKET_CANCEL => engine.touch_cancel(pointer_id),
        TOUCH_PACKET_CLEAR => {
            engine.clear_touches();
            Ok(RouteResult::passthrough())
        }
        _ => Err(Status::InvalidArgument),
    };
}

fn handle_touch_stream_v1(
    stream: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    engine: &Arc<RuntimeEngine>,
    shutdown: &Arc<AtomicBool>,
) -> std::io::Result<()> {
    stream.write_all(b"OK STREAM_TOUCH_V1\n")?;
    stream.flush()?;

    let mut packet = [0u8; TOUCH_PACKET_BYTES];
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        match reader.read_exact(&mut packet) {
            Ok(()) => apply_touch_packet(engine, &packet),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn kill_process_quiet(pid: i32, sig: i32) {
    if pid > 0 {
        let _ = unsafe { libc::kill(pid, sig) };
    }
}

fn supervise_worker(
    name: &'static str,
    cmd: WorkerCommand,
    base_restart_delay: Duration,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut failures = 0u32;
        while !shutdown.load(Ordering::SeqCst) {
            let started_at = Instant::now();
            let mut child = match Command::new(&cmd.program).args(&cmd.args).spawn() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("daemon_worker_error=name={} action=spawn err={}", name, e);
                    failures = failures.saturating_add(1);
                    thread::sleep(backoff_delay(base_restart_delay, failures));
                    continue;
                }
            };

            let pid = child.id() as i32;
            println!(
                "daemon_worker_status=name={} state=started pid={}",
                name, pid
            );

            loop {
                if shutdown.load(Ordering::SeqCst) {
                    kill_process_quiet(pid, libc::SIGTERM);
                    thread::sleep(Duration::from_millis(120));
                    kill_process_quiet(pid, libc::SIGKILL);
                    let _ = child.wait();
                    println!("daemon_worker_status=name={} state=stopped", name);
                    return;
                }

                match child.try_wait() {
                    Ok(Some(status)) => {
                        let runtime_ms = started_at.elapsed().as_millis() as u64;
                        if runtime_ms >= 10_000 {
                            failures = 0;
                        } else {
                            failures = failures.saturating_add(1);
                        }
                        let restart_delay = backoff_delay(base_restart_delay, failures);
                        eprintln!(
                            "daemon_worker_warn=name={} state=exited status={} runtime_ms={} restart_in_ms={}",
                            name,
                            status,
                            runtime_ms,
                            restart_delay.as_millis()
                        );
                        break;
                    }
                    Ok(None) => {
                        thread::sleep(Duration::from_millis(250));
                    }
                    Err(e) => {
                        eprintln!("daemon_worker_error=name={} action=wait err={}", name, e);
                        break;
                    }
                }
            }

            if !shutdown.load(Ordering::SeqCst) {
                thread::sleep(backoff_delay(base_restart_delay, failures));
            }
        }
    })
}

struct ActiveConnectionGuard {
    active_connections: Arc<AtomicUsize>,
}

impl ActiveConnectionGuard {
    fn new(active_connections: Arc<AtomicUsize>) -> Self {
        Self { active_connections }
    }
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.active_connections.fetch_sub(1, Ordering::AcqRel);
    }
}

fn try_acquire_connection(active_connections: &Arc<AtomicUsize>, max_connections: usize) -> bool {
    let mut current = active_connections.load(Ordering::Acquire);
    loop {
        if current >= max_connections {
            return false;
        }
        match active_connections.compare_exchange_weak(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(next) => current = next,
        }
    }
}

struct DispatchContext {
    allowed_uids: Arc<std::collections::HashSet<u32>>,
    active_connections: Arc<AtomicUsize>,
    max_connections: usize,
    dispatch_workers: usize,
    socket_rw_timeout_ms: u64,
    command_max_bytes: usize,
    engine: Arc<RuntimeEngine>,
    shutdown: Arc<AtomicBool>,
}

struct WorkerJob {
    stream: UnixStream,
    socket_kind: &'static str,
}

struct DispatchPool {
    senders: Vec<SyncSender<WorkerJob>>,
    handles: Vec<thread::JoinHandle<()>>,
    next_worker: AtomicUsize,
}

impl DispatchPool {
    fn start(ctx: &DispatchContext) -> Self {
        let worker_count = ctx.dispatch_workers.max(1);
        let mut senders = Vec::with_capacity(worker_count);
        let mut handles = Vec::with_capacity(worker_count);

        for idx in 0..worker_count {
            let (tx, rx) = mpsc::sync_channel::<WorkerJob>(ctx.max_connections.max(1));
            let engine_ref = Arc::clone(&ctx.engine);
            let shutdown_ref = Arc::clone(&ctx.shutdown);
            let active_connections_ref = Arc::clone(&ctx.active_connections);
            let command_max_bytes = ctx.command_max_bytes;
            let handle = thread::spawn(move || {
                loop {
                    if shutdown_ref.load(Ordering::SeqCst) {
                        match rx.try_recv() {
                            Ok(job) => {
                                let _active_guard =
                                    ActiveConnectionGuard::new(Arc::clone(&active_connections_ref));
                                dispatch_worker_job(
                                    job,
                                    &engine_ref,
                                    &shutdown_ref,
                                    command_max_bytes,
                                );
                                continue;
                            }
                            Err(_) => break,
                        }
                    }

                    let job = match rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(v) => v,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };

                    let _active_guard =
                        ActiveConnectionGuard::new(Arc::clone(&active_connections_ref));
                    dispatch_worker_job(job, &engine_ref, &shutdown_ref, command_max_bytes);
                }
                println!("daemon_dispatch_worker_status=index={} state=stopped", idx);
            });

            senders.push(tx);
            handles.push(handle);
        }

        Self {
            senders,
            handles,
            next_worker: AtomicUsize::new(0),
        }
    }

    fn submit(&self, job: WorkerJob) -> Result<(), WorkerJob> {
        if self.senders.is_empty() {
            return Err(job);
        }
        let idx = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.senders.len();
        match self.senders[idx].try_send(job) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(job)) | Err(TrySendError::Disconnected(job)) => Err(job),
        }
    }

    fn stop(self) {
        drop(self.senders);
        for handle in self.handles {
            let _ = handle.join();
        }
    }
}

fn dispatch_worker_job(
    job: WorkerJob,
    engine: &Arc<RuntimeEngine>,
    shutdown: &Arc<AtomicBool>,
    command_max_bytes: usize,
) {
    match job.socket_kind {
        "control" => handle_control_client(
            job.stream,
            Arc::clone(engine),
            Arc::clone(shutdown),
            command_max_bytes,
        ),
        "data" => handle_data_client(
            job.stream,
            Arc::clone(engine),
            Arc::clone(shutdown),
            command_max_bytes,
        ),
        _ => {}
    }
}

fn dispatch_accepted_client(
    mut stream: UnixStream,
    socket_kind: &'static str,
    ctx: &DispatchContext,
    dispatch_pool: &DispatchPool,
) {
    match socket_peer_uid(&stream) {
        Ok(uid) if ctx.allowed_uids.contains(&uid) => {
            if !try_acquire_connection(&ctx.active_connections, ctx.max_connections) {
                write_busy(&mut stream);
                eprintln!(
                    "daemon_warn=too_many_connections socket_kind={} active={} limit={}",
                    socket_kind,
                    ctx.active_connections.load(Ordering::Acquire),
                    ctx.max_connections
                );
                return;
            }

            let socket_timeout = if ctx.socket_rw_timeout_ms == 0 {
                None
            } else {
                Some(Duration::from_millis(ctx.socket_rw_timeout_ms))
            };

            if let Err(e) = stream.set_nonblocking(false) {
                let _ = stream.write_all(b"ERR INTERNAL_ERROR set_blocking_failed\n");
                let _ = stream.flush();
                ctx.active_connections.fetch_sub(1, Ordering::AcqRel);
                eprintln!(
                    "daemon_warn=set_blocking_failed socket_kind={} err={}",
                    socket_kind, e
                );
                return;
            }
            if let Err(e) = stream.set_read_timeout(socket_timeout) {
                let _ = stream.write_all(b"ERR INTERNAL_ERROR set_read_timeout_failed\n");
                let _ = stream.flush();
                ctx.active_connections.fetch_sub(1, Ordering::AcqRel);
                eprintln!(
                    "daemon_warn=set_read_timeout_failed socket_kind={} err={}",
                    socket_kind, e
                );
                return;
            }
            if let Err(e) = stream.set_write_timeout(socket_timeout) {
                let _ = stream.write_all(b"ERR INTERNAL_ERROR set_write_timeout_failed\n");
                let _ = stream.flush();
                ctx.active_connections.fetch_sub(1, Ordering::AcqRel);
                eprintln!(
                    "daemon_warn=set_write_timeout_failed socket_kind={} err={}",
                    socket_kind, e
                );
                return;
            }

            let job = WorkerJob {
                stream,
                socket_kind,
            };
            if let Err(mut rejected) = dispatch_pool.submit(job) {
                let _ = rejected.stream.write_all(b"ERR BUSY\n");
                let _ = rejected.stream.flush();
                ctx.active_connections.fetch_sub(1, Ordering::AcqRel);
                eprintln!(
                    "daemon_warn=dispatch_queue_busy socket_kind={} active={} limit={}",
                    socket_kind,
                    ctx.active_connections.load(Ordering::Acquire),
                    ctx.max_connections
                );
            }
        }
        Ok(uid) => {
            let _ = stream.write_all(b"ERR FORBIDDEN\n");
            let _ = stream.flush();
            eprintln!(
                "daemon_warn=forbidden_peer socket_kind={} uid={}",
                socket_kind, uid
            );
        }
        Err(e) => {
            let _ = stream.write_all(b"ERR INTERNAL_ERROR peer_cred_failed\n");
            let _ = stream.flush();
            eprintln!(
                "daemon_warn=peer_cred_failed socket_kind={} err={}",
                socket_kind, e
            );
        }
    }
}

const TOKEN_CONTROL_LISTENER: Token = Token(1);
const TOKEN_DATA_LISTENER: Token = Token(2);

fn accept_pending(
    listener: &UnixListener,
    socket_kind: &'static str,
    ctx: &DispatchContext,
    dispatch_pool: &DispatchPool,
) {
    loop {
        match listener.accept() {
            Ok((stream, _addr)) => {
                dispatch_accepted_client(stream, socket_kind, ctx, dispatch_pool);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => {
                eprintln!(
                    "daemon_warn=accept_failed socket_kind={} err={}",
                    socket_kind, e
                );
                break;
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg = match parse_daemon_config(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("daemon_error=parse_args_failed reason={}", e);
            std::process::exit(1);
        }
    };
    let control_socket_path = PathBuf::from(cfg.control_socket_path.clone());
    let data_socket_path = PathBuf::from(cfg.data_socket_path.clone());

    if let Err(e) = ensure_parent(&control_socket_path) {
        eprintln!("daemon_error=mkdir_failed socket_kind=control err={}", e);
        std::process::exit(2);
    }
    if let Err(e) = ensure_parent(&data_socket_path) {
        eprintln!("daemon_error=mkdir_failed socket_kind=data err={}", e);
        std::process::exit(2);
    }

    if let Err(e) = remove_stale_socket(&control_socket_path) {
        eprintln!(
            "daemon_error=remove_stale_socket_failed socket_kind=control path={} err={}",
            control_socket_path.display(),
            e
        );
        std::process::exit(2);
    }
    if let Err(e) = remove_stale_socket(&data_socket_path) {
        eprintln!(
            "daemon_error=remove_stale_socket_failed socket_kind=data path={} err={}",
            data_socket_path.display(),
            e
        );
        std::process::exit(2);
    }

    let control_listener = match UnixListener::bind(&control_socket_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "daemon_error=bind_failed socket_kind=control path={} err={}",
                control_socket_path.display(),
                e
            );
            std::process::exit(3);
        }
    };
    let data_listener = match UnixListener::bind(&data_socket_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "daemon_error=bind_failed socket_kind=data path={} err={}",
                data_socket_path.display(),
                e
            );
            std::process::exit(3);
        }
    };
    if let Err(e) = secure_socket_permissions(&control_socket_path) {
        eprintln!(
            "daemon_error=set_permissions_failed socket_kind=control path={} err={}",
            control_socket_path.display(),
            e
        );
        std::process::exit(4);
    }
    if let Err(e) = secure_socket_permissions(&data_socket_path) {
        eprintln!(
            "daemon_error=set_permissions_failed socket_kind=data path={} err={}",
            data_socket_path.display(),
            e
        );
        std::process::exit(4);
    }

    if let Err(e) = control_listener.set_nonblocking(true) {
        eprintln!(
            "daemon_error=set_nonblocking_failed socket_kind=control err={}",
            e
        );
        std::process::exit(4);
    }
    if let Err(e) = data_listener.set_nonblocking(true) {
        eprintln!(
            "daemon_error=set_nonblocking_failed socket_kind=data err={}",
            e
        );
        std::process::exit(4);
    }

    let _control_guard = SocketGuard::new(control_socket_path.clone());
    let _data_guard = SocketGuard::new(data_socket_path.clone());

    let mut allowed_uids_sorted: Vec<u32> = cfg.allowed_uids.iter().copied().collect();
    allowed_uids_sorted.sort_unstable();
    let allowed_uids = Arc::new(cfg.allowed_uids.clone());
    let allowed_uids_csv = allowed_uids_sorted
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",");
    println!(
        "daemon_status=started control_socket={} data_socket={}",
        control_socket_path.display(),
        data_socket_path.display()
    );
    println!("daemon_auth_allowed_uids={}", allowed_uids_csv);

    let engine = Arc::new(RuntimeEngine::new_with_render_output_dir(
        cfg.render_output_dir.clone(),
    ));
    let shutdown = Arc::new(AtomicBool::new(false));
    let active_connections = Arc::new(AtomicUsize::new(0));
    let dispatch_ctx = DispatchContext {
        allowed_uids: Arc::clone(&allowed_uids),
        active_connections: Arc::clone(&active_connections),
        max_connections: cfg.max_connections,
        dispatch_workers: cfg.dispatch_workers,
        socket_rw_timeout_ms: cfg.socket_rw_timeout_ms,
        command_max_bytes: cfg.command_max_bytes,
        engine: Arc::clone(&engine),
        shutdown: Arc::clone(&shutdown),
    };
    let mut supervise_handles: Vec<thread::JoinHandle<()>> = Vec::new();

    println!("daemon_render_output_dir={}", cfg.render_output_dir);
    println!("daemon_max_connections={}", cfg.max_connections);
    println!("daemon_dispatch_workers={}", cfg.dispatch_workers);
    println!("daemon_socket_rw_timeout_ms={}", cfg.socket_rw_timeout_ms);
    println!("daemon_command_max_bytes={}", cfg.command_max_bytes);

    let restart_delay = Duration::from_millis(cfg.supervise_restart_ms);
    if let Some(raw_cmd) = cfg.supervise_presenter_cmd.clone() {
        let cmd = match parse_worker_command(&raw_cmd) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "daemon_error=invalid_supervise_cmd name=presenter err={}",
                    e
                );
                std::process::exit(1);
            }
        };
        println!(
            "daemon_worker_config=name=presenter program={} arg_count={}",
            cmd.program,
            cmd.args.len()
        );
        supervise_handles.push(supervise_worker(
            "presenter",
            cmd,
            restart_delay,
            Arc::clone(&shutdown),
        ));
    }
    if let Some(raw_cmd) = cfg.supervise_input_cmd.clone() {
        let cmd = match parse_worker_command(&raw_cmd) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("daemon_error=invalid_supervise_cmd name=input err={}", e);
                std::process::exit(1);
            }
        };
        println!(
            "daemon_worker_config=name=input program={} arg_count={}",
            cmd.program,
            cmd.args.len()
        );
        supervise_handles.push(supervise_worker(
            "input",
            cmd,
            restart_delay,
            Arc::clone(&shutdown),
        ));
    }

    let dispatch_pool = DispatchPool::start(&dispatch_ctx);

    let mut poll = match Poll::new() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("daemon_error=poll_create_failed err={}", e);
            shutdown.store(true, Ordering::SeqCst);
            dispatch_pool.stop();
            for handle in supervise_handles {
                let _ = handle.join();
            }
            std::process::exit(4);
        }
    };
    let control_raw_fd = control_listener.as_raw_fd();
    let data_raw_fd = data_listener.as_raw_fd();
    let mut control_source = SourceFd(&control_raw_fd);
    let mut data_source = SourceFd(&data_raw_fd);

    if let Err(e) = poll.registry().register(
        &mut control_source,
        TOKEN_CONTROL_LISTENER,
        Interest::READABLE,
    ) {
        eprintln!(
            "daemon_error=poll_register_failed socket_kind=control err={}",
            e
        );
        shutdown.store(true, Ordering::SeqCst);
        dispatch_pool.stop();
        for handle in supervise_handles {
            let _ = handle.join();
        }
        std::process::exit(4);
    }

    if let Err(e) =
        poll.registry()
            .register(&mut data_source, TOKEN_DATA_LISTENER, Interest::READABLE)
    {
        eprintln!(
            "daemon_error=poll_register_failed socket_kind=data err={}",
            e
        );
        shutdown.store(true, Ordering::SeqCst);
        dispatch_pool.stop();
        for handle in supervise_handles {
            let _ = handle.join();
        }
        std::process::exit(4);
    }

    let mut events = Events::with_capacity(256);
    while !shutdown.load(Ordering::SeqCst) {
        match poll.poll(&mut events, Some(Duration::from_millis(250))) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                eprintln!("daemon_warn=poll_wait_failed err={}", e);
                thread::sleep(Duration::from_millis(20));
                continue;
            }
        }

        for event in events.iter() {
            match event.token() {
                TOKEN_CONTROL_LISTENER => {
                    accept_pending(&control_listener, "control", &dispatch_ctx, &dispatch_pool);
                }
                TOKEN_DATA_LISTENER => {
                    accept_pending(&data_listener, "data", &dispatch_ctx, &dispatch_pool);
                }
                _ => {}
            }
        }
    }

    shutdown.store(true, Ordering::SeqCst);
    dispatch_pool.stop();
    for handle in supervise_handles {
        let _ = handle.join();
    }

    println!("daemon_status=stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch_packet(kind: u8, pointer_id: i32, x: f32, y: f32) -> [u8; TOUCH_PACKET_BYTES] {
        let mut out = [0u8; TOUCH_PACKET_BYTES];
        out[0] = kind;
        out[4..8].copy_from_slice(&pointer_id.to_le_bytes());
        out[8..12].copy_from_slice(&x.to_le_bytes());
        out[12..16].copy_from_slice(&y.to_le_bytes());
        out
    }

    #[test]
    fn parse_daemon_config_parses_supervise_fields() {
        let args = vec![
            "dsapid".to_string(),
            "--socket".to_string(),
            "/tmp/a.sock".to_string(),
            "--render-output-dir".to_string(),
            "/tmp/render".to_string(),
            "--supervise-presenter".to_string(),
            "echo presenter".to_string(),
            "--supervise-input".to_string(),
            "echo input".to_string(),
            "--supervise-restart-ms".to_string(),
            "777".to_string(),
            "--max-connections".to_string(),
            "321".to_string(),
            "--dispatch-workers".to_string(),
            "8".to_string(),
        ];
        let cfg = parse_daemon_config(&args).expect("parse daemon cfg");
        assert_eq!(cfg.control_socket_path, "/tmp/a.sock");
        assert_eq!(cfg.data_socket_path, "/tmp/a.data.sock");
        assert_eq!(cfg.render_output_dir, "/tmp/render");
        assert_eq!(
            cfg.supervise_presenter_cmd.as_deref(),
            Some("echo presenter")
        );
        assert_eq!(cfg.supervise_input_cmd.as_deref(), Some("echo input"));
        assert_eq!(cfg.supervise_restart_ms, 777);
        assert_eq!(cfg.max_connections, 321);
        assert_eq!(cfg.dispatch_workers, 8);
        assert!(!cfg.allowed_uids.is_empty());
    }

    #[test]
    fn parse_daemon_config_rejects_unknown_arg() {
        let args = vec!["dsapid".to_string(), "--bad".to_string()];
        let err = parse_daemon_config(&args).expect_err("must fail");
        assert!(err.contains("unknown_arg"));
    }

    #[test]
    fn parse_worker_command_accepts_simple_command() {
        let cmd = parse_worker_command("./scripts/dsapi.sh presenter run").expect("parse");
        assert_eq!(cmd.program, "./scripts/dsapi.sh");
        assert_eq!(cmd.args, vec!["presenter".to_string(), "run".to_string()]);
    }

    #[test]
    fn parse_worker_command_rejects_shell_meta() {
        let err = parse_worker_command("echo ok; id").expect_err("must reject");
        assert!(err.contains("shell_meta"));
    }

    #[test]
    fn parse_frame_bind_shm_request_accepts_header() {
        let out = parse_frame_bind_shm_request("RENDER_FRAME_BIND_SHM").expect("parse bind shm");
        assert!(out);
    }

    #[test]
    fn parse_frame_bind_shm_request_rejects_extra_tokens() {
        let out = parse_frame_bind_shm_request("RENDER_FRAME_BIND_SHM x").expect_err("reject");
        assert_eq!(out, Status::InvalidArgument);
    }

    #[test]
    fn parse_frame_wait_shm_present_request_accepts() {
        let out = parse_frame_wait_shm_present_request("RENDER_FRAME_WAIT_SHM_PRESENT 7 16")
            .expect("parse")
            .expect("request");
        assert_eq!(out.0, 7);
        assert_eq!(out.1, 16);
    }

    #[test]
    fn parse_frame_wait_shm_present_request_rejects_extra_tokens() {
        let out = parse_frame_wait_shm_present_request("RENDER_FRAME_WAIT_SHM_PRESENT 7 16 x")
            .expect_err("reject");
        assert_eq!(out, Status::InvalidArgument);
    }

    #[test]
    fn touch_stream_binary_packets_drive_touch_state() {
        let engine = RuntimeEngine::default();
        apply_touch_packet(&engine, &touch_packet(TOUCH_PACKET_DOWN, 7, 11.0, 22.0));
        assert_eq!(engine.active_touch_count(), 1);

        apply_touch_packet(&engine, &touch_packet(TOUCH_PACKET_MOVE, 7, 12.0, 23.0));
        assert_eq!(engine.active_touch_count(), 1);

        apply_touch_packet(&engine, &touch_packet(TOUCH_PACKET_UP, 7, 12.0, 23.0));
        assert_eq!(engine.active_touch_count(), 0);
    }

    #[test]
    fn touch_stream_clear_packet_resets_active_touches() {
        let engine = RuntimeEngine::default();
        apply_touch_packet(&engine, &touch_packet(TOUCH_PACKET_DOWN, 3, 1.0, 1.0));
        assert_eq!(engine.active_touch_count(), 1);

        apply_touch_packet(&engine, &touch_packet(TOUCH_PACKET_CLEAR, 0, 0.0, 0.0));
        assert_eq!(engine.active_touch_count(), 0);
    }
}
