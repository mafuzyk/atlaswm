use std::path::Path;

use atlas_config::parse_config;
use atlas_core::backend::winit;

fn main() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().init();
    }

    let config_path = Path::new("atlas.kdl");
    let deco_config = if config_path.exists() {
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
                Some(cfg.decoration)
            }
            Err(e) => {
                tracing::warn!("Failed to parse atlas.kdl: {e}, using defaults");
                None
            }
        }
    } else {
        tracing::info!("No atlas.kdl found, using default config");
        None
    };

    if let Err(e) = winit::run_winit(deco_config) {
        tracing::error!("Compositor exited with error: {}", e);
    }
}
