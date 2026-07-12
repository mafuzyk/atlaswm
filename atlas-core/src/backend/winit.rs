use std::sync::Arc;

use smithay::{
    backend::{
        input::{
            AbsolutePositionEvent, Axis, ButtonState, InputBackend, InputEvent, KeyboardKeyEvent,
            PointerAxisEvent, PointerButtonEvent,
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
    desktop::{Window, WindowSurfaceType, layer_map_for_output, space::render_output},
    input::{
        keyboard::FilterResult,
        pointer::{ButtonEvent, MotionEvent, PointerHandle},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{EventLoop, Interest, Mode as LoopMode, PostAction, generic::Generic},
        wayland_server::Display,
        wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration_manager::Mode as SsdMode,
    },
    utils::{IsAlive, Logical, Physical, Point, Size, Transform},
    wayland::{
        socket::ListeningSocketSource,
    },
};
use tracing::{error, info, warn};

use atlas_config::RuntimeConfig;
use atlas_space::{GlobalSpace, Size as GsSize, Point as GsPoint, Viewport};

use crate::state::{AtlasState, ClientState, GrabState, GrabKind};

const PAN_SPEED: f64 = 50.0;
const MOD_KEY_EVDEV: i32 = 125;
const KEY_ENTER: i32 = 28;
const KEY_U: i32 = 22;
const KEY_Q: i32 = 16;
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const MIN_WIN_SIZE: f64 = 100.0;
const ZOOM_FACTOR: f64 = 1.15;
const MIN_ZOOM: f64 = 0.05;
const MAX_ZOOM: f64 = 20.0;

/// ── Spatial helpers ──────────────────────────────────────────────

pub fn sync_space_with_viewport(
    state: &mut AtlasState,
    screen_size: smithay::utils::Size<i32, smithay::utils::Physical>,
) {
    // Set the output's fractional scale so render_output scales everything by zoom.
    use smithay::output::Scale;
    state.output.change_current_state(None, None, Some(Scale::Fractional(state.viewport.zoom)), None);

    // Position the output so that (viewport.x, viewport.y) in canvas
    // maps to the top-left of the screen. Elements at canvas coordinates
    // will then be rendered at (canvas − viewport) * zoom by render_output.
    let output_pos = Point::from((state.viewport.x as i32, state.viewport.y as i32));
    state.space.map_output(&state.output, output_pos);

    let gs_size = GsSize::new(screen_size.w as f64, screen_size.h as f64);
    let visible = state
        .global_space
        .windows_visible_in(&state.viewport, gs_size);

    let mut mapped: Vec<u64> = Vec::with_capacity(visible.len());

    for (gid, _, _) in &visible {
        // Canvas-coordinate position (no zoom multiplication)
        let canvas = state.global_space.window_position(*gid).unwrap_or(GsPoint::new(0.0, 0.0));
        let sp = Point::from((canvas.x as i32, canvas.y as i32));
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
        if w == window { Some(*gid) } else { None }
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
    let border = state.config.decoration.border_width;
    let focused = hex_to_color32f(&state.config.decoration.active_color);
    let unfocused = hex_to_color32f(&state.config.decoration.inactive_color);
    let zoom = state.viewport.zoom;
    let vpx = state.viewport.x;
    let vpy = state.viewport.y;

    for (gid, window) in &state.windows {
        if let Some(geo) = state.space.element_geometry(window) {
            let color = if state.focused_gid == Some(*gid) {
                focused
            } else {
                unfocused
            };
            // geo is in CANVAS coordinates — convert to screen
            let cx = geo.loc.x as f64;
            let cy = geo.loc.y as f64;
            let cw = geo.size.w as f64;
            let ch = geo.size.h as f64;

            let sx = (cx - vpx) * zoom;
            let sy = (cy - vpy) * zoom;
            let sw = cw * zoom;
            let sh = ch * zoom;

            let bw = sw + 2.0 * border;

            // top
            let mut buf = SolidColorBuffer::new((bw as i32, border as i32), color);
            elements.push(SolidColorRenderElement::from_buffer(
                &buf, Point::from(((sx - border) as i32, (sy - border) as i32)),
                1.0, 1.0, Kind::Unspecified,
            ));
            // bottom
            buf = SolidColorBuffer::new((bw as i32, border as i32), color);
            elements.push(SolidColorRenderElement::from_buffer(
                &buf, Point::from(((sx - border) as i32, (sy + sh) as i32)),
                1.0, 1.0, Kind::Unspecified,
            ));
            // left
            buf = SolidColorBuffer::new((border as i32, sh as i32), color);
            elements.push(SolidColorRenderElement::from_buffer(
                &buf, Point::from(((sx - border) as i32, sy as i32)),
                1.0, 1.0, Kind::Unspecified,
            ));
            // right
            buf = SolidColorBuffer::new((border as i32, sh as i32), color);
            elements.push(SolidColorRenderElement::from_buffer(
                &buf, Point::from(((sx + sw) as i32, sy as i32)),
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

pub fn spawn_terminal(socket_name: &str) {
    for cmd in &["comet", "kitty", "alacritty", "foot", "gnome-terminal", "wezterm", "weston-terminal"] {
        if std::process::Command::new("which")
            .arg(cmd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            let mut child = std::process::Command::new(cmd);
            child.env("WAYLAND_DISPLAY", socket_name);
            match child.spawn() {
                Ok(_) => { info!("Spawned terminal: {}", cmd); return; }
                Err(e) => { warn!("Failed to spawn {}: {}", cmd, e); }
            }
        }
    }
    let mut child = std::process::Command::new("xterm");
    child.env("WAYLAND_DISPLAY", socket_name);
    match child.spawn() {
        Ok(_) => info!("Spawned xterm"),
        Err(e) => warn!("Failed to spawn xterm: {}", e),
    }
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
            KEY_ENTER | KEY_U => { spawn_terminal(&state.socket_name); return; }
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
        state, event.key_code(), event.state(), smithay::utils::SERIAL_COUNTER.next_serial().into(), 0,
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

    // ── Canvas pan drag ──────────────────────────────────────────
    if let Some(origin) = state.canvas_drag_origin {
        let dx = phys.x - origin.x;
        let dy = phys.y - origin.y;
        state.viewport.x -= dx / state.viewport.zoom;
        state.viewport.y -= dy / state.viewport.zoom;
        state.canvas_drag_origin = Some(phys);
        sync_space_with_viewport(state, state.screen_size);
        state.space.refresh();
        pointer.frame(state);
        return;
    }

    let focus = state
        .space
        .element_under(logical)
        .and_then(|(w, loc)| {
            let rel = logical - loc.to_f64();
            w.surface_under(rel, WindowSurfaceType::ALL)
                .map(|(s, surf_loc)| (s, (loc + surf_loc).to_f64()))
        });

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
    // Keep Smithay space in sync with viewport before element_under checks
    sync_space_with_viewport(state, state.screen_size);
    state.space.refresh();

    let is_left = code == BTN_LEFT;
    let is_right = code == BTN_RIGHT;

    // ── Press (Mod+Click for drag/resize) ─────────────────────────
    if is_press && (is_left || is_right) && state.mod_pressed {
        let logical = Point::<f64, Logical>::from((
            state.pointer_location.x / state.viewport.zoom + state.viewport.x,
            state.pointer_location.y / state.viewport.zoom + state.viewport.y,
        ));
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

    // ── Canvas pan (left click on empty space) ────────────────────
    if is_press && is_left && !state.mod_pressed {
        let logical = Point::<f64, Logical>::from((
            state.pointer_location.x / state.viewport.zoom + state.viewport.x,
            state.pointer_location.y / state.viewport.zoom + state.viewport.y,
        ));
        if state.space.element_under(logical).is_none() {
            state.canvas_drag_origin = Some(state.pointer_location);
        }
    }

    // ── Release ends canvas pan ───────────────────────────────────
    if !is_press && is_left {
        state.canvas_drag_origin = None;
    }

    // ── Click-to-focus (plain left click) ─────────────────────────
    if is_press && is_left && !state.mod_pressed {
        let logical = Point::<f64, Logical>::from((
            state.pointer_location.x / state.viewport.zoom + state.viewport.x,
            state.pointer_location.y / state.viewport.zoom + state.viewport.y,
        ));
        if let Some((window, loc)) = state.space.element_under(logical) {
            let window_id = find_gid(state, window);
            state.focused_gid = window_id;
            let rel = logical - loc.to_f64();
            // Prefer the subsurface for keyboard focus (handles CSD click regions)
            let focus_surface = window
                .surface_under(rel, WindowSurfaceType::ALL)
                .map(|(s, _)| s)
                .or_else(|| surface_from_window(window));
            if let Some(surface) = focus_surface {
                keyboard.set_focus(state, Some(surface), smithay::utils::SERIAL_COUNTER.next_serial().into());
            }
            if let Some(gid) = window_id {
                if let Some(w) = state.windows.get(&gid) {
                    state.space.raise_element(w, true);
                }
                state.global_space.raise_window(gid);
            }
        }
    }

    // Re-evaluate pointer focus after space sync (pan may have moved windows)
    let logical = Point::<f64, Logical>::from((
        state.pointer_location.x / state.viewport.zoom + state.viewport.x,
        state.pointer_location.y / state.viewport.zoom + state.viewport.y,
    ));
    let focus = state
        .space
        .element_under(logical)
        .and_then(|(w, loc)| {
            let rel = logical - loc.to_f64();
            w.surface_under(rel, WindowSurfaceType::ALL)
                .map(|(s, surf_loc)| (s, (loc + surf_loc).to_f64()))
        });
    state.serial_counter += 1;
    pointer.motion(state, focus, &MotionEvent { location: logical, serial: state.serial_counter.into(), time: 0 });

    let button_event = ButtonEvent { serial: serial.into(), time: 0, button: code, state: btn_state };
    pointer.button(state, &button_event);
    pointer.frame(state);
}

/// ── Main loop ────────────────────────────────────────────────────

pub fn run_winit(config: RuntimeConfig) -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<AtlasState> = EventLoop::try_new()?;
    let display: Display<AtlasState> = Display::new()?;
    let dh = display.handle();

    let compositor_state = smithay::wayland::compositor::CompositorState::new::<AtlasState>(&dh);
    let shm_state = smithay::wayland::shm::ShmState::new::<AtlasState>(&dh, vec![]);
    let mut seat_state = smithay::input::SeatState::new();
    let mut seat = seat_state.new_wl_seat(&dh, "atlas");
    let data_device_state =
        smithay::wayland::selection::data_device::DataDeviceState::new::<AtlasState>(&dh);

    let (mut backend, winit) = winit::init::<GlesRenderer>()?;

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
        config,
        global_space,
        viewport,
        windows: std::collections::HashMap::new(),
        grab: None,
        pointer_location: Point::from((0.0f64, 0.0f64)),
        mod_pressed: false,
        ctrl_pressed: false,
        serial_counter: 0,
        focused_gid: None,
        canvas_drag_origin: None,
        screen_size: size,
        cursor_status: smithay::input::pointer::CursorImageStatus::default_named(),
        kde_decoration_state,
        layer_shell_state,
        layer_surfaces: Vec::new(),
        popups: smithay::desktop::PopupManager::default(),
    };

    info!("Initialization completed, starting the main loop.");

    let keyboard = state
        .seat
        .add_keyboard(smithay::input::keyboard::XkbConfig::default(), 200, 200)
        .map_err(|e| format!("Failed to initialize keyboard: {}", e))?;

    let start_time = std::time::Instant::now();
    let loop_signal = event_loop.get_signal();

    backend.window().request_redraw();

    event_loop.handle().insert_source(winit, move |event, _, state| {
        match event {
            WinitEvent::Resized { size, .. } => {
                state.screen_size = size;
                let mode = Mode { size, refresh: 60_000 };
                state.output.change_current_state(Some(mode), None, None, None);
                state.output.set_preferred(mode);
            }
            WinitEvent::Input(event) => {
                match event {
                    InputEvent::Keyboard { event } => {
                        let evdev = event.key_code().raw() as i32 - 8;
                        handle_keyboard_event(state, &event, &keyboard, evdev);
                    }
                    InputEvent::PointerMotionAbsolute { event } => {
                        let phys = Point::<f64, Physical>::from((event.x(), event.y()));
                        let logical = Point::<f64, Logical>::from((
                            phys.x / state.viewport.zoom + state.viewport.x,
                            phys.y / state.viewport.zoom + state.viewport.y,
                        ));
                        handle_motion_event(state, &pointer, phys, logical);
                    }
                    InputEvent::PointerButton { event } => {
                        state.serial_counter += 1;
                        let serial = state.serial_counter;
                        handle_button_event(
                            state, &pointer, &keyboard,
                            event.state() == ButtonState::Pressed,
                            event.button_code(), event.state(), serial,
                        );
                    }
                    InputEvent::PointerAxis { event } => {
                        let dy = event.amount_v120(Axis::Vertical).unwrap_or(0.0);
                        if dy != 0.0 {
                            let cursor = state.pointer_location;
                            let canvas_pt = screen_to_canvas(state, cursor);
                            let old_zoom = state.viewport.zoom;
                            let new_zoom = if dy > 0.0 {
                                (old_zoom / ZOOM_FACTOR).max(MIN_ZOOM)
                            } else {
                                (old_zoom * ZOOM_FACTOR).min(MAX_ZOOM)
                            };
                            state.viewport.x = canvas_pt.x - cursor.x / new_zoom;
                            state.viewport.y = canvas_pt.y - cursor.y / new_zoom;
                            state.viewport.zoom = new_zoom;
                            backend.window().request_redraw();
                        }
                    }
                    _ => {},
                }
            }
            WinitEvent::Redraw => {
                state.screen_size = backend.window_size();
                sync_space_with_viewport(state, state.screen_size);
                state.space.refresh();
                prune_layer_surfaces(state);

                let border_elements = build_border_elements(state);
                let age = backend.buffer_age().unwrap_or(0);

                let (damage_to_submit, frame_time) = {
                    let (renderer, mut framebuffer) = match backend.bind() {
                        Ok(ret) => ret,
                        Err(err) => { error!("Failed to bind renderer: {}", err); return; }
                    };
                    let result = render_output(
                        &state.output,
                        renderer,
                        &mut framebuffer,
                        1.0,
                        age,
                        std::slice::from_ref(&state.space),
                        &border_elements,
                        &mut state.damage_tracker,
                        hex_to_color32f(&state.config.canvas.background_color),
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
                            &output_for_frames,
                            frame_time,
                            None,
                            |_, _| Some(output_for_frames.clone()),
                        );
                    }
                }

                if let Err(err) = state.display_handle.flush_clients() {
                    warn!("Failed to flush clients: {:?}", err);
                }

                backend.window().request_redraw();
            }
            WinitEvent::CloseRequested => {
                loop_signal.stop();
            }
            _ => (),
        }
    })?;

    event_loop.run(None, &mut state, |_| {})?;

    info!("Winit backend shutting down");

    Ok(())
}
