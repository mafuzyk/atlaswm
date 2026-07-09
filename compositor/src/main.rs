use atlas_core::backend::winit;

fn main() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().init();
    }

    if let Err(e) = winit::run_winit() {
        tracing::error!("Compositor exited with error: {}", e);
    }
}
