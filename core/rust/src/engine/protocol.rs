use crate::api::{Decision, RectRegion, Status, TouchEvent};
use crate::engine::RuntimeEngine;
use crate::DIRECTSCREEN_CORE_VERSION;

#[derive(Debug, Clone)]
pub struct CommandOutcome {
    pub response_line: String,
    pub should_shutdown: bool,
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Ok => "OK",
        Status::NullPointer => "NULL_POINTER",
        Status::InvalidArgument => "INVALID_ARGUMENT",
        Status::OutOfRange => "OUT_OF_RANGE",
        Status::InternalError => "INTERNAL_ERROR",
    }
}

fn parse_decision(token: &str) -> Result<Decision, Status> {
    match token {
        "pass" | "PASS" => Ok(Decision::Pass),
        "block" | "BLOCK" => Ok(Decision::Block),
        _ => Err(Status::InvalidArgument),
    }
}

fn parse_i32(token: &str) -> Result<i32, Status> {
    token.parse::<i32>().map_err(|_| Status::InvalidArgument)
}

fn parse_u32(token: &str) -> Result<u32, Status> {
    token.parse::<u32>().map_err(|_| Status::InvalidArgument)
}

fn parse_u64(token: &str) -> Result<u64, Status> {
    token.parse::<u64>().map_err(|_| Status::InvalidArgument)
}

fn parse_f32(token: &str) -> Result<f32, Status> {
    token.parse::<f32>().map_err(|_| Status::InvalidArgument)
}

fn parse_touch_event(
    tokens: &[&str],
    pointer_idx: usize,
    x_idx: usize,
    y_idx: usize,
) -> Result<TouchEvent, Status> {
    Ok(TouchEvent {
        pointer_id: parse_i32(tokens[pointer_idx])?,
        x: parse_f32(tokens[x_idx])?,
        y: parse_f32(tokens[y_idx])?,
    })
}

pub fn execute_command(engine: &RuntimeEngine, line: &str) -> CommandOutcome {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return CommandOutcome {
            response_line: "ERR INVALID_ARGUMENT empty_command".to_string(),
            should_shutdown: false,
        };
    }

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let cmd = tokens[0].to_ascii_uppercase();

    let result = match cmd.as_str() {
        "PING" => Ok("OK PONG".to_string()),
        "VERSION" => Ok(format!("OK {}", DIRECTSCREEN_CORE_VERSION)),
        "DISPLAY_GET" => {
            let d = engine.display_state();
            Ok(format!(
                "OK {} {} {:.2} {} {}",
                d.width, d.height, d.refresh_hz, d.density_dpi, d.rotation
            ))
        }
        "DISPLAY_SET" => {
            if tokens.len() != 6 {
                Err(Status::InvalidArgument)
            } else {
                let width = parse_u32(tokens[1]);
                let height = parse_u32(tokens[2]);
                let hz = parse_f32(tokens[3]);
                let dpi = parse_u32(tokens[4]);
                let rotation = parse_u32(tokens[5]);

                match (width, height, hz, dpi, rotation) {
                    (Ok(width), Ok(height), Ok(refresh_hz), Ok(density_dpi), Ok(rotation)) => {
                        match engine.set_display_state(crate::api::DisplayState {
                            width,
                            height,
                            refresh_hz,
                            density_dpi,
                            rotation,
                        }) {
                            Ok(()) => Ok("OK".to_string()),
                            Err(e) => Err(e),
                        }
                    }
                    _ => Err(Status::InvalidArgument),
                }
            }
        }
        "ROUTE_DEFAULT" => {
            if tokens.len() != 2 {
                Err(Status::InvalidArgument)
            } else {
                match parse_decision(tokens[1]) {
                    Ok(decision) => {
                        engine.set_default_decision(decision);
                        Ok("OK".to_string())
                    }
                    Err(e) => Err(e),
                }
            }
        }
        "ROUTE_CLEAR" => {
            engine.clear_regions();
            Ok("OK".to_string())
        }
        "ROUTE_ADD_RECT" => {
            if tokens.len() != 7 {
                Err(Status::InvalidArgument)
            } else {
                let region_id = parse_i32(tokens[1]);
                let decision = parse_decision(tokens[2]);
                let x = parse_f32(tokens[3]);
                let y = parse_f32(tokens[4]);
                let w = parse_f32(tokens[5]);
                let h = parse_f32(tokens[6]);

                match (region_id, decision, x, y, w, h) {
                    (Ok(region_id), Ok(decision), Ok(x), Ok(y), Ok(w), Ok(h)) => {
                        match engine.add_region_rect(RectRegion {
                            region_id,
                            decision,
                            x,
                            y,
                            w,
                            h,
                        }) {
                            Ok(()) => Ok("OK".to_string()),
                            Err(e) => Err(e),
                        }
                    }
                    _ => Err(Status::InvalidArgument),
                }
            }
        }
        "ROUTE_POINT" => {
            if tokens.len() != 3 {
                Err(Status::InvalidArgument)
            } else {
                match (parse_f32(tokens[1]), parse_f32(tokens[2])) {
                    (Ok(x), Ok(y)) => {
                        let routed = engine.route_point(x, y);
                        Ok(format!(
                            "OK {} {}",
                            routed.decision as i32, routed.region_id
                        ))
                    }
                    _ => Err(Status::InvalidArgument),
                }
            }
        }
        "TOUCH_DOWN" => {
            if tokens.len() != 4 {
                Err(Status::InvalidArgument)
            } else {
                match parse_touch_event(&tokens, 1, 2, 3) {
                    Ok(event) => match engine.touch_down(event) {
                        Ok(routed) => Ok(format!(
                            "OK {} {}",
                            routed.decision as i32, routed.region_id
                        )),
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                }
            }
        }
        "TOUCH_MOVE" => {
            if tokens.len() != 4 {
                Err(Status::InvalidArgument)
            } else {
                match parse_touch_event(&tokens, 1, 2, 3) {
                    Ok(event) => match engine.touch_move(event) {
                        Ok(routed) => Ok(format!(
                            "OK {} {}",
                            routed.decision as i32, routed.region_id
                        )),
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                }
            }
        }
        "TOUCH_UP" => {
            if tokens.len() != 4 {
                Err(Status::InvalidArgument)
            } else {
                match parse_touch_event(&tokens, 1, 2, 3) {
                    Ok(event) => match engine.touch_up(event) {
                        Ok(routed) => Ok(format!(
                            "OK {} {}",
                            routed.decision as i32, routed.region_id
                        )),
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                }
            }
        }
        "TOUCH_CANCEL" => {
            if tokens.len() != 2 {
                Err(Status::InvalidArgument)
            } else {
                match parse_i32(tokens[1]) {
                    Ok(pointer_id) => match engine.touch_cancel(pointer_id) {
                        Ok(routed) => Ok(format!(
                            "OK {} {}",
                            routed.decision as i32, routed.region_id
                        )),
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                }
            }
        }
        "TOUCH_CLEAR" => {
            engine.clear_touches();
            Ok("OK".to_string())
        }
        "TOUCH_COUNT" => Ok(format!("OK {}", engine.active_touch_count())),
        "RENDER_SUBMIT" => {
            if tokens.len() != 4 {
                Err(Status::InvalidArgument)
            } else {
                match (
                    parse_u32(tokens[1]),
                    parse_u32(tokens[2]),
                    parse_u32(tokens[3]),
                ) {
                    (Ok(draw_calls), Ok(frost_passes), Ok(text_calls)) => {
                        let stats =
                            engine.submit_render_stats(draw_calls, frost_passes, text_calls);
                        Ok(format!(
                            "OK {} {} {} {}",
                            stats.frame_seq, stats.draw_calls, stats.frost_passes, stats.text_calls
                        ))
                    }
                    _ => Err(Status::InvalidArgument),
                }
            }
        }
        "RENDER_GET" => {
            if tokens.len() != 1 {
                Err(Status::InvalidArgument)
            } else {
                let stats = engine.render_stats();
                Ok(format!(
                    "OK {} {} {} {}",
                    stats.frame_seq, stats.draw_calls, stats.frost_passes, stats.text_calls
                ))
            }
        }
        "RENDER_FRAME_GET" => {
            if tokens.len() != 1 {
                Err(Status::InvalidArgument)
            } else {
                match engine.render_frame_info() {
                    Some(frame) => Ok(format!(
                        "OK {} {} {} RGBA8888 {} {}",
                        frame.frame_seq,
                        frame.width,
                        frame.height,
                        frame.byte_len,
                        frame.checksum_fnv1a32
                    )),
                    None => Err(Status::OutOfRange),
                }
            }
        }
        "RENDER_FRAME_WAIT" => {
            if tokens.len() != 3 {
                Err(Status::InvalidArgument)
            } else {
                match (parse_u64(tokens[1]), parse_u32(tokens[2])) {
                    (Ok(last_seq), Ok(timeout_ms)) => {
                        match engine.wait_for_frame_after(last_seq, timeout_ms) {
                            Ok(Some(frame)) => Ok(format!(
                                "OK {} {} {} RGBA8888 {} {}",
                                frame.frame_seq,
                                frame.width,
                                frame.height,
                                frame.byte_len,
                                frame.checksum_fnv1a32
                            )),
                            Ok(None) => Ok("OK TIMEOUT".to_string()),
                            Err(e) => Err(e),
                        }
                    }
                    _ => Err(Status::InvalidArgument),
                }
            }
        }
        "RENDER_FRAME_CLEAR" => {
            if tokens.len() != 1 {
                Err(Status::InvalidArgument)
            } else {
                engine.clear_render_frame();
                Ok("OK".to_string())
            }
        }
        "RENDER_PRESENT" => {
            if tokens.len() != 1 {
                Err(Status::InvalidArgument)
            } else {
                match engine.render_present() {
                    Ok(present) => Ok(format!(
                        "OK {} {} {} {} RGBA8888 {} {}",
                        present.present_seq,
                        present.frame_seq,
                        present.width,
                        present.height,
                        present.byte_len,
                        present.checksum_fnv1a32
                    )),
                    Err(e) => Err(e),
                }
            }
        }
        "RENDER_PRESENT_GET" => {
            if tokens.len() != 1 {
                Err(Status::InvalidArgument)
            } else {
                match engine.render_present_get() {
                    Some(present) => Ok(format!(
                        "OK {} {} {} {} RGBA8888 {} {}",
                        present.present_seq,
                        present.frame_seq,
                        present.width,
                        present.height,
                        present.byte_len,
                        present.checksum_fnv1a32
                    )),
                    None => Err(Status::OutOfRange),
                }
            }
        }
        "RENDER_DUMP_PPM" => {
            if tokens.len() != 1 {
                Err(Status::InvalidArgument)
            } else {
                match engine.render_dump_ppm() {
                    Ok(path) => Ok(format!("OK {}", path)),
                    Err(e) => Err(e),
                }
            }
        }
        "SHUTDOWN" => Ok("OK SHUTDOWN".to_string()),
        _ => Err(Status::InvalidArgument),
    };

    match result {
        Ok(line) => CommandOutcome {
            response_line: line,
            should_shutdown: cmd == "SHUTDOWN",
        },
        Err(status) => CommandOutcome {
            response_line: format!("ERR {}", status_name(status)),
            should_shutdown: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_and_shutdown_commands() {
        let engine = RuntimeEngine::default();
        let ping = execute_command(&engine, "PING");
        assert_eq!(ping.response_line, "OK PONG");
        assert!(!ping.should_shutdown);

        let shutdown = execute_command(&engine, "SHUTDOWN");
        assert_eq!(shutdown.response_line, "OK SHUTDOWN");
        assert!(shutdown.should_shutdown);
    }

    #[test]
    fn touch_flow_commands_keep_pointer_state() {
        let engine = RuntimeEngine::default();
        let add = execute_command(&engine, "ROUTE_ADD_RECT 10 block 0 0 100 100");
        assert_eq!(add.response_line, "OK");

        let down = execute_command(&engine, "TOUCH_DOWN 1 10 10");
        assert_eq!(down.response_line, "OK 1 10");

        let move_cmd = execute_command(&engine, "TOUCH_MOVE 1 200 200");
        assert_eq!(move_cmd.response_line, "OK 1 10");

        let count = execute_command(&engine, "TOUCH_COUNT");
        assert_eq!(count.response_line, "OK 1");

        let up = execute_command(&engine, "TOUCH_UP 1 200 200");
        assert_eq!(up.response_line, "OK 1 10");

        let count_after = execute_command(&engine, "TOUCH_COUNT");
        assert_eq!(count_after.response_line, "OK 0");
    }

    #[test]
    fn render_submit_and_get_commands() {
        let engine = RuntimeEngine::default();

        let first = execute_command(&engine, "RENDER_SUBMIT 8 2 3");
        assert_eq!(first.response_line, "OK 1 8 2 3");

        let second = execute_command(&engine, "RENDER_SUBMIT 10 4 5");
        assert_eq!(second.response_line, "OK 2 10 4 5");

        let get = execute_command(&engine, "RENDER_GET");
        assert_eq!(get.response_line, "OK 2 10 4 5");
    }

    #[test]
    fn render_submit_invalid_argument_rejected() {
        let engine = RuntimeEngine::default();

        let bad = execute_command(&engine, "RENDER_SUBMIT x 1 2");
        assert_eq!(bad.response_line, "ERR INVALID_ARGUMENT");

        let get_bad = execute_command(&engine, "RENDER_GET extra");
        assert_eq!(get_bad.response_line, "ERR INVALID_ARGUMENT");
    }

    #[test]
    fn render_response_tokens_are_parsable_numbers() {
        let engine = RuntimeEngine::default();
        let out = execute_command(&engine, "RENDER_SUBMIT 12 6 7");
        let tokens: Vec<&str> = out.response_line.split_whitespace().collect();
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0], "OK");
        assert!(tokens[1].parse::<u64>().is_ok());
        assert!(tokens[2].parse::<u32>().is_ok());
        assert!(tokens[3].parse::<u32>().is_ok());
        assert!(tokens[4].parse::<u32>().is_ok());
    }

    #[test]
    fn render_frame_get_and_clear_commands() {
        let engine = RuntimeEngine::default();
        let pixels = vec![
            255u8, 0u8, 0u8, 255u8, 0u8, 255u8, 0u8, 255u8, 0u8, 0u8, 255u8, 255u8, 255u8, 255u8,
            255u8, 255u8,
        ];
        let submit = engine
            .submit_render_frame_rgba(2, 2, pixels)
            .expect("submit frame");

        let get = execute_command(&engine, "RENDER_FRAME_GET");
        assert_eq!(
            get.response_line,
            format!(
                "OK {} {} {} RGBA8888 {} {}",
                submit.frame_seq,
                submit.width,
                submit.height,
                submit.byte_len,
                submit.checksum_fnv1a32
            )
        );

        let clear = execute_command(&engine, "RENDER_FRAME_CLEAR");
        assert_eq!(clear.response_line, "OK");

        let get_after_clear = execute_command(&engine, "RENDER_FRAME_GET");
        assert_eq!(get_after_clear.response_line, "ERR OUT_OF_RANGE");
    }

    #[test]
    fn render_frame_wait_command_times_out_without_new_frame() {
        let engine = RuntimeEngine::default();
        let wait = execute_command(&engine, "RENDER_FRAME_WAIT 0 1");
        assert_eq!(wait.response_line, "OK TIMEOUT");
    }

    #[test]
    fn render_frame_wait_command_returns_frame_when_available() {
        let engine = RuntimeEngine::default();
        engine
            .submit_render_frame_rgba(1, 1, vec![1u8, 2u8, 3u8, 4u8])
            .expect("submit frame");

        let wait = execute_command(&engine, "RENDER_FRAME_WAIT 0 10");
        let tokens: Vec<&str> = wait.response_line.split_whitespace().collect();
        assert_eq!(tokens.len(), 7);
        assert_eq!(tokens[0], "OK");
        assert_eq!(tokens[2], "1");
        assert_eq!(tokens[3], "1");
        assert_eq!(tokens[4], "RGBA8888");
        assert_eq!(tokens[5], "4");
        assert!(tokens[1].parse::<u64>().is_ok());
        assert!(tokens[6].parse::<u32>().is_ok());
    }

    #[test]
    fn render_present_and_get_commands() {
        let engine = RuntimeEngine::new_with_render_output_dir("artifacts/test_protocol_present");
        let pixels = vec![
            255u8, 0u8, 0u8, 255u8, 0u8, 255u8, 0u8, 255u8, 0u8, 0u8, 255u8, 255u8, 255u8, 255u8,
            255u8, 255u8,
        ];
        engine
            .submit_render_frame_rgba(2, 2, pixels)
            .expect("submit frame");

        let present = execute_command(&engine, "RENDER_PRESENT");
        let present_tokens: Vec<&str> = present.response_line.split_whitespace().collect();
        assert_eq!(present_tokens.len(), 8);
        assert_eq!(present_tokens[0], "OK");
        assert_eq!(present_tokens[3], "2");
        assert_eq!(present_tokens[4], "2");
        assert_eq!(present_tokens[5], "RGBA8888");
        assert_eq!(present_tokens[6], "16");
        assert!(present_tokens[1].parse::<u64>().is_ok());
        assert!(present_tokens[2].parse::<u64>().is_ok());
        assert!(present_tokens[7].parse::<u32>().is_ok());

        let get = execute_command(&engine, "RENDER_PRESENT_GET");
        assert_eq!(get.response_line, present.response_line);

        let dump = execute_command(&engine, "RENDER_DUMP_PPM");
        assert!(dump.response_line.starts_with("OK "));
        let dump_path = dump.response_line.trim_start_matches("OK ").trim();
        assert!(std::path::Path::new(dump_path).exists());
    }

    #[test]
    fn render_present_without_frame_is_out_of_range() {
        let engine = RuntimeEngine::default();
        let present = execute_command(&engine, "RENDER_PRESENT");
        assert_eq!(present.response_line, "ERR OUT_OF_RANGE");
        let get = execute_command(&engine, "RENDER_PRESENT_GET");
        assert_eq!(get.response_line, "ERR OUT_OF_RANGE");
    }
}
