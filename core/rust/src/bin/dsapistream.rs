use std::io::{self, BufRead};
use std::os::unix::net::UnixStream;

use directscreen_core::util::{default_control_socket_path, timeout_from_env};

use directscreen_core::util::ctl_wire;

struct SocketClient {
    stream: UnixStream,
    seq: u64,
}

impl SocketClient {
    fn connect(socket: &str) -> io::Result<Self> {
        let stream = UnixStream::connect(socket)?;
        let timeout = timeout_from_env("DSAPI_CLIENT_TIMEOUT_MS", 5000, 100);
        stream.set_read_timeout(timeout)?;
        stream.set_write_timeout(timeout)?;
        Ok(Self { stream, seq: 1 })
    }

    fn send_command(&mut self, cmd: &ctl_wire::BinaryCtlCommand) -> io::Result<String> {
        ctl_wire::write_request(&mut self.stream, self.seq, cmd)?;
        self.seq = self.seq.saturating_add(1);

        let Some(resp) = ctl_wire::read_response(&mut self.stream)? else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "socket_closed",
            ));
        };
        Ok(ctl_wire::format_response(cmd, &resp))
    }
}

fn usage() {
    println!("usage:");
    println!("  dsapistream [--control-socket <path>] [--quiet]");
    println!("read commands from stdin, write one response per input line");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut control_socket = default_control_socket_path();
    let mut quiet = false;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--control-socket" => {
                if i + 1 >= args.len() {
                    usage();
                    std::process::exit(1);
                }
                control_socket = args[i + 1].clone();
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

    let mut client = match SocketClient::connect(&control_socket) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "stream_error=connect_failed control_socket={} err={}",
                control_socket, e
            );
            std::process::exit(2);
        }
    };

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

        let cmd_line = line.trim();
        if cmd_line.is_empty() {
            continue;
        }

        let cmd = match ctl_wire::parse_command_line(cmd_line) {
            Ok(v) => v,
            Err(e) => {
                let msg = format!("ERR INVALID_ARGUMENT {}", e);
                if !quiet {
                    println!("{}", msg);
                } else {
                    eprintln!("stream_warn=parse_failed cmd={} reason={}", cmd_line, e);
                }
                continue;
            }
        };

        let mut retried = false;
        let response = loop {
            match client.send_command(&cmd) {
                Ok(resp) => break resp,
                Err(e) => {
                    if retried {
                        eprintln!("stream_error=send_failed cmd={} err={}", cmd_line, e);
                        std::process::exit(4);
                    }
                    retried = true;
                    client = match SocketClient::connect(&control_socket) {
                        Ok(v) => v,
                        Err(conn_err) => {
                            eprintln!(
                                "stream_error=reconnect_failed control_socket={} err={}",
                                control_socket, conn_err
                            );
                            std::process::exit(5);
                        }
                    };
                }
            }
        };

        if !quiet {
            println!("{}", response);
        } else if !response.starts_with("OK") {
            eprintln!(
                "stream_warn=daemon_error cmd={} response={}",
                cmd_line, response
            );
        }
    }
}
