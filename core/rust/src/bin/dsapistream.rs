use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use directscreen_core::util::{default_control_socket_path, timeout_from_env};

struct SocketClient {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

impl SocketClient {
    fn connect(socket: &str) -> io::Result<Self> {
        let writer = UnixStream::connect(socket)?;
        let timeout = timeout_from_env("DSAPI_CLIENT_TIMEOUT_MS", 5000, 100);
        writer.set_read_timeout(timeout)?;
        writer.set_write_timeout(timeout)?;

        let reader_stream = writer.try_clone()?;
        reader_stream.set_read_timeout(timeout)?;
        reader_stream.set_write_timeout(timeout)?;
        let reader = BufReader::new(reader_stream);
        Ok(Self { writer, reader })
    }

    fn send_line(&mut self, line: &str) -> io::Result<String> {
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;

        let mut response = String::new();
        let n = self.reader.read_line(&mut response)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "socket_closed",
            ));
        }
        Ok(response.trim_end().to_string())
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

        let cmd = line.trim();
        if cmd.is_empty() {
            continue;
        }

        let mut retried = false;
        let response = loop {
            match client.send_line(cmd) {
                Ok(resp) => break resp,
                Err(e) => {
                    if retried {
                        eprintln!("stream_error=send_failed cmd={} err={}", cmd, e);
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
            eprintln!("stream_warn=daemon_error cmd={} response={}", cmd, response);
        }
    }
}
