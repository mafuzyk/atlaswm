use std::sync::Arc;
use std::time::Duration;

use smithay::{
    backend::{
        input::{InputEvent, KeyboardKeyEvent},
        renderer::{
            element::solid::SolidColorRenderElement,
            gles::GlesRenderer,
            Color32F,
        },
        winit::{self, WinitEvent},
    },
    desktop::space::render_output,
    input::keyboard::FilterResult,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{EventLoop, Interest, Mode as LoopMode, PostAction, generic::Generic},
        wayland_server::Display,
        winit::event_loop::pump_events::PumpStatus,
    },
    utils::Transform,
    wayland::{
        socket::ListeningSocketSource,
    },
};
use tracing::{error, info, warn};

use crate::state::{AtlasState, ClientState};

pub fn run_winit() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<AtlasState> = EventLoop::try_new()?;
    let display: Display<AtlasState> = Display::new()?;
    let dh = display.handle();

    let compositor_state = smithay::wayland::compositor::CompositorState::new::<AtlasState>(&dh);
    let shm_state = smithay::wayland::shm::ShmState::new::<AtlasState>(&dh, vec![]);
    let mut seat_state = smithay::input::SeatState::new();
    let seat = seat_state.new_wl_seat(&dh, "atlas");
    let data_device_state =
        smithay::wayland::selection::data_device::DataDeviceState::new::<AtlasState>(&dh);

    let (mut backend, mut winit) = winit::init::<GlesRenderer>()?;

    let size = backend.window_size();
    let output = Output::new(
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Atlas".into(),
            model: "Winit".into(),
            serial_number: "Unknown".into(),
        },
    );
    let mode = Mode {
        size,
        refresh: 60_000,
    };
    output.create_global::<AtlasState>(&dh);
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);

    let damage_tracker = smithay::backend::renderer::damage::OutputDamageTracker::from_output(&output);

    let xdg_shell_state = smithay::wayland::shell::xdg::XdgShellState::new::<AtlasState>(&dh);

    let socket_source = ListeningSocketSource::new_auto()?;
    let socket_name = socket_source.socket_name().to_string_lossy().into_owned();
    info!(name = socket_name, "Listening on wayland socket");

    event_loop.handle().insert_source(
        socket_source,
        |client_stream, _, data: &mut AtlasState| {
            if let Err(err) = data
                .display_handle
                .insert_client(client_stream, Arc::new(ClientState::default()))
            {
                warn!("Error adding wayland client: {}", err);
            }
        },
    )?;

    event_loop.handle().insert_source(
        Generic::new(display, Interest::READ, LoopMode::Level),
        |_, display, data| {
            unsafe {
                display.get_mut().dispatch_clients(data).unwrap();
            }
            Ok(PostAction::Continue)
        },
    )?;

    let mut space = smithay::desktop::Space::default();
    space.map_output(&output, (0, 0));

    let mut state = AtlasState {
        display_handle: dh.clone(),
        compositor_state,
        xdg_shell_state,
        shm_state,
        seat_state,
        data_device_state,
        seat,
        output,
        socket_name,
        space,
        damage_tracker,
        running: true,
    };

    info!("Initialization completed, starting the main loop.");

    let keyboard = state
        .seat
        .add_keyboard(smithay::input::keyboard::XkbConfig::default(), 200, 200)
        .map_err(|e| format!("Failed to initialize keyboard: {}", e))?;

    let start_time = std::time::Instant::now();
    let mut full_redraw: u8 = 4;

    while state.running {
        let status = winit.dispatch_new_events(|event| match event {
            WinitEvent::Resized { size, .. } => {
                let mode = Mode {
                    size,
                    refresh: 60_000,
                };
                state.output.change_current_state(Some(mode), None, None, None);
                state.output.set_preferred(mode);
            }
            WinitEvent::Input(event) => match event {
                InputEvent::Keyboard { event } => {
                    keyboard.input::<(), _>(
                        &mut state,
                        event.key_code(),
                        event.state(),
                        0.into(),
                        0,
                        |_, _, _| FilterResult::Forward,
                    );
                }
                InputEvent::PointerMotionAbsolute { .. } => {
                    if let Some(surface) = state
                        .xdg_shell_state
                        .toplevel_surfaces()
                        .iter()
                        .next()
                        .cloned()
                    {
                        let surface = surface.wl_surface().clone();
                        keyboard.set_focus(&mut state, Some(surface), 0.into());
                    }
                }
                _ => {}
            },
            _ => (),
        });

        match status {
            PumpStatus::Continue => (),
            PumpStatus::Exit(_) => {
                state.running = false;
                break;
            }
        }

        state.space.refresh();

        let age = if full_redraw > 0 {
            full_redraw -= 1;
            0
        } else {
            backend.buffer_age().unwrap_or(0)
        };

        let clear_color = Color32F::new(0.1, 0.0, 0.0, 1.0);

        // Rendering scope: framebuffer holds a borrow on backend
        let (damage_to_submit, frame_time) = {
            let (renderer, mut framebuffer) = match backend.bind() {
                Ok(ret) => ret,
                Err(err) => {
                    error!("Failed to bind renderer: {}", err);
                    break;
                }
            };

            let custom_elements: &[SolidColorRenderElement] = &[];

            let result = render_output(
                &state.output,
                renderer,
                &mut framebuffer,
                1.0,
                age,
                std::slice::from_ref(&state.space),
                custom_elements,
                &mut state.damage_tracker,
                clear_color,
            );

            let frame_time = start_time.elapsed();

            match result {
                Ok(render_output_result) => {
                    (render_output_result.damage.cloned(), frame_time)
                }
                Err(err) => {
                    warn!("Rendering error: {:?}", err);
                    (None, frame_time)
                }
            }
        }; // Drop framebuffer/renderer → backend borrow released

        if let Some(ref damage) = damage_to_submit {
            if !damage.is_empty() {
                if let Err(err) = backend.submit(Some(damage)) {
                    warn!("Failed to submit buffer: {}", err);
                }
            }
        }

        // Send frame events to clients
        let output_for_frames = state.output.clone();
        for window in state.space.elements() {
            if state.space.outputs_for_element(window).contains(&output_for_frames) {
                window.send_frame(
                    &output_for_frames,
                    frame_time,
                    None,
                    |_, _| Some(output_for_frames.clone()),
                );
            }
        }

        let result = event_loop.dispatch(Some(Duration::from_millis(1)), &mut state);
        if result.is_err() {
            error!("Event loop dispatch error");
            state.running = false;
            break;
        }

        if let Err(err) = state.display_handle.flush_clients() {
            warn!("Failed to flush clients: {:?}", err);
        }
    }

    Ok(())
}
