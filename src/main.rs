#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use stream_timer::app::{AppConfig, run_app};
use stream_timer::win32_stealth::{
    InstanceAcquisition, MinimizeMode, acquire_or_wake_existing, enable_per_monitor_dpi,
};

fn parse_args() -> (AppConfig, bool) {
    let mut cfg = AppConfig::default();
    let mut allow_multi = false;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                if let Some(val) = args.next()
                    && let Some(m) = MinimizeMode::parse(&val)
                {
                    cfg.minimize_mode = m;
                }
            }
            "--port" => {
                if let Some(val) = args.next()
                    && let Ok(p) = val.parse::<u16>()
                {
                    cfg.preferred_port = p;
                }
            }
            "--width" => {
                if let Some(val) = args.next()
                    && let Ok(w) = val.parse::<i32>()
                {
                    cfg.width = w.max(180);
                }
            }
            "--height" => {
                if let Some(val) = args.next()
                    && let Ok(h) = val.parse::<i32>()
                {
                    cfg.height = h.max(68);
                }
            }
            "--session-dir" => {
                if let Some(dir) = args.next() {
                    cfg.session_dir = Some(PathBuf::from(dir));
                }
            }
            "--allow-multi" => {
                allow_multi = true;
            }
            _ => {}
        }
    }

    (cfg, allow_multi)
}

fn main() {
    enable_per_monitor_dpi();
    let (config, allow_multi) = parse_args();

    let single_instance = if allow_multi {
        None
    } else {
        match acquire_or_wake_existing() {
            InstanceAcquisition::Primary(guard) => Some(guard),
            InstanceAcquisition::SecondarySignaled => return,
        }
    };

    if let Err(err) = run_app(config, single_instance) {
        eprintln!("StreamTimer error: {err:?}");
    }
}
