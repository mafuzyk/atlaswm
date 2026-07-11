use std::collections::HashMap;

use tracing::info;

use smithay::{
    backend::renderer::damage::OutputDamageTracker,
    desktop::{
        PopupManager, Space, Window, LayerSurface, layer_map_for_output,
    },
    input::{
        Seat, SeatHandler, SeatState,
        pointer::CursorImageStatus,
    },
    output::Output,
    reexports::{
        wayland_server::{
            Client, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{
                wl_buffer, wl_output, wl_seat,
                wl_surface::WlSurface,
            },
        },
    },
    utils::{Point, Physical, Serial, Size, SERIAL_COUNTER},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState,
            get_parent, is_sync_subsurface,
        },
        output::OutputHandler,
        selection::{
            SelectionHandler,
            data_device::{DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler},
        },
        shell::{
            kde::decoration::{KdeDecorationHandler, KdeDecorationState},
            wlr_layer::{
                WlrLayerShellHandler, WlrLayerShellState,
                LayerSurface as WlrLayerSurface, Layer,
            },
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface,
                XdgShellHandler, XdgShellState,
            },
        },
        shm::{ShmHandler, ShmState},
    },
};

use atlas_config::RuntimeConfig;
use atlas_space::{GlobalSpace, Viewport, Size as GsSize, Point as GsPoint};

// ── Delegate macros ───────────────────────────────────────────────────────────
// This version of Smithay uses a single unified delegate_dispatch2! macro
// that covers all wayland object dispatch.  Individual per-protocol delegates
// (delegate_compositor!, delegate_xdg_shell!, …) do not exist in this fork.
smithay::delegate_dispatch2!(AtlasState);

// ── Client state ─────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

// ── Grab ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrabKind {
    Move,
    Resize,
}

pub struct GrabState {
    pub kind: GrabKind,
    pub window_id: u64,
    pub initial_window_pos: GsPoint,
    pub grab_anchor: GsPoint,
    pub initial_window_size: GsSize,
}

// ── AtlasState ───────────────────────────────────────────────────────────────

pub struct AtlasState {
    pub display_handle: DisplayHandle,
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<AtlasState>,
    pub data_device_state: DataDeviceState,
    pub seat: Seat<AtlasState>,
    pub output: Output,
    pub socket_name: String,
    pub space: Space<Window>,
    pub damage_tracker: OutputDamageTracker,
    pub config: RuntimeConfig,
    pub global_space: GlobalSpace,
    pub viewport: Viewport,
    pub windows: HashMap<u64, Window>,
    pub grab: Option<GrabState>,
    pub pointer_location: Point<f64, Physical>,
    pub mod_pressed: bool,
    pub ctrl_pressed: bool,
    pub serial_counter: u32,
    pub focused_gid: Option<u64>,
    pub canvas_drag_origin: Option<Point<f64, Physical>>,
    pub screen_size: Size<i32, Physical>,
    pub cursor_status: CursorImageStatus,
    // ── Popup management ──────────────────────────────────────────
    pub popups: PopupManager,
    // ── KDE Server-Side Decorations ───────────────────────────────
    pub kde_decoration_state: KdeDecorationState,
    // ── Layer Shell (wlr-layer-shell) ─────────────────────────────
    pub layer_shell_state: WlrLayerShellState,
    pub layer_surfaces: Vec<LayerSurface>,
}

// ── BufferHandler ─────────────────────────────────────────────────────────────

impl BufferHandler for AtlasState {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

// ── XdgShellHandler ──────────────────────────────────────────────────────────

impl XdgShellHandler for AtlasState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        let default_win_size = GsSize::new(800.0, 600.0);

        let pos = if let Some(region) = self.global_space.first_region() {
            self.global_space
                .position_in_region(region.id, default_win_size)
                .unwrap_or(GsPoint::new(100.0, 100.0))
        } else {
            let (sw, sh) = self
                .output
                .current_mode()
                .map(|m| (m.size.w, m.size.h))
                .unwrap_or((1920i32, 1080i32));
            self.global_space.viewport_center_position(
                &self.viewport,
                GsSize::new(sw as f64, sh as f64),
            )
        };

        let gid = self.global_space.add_window(pos, default_win_size, None);
        self.windows.insert(gid, window.clone());

        // Auto-focus the new window
        self.focused_gid = Some(gid);
        if let Some(keyboard) = self.seat.get_keyboard() {
            if let Some(surf) = window.toplevel().map(|t| t.wl_surface().clone()) {
                let serial = SERIAL_COUNTER.next_serial();
                keyboard.set_focus(self, Some(surf), serial);
            }
        }

        // Send initial configure so the client knows the expected size and can
        // start drawing.  The window will be mapped into the Space only after
        // the first commit (see CompositorHandler::commit below), which is the
        // correct Wayland flow: configure → client draws → commit → map.
        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|state| {
                state.size = Some(smithay::utils::Size::from((
                    default_win_size.width as i32,
                    default_win_size.height as i32,
                )));
            });
            toplevel.send_configure();
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        use smithay::desktop::PopupKind;
        if let Err(e) = self.popups.track_popup(PopupKind::from(surface)) {
            tracing::warn!("Failed to track popup: {e}");
        }
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {}
}

// ── KDE Server Decoration (SSD) ──────────────────────────────────────────────

impl KdeDecorationHandler for AtlasState {
    fn kde_decoration_state(&self) -> &KdeDecorationState {
        &self.kde_decoration_state
    }
}

// ── Layer Shell (wlr-layer-shell) ─────────────────────────────────────────────

impl WlrLayerShellHandler for AtlasState {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        wl_output: Option<wl_output::WlOutput>,
        layer: Layer,
        namespace: String,
    ) {
        info!(namespace, layer = ?layer, "New layer surface");
        let output = wl_output
            .as_ref()
            .and_then(Output::from_resource)
            .unwrap_or_else(|| self.output.clone());
        // Must send configure before consuming surface into desktop::LayerSurface
        surface.send_configure();
        let dlayer = LayerSurface::new(surface, namespace);
        let mut map = layer_map_for_output(&output);
        if let Err(e) = map.map_layer(&dlayer) {
            tracing::warn!("Failed to map layer: {:?}", e);
        }
        self.layer_surfaces.push(dlayer);
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        info!("Layer surface destroyed");
        self.layer_surfaces.retain(|l| l.layer_surface() != &surface);
        if let Some((mut map, layer)) = self.space.outputs().find_map(|o| {
            let map = layer_map_for_output(o);
            let layer = map.layers().find(|l| l.layer_surface() == &surface).cloned();
            layer.map(|layer| (map, layer))
        }) {
            map.unmap_layer(&layer);
        }
    }
}

// ── Selections & Output ───────────────────────────────────────────────────────

impl SelectionHandler for AtlasState {
    type SelectionUserData = ();
}

impl DataDeviceHandler for AtlasState {
    fn data_device_state(&mut self) -> &mut DataDeviceState {
        &mut self.data_device_state
    }
}

impl WaylandDndGrabHandler for AtlasState {}

impl OutputHandler for AtlasState {}

// ── CompositorHandler ─────────────────────────────────────────────────────────

impl CompositorHandler for AtlasState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        // 1. Import any attached buffer into the renderer.
        smithay::backend::renderer::utils::on_commit_buffer_handler::<Self>(surface);

        // 2. For non-sync subsurfaces, walk up to the root surface and notify
        //    the window so it refreshes its cached geometry.
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self
                .windows
                .values()
                .find(|w| {
                    w.toplevel()
                        .map(|t| t.wl_surface() == &root)
                        .unwrap_or(false)
                })
                .cloned()
            {
                window.on_commit();

                // Map the window into the Space on its very first commit (i.e.
                // when the client has submitted a buffer for the first time).
                let gid = self
                    .windows
                    .iter()
                    .find_map(|(id, w)| if w == &window { Some(*id) } else { None });

                if let Some(gid) = gid {
                    let canvas_pos = self
                        .global_space
                        .window_position(gid)
                        .unwrap_or(GsPoint::new(0.0, 0.0));
                    let sp = Point::from((canvas_pos.x as i32, canvas_pos.y as i32));
                    if self.space.element_geometry(&window).is_none() {
                        // First commit — map the window into the space.
                        self.space.map_element(window.clone(), sp, true);
                    }
                }
            }
        }

        // 3. Commit any pending popup state.
        self.popups.commit(surface);
    }
}

// ── ShmHandler ────────────────────────────────────────────────────────────────

impl ShmHandler for AtlasState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

// ── SeatHandler ───────────────────────────────────────────────────────────────

impl SeatHandler for AtlasState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<AtlasState> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }
}
