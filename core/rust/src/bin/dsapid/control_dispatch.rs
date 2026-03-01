use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use directscreen_core::api::Status;
use directscreen_core::engine::{execute_command, RuntimeEngine};

use super::dsapid_parse::parse_display_stream_v1_request;
use super::{
    handle_display_stream_v1, read_command_line, write_internal_error, write_status_error,
};

pub(super) fn handle_control_client(
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
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        match read_command_line(&mut reader, command_max_bytes) {
            Ok(None) => break,
            Ok(Some(line)) => {
                let display_stream = match parse_display_stream_v1_request(&line) {
                    Ok(v) => v,
                    Err(status) => {
                        write_status_error(&mut stream, status);
                        continue;
                    }
                };
                if display_stream {
                    if let Err(e) = handle_display_stream_v1(&mut stream, &engine, &shutdown) {
                        write_internal_error(&mut stream, &format!("display_stream_failed:{}", e));
                    }
                    break;
                }

                if line.trim().eq_ignore_ascii_case("STREAM_TOUCH_V1")
                    || line.trim().starts_with("RENDER_FRAME_SUBMIT_RGBA_RAW ")
                    || line.trim().eq_ignore_ascii_case("RENDER_FRAME_GET_RAW")
                    || line.trim().eq_ignore_ascii_case("RENDER_FRAME_GET_FD")
                    || line.trim().eq_ignore_ascii_case("RENDER_FRAME_BIND_FD")
                    || line.trim().eq_ignore_ascii_case("RENDER_FRAME_GET_BOUND")
                    || line.trim().starts_with("RENDER_FRAME_WAIT_BOUND_PRESENT ")
                {
                    write_status_error(&mut stream, Status::InvalidArgument);
                    continue;
                }

                let outcome = execute_command(&engine, &line);

                let response = format!("{}\\n", outcome.response_line);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();

                if outcome.should_shutdown {
                    shutdown.store(true, Ordering::SeqCst);
                    break;
                }
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
