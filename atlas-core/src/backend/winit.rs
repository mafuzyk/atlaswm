use std::sync::Arc;
use std::time::Duration;

use smithay::{
    backend::{
        input::{
            AbsolutePositionEvent, ButtonState, InputBackend, InputEvent, KeyboardKeyEvent,
            PointerButtonEvent,
        },
        renderer::{
            element::{
                Kind,
                solid::{SolidColorBuffer, SolidColorRenderElement},
            },
            gles::GlesRenderer,
            Color32F,
        },
        winit::{self, WinitEvent},
    },
    desktop::{Window, layer_map_for_output, space::render_output},
    input::{
        keyboard::FilterResult,
        pointer::{ButtonEvent, MotionEvent, PointerHandle},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{EventLoop, Interest, Mode as LoopMode, PostAction, generic::Generic},
        wayland_server::Display,
        winit::event_loop::pump_events::PumpStatus,
        wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration_manager::Mode as SsdMode,
    },
    utils::{IsAlive, Logical, Physical, Point, Size, Transform},
    wayland::{
        socket::ListeningSocketSource,
    },
};
use tracing::{error, info, warn};

use atlas_config::DecorationConfig;
use atlas_space::{GlobalSpace, Size as GsSize, Point as GsPoint, Viewport};

use crate::state::{AtlasState, ClientState, GrabState, GrabKind};

const PAN_SPEED: f64 = 50.0;
const MOD_KEY_EVDEV: i32 = 125;
const KEY_ENTER: i32 = 28;
const KEY_Q: i32 = 16;
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const MIN_WIN_SIZE: f64 = 100.0;

/// ── Spatial helpers ──────────────────────────────────────────────

pub fn sync_space_with_viewport(
    state: &mut AtlasState,
    screen_size: smithay::utils::Size<i32, smithay::utils::Physical>,
) {
    let gs_size = GsSize::new(screen_size.w as f64, screen_size.h as f64);
    let visible = state
        .global_space
        .windows_visible_in(&state.viewport, gs_size);

    let mut mapped: Vec<u64> = Vec::with_capacity(visible.len());

    for (gid, screen_pos, _) in &visible {
        let sp = smithay::utils::Point::from((
            screen_pos.x as i32,
            screen_pos.y as i32,
        ));
        if let Some(window) = state.windows.get(gid) {
            if state.space.element_geometry(window).is_some() {
                state.space.relocate_element(window, sp);
            } else {
                state.space.map_element(window.clone(), sp, false);
            }
            mapped.push(*gid);
        }
    }

    let known: Vec<u64> = state.windows.keys().copied().collect();
    for gid in &known {
        if !mapped.contains(gid) {
            if let Some(window) = state.windows.get(gid) {
                if state.space.element_geometry(window).is_some() {
                    state.space.unmap_elem(window);
                }
            }
        }
    }
}

fn screen_to_canvas(state: &AtlasState, phys: Point<f64, Physical>) -> GsPoint {
    state
        .global_space
        .screen_to_canvas(GsPoint::new(phys.x, phys.y), &state.viewport)
}

fn surface_from_window(window: &Window) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
    window.toplevel().map(|t| t.wl_surface().clone())
}

fn find_gid(state: &AtlasState, window: &Window) -> Option<u64> {
    state.windows.iter().find_map(|(gid, w)| {
        if std::ptr::eq(w as *const Window, window as *const Window) {
            Some(*gid)
        } else {
            None
        }
    })
}

pub fn hex_to_color32f(hex: &str) -> Color32F {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f32 / 255.0;
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f32 / 255.0;
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f32 / 255.0;
    Color32F::new(r, g, b, 1.0)
}

/// Build border `SolidColorRenderElement`s for every window currently
/// mapped in the Smithay Space.
pub fn build_border_elements(
    state: &AtlasState,
) -> Vec<SolidColorRenderElement> {
    let mut elements = Vec::new();
    let border = state.deco_config.border_width;
    let focused = hex_to_color32f(&state.deco_config.active_color);
    let unfocused = hex_to_color32f(&state.deco_config.inactive_color);

    for (gid, window) in &state.windows {
        if let Some(geo) = state.space.element_geometry(window) {
            let color = if state.focused_gid == Some(*gid) {
                focused
            } else {
                unfocused
            };
            let (x, y, w, h) = (
                geo.loc.x as f64,
                geo.loc.y as f64,
                geo.size.w as f64,
                geo.size.h as f64,
            );

            let bw = w + 2.0 * border;

            // top
            let mut buf = SolidColorBuffer::new((bw as i32, border as i32), color);
            elements.push(SolidColorRenderElement::from_buffer(
                &buf, Point::from(((x - border) as i32, (y - border) as i32)),
                1.0, 1.0, Kind::Unspecified,
            ));
            // bottom
            buf = SolidColorBuffer::new((bw as i32, border as i32), color);
            elements.push(SolidColorRenderElement::from_buffer(
                &buf, Point::from(((x - border) as i32, (y + h) as i32)),
                1.0, 1.0, Kind::Unspecified,
            ));
            // left
            buf = SolidColorBuffer::new((border as i32, h as i32), color);
            elements.push(SolidColorRenderElement::from_buffer(
                &buf, Point::from(((x - border) as i32, y as i32)),
                1.0, 1.0, Kind::Unspecified,
            ));
            // right
            buf = SolidColorBuffer::new((border as i32, h as i32), color);
            elements.push(SolidColorRenderElement::from_buffer(
                &buf, Point::from(((x + w) as i32, y as i32)),
                1.0, 1.0, Kind::Unspecified,
            ));
        }
    }
    elements
}

/// Truncate dead layer surfaces from the list and unmap from the output's LayerMap.
pub fn prune_layer_surfaces(state: &mut AtlasState) {
    let outputs: Vec<_> = state.space.outputs().cloned().collect();
    state.layer_surfaces.retain(|s| {
        if !s.alive() {
            for o in &outputs {
                let mut map = layer_map_for_output(o);
                map.unmap_layer(s);
            }
            false
        } else {
            true
        }
    });
}

/// ── Keyboard ─────────────────────────────────────────────────────

pub fn spawn_terminal() {
    for cmd in &["fish", "gnome-terminal", "alacritty", "kitty", "foot", "weston-terminal"] {
        if std::process::Command::new("which")
            .arg(cmd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        {
            let _ = std::process::Command::new(cmd)
                .spawn()
                .map(|_| info!("Spawned terminal: {}", cmd));
            return;
        }
    }
    let _ = std::process::Command::new("xterm")
        .env("WAYLAND_DISPLAY", std::env::var("WAYLAND_DISPLAY").unwrap_or_default())
        .spawn()
        .map(|_| info!("Spawned xterm"));
}

pub fn handle_keyboard_event<B: InputBackend>(
    state: &mut AtlasState,
    event: &impl KeyboardKeyEvent<B>,
    keyboard: &smithay::input::keyboard::KeyboardHandle<AtlasState>,
    evdev_code: i32,
) {
    let pressed = event.state() == smithay::backend::input::KeyState::Pressed;
    let evdev = evdev_code;

    if evdev == MOD_KEY_EVDEV {
        state.mod_pressed = pressed;
    }

    // ── Keybinds ─────────────────────────────────────────────────
    if pressed && state.mod_pressed {
        match evdev {
            KEY_ENTER => { spawn_terminal(); return; }
            KEY_Q => {
                if let Some(gid) = state.focused_gid {
                    if let Some(window) = state.windows.get(&gid) {
                        if let Some(toplevel) = window.toplevel() {
                            toplevel.send_close();
                            info!("Sent close to window {}", gid);
                        }
                    }
                }
                return;
            }
            _ => {}
        }
    }

    // ── Camera pan (plain arrows) ────────────────────────────────
    if pressed && !state.mod_pressed {
        match evdev {
            103 => state.viewport.y -= PAN_SPEED / state.viewport.zoom,
            108 => state.viewport.y += PAN_SPEED / state.viewport.zoom,
            105 => state.viewport.x -= PAN_SPEED / state.viewport.zoom,
            106 => state.viewport.x += PAN_SPEED / state.viewport.zoom,
            _ => {}
        }
    }

    // ── Mod+Arrow nudge ──────────────────────────────────────────
    if pressed && state.mod_pressed {
        if let Some(focused_gid) = state.focused_gid {
            let step = 20.0;
            if let Some(p) = state.global_space.window_position(focused_gid) {
                let new_pos = match evdev {
                    103 => GsPoint::new(p.x, p.y - step),
                    108 => GsPoint::new(p.x, p.y + step),
                    105 => GsPoint::new(p.x - step, p.y),
                    106 => GsPoint::new(p.x + step, p.y),
                    _ => p,
                };
                state.global_space.move_window(focused_gid, new_pos);
            }
        }
    }

    keyboard.input::<(), _>(
        state, event.key_code(), event.state(), 0.into(), 0,
        |_, _, _| FilterResult::Forward,
    );
}

/// ── Pointer motion ───────────────────────────────────────────────

pub fn handle_motion_event(
    state: &mut AtlasState,
    pointer: &PointerHandle<AtlasState>,
    phys: Point<f64, Physical>,
    logical: Point<f64, Logical>,
) {
    state.pointer_location = phys;

    let grab_update = state.grab.as_ref().map(|g| {
        let current_canvas = screen_to_canvas(state, phys);
        let dx = current_canvas.x - g.grab_anchor.x;
        let dy = current_canvas.y - g.grab_anchor.y;

        match g.kind {
            GrabKind::Move => (g.window_id, g.initial_window_pos.x + dx, g.initial_window_pos.y + dy, None),
            GrabKind::Resize => {
                let nw = (g.initial_window_size.width + dx).max(MIN_WIN_SIZE);
                let nh = (g.initial_window_size.height + dy).max(MIN_WIN_SIZE);
                (g.window_id, g.initial_window_pos.x, g.initial_window_pos.y, Some((nw, nh)))
            }
        }
    });

    if let Some((gid, nx, ny, resize_opt)) = grab_update {
        state.global_space.move_window(gid, GsPoint::new(nx, ny));
        if let Some((nw, nh)) = resize_opt {
            let ns = GsSize::new(nw, nh);
            state.global_space.resize_window(gid, ns);
            if let Some(window) = state.windows.get(&gid) {
                if let Some(toplevel) = window.toplevel() {
                    toplevel.with_pending_state(|s| {
                        s.size = Some(Size::from((nw as i32, nh as i32)));
                    });
                    toplevel.send_configure();
                }
            }
        }
    }

    let surface = state
        .space
        .element_under(logical)
        .and_then(|(w, _)| surface_from_window(w));

    let focus = surface.map(|s| (s, logical));

    state.serial_counter += 1;
    let serial = state.serial_counter;

    pointer.motion(
        state,
        focus,
        &MotionEvent { location: logical, serial: serial.into(), time: 0 },
    );
    pointer.frame(state);
}

/// ── Pointer button ───────────────────────────────────────────────

pub fn handle_button_event(
    state: &mut AtlasState,
    pointer: &PointerHandle<AtlasState>,
    keyboard: &smithay::input::keyboard::KeyboardHandle<AtlasState>,
    is_press: bool,
    code: u32,
    btn_state: ButtonState,
    serial: u32,
) {
    let is_left = code == BTN_LEFT;
    let is_right = code == BTN_RIGHT;

    // ── Press (Mod+Click for drag/resize) ─────────────────────────
    if is_press && (is_left || is_right) && state.mod_pressed {
        let logical = Point::<f64, Logical>::from((state.pointer_location.x, state.pointer_location.y));
        let hit = state.space.element_under(logical).and_then(|(w, _)| find_gid(state, w));

        if let Some(gid) = hit {
            let canvas = screen_to_canvas(state, state.pointer_location);
            let win_pos = state.global_space.window_position(gid).unwrap_or(GsPoint::new(0.0, 0.0));
            let win_size = state.global_space.window_entry(gid).map(|e| e.size).unwrap_or(GsSize::new(800.0, 600.0));

            state.grab = Some(GrabState {
                kind: if is_left { GrabKind::Move } else { GrabKind::Resize },
                window_id: gid,
                initial_window_pos: win_pos,
                grab_anchor: canvas,
                initial_window_size: win_size,
            });
        }
    }

    // ── Release (end drag/resize) ─────────────────────────────────
    if !is_press && (is_left || is_right) {
        let grab_end = state.grab.as_ref().map(|g| {
            let current_canvas = screen_to_canvas(state, state.pointer_location);
            let dx = current_canvas.x - g.grab_anchor.x;
            let dy = current_canvas.y - g.grab_anchor.y;
            match g.kind {
                GrabKind::Move => (g.window_id, g.initial_window_pos.x + dx, g.initial_window_pos.y + dy, None),
                GrabKind::Resize => {
                    let nw = (g.initial_window_size.width + dx).max(MIN_WIN_SIZE);
                    let nh = (g.initial_window_size.height + dy).max(MIN_WIN_SIZE);
                    (g.window_id, g.initial_window_pos.x, g.initial_window_pos.y, Some((nw, nh)))
                }
            }
        });
        if let Some((gid, nx, ny, resize_opt)) = grab_end {
            state.global_space.move_window(gid, GsPoint::new(nx, ny));
            if let Some((nw, nh)) = resize_opt {
                state.global_space.resize_window(gid, GsSize::new(nw, nh));
                if let Some(window) = state.windows.get(&gid) {
                    if let Some(toplevel) = window.toplevel() {
                        toplevel.with_pending_state(|s| s.size = Some(Size::from((nw as i32, nh as i32))));
                        toplevel.send_configure();
                    }
                }
            }
        }
        state.grab = None;
    }

    // ── Click-to-focus (plain left click) ─────────────────────────
    if is_press && is_left && !state.mod_pressed {
        let logical = Point::<f64, Logical>::from((state.pointer_location.x, state.pointer_location.y));
        if let Some((window, _loc)) = state.space.element_under(logical) {
            let window_id = find_gid(state, window);
            state.focused_gid = window_id;
            if let Some(surface) = surface_from_window(window) {
                keyboard.set_focus(state, Some(surface), 0.into());
            }
            if let Some(gid) = window_id {
                if let Some(w) = state.windows.get(&gid) {
                    state.space.raise_element(w, true);
                }
                state.global_space.raise_window(gid);
            }
        }
    }

    let button_event = ButtonEvent { serial: serial.into(), time: 0, button: code, state: btn_state };
    pointer.button(state, &button_event);
    pointer.frame(state);
}

/// ── Main loop ────────────────────────────────────────────────────

pub fn run_winit(deco_config: Option<DecorationConfig>) -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<AtlasState> = EventLoop::try_new()?;
    let display: Display<AtlasState> = Display::new()?;
    let dh = display.handle();

    let compositor_state = smithay::wayland::compositor::CompositorState::new::<AtlasState>(&dh);
    let shm_state = smithay::wayland::shm::ShmState::new::<AtlasState>(&dh, vec![]);
    let mut seat_state = smithay::input::SeatState::new();
    let mut seat = seat_state.new_wl_seat(&dh, "atlas");
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
    let mode = Mode { size, refresh: 60_000 };
    output.create_global::<AtlasState>(&dh);
    output.change_current_state(Some(mode), Some(Transform::Flipped180), None, Some((0, 0).into()));
    output.set_preferred(mode);

    let damage_tracker = smithay::backend::renderer::damage::OutputDamageTracker::from_output(&output);
    let xdg_shell_state = smithay::wayland::shell::xdg::XdgShellState::new::<AtlasState>(&dh);

    // ── KDE Server-Side Decorations (SSD) ──────────────────────────
    let kde_decoration_state = smithay::wayland::shell::kde::decoration::KdeDecorationState::new::<AtlasState>(
        &dh, SsdMode::Server,
    );

    // ── Layer Shell (wlr-layer-shell) ──────────────────────────────
    let layer_shell_state = smithay::wayland::shell::wlr_layer::WlrLayerShellState::new::<AtlasState>(&dh);

    let socket_source = ListeningSocketSource::new_auto()?;
    let socket_name = socket_source.socket_name().to_string_lossy().into_owned();
    info!(name = socket_name, "Listening on wayland socket");

    event_loop.handle().insert_source(
        socket_source,
        |client_stream, _, data: &mut AtlasState| {
            if let Err(err) = data.display_handle.insert_client(client_stream, Arc::new(ClientState::default())) {
                warn!("Error adding wayland client: {}", err);
            }
        },
    )?;

    event_loop.handle().insert_source(
        Generic::new(display, Interest::READ, LoopMode::Level),
        |_, display, data| {
            unsafe { display.get_mut().dispatch_clients(data).unwrap(); }
            Ok(PostAction::Continue)
        },
    )?;

    let pointer: PointerHandle<AtlasState> = seat.add_pointer();

    let mut space = smithay::desktop::Space::default();
    space.map_output(&output, (0, 0));

    let global_space = GlobalSpace::new();
    let viewport = Viewport::new("winit");

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
        deco_config: deco_config.unwrap_or_default(),
        global_space,
        viewport,
        windows: std::collections::HashMap::new(),
        running: true,
        grab: None,
        pointer_location: Point::from((0.0f64, 0.0f64)),
        mod_pressed: false,
        ctrl_pressed: false,
        serial_counter: 0,
        focused_gid: None,
        cursor_status: smithay::input::pointer::CursorImageStatus::default_named(),
        kde_decoration_state,
        layer_shell_state,
        layer_surfaces: Vec::new(),
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
                let mode = Mode { size, refresh: 60_000 };
                state.output.change_current_state(Some(mode), None, None, None);
                state.output.set_preferred(mode);
            }
            WinitEvent::Input(event) => match event {
                InputEvent::Keyboard { event } => {
                    let evdev = event.key_code().raw() as i32 - 8;
                    handle_keyboard_event(&mut state, &event, &keyboard, evdev);
                }
                InputEvent::PointerMotionAbsolute { event } => {
                    let phys = Point::<f64, Physical>::from((event.x(), event.y()));
                    let logical = Point::<f64, Logical>::from((phys.x, phys.y));
                    handle_motion_event(&mut state, &pointer, phys, logical);
                }
                InputEvent::PointerButton { event } => {
                    state.serial_counter += 1;
                    let serial = state.serial_counter;
                    handle_button_event(
                        &mut state, &pointer, &keyboard,
                        event.state() == ButtonState::Pressed,
                        event.button_code(), event.state(), serial,
                    );
                }
                _ => {}
            },
            _ => (),
        });

        match status {
            PumpStatus::Continue => (),
            PumpStatus::Exit(_) => { state.running = false; break; }
        }

        // ── Spatial sync ──────────────────────────────────────────
        let screen_size = backend.window_size();
        sync_space_with_viewport(&mut state, screen_size);
        state.space.refresh();
        prune_layer_surfaces(&mut state);

        // ── Build border elements ──────────────────────────────────
        let border_elements = build_border_elements(&state);

        let age = if full_redraw > 0 { full_redraw -= 1; 0 } else { backend.buffer_age().unwrap_or(0) };

        // ── Render ────────────────────────────────────────────────
        let (damage_to_submit, frame_time) = {
            let (renderer, mut framebuffer) = match backend.bind() {
                Ok(ret) => ret,
                Err(err) => { error!("Failed to bind renderer: {}", err); break; }
            };

            // render_output automatically handles LayerMap layer surfaces
            // via space_render_elements (anchored to output, not affected by viewport).
            let result = render_output(
                &state.output,
                renderer,
                &mut framebuffer,
                1.0,
                age,
                std::slice::from_ref(&state.space),
                &border_elements,
                &mut state.damage_tracker,
                Color32F::new(0.1, 0.0, 0.0, 1.0),
            );

            let frame_time = start_time.elapsed();
            match result {
                Ok(r) => (r.damage.cloned(), frame_time),
                Err(err) => { warn!("Rendering error: {:?}", err); (None, frame_time) }
            }
        };

        if let Some(ref damage) = damage_to_submit {
            if !damage.is_empty() {
                if let Err(err) = backend.submit(Some(damage)) {
                    warn!("Failed to submit buffer: {}", err);
                }
            }
        }

        let output_for_frames = state.output.clone();
        for window in state.space.elements() {
            if state.space.outputs_for_element(window).contains(&output_for_frames) {
                window.send_frame(
                    &output_for_frames, frame_time, None,
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
