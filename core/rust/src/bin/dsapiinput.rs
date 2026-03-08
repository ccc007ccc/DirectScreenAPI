use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::os::unix::net::UnixStream;

use directscreen_core::util::{
    default_control_socket_path, derive_data_socket_path_text, timeout_from_env,
};

const EV_SYN: u16 = 0x00;
const EV_ABS: u16 = 0x03;

const SYN_REPORT: u16 = 0;

const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;

const TOUCH_PACKET_BYTES: usize = 16;
const TOUCH_PACKET_DOWN: u8 = 1;
const TOUCH_PACKET_MOVE: u8 = 2;
const TOUCH_PACKET_UP: u8 = 3;
const TOUCH_PACKET_CLEAR: u8 = 5;

#[derive(Debug, Clone, Copy)]
struct Config {
    max_x: i32,
    max_y: i32,
    width: i32,
    height: i32,
    rotation: i32,
}

#[derive(Debug, Clone, Copy, Default)]
struct SlotState {
    active: bool,
    pointer_id: i32,
    raw_x: i32,
    raw_y: i32,
}

#[derive(Debug, Clone, Copy)]
struct PendingTouch {
    kind: u8,
    pointer_id: i32,
    raw_x: i32,
    raw_y: i32,
}

#[derive(Debug)]
struct Args {
    data_socket: String,
    device: String,
    cfg: Config,
    quiet: bool,
}

fn usage() {
    eprintln!("usage:");
    eprintln!(
        "  dsapiinput --device <event_path> --max-x <n> --max-y <n> --width <n> --height <n> [--rotation <0-3>] [--control-socket <path>] [--data-socket <path>] [--quiet]"
    );
}

fn parse_i32_arg(v: &str, key: &str) -> Result<i32, String> {
    v.parse::<i32>()
        .map_err(|_| format!("invalid_{}_value", key))
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut control_socket = default_control_socket_path();
    let mut data_socket: Option<String> = None;
    let mut device = String::new();
    let mut max_x = None::<i32>;
    let mut max_y = None::<i32>;
    let mut width = None::<i32>;
    let mut height = None::<i32>;
    let mut rotation = 0i32;
    let mut quiet = false;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--control-socket" => {
                if i + 1 >= args.len() {
                    return Err("missing_control_socket_path".to_string());
                }
                control_socket = args[i + 1].clone();
                i += 2;
            }
            "--data-socket" => {
                if i + 1 >= args.len() {
                    return Err("missing_data_socket_path".to_string());
                }
                data_socket = Some(args[i + 1].clone());
                i += 2;
            }
            "--device" => {
                if i + 1 >= args.len() {
                    return Err("missing_device_path".to_string());
                }
                device = args[i + 1].clone();
                i += 2;
            }
            "--max-x" => {
                if i + 1 >= args.len() {
                    return Err("missing_max_x".to_string());
                }
                max_x = Some(parse_i32_arg(&args[i + 1], "max_x")?);
                i += 2;
            }
            "--max-y" => {
                if i + 1 >= args.len() {
                    return Err("missing_max_y".to_string());
                }
                max_y = Some(parse_i32_arg(&args[i + 1], "max_y")?);
                i += 2;
            }
            "--width" => {
                if i + 1 >= args.len() {
                    return Err("missing_width".to_string());
                }
                width = Some(parse_i32_arg(&args[i + 1], "width")?);
                i += 2;
            }
            "--height" => {
                if i + 1 >= args.len() {
                    return Err("missing_height".to_string());
                }
                height = Some(parse_i32_arg(&args[i + 1], "height")?);
                i += 2;
            }
            "--rotation" => {
                if i + 1 >= args.len() {
                    return Err("missing_rotation".to_string());
                }
                rotation = parse_i32_arg(&args[i + 1], "rotation")?;
                i += 2;
            }
            "--quiet" => {
                quiet = true;
                i += 1;
            }
            _ => return Err(format!("unknown_arg:{}", args[i])),
        }
    }

    if device.is_empty() {
        return Err("missing_device".to_string());
    }

    let cfg = Config {
        max_x: max_x.ok_or_else(|| "missing_max_x".to_string())?,
        max_y: max_y.ok_or_else(|| "missing_max_y".to_string())?,
        width: width.ok_or_else(|| "missing_width".to_string())?,
        height: height.ok_or_else(|| "missing_height".to_string())?,
        rotation,
    };

    if cfg.max_x <= 0 || cfg.max_y <= 0 || cfg.width <= 0 || cfg.height <= 0 {
        return Err("mapping_values_must_be_positive".to_string());
    }
    if !(0..=3).contains(&cfg.rotation) {
        return Err("rotation_must_be_0_3".to_string());
    }

    let data_socket = data_socket.unwrap_or_else(|| derive_data_socket_path_text(&control_socket));

    Ok(Args {
        data_socket,
        device,
        cfg,
        quiet,
    })
}

fn connect_touch_stream(socket: &str) -> io::Result<BufWriter<UnixStream>> {
    let mut stream = UnixStream::connect(socket)?;
    let timeout = timeout_from_env("DSAPI_CLIENT_TIMEOUT_MS", 5000, 100);
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)?;

    stream.write_all(b"STREAM_TOUCH_V1\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "touch_stream_handshake_eof",
        ));
    }

    let trimmed = line.trim();
    if trimmed != "OK STREAM_TOUCH_V1" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("touch_stream_handshake_bad:{}", trimmed),
        ));
    }

    Ok(BufWriter::new(stream))
}

fn clamp01(v: f32) -> f32 {
    if v < 0.0 {
        return 0.0;
    }
    if v > 1.0 {
        return 1.0;
    }
    v
}

fn map_point(cfg: &Config, raw_x: i32, raw_y: i32) -> (f32, f32) {
    let ux = clamp01(raw_x as f32 / cfg.max_x as f32);
    let uy = clamp01(raw_y as f32 / cfg.max_y as f32);

    let (lx, ly) = match cfg.rotation {
        1 => (uy, 1.0 - ux),
        2 => (1.0 - ux, 1.0 - uy),
        3 => (1.0 - uy, ux),
        _ => (ux, uy),
    };

    (
        lx * (cfg.width.saturating_sub(1)) as f32,
        ly * (cfg.height.saturating_sub(1)) as f32,
    )
}

fn write_touch_packet(
    writer: &mut BufWriter<UnixStream>,
    packet: PendingTouch,
    cfg: &Config,
) -> io::Result<()> {
    let (x, y) = map_point(cfg, packet.raw_x, packet.raw_y);
    let mut out = [0u8; TOUCH_PACKET_BYTES];
    out[0] = packet.kind;
    out[4..8].copy_from_slice(&packet.pointer_id.to_le_bytes());
    out[8..12].copy_from_slice(&x.to_le_bytes());
    out[12..16].copy_from_slice(&y.to_le_bytes());
    writer.write_all(&out)
}

fn read_input_event(file: &mut File) -> io::Result<libc::input_event> {
    let mut ev = std::mem::MaybeUninit::<libc::input_event>::uninit();
    let ev_bytes = unsafe {
        std::slice::from_raw_parts_mut(
            ev.as_mut_ptr() as *mut u8,
            std::mem::size_of::<libc::input_event>(),
        )
    };
    file.read_exact(ev_bytes)?;
    Ok(unsafe { ev.assume_init() })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg = match parse_args(&args) {
        Ok(v) => v,
        Err(e) => {
            usage();
            eprintln!("input_bridge_error=parse_args_failed reason={}", e);
            std::process::exit(2);
        }
    };

    let mut input = match File::open(&cfg.device) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "input_bridge_error=open_device_failed device={} err={}",
                cfg.device, e
            );
            std::process::exit(3);
        }
    };

    let mut writer = match connect_touch_stream(&cfg.data_socket) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "input_bridge_error=daemon_connect_failed socket={} err={}",
                cfg.data_socket, e
            );
            std::process::exit(4);
        }
    };

    if let Err(e) = writer.write_all(&{
        let mut out = [0u8; TOUCH_PACKET_BYTES];
        out[0] = TOUCH_PACKET_CLEAR;
        out
    }) {
        eprintln!("input_bridge_error=touch_clear_failed err={}", e);
        std::process::exit(5);
    }
    if let Err(e) = writer.flush() {
        eprintln!("input_bridge_error=touch_clear_flush_failed err={}", e);
        std::process::exit(6);
    }

    if !cfg.quiet {
        eprintln!(
            "input_bridge_status=running device={} max_x={} max_y={} width={} height={} rotation={}",
            cfg.device, cfg.cfg.max_x, cfg.cfg.max_y, cfg.cfg.width, cfg.cfg.height, cfg.cfg.rotation
        );
    }

    let mut cur_slot: i32 = 0;
    let mut slots: HashMap<i32, SlotState> = HashMap::new();
    let mut pending: BTreeMap<i32, PendingTouch> = BTreeMap::new();

    loop {
        let ev = match read_input_event(&mut input) {
            Ok(v) => v,
            Err(e) => {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    break;
                }
                eprintln!("input_bridge_error=read_event_failed err={}", e);
                std::process::exit(7);
            }
        };

        match ev.type_ {
            EV_ABS => match ev.code {
                ABS_MT_SLOT => {
                    cur_slot = ev.value;
                }
                ABS_MT_TRACKING_ID => {
                    let slot = slots.entry(cur_slot).or_default();
                    if ev.value < 0 {
                        if slot.active {
                            pending.insert(
                                cur_slot,
                                PendingTouch {
                                    kind: TOUCH_PACKET_UP,
                                    pointer_id: slot.pointer_id,
                                    raw_x: slot.raw_x,
                                    raw_y: slot.raw_y,
                                },
                            );
                        }
                        slot.active = false;
                        slot.pointer_id = -1;
                    } else {
                        slot.active = true;
                        slot.pointer_id = ev.value;
                        pending.insert(
                            cur_slot,
                            PendingTouch {
                                kind: TOUCH_PACKET_DOWN,
                                pointer_id: slot.pointer_id,
                                raw_x: slot.raw_x,
                                raw_y: slot.raw_y,
                            },
                        );
                    }
                }
                ABS_MT_POSITION_X => {
                    let slot = slots.entry(cur_slot).or_default();
                    slot.raw_x = ev.value;
                    if slot.active {
                        pending
                            .entry(cur_slot)
                            .and_modify(|p| p.raw_x = slot.raw_x)
                            .or_insert(PendingTouch {
                                kind: TOUCH_PACKET_MOVE,
                                pointer_id: slot.pointer_id,
                                raw_x: slot.raw_x,
                                raw_y: slot.raw_y,
                            });
                    }
                }
                ABS_MT_POSITION_Y => {
                    let slot = slots.entry(cur_slot).or_default();
                    slot.raw_y = ev.value;
                    if slot.active {
                        pending
                            .entry(cur_slot)
                            .and_modify(|p| p.raw_y = slot.raw_y)
                            .or_insert(PendingTouch {
                                kind: TOUCH_PACKET_MOVE,
                                pointer_id: slot.pointer_id,
                                raw_x: slot.raw_x,
                                raw_y: slot.raw_y,
                            });
                    }
                }
                _ => {}
            },
            EV_SYN if ev.code == SYN_REPORT => {
                for p in pending.values() {
                    if p.pointer_id < 0 {
                        continue;
                    }
                    if let Err(e) = write_touch_packet(&mut writer, *p, &cfg.cfg) {
                        eprintln!("input_bridge_error=write_packet_failed err={}", e);
                        std::process::exit(8);
                    }
                }
                if !pending.is_empty() {
                    if let Err(e) = writer.flush() {
                        eprintln!("input_bridge_error=flush_failed err={}", e);
                        std::process::exit(9);
                    }
                }
                pending.clear();
            }
            _ => {}
        }
    }
}
