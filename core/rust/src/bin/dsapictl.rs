use std::os::unix::net::UnixStream;

use directscreen_core::util::timeout_from_env;

use directscreen_core::util::ctl_wire;

fn usage() {
    println!("usage:");
    println!("  dsapictl [--socket <path>] <COMMAND ...>");
    println!("supported commands:");
    println!("  PING");
    println!("  VERSION");
    println!("  DISPLAY_GET");
    println!("  DISPLAY_WAIT <last_seq> <timeout_ms>");
    println!("  DISPLAY_SET <w> <h> <hz> <dpi> <rotation>");
    println!("  TOUCH_CLEAR");
    println!("  TOUCH_COUNT");
    println!("  TOUCH_MOVE <pointer_id> <x> <y>");
    println!("  RENDER_SUBMIT <draw_calls> <frost_passes> <text_calls>");
    println!("  RENDER_GET");
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

    if line.starts_with("OK") {
        std::process::exit(0);
    }

    std::process::exit(5);
}
