use std::os::unix::net::UnixStream;

use directscreen_core::util::timeout_from_env;
use directscreen_core::api::Status;

use directscreen_core::util::ctl_wire;

fn usage() {
    println!("usage:");
    println!("  dsapictl [--socket <path>] <COMMAND ...>");
    println!("supported commands:");
    println!("  PING");
    println!("  VERSION");
    println!("  READY");
    println!("  DISPLAY_GET");
    println!("  DISPLAY_WAIT <last_seq> <timeout_ms>");
    println!("  DISPLAY_SET <w> <h> <hz> <dpi> <rotation>");
    println!("  TOUCH_CLEAR");
    println!("  TOUCH_COUNT");
    println!("  TOUCH_MOVE <pointer_id> <x> <y>");
    println!("  RENDER_SUBMIT <draw_calls> <frost_passes> <text_calls>");
    println!("  RENDER_GET");
    println!("  FILTER_SET_GAUSSIAN <radius> <sigma>");
    println!("  FILTER_CHAIN_SET <count> <radius1> <sigma1> ...");
    println!("  FILTER_CLEAR");
    println!("  FILTER_GET");
    println!("  KEYBOARD_CHAR <codepoint|single_char>");
    println!("  KEYBOARD_BACKSPACE");
    println!("  KEYBOARD_DONE");
    println!("  KEYBOARD_FOCUS_ON");
    println!("  KEYBOARD_FOCUS_OFF");
    println!("  KEYBOARD_INJECT <kind> <codepoint>");
    println!("  KEYBOARD_WAIT <last_seq> <timeout_ms>");
    println!("  RENDER_FRAME_BIND_SHM");
    println!("  RENDER_FRAME_WAIT_SHM_PRESENT <last_seq> <timeout_ms>");
    println!("  RENDER_FRAME_SUBMIT_SHM <w> <h> <byte_len> <offset> [origin_x origin_y]");
    println!("  MODULE_SYNC");
    println!("  MODULE_LIST");
    println!("  MODULE_STATUS <id>");
    println!("  MODULE_DETAIL <id>");
    println!("  MODULE_START <id> [package user]");
    println!("  MODULE_STOP <id>");
    println!("  MODULE_RELOAD <id> [package user]");
    println!("  MODULE_RELOAD_ALL [package user]");
    println!("  MODULE_DISABLE <id>");
    println!("  MODULE_ENABLE <id>");
    println!("  MODULE_REMOVE <id>");
    println!("  MODULE_ACTION_LIST <id>");
    println!("  MODULE_ACTION_RUN <id> <action> [package user]");
    println!("  MODULE_ENV_LIST <id>");
    println!("  MODULE_ENV_SET <id> <key> <value>");
    println!("  MODULE_ENV_UNSET <id> <key>");
    println!("  MODULE_SCOPE_LIST [id]");
    println!("  MODULE_SCOPE_SET <id> <package|*> <user|-1> <allow|deny>");
    println!("  MODULE_SCOPE_CLEAR <id>");
    println!("  SHUTDOWN");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        std::process::exit(1);
    }

    let mut socket = directscreen_core::util::default_control_socket_path();
    let mut cmd_start = 1usize;
    if args.len() >= 4 && args[1] == "--socket" {
        socket = args[2].clone();
        cmd_start = 3;
    }

    if cmd_start >= args.len() {
        usage();
        std::process::exit(1);
    }

    let cmd_tokens: Vec<&str> = args[cmd_start..].iter().map(String::as_str).collect();
    let cmd = match ctl_wire::parse_command_tokens(&cmd_tokens) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("ctl_error=command_parse_failed reason={}", e);
            std::process::exit(1);
        }
    };

    let mut stream = match UnixStream::connect(&socket) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ctl_error=connect_failed socket={} err={}", socket, e);
            std::process::exit(2);
        }
    };
    let timeout = timeout_from_env("DSAPI_CLIENT_TIMEOUT_MS", 5000, 100);
    if let Err(e) = stream.set_read_timeout(timeout) {
        eprintln!("ctl_error=set_read_timeout_failed err={}", e);
        std::process::exit(6);
    }
    if let Err(e) = stream.set_write_timeout(timeout) {
        eprintln!("ctl_error=set_write_timeout_failed err={}", e);
        std::process::exit(6);
    }

    if let Err(e) = ctl_wire::write_request(&mut stream, 1, &cmd) {
        eprintln!("ctl_error=write_failed err={}", e);
        std::process::exit(3);
    }

    let response = match ctl_wire::read_response(&mut stream) {
        Ok(Some(v)) => v,
        Ok(None) => {
            eprintln!("ctl_error=read_failed err=socket_closed");
            std::process::exit(4);
        }
        Err(e) => {
            eprintln!("ctl_error=read_failed err={}", e);
            std::process::exit(4);
        }
    };

    let line = ctl_wire::format_response(&cmd, &response);
    println!("{}", line);

    if response.status == Status::Ok {
        std::process::exit(0);
    }

    std::process::exit(5);
}
