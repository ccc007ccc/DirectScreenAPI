use directscreen_core::api::{Decision, DisplayState, RectRegion, Status};
use directscreen_core::engine::RuntimeEngine;
use directscreen_core::DIRECTSCREEN_CORE_VERSION;

fn usage() {
    println!("usage:");
    println!("  dsapi version");
    println!("  dsapi display-get");
    println!("  dsapi display-set <w> <h> <hz> <dpi> <rotation>");
    println!("  dsapi route-point <x> <y> [rect:block|pass:id:x:y:w:h ...]");
}

fn parse_u32(s: &str) -> Result<u32, Status> {
    s.parse::<u32>().map_err(|_| Status::InvalidArgument)
}

fn parse_f32(s: &str) -> Result<f32, Status> {
    s.parse::<f32>().map_err(|_| Status::InvalidArgument)
}

fn parse_i32(s: &str) -> Result<i32, Status> {
    s.parse::<i32>().map_err(|_| Status::InvalidArgument)
}

fn parse_region(token: &str) -> Result<RectRegion, Status> {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() != 7 || parts[0] != "rect" {
        return Err(Status::InvalidArgument);
    }

    let decision = match parts[1] {
        "pass" => Decision::Pass,
        "block" => Decision::Block,
        _ => return Err(Status::InvalidArgument),
    };

    Ok(RectRegion {
        region_id: parse_i32(parts[2])?,
        decision,
        x: parse_f32(parts[3])?,
        y: parse_f32(parts[4])?,
        w: parse_f32(parts[5])?,
        h: parse_f32(parts[6])?,
    })
}

fn parse_or_exit_u32(label: &str, input: &str) -> u32 {
    match parse_u32(input) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("dsapi_error=invalid_{} value={}", label, input);
            std::process::exit(2);
        }
    }
}

fn parse_or_exit_f32(label: &str, input: &str) -> f32 {
    match parse_f32(input) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("dsapi_error=invalid_{} value={}", label, input);
            std::process::exit(2);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "version" => {
            println!("directscreen_core_version={}", DIRECTSCREEN_CORE_VERSION);
        }
        "display-get" => {
            let engine = RuntimeEngine::default();
            let d = engine.display_state();
            println!(
                "display={}x{}@{:.2} dpi={} rot={}",
                d.width, d.height, d.refresh_hz, d.density_dpi, d.rotation
            );
        }
        "display-set" => {
            if args.len() != 7 {
                usage();
                std::process::exit(1);
            }
            let display = DisplayState {
                width: parse_or_exit_u32("width", &args[2]),
                height: parse_or_exit_u32("height", &args[3]),
                refresh_hz: parse_or_exit_f32("refresh_hz", &args[4]),
                density_dpi: parse_or_exit_u32("density_dpi", &args[5]),
                rotation: parse_or_exit_u32("rotation", &args[6]),
            };

            let engine = RuntimeEngine::default();
            match engine.set_display_state(display) {
                Ok(()) => println!("status=0"),
                Err(e) => println!("status={}", e as i32),
            }
        }
        "route-point" => {
            if args.len() < 4 {
                usage();
                std::process::exit(1);
            }

            let x = parse_or_exit_f32("x", &args[2]);
            let y = parse_or_exit_f32("y", &args[3]);
            let engine = RuntimeEngine::default();
            engine.set_default_decision(Decision::Pass);

            for token in args.iter().skip(4) {
                match parse_region(token) {
                    Ok(region) => {
                        if let Err(e) = engine.add_region_rect(region) {
                            println!("status={}", e as i32);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        println!("status={}", e as i32);
                        std::process::exit(1);
                    }
                }
            }

            let routed = engine.route_point(x, y);
            println!("decision={}", routed.decision as i32);
            println!("region_id={}", routed.region_id);
        }
        _ => {
            usage();
            std::process::exit(1);
        }
    }
}
