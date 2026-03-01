use directscreen_core::api::Status;
use directscreen_core::engine::RuntimeEngine;
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::dsapid_frame_fd::{
    handle_frame_bind_shm, handle_frame_submit_shm, handle_frame_wait_shm_present, BoundFrameFd,
};
use super::dsapid_parse::{
    parse_frame_bind_shm_request, parse_frame_submit_shm_request,
    parse_frame_wait_shm_present_request,
};
use super::{handle_touch_stream_v1, read_command_line, write_internal_error, write_status_error};

pub(super) fn handle_data_client(
    mut stream: UnixStream,
    engine: Arc<RuntimeEngine>,
    shutdown: Arc<AtomicBool>,
    command_max_bytes: usize,
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
                let bind_shm = match parse_frame_bind_shm_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if bind_shm {
                    if let Err(e) = handle_frame_bind_shm(&stream, &engine, &mut bound_frame_fd) {
                        write_internal_error(&mut stream, &format!("bind_shm_failed:{}", e));
                        break;
                    }
                    continue;
                }

                let wait_shm_present = match parse_frame_wait_shm_present_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if let Some((last_seq, timeout_ms)) = wait_shm_present {
                    let Some(bound) = bound_frame_fd.as_mut() else {
                        write_status_error(&mut stream, Status::InvalidArgument);
                        continue;
                    };
                    if let Err(e) = handle_frame_wait_shm_present(
                        &mut stream,
                        &engine,
                        bound,
                        last_seq,
                        timeout_ms,
                    ) {
                        write_internal_error(
                            &mut stream,
                            &format!("wait_shm_present_failed:{}", e),
                        );
                        break;
                    }
                    continue;
                }

                let submit_shm = match parse_frame_submit_shm_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if let Some(req) = submit_shm {
                    let Some(bound) = bound_frame_fd.as_mut() else {
                        write_status_error(&mut stream, Status::InvalidArgument);
                        continue;
                    };
                    if let Err(e) = handle_frame_submit_shm(&mut stream, &engine, bound, req) {
                        write_internal_error(&mut stream, &format!("submit_shm_failed:{}", e));
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
}
