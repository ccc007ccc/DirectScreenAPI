use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use directscreen_core::api::Status;
use directscreen_core::engine::RuntimeEngine;

use super::dsapid_frame_fd::{
    close_fd, handle_frame_bind_fd, handle_frame_get_bound, handle_frame_get_fd,
    handle_frame_wait_bound_present, BoundFrameFd,
};
use super::dsapid_parse::{
    parse_frame_bind_fd_request, parse_frame_get_bound_request, parse_frame_get_fd_request,
    parse_frame_get_raw_request, parse_frame_wait_bound_present_request,
    parse_raw_frame_submit_request,
};
use super::{
    handle_frame_get_raw, handle_raw_frame_submit, handle_touch_stream_v1, read_command_line,
    write_internal_error, write_status_error,
};

pub(super) fn handle_data_client(
    mut stream: UnixStream,
    engine: Arc<RuntimeEngine>,
    shutdown: Arc<AtomicBool>,
    command_max_bytes: usize,
    raw_frame_max_bytes: usize,
    raw_read_timeout: Duration,
) {
    let reader_stream = match stream.try_clone() {
        Ok(v) => v,
        Err(e) => {
            let _ =
                stream.write_all(format!("ERR INTERNAL_ERROR clone_failed:{}\\n", e).as_bytes());
            return;
        }
    };

    let mut reader = BufReader::new(reader_stream);
    let mut bound_frame_fd: Option<BoundFrameFd> = None;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        match read_command_line(&mut reader, command_max_bytes) {
            Ok(None) => break,
            Ok(Some(line)) => {
                let raw_get = match parse_frame_get_raw_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if raw_get {
                    if let Err(e) = handle_frame_get_raw(&mut stream, &engine) {
                        write_internal_error(&mut stream, &format!("raw_get_failed:{}", e));
                        break;
                    }
                    continue;
                }

                let bind_fd = match parse_frame_bind_fd_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if bind_fd {
                    if let Err(e) = handle_frame_bind_fd(&stream, &engine, &mut bound_frame_fd) {
                        write_internal_error(&mut stream, &format!("bind_fd_failed:{}", e));
                        break;
                    }
                    continue;
                }

                let get_bound = match parse_frame_get_bound_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if get_bound {
                    let Some(bound) = bound_frame_fd.as_ref() else {
                        write_status_error(&mut stream, Status::InvalidArgument);
                        continue;
                    };
                    if let Err(e) = handle_frame_get_bound(&mut stream, &engine, bound) {
                        write_internal_error(&mut stream, &format!("get_bound_failed:{}", e));
                        break;
                    }
                    continue;
                }

                let wait_bound_present = match parse_frame_wait_bound_present_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if let Some((last_seq, timeout_ms)) = wait_bound_present {
                    let Some(bound) = bound_frame_fd.as_ref() else {
                        write_status_error(&mut stream, Status::InvalidArgument);
                        continue;
                    };
                    if let Err(e) = handle_frame_wait_bound_present(
                        &mut stream,
                        &engine,
                        bound,
                        last_seq,
                        timeout_ms,
                    ) {
                        write_internal_error(
                            &mut stream,
                            &format!("wait_bound_present_failed:{}", e),
                        );
                        break;
                    }
                    continue;
                }

                let fd_get = match parse_frame_get_fd_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if fd_get {
                    if let Err(e) = handle_frame_get_fd(&mut stream, &engine) {
                        write_internal_error(&mut stream, &format!("fd_get_failed:{}", e));
                        break;
                    }
                    continue;
                }

                let raw_submit = match parse_raw_frame_submit_request(&line, raw_frame_max_bytes) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };

                if let Some(req) = raw_submit {
                    if let Err(e) = handle_raw_frame_submit(
                        &mut stream,
                        &mut reader,
                        &engine,
                        req,
                        raw_read_timeout,
                    ) {
                        write_internal_error(&mut stream, &format!("raw_read_failed:{}", e));
                        break;
                    }
                    continue;
                }

                if line.trim().eq_ignore_ascii_case("STREAM_TOUCH_V1") {
                    if let Err(e) =
                        handle_touch_stream_v1(&mut stream, &mut reader, &engine, &shutdown)
                    {
                        write_internal_error(&mut stream, &format!("touch_stream_failed:{}", e));
                    }
                    break;
                }

                write_status_error(&mut stream, Status::InvalidArgument);
            }
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                write_status_error(&mut stream, Status::InvalidArgument);
                continue;
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                let _ = stream.write_all(b"ERR INTERNAL_ERROR read_timeout\n");
                let _ = stream.flush();
                break;
            }
            Err(e) => {
                let _ =
                    stream.write_all(format!("ERR INTERNAL_ERROR read_failed:{}\\n", e).as_bytes());
                break;
            }
        }
    }

    if let Some(bound) = bound_frame_fd.take() {
        close_fd(bound.fd);
    }
}
