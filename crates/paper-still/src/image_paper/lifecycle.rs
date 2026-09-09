use super::buffer_set::BufferSet;
use super::decode::decode_image;
use super::model::{App, SurfaceState};
use super::shm_pixels::pack_pixels;
use crate::fill_mode::{FillMode, apply_fill_mode};
use anyhow::{Context, Result, anyhow};
use paper_control::{Layer as PaperLayer, OutputTarget, StillCommand};
use smithay_client_toolkit::{
    compositor::CompositorState, output::OutputState, registry::RegistryState, shm::Shm,
};
use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd};
use std::sync::{Arc, Mutex};
use wayland_client::protocol::wl_output::Transform;
use wayland_client::{
    Connection, EventQueue,
    globals::{GlobalList, registry_queue_init},
    protocol::wl_output::WlOutput,
};
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{
    Layer as WaylandLayer, ZwlrLayerShellV1,
};

pub fn run(
    target: OutputTarget,
    file_path: &str,
    persist: bool,
    fill_mode: FillMode,
    namespace: &str,
    layer: PaperLayer,
    blur: f32,
    dim: u32,
) -> Result<()> {
    let (img_w, img_h, bytes) = decode_image(file_path, blur, dim)?;
    tracing::info!(w = img_w, h = img_h, ?fill_mode, blur, dim, "image decoded");

    let conn = Connection::connect_to_env().context("wayland connect")?;
    let (globals, mut event_queue): (GlobalList, EventQueue<App>) =
        registry_queue_init(&conn).context("registry_queue_init")?;
    let qh = event_queue.handle();

    let registry_state = RegistryState::new(&globals);
    let output_state = OutputState::new(&globals, &qh);
    let compositor_state =
        CompositorState::bind(&globals, &qh).context("compositor not available")?;
    let layer_shell: ZwlrLayerShellV1 =
        globals.bind(&qh, 1..=4, ()).context("zwlr_layer_shell_v1 not available")?;
    let viewporter: WpViewporter =
        globals.bind(&qh, 1..=1, ()).context("wp_viewporter not available")?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm not available")?;

    let pending_cmd: Arc<Mutex<Option<StillCommand>>> = Arc::new(Mutex::new(None));
    let wake = make_wake_pipe()?;
    let mut app = App {
        registry_state,
        output_state,
        compositor_state,
        shm,
        layer_shell,
        viewporter,
        qh,
        target,
        path: file_path.to_string(),
        raw_pixels: bytes,
        raw_w: img_w,
        raw_h: img_h,
        fill_mode,
        buffers: HashMap::new(),
        surfaces: Vec::new(),
        ready_signaled: false,
        persist,
        pending_cmd: pending_cmd.clone(),
        namespace: namespace.to_string(),
        layer,
        blur,
        dim,
        preloaded: HashMap::new(),
        slide: None,
        pending_preload: Vec::new(),
        retired: Vec::new(),
    };

    if persist {
        spawn_image_stdin_reader(pending_cmd, wake.clone());
    }

    event_queue.roundtrip(&mut app)?;
    app.spawn_initial_surfaces();

    run_event_loop(&mut app, &mut event_queue, &conn, wake)
}

fn make_wake_pipe() -> Result<paper_runtime::wake::Pipe> {
    paper_runtime::wake::make_pipe()
        .ok_or_else(|| anyhow!("pipe2 failed: {}", std::io::Error::last_os_error()))
}

#[allow(clippy::needless_pass_by_value)]
fn run_event_loop(
    app: &mut App,
    event_queue: &mut EventQueue<App>,
    conn: &Connection,
    wake: paper_runtime::wake::Pipe,
) -> Result<()> {
    let wl_fd = conn.as_fd().as_raw_fd();
    let wake_fd = wake.read_fd();
    loop {
        event_queue.dispatch_pending(app)?;
        app.try_consume_pending_cmd();
        app.try_release_pool();

        let Some(read_guard) = event_queue.prepare_read() else {
            continue;
        };
        let _ = conn.flush();

        let mut fds = [
            libc::pollfd { fd: wl_fd, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: wake_fd, events: libc::POLLIN, revents: 0 },
        ];
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                drop(read_guard);
                continue;
            }
            return Err(anyhow!("poll failed: {err}"));
        }

        if (fds[0].revents & libc::POLLIN) != 0 {
            match read_guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(err))
                    if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) => return Err(err.into()),
            }
        } else {
            drop(read_guard);
        }
        if (fds[1].revents & libc::POLLIN) != 0 {
            wake.drain();
        }
    }
}

fn spawn_image_stdin_reader(
    pending: Arc<Mutex<Option<StillCommand>>>,
    wake: paper_runtime::wake::Pipe,
) {
    paper_control::spawn_stdin_line_reader("skwd-wall-still persist", move |line| {
        match serde_json::from_str::<StillCommand>(line) {
            Ok(cmd) => {
                tracing::info!(path = %cmd.path, "image persist: command received");
                *pending.lock().unwrap() = Some(cmd);
                wake.poke();
            }
            Err(err) => {
                tracing::warn!(error = %err, line = %line, "image persist: bad command");
            }
        }
    });
}

pub(super) fn physical_size(
    surface: (u32, u32),
    logical: Option<(i32, i32)>,
    mode: Option<(i32, i32)>,
    transform: Transform,
    fallback_scale: i32,
) -> (u32, u32) {
    let rotated = matches!(
        transform,
        Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270
    );
    let pixels = mode.map(|(w, h)| if rotated { (h, w) } else { (w, h) });
    match (logical, pixels) {
        (Some((lw, lh)), Some((pw, ph))) if lw > 0 && lh > 0 && pw > 0 && ph > 0 => (
            (f64::from(surface.0) * f64::from(pw) / f64::from(lw)).round() as u32,
            (f64::from(surface.1) * f64::from(ph) / f64::from(lh)).round() as u32,
        ),
        _ => {
            let scale = fallback_scale.max(1) as u32;
            (surface.0 * scale, surface.1 * scale)
        }
    }
}

impl App {
    pub(super) fn spawn_initial_surfaces(&mut self) {
        let outputs: Vec<WlOutput> = self.output_state.outputs().collect();
        for output in outputs {
            self.maybe_create_surface(output);
        }
    }

    pub(super) fn maybe_create_surface(&mut self, output: WlOutput) {
        let Some(info) = self.output_state.info(&output) else {
            return;
        };
        let name = info.name.clone().unwrap_or_default();
        let pos = info.logical_position.unwrap_or(info.location);

        if !self.target.matches(&name) {
            return;
        }

        if self.surfaces.iter().any(|surf| surf.output_name == name) {
            return;
        }

        let surface = self.compositor_state.create_surface(&self.qh);
        let layer = self.layer_shell.get_layer_surface(
            &surface,
            Some(&output),
            match self.layer {
                PaperLayer::Background => WaylandLayer::Background,
                PaperLayer::Bottom => WaylandLayer::Bottom,
                PaperLayer::Top => WaylandLayer::Top,
                PaperLayer::Overlay => WaylandLayer::Overlay,
            },
            self.namespace.clone(),
            &self.qh,
            (),
        );
        paper_runtime::layer_surface_defaults!(layer);
        let viewport = self.viewporter.get_viewport(&surface, &self.qh, ());
        surface.commit();

        tracing::info!(output = %name, "created layer surface (image mode)");

        self.surfaces.push(SurfaceState {
            output,
            output_name: name,
            surface,
            layer,
            viewport,
            width: 0,
            height: 0,
            scale: 1,
            pos,
            attached: false,
            span_pool: None,
            span_keepalive: None,
        });
    }

    pub(super) fn surface_physical(&self, surf: &SurfaceState) -> (u32, u32) {
        let info = self.output_state.info(&surf.output);
        let logical = info.as_ref().and_then(|info| info.logical_size);
        let mode = info
            .as_ref()
            .and_then(|info| info.modes.iter().find(|mode| mode.current))
            .map(|mode| mode.dimensions);
        let transform = info.as_ref().map_or(Transform::Normal, |info| info.transform);
        physical_size((surf.width, surf.height), logical, mode, transform, surf.scale)
    }

    pub(super) fn ensure_buffer_for(&mut self, surf_w: u32, surf_h: u32) -> Result<bool> {
        if surf_w == 0 || surf_h == 0 || self.buffers.contains_key(&(surf_w, surf_h)) {
            return Ok(false);
        }
        let (raw_w, raw_h, raw) = if self.raw_pixels.is_empty() {
            decode_image(&self.path, self.blur, self.dim)?
        } else {
            (self.raw_w, self.raw_h, std::mem::take(&mut self.raw_pixels))
        };
        let (bw, bh, pixels) = apply_fill_mode(raw_w, raw_h, &raw, surf_w, surf_h, self.fill_mode);
        drop(raw);
        let buffer = BufferSet::new(&self.shm, bw, bh, self.surfaces.len(), |canvas, format| {
            pack_pixels(canvas, &pixels, format);
        })?;
        self.buffers.insert((surf_w, surf_h), buffer);
        Ok(true)
    }

    pub(super) fn distinct_targets(&self) -> Vec<(u32, u32)> {
        let mut targets = Vec::new();
        for surf in &self.surfaces {
            if surf.width == 0 || surf.height == 0 {
                continue;
            }
            let target = self.surface_physical(surf);
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        targets
    }

    pub(super) fn try_release_pool(&mut self) {
        self.retired.retain(BufferSet::has_active_buffers);
        self.buffers.retain(|_, buffer| buffer.has_active_buffers());
    }

    pub(super) fn retire_current(&mut self) {
        for (_, buffer) in self.buffers.drain() {
            if buffer.has_active_buffers() {
                self.retired.push(buffer);
            }
        }
    }

    pub(super) fn attach_to(&mut self, idx: usize) {
        if self.surfaces[idx].width == 0 || self.surfaces[idx].height == 0 {
            return;
        }
        if self.slide.is_some() {
            self.finish_slide();
        }
        if self.fill_mode == FillMode::Span {
            self.attach_span(idx);
            return;
        }
        let (sw, sh) = self.surface_physical(&self.surfaces[idx]);
        let rebuilt = match self.ensure_buffer_for(sw, sh) {
            Ok(rebuilt) => rebuilt,
            Err(err) => {
                tracing::error!(error = %err, "ensure_buffer_for failed");
                return;
            }
        };
        let Some((bw, bh)) = self.buffers.get(&(sw, sh)).map(BufferSet::dimensions) else {
            return;
        };
        let indices: Vec<usize> = if rebuilt {
            (0..self.surfaces.len())
                .filter(|&other| self.surface_physical(&self.surfaces[other]) == (sw, sh))
                .collect()
        } else {
            vec![idx]
        };
        for attach_idx in indices {
            let surf = &mut self.surfaces[attach_idx];
            if surf.width == 0 || surf.height == 0 {
                continue;
            }
            surf.viewport.set_source(0.0, 0.0, bw as f64, bh as f64);
            surf.viewport.set_destination(surf.width as i32, surf.height as i32);
            let Some(buffer) = self.buffers.get_mut(&(sw, sh)) else {
                return;
            };
            if let Err(err) = buffer.attach_to(attach_idx, &surf.surface) {
                tracing::error!(error = %err, "attach buffer failed");
                return;
            }
            surf.surface.damage_buffer(0, 0, bw as i32, bh as i32);
            surf.attached = true;
            surf.surface.commit();
        }
        if let Some(buffer) = self.buffers.get_mut(&(sw, sh)) {
            buffer.drop_local_pages();
        }
        if !self.ready_signaled {
            crate::ipc::signal_ready();
            self.ready_signaled = true;
        }
        if !self.pending_preload.is_empty() {
            let deferred = std::mem::take(&mut self.pending_preload);
            self.do_preload(&deferred);
        }
    }
}
