use std::collections::HashMap;

use tracing::info;

use smithay::{
    backend::renderer::damage::OutputDamageTracker,
    desktop::{Space, Window, LayerSurface, layer_map_for_output},
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
    utils::{Point, Physical, Serial},
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorClientState, CompositorHandler, CompositorState},
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
            xdg::{PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState},
        },
        shm::{ShmHandler, ShmState},
    },
};

use atlas_config::DecorationConfig;
use atlas_space::{GlobalSpace, Viewport, Size as GsSize, Point as GsPoint};

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

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
    pub deco_config: DecorationConfig,
    pub global_space: GlobalSpace,
    pub viewport: Viewport,
    pub windows: HashMap<u64, Window>,
    pub running: bool,
    pub grab: Option<GrabState>,
    pub pointer_location: Point<f64, Physical>,
    pub mod_pressed: bool,
    pub ctrl_pressed: bool,
    pub serial_counter: u32,
    pub focused_gid: Option<u64>,
    pub cursor_status: CursorImageStatus,
    // ── KDE Server-Side Decorations ───────────────────────────────
    pub kde_decoration_state: KdeDecorationState,
    // ── Layer Shell (wlr-layer-shell) ─────────────────────────────
    pub layer_shell_state: WlrLayerShellState,
    pub layer_surfaces: Vec<LayerSurface>,
}

impl BufferHandler for AtlasState {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

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
        self.windows.insert(gid, window);
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
    }
}

// ── KDE Server Decoration (SSD) ─────────────────────────────────

impl KdeDecorationHandler for AtlasState {
    fn kde_decoration_state(&self) -> &KdeDecorationState {
        &self.kde_decoration_state
    }
}

// ── Layer Shell (wlr-layer-shell) ───────────────────────────────

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
        map.map_layer(&dlayer).unwrap();
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

// ── Selections & Output ─────────────────────────────────────────

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

impl CompositorHandler for AtlasState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        smithay::backend::renderer::utils::on_commit_buffer_handler::<Self>(surface);
    }
}

impl ShmHandler for AtlasState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

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

smithay::delegate_dispatch2!(AtlasState);
