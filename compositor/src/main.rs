use std::path::Path;
use std::env;

use atlas_config::parse_config;
use atlas_core::backend::{winit, udev};

fn main() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().init();
    }

    let args: Vec<String> = env::args().collect();
    let backend = args.iter().find_map(|a| {
        if a.starts_with("--backend=") { Some(a.trim_start_matches("--backend=").to_string()) }
        else if a == "--winit" { Some("winit".into()) }
        else if a == "--tty-udev" { Some("udev".into()) }
        else { None }
    }).unwrap_or_else(|| "winit".into());

    let config_path = Path::new("atlas.kdl");
    let runtime_config = if config_path.exists() {
        match parse_config(config_path) {
            Ok(cfg) => {
                tracing::info!(
                    "Loaded config: canvas_bg={}, decoration=({}, {}, {}, {}), outputs={}, regions={}, bindings={}",
                    cfg.canvas.background_color,
                    cfg.decoration.border_width,
                    cfg.decoration.border_radius,
                    cfg.decoration.active_color,
                    cfg.decoration.inactive_color,
                    cfg.outputs.len(),
                    cfg.regions.len(),
                    cfg.bindings.len(),
                );
                cfg
            }
            Err(e) => {
                tracing::warn!("Failed to parse atlas.kdl: {e}, using defaults");
                atlas_config::RuntimeConfig::default()
            }
        }
    } else {
        tracing::info!("No atlas.kdl found, using default config");
        atlas_config::RuntimeConfig::default()
    };

    match backend.as_str() {
        "udev" | "tty-udev" => {
            tracing::info!("Starting Atlas with udev/DRM backend");
            udev::run_udev(runtime_config);
        }
        _ => {
            tracing::info!("Starting Atlas with winit backend");
            if let Err(e) = winit::run_winit(runtime_config) {
                tracing::error!("Compositor exited with error: {}", e);
            }
        }
    }
}
