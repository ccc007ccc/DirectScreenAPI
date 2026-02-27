use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

fn default_socket_path() -> String {
    "artifacts/run/dsapi.sock".to_string()
}

fn usage() {
    println!("usage:");
    println!("  dsapictl [--socket <path>] <COMMAND ...>");
    println!("example:");
    println!("  dsapictl PING");
    println!("  dsapictl DISPLAY_SET 1440 3168 120 640 0");
    println!("  dsapictl ROUTE_ADD_RECT 10 block 100 100 300 300");
    println!("  dsapictl ROUTE_POINT 120 140");
    println!("  dsapictl RENDER_SUBMIT 12 2 3");
    println!("  dsapictl RENDER_GET");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        std::process::exit(1);
    }

    let mut socket = default_socket_path();
    let mut cmd_start = 1usize;
    if args.len() >= 4 && args[1] == "--socket" {
        socket = args[2].clone();
        cmd_start = 3;
    }

    if cmd_start >= args.len() {
        usage();
        std::process::exit(1);
    }

    let cmd = args[cmd_start..].join(" ");

    let mut stream = match UnixStream::connect(&socket) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ctl_error=connect_failed socket={} err={}", socket, e);
            std::process::exit(2);
        }
    };

    if let Err(e) = stream.write_all(format!("{}\n", cmd).as_bytes()) {
        eprintln!("ctl_error=write_failed err={}", e);
        std::process::exit(3);
    }

    let mut response = String::new();
    let mut reader = BufReader::new(&stream);
    if let Err(e) = reader.read_line(&mut response) {
        eprintln!("ctl_error=read_failed err={}", e);
        std::process::exit(4);
    }

    let response = response.trim_end().to_string();
    println!("{}", response);

    if response.starts_with("OK") {
        std::process::exit(0);
    }

    std::process::exit(5);
}
