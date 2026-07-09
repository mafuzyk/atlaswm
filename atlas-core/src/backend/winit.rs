use smithay::{
    backend::{
        input::{InputEvent, KeyboardKeyEvent},
        renderer::{
            element::surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            gles::GlesRenderer,
            utils::draw_render_elements,
            Color32F, Frame, Renderer,
        },
        winit::{self, WinitEvent},
    },
    input::keyboard::FilterResult,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::EventLoop,
        wayland_server::Display,
        winit::event_loop::pump_events::PumpStatus,
    },
    utils::{Rectangle, Transform},
    wayland::compositor::{SurfaceAttributes, TraversalAction, with_surface_tree_downward},
};

use tracing::error;

use crate::state::AtlasState;

fn send_frames_surface_tree(surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface, time: u32) {
    with_surface_tree_downward(
        surface,
        (),
        |_, _, &()| TraversalAction::DoChildren(()),
        |_surf, states, &()| {
            for callback in states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .frame_callbacks
                .drain(..)
            {
                callback.done(time);
            }
        },
        |_, _, &()| true,
    );
}

pub fn run_winit() -> Result<(), Box<dyn std::error::Error>> {
    let _event_loop: EventLoop<AtlasState> = EventLoop::try_new()?;
    let mut display: Display<AtlasState> = Display::new()?;
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

    let mut state = AtlasState {
        display_handle: dh.clone(),
        compositor_state,
        xdg_shell_state: smithay::wayland::shell::xdg::XdgShellState::new::<AtlasState>(&dh),
        shm_state,
        seat_state,
        data_device_state,
        seat,
        output,
        running: true,
    };

    tracing::info!("Initialization completed, starting the main loop.");

    let keyboard = state
        .seat
        .add_keyboard(smithay::input::keyboard::XkbConfig::default(), 200, 200)
        .map_err(|e| format!("Failed to initialize keyboard: {}", e))?;

    let start_time = std::time::Instant::now();

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

        let size = backend.window_size();
        let damage = Rectangle::from_size(size);

        {
            let (renderer, mut framebuffer) = match backend.bind() {
                Ok(ret) => ret,
                Err(err) => {
                    error!("Failed to bind renderer: {}", err);
                    break;
                }
            };

            let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = state
                .xdg_shell_state
                .toplevel_surfaces()
                .iter()
                .flat_map(|surface| {
                    render_elements_from_surface_tree(
                        renderer,
                        surface.wl_surface(),
                        (0, 0),
                        1.0,
                        1.0,
                        smithay::backend::renderer::element::Kind::Unspecified,
                    )
                })
                .collect();

            let mut frame = renderer
                .render(&mut framebuffer, size, Transform::Flipped180)
                .map_err(|e| format!("Render error: {}", e))?;

            frame
                .clear(Color32F::new(0.1, 0.0, 0.0, 1.0), &[damage])
                .map_err(|e| format!("Clear error: {}", e))?;

            draw_render_elements(&mut frame, 1.0, &elements, &[damage])
                .map_err(|e| format!("Draw error: {}", e))?;

            let _ = frame.finish().map_err(|e| format!("Finish error: {}", e))?;

            for surface in state.xdg_shell_state.toplevel_surfaces() {
                send_frames_surface_tree(
                    surface.wl_surface(),
                    start_time.elapsed().as_millis() as u32,
                );
            }

            display.dispatch_clients(&mut state)?;
            display.flush_clients()?;
        }

        backend
            .submit(Some(&[damage]))
            .map_err(|e| format!("Submit error: {}", e))?;
    }

    Ok(())
}
