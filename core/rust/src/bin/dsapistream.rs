use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

fn send_line(socket: &str, line: &str) -> io::Result<String> {
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    let n = reader.read_line(&mut response)?;
    if n == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "socket_closed",
        ));
    }
    Ok(response.trim_end().to_string())
}

fn default_socket_path() -> String {
    "artifacts/run/dsapi.sock".to_string()
}

fn usage() {
    println!("usage:");
    println!("  dsapistream [--socket <path>] [--quiet]");
    println!("read commands from stdin, write one response per input line");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut socket = default_socket_path();
    let mut quiet = false;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" => {
                if i + 1 >= args.len() {
                    usage();
                    std::process::exit(1);
                }
                socket = args[i + 1].clone();
                i += 2;
            }
            "--quiet" => {
                quiet = true;
                i += 1;
            }
            _ => {
                usage();
                std::process::exit(1);
            }
        }
    }

    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut line = String::new();

    loop {
        line.clear();
        let n = match input.read_line(&mut line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("stream_error=stdin_read_failed err={}", e);
                std::process::exit(3);
            }
        };
        if n == 0 {
            break;
        }

        let cmd = line.trim();
        if cmd.is_empty() {
            continue;
        }

        let response = match send_line(&socket, cmd) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("stream_error=send_failed cmd={} err={}", cmd, e);
                std::process::exit(4);
            }
        };

        if !quiet {
            println!("{}", response);
        } else if !response.starts_with("OK") {
            eprintln!("stream_warn=daemon_error cmd={} response={}", cmd, response);
        }
    }
}
