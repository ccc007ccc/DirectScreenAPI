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

pub fn execute_command(engine: &mut RuntimeEngine, line: &str) -> CommandOutcome {
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
        let mut engine = RuntimeEngine::default();
        let ping = execute_command(&mut engine, "PING");
        assert_eq!(ping.response_line, "OK PONG");
        assert!(!ping.should_shutdown);

        let shutdown = execute_command(&mut engine, "SHUTDOWN");
        assert_eq!(shutdown.response_line, "OK SHUTDOWN");
        assert!(shutdown.should_shutdown);
    }

    #[test]
    fn touch_flow_commands_keep_pointer_state() {
        let mut engine = RuntimeEngine::default();
        let add = execute_command(&mut engine, "ROUTE_ADD_RECT 10 block 0 0 100 100");
        assert_eq!(add.response_line, "OK");

        let down = execute_command(&mut engine, "TOUCH_DOWN 1 10 10");
        assert_eq!(down.response_line, "OK 1 10");

        let move_cmd = execute_command(&mut engine, "TOUCH_MOVE 1 200 200");
        assert_eq!(move_cmd.response_line, "OK 1 10");

        let count = execute_command(&mut engine, "TOUCH_COUNT");
        assert_eq!(count.response_line, "OK 1");

        let up = execute_command(&mut engine, "TOUCH_UP 1 200 200");
        assert_eq!(up.response_line, "OK 1 10");

        let count_after = execute_command(&mut engine, "TOUCH_COUNT");
        assert_eq!(count_after.response_line, "OK 0");
    }
}
