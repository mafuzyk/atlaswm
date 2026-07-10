use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use smithay::{
    backend::{
        allocator::{
            Fourcc, Modifier,
            format::FormatSet,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata,
            compositor::FrameFlags,
            exporter::gbm::GbmFramebufferExporter,
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
        },
        egl::{EGLContext, EGLDisplay, EGLDevice, context::ContextPriority},
        input::{
            InputBackend, InputEvent,
            PointerButtonEvent, PointerMotionEvent, Device,
        },
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            gles::GlesRenderer,
            multigpu::{GpuManager, gbm::GbmGlesBackend},
            Color32F,
        },
        session::{
            Event as SessionEvent, Session,
            libseat::LibSeatSession,
        },
        udev::{UdevBackend, UdevEvent},
    },
    desktop::utils::OutputPresentationFeedback,
    output::{Mode as WlMode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{
            EventLoop, LoopHandle, Mode, Interest, PostAction, RegistrationToken,
            generic::Generic,
        },
        drm::{
            control::{connector, crtc, ModeTypeFlags},
        },
        input::Libinput,
        rustix::fs::OFlags,
        wayland_server::Display,
    },
    utils::{DeviceFd, Logical, Physical, Point, Transform},
    wayland::socket::ListeningSocketSource,
};
use smithay_drm_extras::drm_scanner::{DrmScanEvent, DrmScanner};
use tracing::{error, info, warn};

use atlas_config::DecorationConfig;
use crate::state::{AtlasState, ClientState};
use crate::backend::winit;

const COLOR_FORMATS: &[Fourcc] = &[Fourcc::Argb8888, Fourcc::Xrgb8888];

type GbmDrmOutputUserData = Option<OutputPresentationFeedback>;

// ────── Data ───────────────────────────────────────────────────────

pub struct SurfaceData {
    drm_output: DrmOutput<
        GbmAllocator<DrmDeviceFd>,
        GbmFramebufferExporter<DrmDeviceFd>,
        GbmDrmOutputUserData,
        DrmDeviceFd,
    >,
    output: Output,
}

pub struct DeviceBackend {
    mgr: DrmOutputManager<
        GbmAllocator<DrmDeviceFd>,
        GbmFramebufferExporter<DrmDeviceFd>,
        GbmDrmOutputUserData,
        DrmDeviceFd,
    >,
    scanner: DrmScanner,
    surfaces: HashMap<crtc::Handle, SurfaceData>,
    render_node: Option<DrmNode>,
    _token: RegistrationToken,
}

/// Top-level udev event-loop state.
///
/// The `'l` lifetime is tied to the event loop; `LoopHandle` borrows it.
pub struct UdevState {
    pub session: LibSeatSession,
    pub backends: HashMap<DrmNode, DeviceBackend>,
    pub gpus: GpuManager<GbmGlesBackend<GlesRenderer, DrmDeviceFd>>,
    pub primary_gpu: DrmNode,
    pub atlas: AtlasState,
    pub running: Arc<AtomicBool>,
}

use smithay::backend::drm::DrmNode;

// ────── Entry point ────────────────────────────────────────────────

pub fn run_udev(deco_config: Option<DecorationConfig>) {
    let mut event_loop: EventLoop<UdevState> = match EventLoop::try_new() {
        Ok(el) => el,
        Err(e) => { error!("event loop: {e}"); return; }
    };
    let handle = event_loop.handle();
    let display: Display<AtlasState> = match Display::new() {
        Ok(d) => d,
        Err(e) => { error!("display: {e}"); return; }
    };
    let dh = display.handle();

    // ── Session ────────────────────────────────────────────────
    let (session, session_notifier) = match LibSeatSession::new() {
        Ok(s) => s,
        Err(e) => { error!("libseat: {e}"); return; }
    };
    let seat_name = session.seat();
    info!(seat = %seat_name, "Session acquired");

    // ── Primary GPU ────────────────────────────────────────────
    let primary_gpu = if let Ok(path) = std::env::var("ATLAS_DRM_DEVICE") {
        match DrmNode::from_path(path) {
            Ok(n) => n,
            Err(e) => { error!("bad node from env: {e}"); return; }
        }
    } else {
        match smithay::backend::udev::primary_gpu(&seat_name) {
            Ok(Some(p)) => match DrmNode::from_path(&p) {
                Ok(n) => n,
                Err(e) => { error!("bad primary node: {e}"); return; }
            }
            Ok(None) => { error!("no primary GPU"); return; }
            Err(e) => { error!("primary_gpu: {e}"); return; }
        }
    };
    info!(?primary_gpu, "Primary GPU");

    // ── GpuManager ─────────────────────────────────────────────
    let gpus = match GpuManager::new(GbmGlesBackend::with_factory(|display| {
        let ctx = EGLContext::new_with_priority(display, ContextPriority::High)?;
        let caps = unsafe { GlesRenderer::supported_capabilities(&ctx)? };
        Ok(unsafe { GlesRenderer::with_capabilities(ctx, caps)? })
    })) {
        Ok(g) => g,
        Err(e) => { error!("GpuManager: {e}"); return; }
    };

    // ── Wayland state ──────────────────────────────────────────
    let compositor_state = smithay::wayland::compositor::CompositorState::new::<AtlasState>(&dh);
    let shm_state = smithay::wayland::shm::ShmState::new::<AtlasState>(&dh, vec![]);
    let mut seat_state = smithay::input::SeatState::new();
    let mut seat = seat_state.new_wl_seat(&dh, &seat_name);
    let dds = smithay::wayland::selection::data_device::DataDeviceState::new::<AtlasState>(&dh);
    let xdg = smithay::wayland::shell::xdg::XdgShellState::new::<AtlasState>(&dh);
    let kde = smithay::wayland::shell::kde::decoration::KdeDecorationState::new::<AtlasState>(
        &dh,
        smithay::reexports::wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration_manager::Mode::Server,
    );
    let lshell = smithay::wayland::shell::wlr_layer::WlrLayerShellState::new::<AtlasState>(&dh);

    // ── Socket ─────────────────────────────────────────────────
    let sock = match ListeningSocketSource::new_auto() {
        Ok(s) => s,
        Err(e) => { error!("socket: {e}"); return; }
    };
    let sock_name = sock.socket_name().to_string_lossy().into_owned();
    info!(name = %sock_name, "Wayland socket");

    // ── Udev ───────────────────────────────────────────────────
    let udev = match UdevBackend::new(&seat_name) {
        Ok(u) => u,
        Err(e) => { error!("udev: {e}"); return; }
    };

    // ── Libinput ───────────────────────────────────────────────
    let mut libinput_ctx = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(
        session.clone().into(),
    );
    if let Err(e) = libinput_ctx.udev_assign_seat(&seat_name) {
        error!("libinput seat: {e:?}"); return;
    }

    // ── AtlasState ─────────────────────────────────────────────
    let deco = deco_config.unwrap_or_default();
    let placeholder = Output::new(
        "placeholder".into(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Atlas".into(),
            model: "DRM".into(),
            serial_number: "".into(),
        },
    );
    let _ = seat.add_keyboard(smithay::input::keyboard::XkbConfig::default(), 200, 200);

    let atlas = AtlasState {
        display_handle: dh.clone(),
        compositor_state,
        xdg_shell_state: xdg,
        shm_state,
        seat_state,
        data_device_state: dds,
        seat,
        output: placeholder,
        socket_name: sock_name,
        space: smithay::desktop::Space::default(),
        damage_tracker: smithay::backend::renderer::damage::OutputDamageTracker::new(
            (0, 0), 1.0, Transform::Normal,
        ),
        deco_config: deco,
        global_space: atlas_space::GlobalSpace::new(),
        viewport: atlas_space::Viewport::new("udev"),
        windows: HashMap::new(),
        running: true,
        grab: None,
        pointer_location: Point::from((0.0f64, 0.0f64)),
        mod_pressed: false,
        serial_counter: 0,
        focused_gid: None,
        cursor_status: smithay::input::pointer::CursorImageStatus::default_named(),
        kde_decoration_state: kde,
        layer_shell_state: lshell,
        layer_surfaces: Vec::new(),
    };

    let mut us = UdevState {
        session,
        backends: HashMap::new(),
        gpus,
        primary_gpu,
        atlas,
        running: Arc::new(AtomicBool::new(true)),
    };

    // ── Register sources ───────────────────────────────────────
    let h = handle.clone();

    // Wayland FD — dispatch clients
    h.insert_source(
        Generic::new(display, Interest::READ, Mode::Level),
        |_, d, data: &mut UdevState| {
            unsafe { d.get_mut().dispatch_clients(&mut data.atlas).unwrap(); }
            Ok(PostAction::Continue)
        },
    ).unwrap();
    // Wayland socket — accept new clients
    h.insert_source(
        sock,
        |stream, _, data: &mut UdevState| {
            if let Err(e) = data.atlas.display_handle.insert_client(stream, Arc::new(ClientState::default())) {
                warn!("client: {e}");
            }
        },
    ).unwrap();
    // libinput — input events
    h.insert_source(
        LibinputInputBackend::new(libinput_ctx.clone()),
        |event, _, data: &mut UdevState| data.on_input::<LibinputInputBackend>(event),
    ).unwrap();
    // Session — pause/resume
    let session_h = h.clone();
    let mut session_li = libinput_ctx.clone();
    h.insert_source(
        session_notifier,
        move |event, _, data: &mut UdevState| match event {
            SessionEvent::PauseSession => {
                info!("pause");
                session_li.suspend();
                for b in data.backends.values_mut() { b.mgr.pause(); }
            }
            SessionEvent::ActivateSession => {
                info!("resume");
                let _ = session_li.resume();
                for b in data.backends.values_mut() { b.mgr.lock().activate(false).expect("activate"); }
                session_h.insert_idle(move |data: &mut UdevState| data.redraw_all());
            }
        },
    ).unwrap();
    // Udev — hotplug
    let udev_h = h.clone();
    h.insert_source(
        udev,
        move |event, _, data: &mut UdevState| match event {
            UdevEvent::Added { device_id: _, path } => {
                if let Ok(node) = DrmNode::from_path(&path) {
                    if let Err(e) = data.device_added(node, &path, &udev_h) {
                        error!("device_added {path:?}: {e}");
                    }
                }
            }
            UdevEvent::Changed { device_id: _ } => {
                let node = data.backends.keys().next().copied().unwrap_or(data.primary_gpu);
                data.device_changed(node);
            }
            UdevEvent::Removed { .. } => {}
        },
    ).unwrap();

    // Init primary GPU
    let primary_gpu_path = {
        let devices = smithay::backend::udev::all_gpus(seat_name)
            .unwrap_or_default();
        devices.iter().find_map(|p| {
            DrmNode::from_path(p).ok().and_then(|n| {
                if n == primary_gpu { Some(p.clone()) } else { None }
            })
        })
    };
    if let Some(path) = primary_gpu_path {
        if let Err(e) = us.device_added(primary_gpu, &path, &h) {
            error!("init primary: {e}");
            return;
        }
    } else {
        error!("could not find primary GPU path");
        return;
    }

    info!("Udev backend ready");

    while us.running.load(Ordering::SeqCst) {
        if event_loop.dispatch(Some(Duration::from_millis(16)), &mut us).is_err() {
            break;
        }
        us.atlas.space.refresh();
        winit::prune_layer_surfaces(&mut us.atlas);
        if let Err(e) = us.atlas.display_handle.flush_clients() {
            warn!("flush: {e:?}");
        }
    }
    info!("exit");
}

// ────── Implementation ────────────────────────────────────────────

impl UdevState {
    fn device_added(&mut self, node: DrmNode, path: &Path, h: &LoopHandle<'_, UdevState>) -> Result<(), String> {
        let fd = self.session.open(
            path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        ).map_err(|e| format!("open: {e}"))?;

        let fd = DrmDeviceFd::new(DeviceFd::from(fd));
        let (drm, notifier) = DrmDevice::new(fd.clone(), true)
            .map_err(|e| format!("DrmDevice: {e}"))?;
        let gbm = GbmDevice::new(fd)
            .map_err(|e| format!("GbmDevice: {e}"))?;

        let render_node = (|| -> Result<DrmNode, String> {
            let egl_display = unsafe { EGLDisplay::new(gbm.clone()) }
                .map_err(|e| format!("EGLDisplay: {e}"))?;
            let egl_dev = EGLDevice::device_for_display(&egl_display)
                .map_err(|e| format!("EGLDevice: {e}"))?;
            let rn = egl_dev.try_get_render_node().ok().flatten().unwrap_or(node);
            self.gpus.as_mut().add_node(rn, gbm.clone())
                .map_err(|e| format!("gpus.add_node: {e}"))?;
            Ok(rn)
        })().ok();

        let alloc = GbmAllocator::new(gbm.clone(), GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT);
        let exporter = GbmFramebufferExporter::new(gbm.clone(), render_node.into());

        let render_node_for_formats = render_node.unwrap_or(self.primary_gpu);
        let mut renderer = self.gpus.single_renderer(&render_node_for_formats)
            .map_err(|e| format!("single_renderer: {e}"))?;
        let render_formats: FormatSet = renderer
            .as_mut()
            .egl_context()
            .dmabuf_render_formats()
            .iter()
            .filter(|format| render_node.is_some() || format.modifier == Modifier::Linear)
            .copied()
            .collect();

        let mgr = DrmOutputManager::new(
            drm,
            alloc,
            exporter,
            Some(gbm),
            COLOR_FORMATS.iter().copied(),
            render_formats,
        );

        let h2 = h.clone();
        let token = h.insert_source(
            notifier,
            move |event, metadata, data: &mut UdevState| match event {
                DrmEvent::VBlank(crtc) => data.on_vblank(node, crtc, metadata, &h2),
                DrmEvent::Error(e) => error!("drm err {e:?}"),
            },
        ).map_err(|e| format!("notifier: {e}"))?;

        self.backends.insert(node, DeviceBackend {
            mgr,
            scanner: DrmScanner::new(),
            surfaces: HashMap::new(),
            render_node,
            _token: token,
        });
        self.device_changed(node);
        Ok(())
    }

    fn device_changed(&mut self, node: DrmNode) {
        let b = match self.backends.get_mut(&node) { Some(b) => b, None => return };
        let scan_result = match b.scanner.scan_connectors(b.mgr.device()) {
            Ok(r) => r,
            Err(e) => { warn!("scan_connectors: {e}"); return; }
        };
        for event in scan_result {
            match event {
                DrmScanEvent::Connected { connector, crtc: Some(crtc) } => {
                    self.connector_connected(node, connector, crtc);
                }
                DrmScanEvent::Disconnected { connector, crtc: Some(crtc) } => {
                    self.connector_disconnected(node, connector, crtc);
                }
                _ => {}
            }
        }
    }

    fn connector_connected(
        &mut self,
        node: DrmNode,
        connector_info: connector::Info,
        crtc: crtc::Handle,
    ) {
        let b = match self.backends.get_mut(&node) { Some(b) => b, None => return };
        let mode_id = connector_info
            .modes()
            .iter()
            .position(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
            .unwrap_or(0);
        let drm_mode = &connector_info.modes()[mode_id];
        let name = format!("{}-{}", connector_info.interface().as_str(), connector_info.interface_id());
        info!(output = %name, "connect");

        let (phys_w, phys_h) = connector_info.size().unwrap_or((0, 0));
        let output = Output::new(
            name,
            PhysicalProperties {
                size: (phys_w as i32, phys_h as i32).into(),
                subpixel: connector_info.subpixel().into(),
                make: "Atlas".into(),
                model: "DRM".into(),
                serial_number: "".into(),
            },
        );
        output.create_global::<AtlasState>(&self.atlas.display_handle);
        let wl_mode = WlMode::from(*drm_mode);
        output.set_preferred(wl_mode);
        output.change_current_state(Some(wl_mode), None, None, Some((0, 0).into()));
        self.atlas.space.map_output(&output, (0, 0));
        self.atlas.output = output.clone();

        let render_node = b.render_node.unwrap_or(self.primary_gpu);
        let mut renderer = match self.gpus.single_renderer(&render_node) {
            Ok(r) => r,
            Err(e) => { warn!("rend: {e}"); return; }
        };

        match b.mgr.lock().initialize_output::<_, smithay::backend::renderer::element::solid::SolidColorRenderElement>(
            crtc, *drm_mode, &[connector_info.handle()], &output, None,
            &mut renderer, &DrmOutputRenderElements::default(),
        ) {
            Ok(o) => {
                b.surfaces.insert(crtc, SurfaceData { drm_output: o, output });
            }
            Err(e) => warn!("init {e:?}"),
        }
    }

    fn connector_disconnected(&mut self, node: DrmNode, _connector_info: connector::Info, crtc: crtc::Handle) {
        if let Some(b) = self.backends.get_mut(&node) {
            if let Some(d) = b.surfaces.remove(&crtc) {
                info!(o = %d.output.name(), "disconnect");
                self.atlas.space.unmap_output(&d.output);
            }
            let render_node = b.render_node.unwrap_or(self.primary_gpu);
            let mut renderer = match self.gpus.single_renderer(&render_node) {
                Ok(r) => r,
                Err(_) => return,
            };
            let _ = b.mgr.lock().try_to_restore_modifiers::<_, smithay::backend::renderer::element::solid::SolidColorRenderElement>(
                &mut renderer,
                &DrmOutputRenderElements::default(),
            );
        }
    }

    fn on_vblank(
        &mut self,
        node: DrmNode,
        crtc: crtc::Handle,
        _metadata: &mut Option<DrmEventMetadata>,
        h: &LoopHandle<'_, UdevState>,
    ) {
        if let Some(b) = self.backends.get_mut(&node) {
            if let Some(d) = b.surfaces.get_mut(&crtc) {
                let _ = d.drm_output.frame_submitted();
            }
        }
        let r = self.running.clone();
        let h = h.clone();
        h.insert_idle(move |data: &mut UdevState| {
            if r.load(Ordering::SeqCst) {
                data.render_one(node, crtc);
            }
        });
    }

    fn render_one(&mut self, node: DrmNode, crtc: crtc::Handle) {
        let b = self.backends.get_mut(&node).unwrap();
        let d = b.surfaces.get_mut(&crtc).unwrap();
        let render_node = b.render_node.unwrap_or(self.primary_gpu);
        let mut renderer = match self.gpus.single_renderer(&render_node) {
            Ok(r) => r,
            Err(e) => { warn!("rend: {e}"); return; }
        };

        if let Some(mode) = d.output.current_mode() {
            winit::sync_space_with_viewport(&mut self.atlas, mode.size);
        }
        self.atlas.space.refresh();

        let borders = winit::build_border_elements(&self.atlas);
        match d.drm_output.render_frame(
            &mut renderer, &borders, Color32F::new(0.1, 0.0, 0.0, 1.0), FrameFlags::empty(),
        ) {
            Ok(result) => {
                if result.needs_sync() {
                    use smithay::backend::drm::compositor::PrimaryPlaneElement;
                    if let PrimaryPlaneElement::Swapchain(primary_swapchain_element) = &result.primary_element {
                        let _ = primary_swapchain_element.sync.wait();
                    }
                }
                let user_data = Some(OutputPresentationFeedback::new(&d.output));
                let _ = d.drm_output.queue_frame(user_data);
            }
            Err(e) => warn!("render: {e:?}"),
        }
    }

    fn redraw_all(&mut self) {
        for node in self.backends.keys().copied().collect::<Vec<_>>() {
            let crtcs: Vec<_> = self.backends.get(&node).map(|b| b.surfaces.keys().copied().collect()).unwrap_or_default();
            for crtc in crtcs { self.render_one(node, crtc); }
        }
    }

    fn on_input<B: InputBackend>(&mut self, event: InputEvent<B>) {
        let Some(kb) = self.atlas.seat.get_keyboard() else { return };
        let Some(pt) = self.atlas.seat.get_pointer() else { return };

        match event {
            InputEvent::Keyboard { event, .. } => {
                winit::handle_keyboard_event::<B>(&mut self.atlas, &event, &kb);
            }
            InputEvent::PointerMotion { event } => {
                let current = self.atlas.pointer_location;
                let new_phys = Point::<f64, Physical>::from((
                    current.x + event.delta_x(),
                    current.y + event.delta_y(),
                ));
                let logical = Point::<f64, Logical>::from((new_phys.x, new_phys.y));
                winit::handle_motion_event(&mut self.atlas, &pt, new_phys, logical);
            }
            InputEvent::PointerButton { event } => {
                self.atlas.serial_counter += 1;
                let serial = self.atlas.serial_counter;
                let btn_state = event.state();
                winit::handle_button_event(
                    &mut self.atlas, &pt, &kb,
                    btn_state == smithay::backend::input::ButtonState::Pressed,
                    event.button_code(), btn_state, serial,
                );
            }
            InputEvent::DeviceAdded { ref device } => {
                if device.has_capability(smithay::backend::input::DeviceCapability::Keyboard) {
                    info!("kbd added");
                }
            }
            _ => {}
        }
    }
}
