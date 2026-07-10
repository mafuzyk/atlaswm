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
            InputBackend, InputEvent, KeyboardKeyEvent,
            PointerButtonEvent, PointerMotionEvent, Device,
        },
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            element::{Kind, solid::{SolidColorBuffer, SolidColorRenderElement}},
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
use tracing::{debug, error, info, warn};
use calloop::signals::{Signal, Signals};

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
    info!(seat = %seat_name, "Session acquired via libseat");

    // ── Primary GPU ────────────────────────────────────────────
    let primary_gpu = if let Ok(path) = std::env::var("ATLAS_DRM_DEVICE") {
        info!(path = %path, "Primary GPU overridden via ATLAS_DRM_DEVICE");
        match DrmNode::from_path(path) {
            Ok(n) => n,
            Err(e) => { error!("ATLAS_DRM_DEVICE is invalid: {e}"); return; }
        }
    } else {
        match smithay::backend::udev::primary_gpu(&seat_name) {
            Ok(Some(p)) => {
                info!(path = %p.display(), "Found primary GPU via udev");
                match DrmNode::from_path(&p) {
                    Ok(n) => n,
                    Err(e) => { error!("primary GPU node not usable: {e}"); return; }
                }
            }
            Ok(None) => { error!("no primary GPU found on seat {seat_name}"); return; }
            Err(e) => { error!("udev primary_gpu query failed: {e}"); return; }
        }
    };
    info!(?primary_gpu, "Primary GPU resolved");

    // ── GpuManager ─────────────────────────────────────────────
    let gpus = match GpuManager::new(GbmGlesBackend::with_factory(|display| {
        info!("Creating GLES renderer from EGL display");
        let ctx = EGLContext::new_with_priority(display, ContextPriority::High)?;
        let caps = unsafe { GlesRenderer::supported_capabilities(&ctx)? };
        Ok(unsafe { GlesRenderer::with_capabilities(ctx, caps)? })
    })) {
        Ok(g) => { info!("GpuManager initialised"); g }
        Err(e) => { error!("GpuManager init failed: {e}"); return; }
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
        Err(e) => { error!("wayland socket: {e}"); return; }
    };
    let sock_name = sock.socket_name().to_string_lossy().into_owned();
    info!(name = %sock_name, "Wayland socket listening");

    // ── Udev ───────────────────────────────────────────────────
    let udev = match UdevBackend::new(&seat_name) {
        Ok(u) => u,
        Err(e) => { error!("udev backend: {e}"); return; }
    };

    // ── Libinput ───────────────────────────────────────────────
    let mut libinput_ctx = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(
        session.clone().into(),
    );
    if let Err(e) = libinput_ctx.udev_assign_seat(&seat_name) {
        error!("libinput seat assign: {e:?}"); return;
    }
    info!("libinput context ready");

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
        ctrl_pressed: false,
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

    h.insert_source(
        Generic::new(display, Interest::READ, Mode::Level),
        |_, d, data: &mut UdevState| {
            if let Err(e) = unsafe { d.get_mut().dispatch_clients(&mut data.atlas) } {
                warn!("dispatch_clients: {e:?}");
            }
            Ok(PostAction::Continue)
        },
    ).expect("wayland display source");

    h.insert_source(
        sock,
        |stream, _, data: &mut UdevState| {
            if let Err(e) = data.atlas.display_handle.insert_client(stream, Arc::new(ClientState::default())) {
                warn!("client: {e}");
            }
        },
    ).expect("wayland socket source");

    h.insert_source(
        LibinputInputBackend::new(libinput_ctx.clone()),
        |event, _, data: &mut UdevState| data.on_input::<LibinputInputBackend>(event),
    ).expect("libinput source");

    // Session — pause/resume
    let session_h = h.clone();
    let mut session_li = libinput_ctx.clone();
    h.insert_source(
        session_notifier,
        move |event, _, data: &mut UdevState| match event {
            SessionEvent::PauseSession => {
                info!("session paused — DRM master released");
                session_li.suspend();
                for b in data.backends.values_mut() { b.mgr.pause(); }
            }
            SessionEvent::ActivateSession => {
                info!("session resumed — DRM master re-acquired");
                if let Err(e) = session_li.resume() {
                    warn!("libinput resume: {e:?}");
                }
                for b in data.backends.values_mut() {
                    if let Err(e) = b.mgr.lock().activate(false) {
                        warn!("drm activate: {e:?}");
                    }
                }
                session_h.insert_idle(move |data: &mut UdevState| data.redraw_all());
            }
        },
    ).expect("session source");

    // Udev — hotplug
    let udev_h = h.clone();
    h.insert_source(
        udev,
        move |event, _, data: &mut UdevState| match event {
            UdevEvent::Added { device_id: _, path } => {
                info!(?path, "Udev device added");
                if let Ok(node) = DrmNode::from_path(&path) {
                    if let Err(e) = data.device_added(node, &path, &udev_h) {
                        error!("device_added {path:?}: {e}");
                    }
                } else {
                    warn!(?path, "Could not open DrmNode for udev device");
                }
            }
            UdevEvent::Changed { device_id: _ } => {
                let node = data.backends.keys().next().copied().unwrap_or(data.primary_gpu);
                debug!(?node, "Udev device changed — rescanning connectors");
                data.device_changed(node, &udev_h);
            }
            UdevEvent::Removed { device_id: _ } => {
                warn!("Udev device removed (not handled yet)");
            }
        },
    ).expect("udev source");

    // ── Signal handler via signalfd (SIGINT, SIGTERM) ──────────
    match Signals::new(&[Signal::SIGINT, Signal::SIGTERM]) {
        Ok(signals) => {
            if h.insert_source(signals, move |event, _, data: &mut UdevState| {
                info!(sig = ?event.signal(), "Received signal — shutting down");
                data.running.store(false, Ordering::SeqCst);
            }).is_ok() {
                info!("Signalfd handler installed for SIGINT/SIGTERM");
            }
        }
        Err(e) => warn!("signalfd setup failed ({e}) — signals won't be caught via event loop"),
    }

    // Init primary GPU
    let primary_gpu_path = {
        let devices = smithay::backend::udev::all_gpus(seat_name).unwrap_or_default();
        devices.iter().find_map(|p| {
            DrmNode::from_path(p).ok().and_then(|n| {
                if n == primary_gpu { Some(p.clone()) } else { None }
            })
        })
    };
    match primary_gpu_path {
        Some(ref path) => {
            info!(?primary_gpu, ?path, "Initialising primary GPU");
            if let Err(e) = us.device_added(primary_gpu, path, &h) {
                error!("Primary GPU init failed: {e}");
                error!("Atlas cannot continue without a working DRM device. Exiting.");
                return;
            }
        }
        None => {
            error!("Could not resolve filesystem path for primary GPU");
            error!("Ensure the device is accessible and seatd is running.");
            return;
        }
    }

    // The initial modeset from initialize_output already submits the first page-flip.
    // VBlank events will drive the rendering loop from here.
    info!("Waiting for VBlank to kickstart rendering — entering event loop");

    while us.running.load(Ordering::SeqCst) {
        if event_loop.dispatch(Some(Duration::from_millis(16)), &mut us).is_err() {
            warn!("event loop dispatch error — exiting");
            break;
        }
        us.atlas.space.refresh();
        winit::prune_layer_surfaces(&mut us.atlas);
        if let Err(e) = us.atlas.display_handle.flush_clients() {
            warn!("flush: {e:?}");
        }
    }
    info!("Atlas (udev) shutting down");
}

// ────── Implementation ────────────────────────────────────────────

impl UdevState {
    fn device_added(&mut self, node: DrmNode, path: &Path, h: &LoopHandle<'_, UdevState>) -> Result<(), String> {
        info!(?path, "device_added — opening DRM fd via libseat");
        let fd = self.session.open(
            path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        ).map_err(|e| {
            error!(?path, err = ?e, "libseat open failed");
            format!("libseat open {path:?}: {e}")
        })?;
        info!(?path, fd = ?fd, "DRM fd opened");

        let fd = DrmDeviceFd::new(DeviceFd::from(fd));

        info!(?path, "Initialising DrmDevice");
        let (drm, notifier) = DrmDevice::new(fd.clone(), true)
            .map_err(|e| format!("DrmDevice::new: {e}"))?;
        info!(?path, "DrmDevice ready");

        info!(?path, "Initialising GbmDevice");
        let gbm = GbmDevice::new(fd)
            .map_err(|e| {
                error!(?path, err = ?e, "GbmDevice creation failed — possible permission issue or missing /dev/dri/card* access");
                format!("GbmDevice::new: {e}")
            })?;
        info!(?path, "GbmDevice ready");

        let render_node = (|| -> Result<DrmNode, String> {
            info!(?path, "Setting up EGL display for GPU");
            let egl_display = unsafe { EGLDisplay::new(gbm.clone()) }
                .map_err(|e| format!("EGLDisplay::new: {e}"))?;
            let egl_dev = EGLDevice::device_for_display(&egl_display)
                .map_err(|e| format!("EGLDevice: {e}"))?;
            let rn = egl_dev.try_get_render_node().ok().flatten().unwrap_or(node);
            info!(?rn, "Registering render node with GpuManager");
            self.gpus.as_mut().add_node(rn, gbm.clone())
                .map_err(|e| format!("gpus.add_node: {e}"))?;
            Ok(rn)
        })();
        let render_node = match render_node {
            Ok(rn) => {
                info!(?rn, "GPU initialised with render node");
                Some(rn)
            }
            Err(e) => {
                warn!("GPU init skipped — falling back to primary GPU for rendering: {e}");
                None
            }
        };

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
        debug!(?render_formats, "Available render formats");

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
                DrmEvent::VBlank(crtc) => {
                    debug!(?crtc, "VBlank event");
                    data.on_vblank(node, crtc, metadata, &h2);
                }
                DrmEvent::Error(e) => error!("drm error event: {e:?}"),
            },
        ).map_err(|e| format!("notifier insert: {e}"))?;

        self.backends.insert(node, DeviceBackend {
            mgr,
            scanner: DrmScanner::new(),
            surfaces: HashMap::new(),
            render_node,
            _token: token,
        });
        info!(?node, "Device backend registered — scanning connectors");
        self.device_changed(node, h);
        Ok(())
    }

    fn device_changed(&mut self, node: DrmNode, h: &LoopHandle<'_, UdevState>) {
        let b = match self.backends.get_mut(&node) {
            Some(b) => b,
            None => { debug!(?node, "device_changed for unknown backend"); return; }
        };
        let scan_result = match b.scanner.scan_connectors(b.mgr.device()) {
            Ok(r) => r,
            Err(e) => { warn!(?node, "scan_connectors: {e}"); return; }
        };
        for event in scan_result {
            match event {
                DrmScanEvent::Connected { connector, crtc: Some(crtc) } => {
                    self.connector_connected(node, connector, crtc, h);
                }
                DrmScanEvent::Disconnected { connector, crtc: Some(crtc) } => {
                    self.connector_disconnected(node, connector, crtc);
                }
                DrmScanEvent::Connected { .. } => {
                    debug!("Connector connected but no CRTC available yet");
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
        h: &LoopHandle<'_, UdevState>,
    ) {
        // ── Obtain hardware planes (anvil: pass Some(planes) to initialize_output) ──
        let planes = {
            let b = match self.backends.get(&node) { Some(b) => b, None => return };
            match b.mgr.device().planes(&crtc) {
                Ok(p) => Some(p),
                Err(e) => { warn!(?node, ?crtc, "Failed to query planes: {e}"); return; }
            }
        };

        let b = match self.backends.get_mut(&node) { Some(b) => b, None => return };

        let modes = connector_info.modes();
        let mode_id = modes.iter()
            .position(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
            .unwrap_or(0);
        let drm_mode = &modes[mode_id];
        let name = format!("{}-{}", connector_info.interface().as_str(), connector_info.interface_id());

        let (mw, mh) = (drm_mode.size().0, drm_mode.size().1);
        let refresh_hz = drm_mode.vrefresh() as f64 / 1000.0;
        info!(
            output = %name, ?crtc,             resolution = format!("{}x{}", mw, mh), refresh = format!("{:.1} Hz", refresh_hz),
            mode_index = mode_id, total_modes = modes.len(),
            "Connector connected — using mode"
        );

        let (phys_w, phys_h) = connector_info.size().unwrap_or((0, 0));
        let output = Output::new(
            name.clone(),
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
            Err(e) => { warn!(?render_node, "single_renderer: {e}"); return; }
        };

        info!(output = %name, "Initialising DrmOutput — modeset with planes");
        match b.mgr.lock().initialize_output::<_, smithay::backend::renderer::element::solid::SolidColorRenderElement>(
            crtc, *drm_mode, &[connector_info.handle()], &output, planes,
            &mut renderer, &DrmOutputRenderElements::default(),
        ) {
            Ok(o) => {
                info!(output = %name, "DrmOutput initialised — page-flip committed");
                b.surfaces.insert(crtc, SurfaceData { drm_output: o, output });

                // Kick-start the rendering loop with an initial render (anvil pattern)
                h.insert_idle(move |data: &mut UdevState| {
                    data.render_one(node, crtc);
                });
            }
            Err(e) => {
                error!(output = %name, err = ?e, "DrmOutput init failed");
            }
        }
    }

    fn connector_disconnected(&mut self, node: DrmNode, _connector_info: connector::Info, crtc: crtc::Handle) {
        if let Some(b) = self.backends.get_mut(&node) {
            if let Some(d) = b.surfaces.remove(&crtc) {
                info!(o = %d.output.name(), ?crtc, "Output disconnected");
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
                if let Err(e) = d.drm_output.frame_submitted() {
                    warn!(?crtc, "frame_submitted error: {e:?}");
                }
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
        let b = match self.backends.get_mut(&node) {
            Some(b) => b,
            None => { debug!(?node, ?crtc, "render_one: unknown backend"); return; }
        };
        let d = match b.surfaces.get_mut(&crtc) {
            Some(d) => d,
            None => { debug!(?node, ?crtc, "render_one: unknown crtc"); return; }
        };
        let render_node = b.render_node.unwrap_or(self.primary_gpu);
        let mut renderer = match self.gpus.single_renderer(&render_node) {
            Ok(r) => r,
            Err(e) => { warn!(?render_node, "render_one single_renderer: {e}"); return; }
        };

        if let Some(mode) = d.output.current_mode() {
            winit::sync_space_with_viewport(&mut self.atlas, mode.size);
        }
        self.atlas.space.refresh();

        let mut elements = winit::build_border_elements(&self.atlas);

        // Software cursor — a small white square at the pointer position
        let (cx, cy) = (self.atlas.pointer_location.x as i32, self.atlas.pointer_location.y as i32);
        let cursor_buf = SolidColorBuffer::new((16, 16), Color32F::new(1.0, 1.0, 1.0, 1.0));
        elements.push(SolidColorRenderElement::from_buffer(
            &cursor_buf,
            Point::from((cx, cy)),
            1.0, 1.0, Kind::Unspecified,
        ));

        match d.drm_output.render_frame(
            &mut renderer, &elements, Color32F::new(0.15, 0.15, 0.35, 1.0), FrameFlags::empty(),
        ) {
            Ok(_) => {
                let user_data = Some(OutputPresentationFeedback::new(&d.output));
                if let Err(e) = d.drm_output.queue_frame(user_data) {
                    warn!(?crtc, "queue_frame: {e:?}");
                }
            }
            Err(e) => warn!(?crtc, "render_frame: {e:?}"),
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
                let pressed = event.state() == smithay::backend::input::KeyState::Pressed;
                let raw = event.key_code().raw() as i32;

                // Track Ctrl modifier for Ctrl+C handling on TTY
                if raw == 29 || raw == 97 {
                    self.atlas.ctrl_pressed = pressed;
                }

                // Escape quits the compositor
                if pressed && raw == 1 {
                    info!("Escape pressed — shutting down");
                    self.running.store(false, Ordering::SeqCst);
                    return;
                }
                // Ctrl+C quits via key combo (TTY raw mode doesn't generate SIGINT)
                if pressed && raw == 46 && self.atlas.ctrl_pressed {
                    info!("Ctrl+C pressed — shutting down");
                    self.running.store(false, Ordering::SeqCst);
                    return;
                }
                // libinput returns evdev codes directly; pass them to handle_keyboard_event
                // (the winit backend's key_code() adds 8, so udev uses raw without adjustment)
                winit::handle_keyboard_event::<B>(&mut self.atlas, &event, &kb, raw);
            }
            InputEvent::PointerMotion { event } => {
                let current = self.atlas.pointer_location;
                let dx = event.delta_x();
                let dy = event.delta_y();
                debug!(dx, dy, pos_before = ?current, "PointerMotion event");
                let new_phys = Point::<f64, Physical>::from((
                    current.x + dx,
                    current.y + dy,
                ));
                let logical = Point::<f64, Logical>::from((new_phys.x, new_phys.y));
                winit::handle_motion_event(&mut self.atlas, &pt, new_phys, logical);
            }
            InputEvent::PointerButton { event } => {
                debug!(code = event.button_code(), state = ?event.state(), "PointerButton event");
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
                debug!(name = device.name(), syspath = ?device.syspath(), "Input device added");
            }
            InputEvent::DeviceRemoved { ref device } => {
                debug!(name = device.name(), "Input device removed");
            }
            _ => {}
        }
    }
}
