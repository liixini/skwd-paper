use crate::model::{BUFFER_COUNT, VideoInfo};
use crate::running;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_shm,
    output::{OutputHandler, OutputState},
    registry::RegistryState,
    shm::{Shm, ShmHandler, raw::RawPool},
};
use std::os::fd::AsRawFd;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::protocol::{wl_buffer, wl_output, wl_shm};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, globals::registry_queue_init,
};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

struct SurfaceState {
    name: String,
    output: Option<wl_output::WlOutput>,
    surface: WlSurface,
    layer_surface: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    viewport: wp_viewport::WpViewport,
    output_width: i32,
    output_height: i32,
    configured: bool,
    closed: bool,
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    surfaces: Vec<SurfaceState>,
    buffer_busy: [bool; BUFFER_COUNT],
    video: VideoInfo,
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        for surface in
            self.surfaces.iter_mut().filter(|surface| surface.output.as_ref() == Some(&output))
        {
            surface.closed = true;
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

paper_runtime::impl_layer_registry!(App);
delegate_shm!(App);
paper_runtime::impl_empty_dispatch!(
    App,
    zwlr_layer_shell_v1::ZwlrLayerShellV1,
    wp_viewporter::WpViewporter,
    wp_viewport::WpViewport,
);

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, String> for App {
    fn event(
        state: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        name: &String,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(surface) = state.surfaces.iter_mut().find(|surface| &surface.name == name) else {
            return;
        };
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                layer.ack_configure(serial);
                configure_surface(surface, state.video, width, height);
                surface.configured = true;
            }
            zwlr_layer_surface_v1::Event::Closed => surface.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, usize> for App {
    fn event(
        state: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_buffer::Event::Release)
            && let Some(busy) = state.buffer_busy.get_mut(*index)
        {
            *busy = false;
        }
    }
}

fn resolve_viewport(video: VideoInfo, width: u32, height: u32) -> (i32, i32, (u32, u32, u32, u32)) {
    let width = if width == 0 { video.width } else { width };
    let height = if height == 0 { video.height } else { height };
    (
        i32::try_from(width).expect("validated output width"),
        i32::try_from(height).expect("validated output height"),
        paper_geom::fill_crop_rect(video.width, video.height, width, height),
    )
}

fn configure_surface(surface: &mut SurfaceState, video: VideoInfo, width: u32, height: u32) {
    let (output_width, output_height, (x, y, source_width, source_height)) =
        resolve_viewport(video, width, height);
    surface.output_width = output_width;
    surface.output_height = output_height;
    surface.viewport.set_source(
        f64::from(x),
        f64::from(y),
        f64::from(source_width),
        f64::from(source_height),
    );
    surface.viewport.set_destination(output_width, output_height);
}

fn pending_output_names(state: &App) -> usize {
    state
        .output_state
        .outputs()
        .filter(|output| {
            output.version() >= 4
                && state.output_state.info(output).and_then(|info| info.name).is_none()
        })
        .count()
}

fn resolve_targets(
    state: &mut App,
    queue: &mut EventQueue<App>,
    requested: Option<Vec<String>>,
) -> Result<Vec<(String, Option<wl_output::WlOutput>)>, String> {
    let Some(requested) = requested else {
        return Ok(vec![("*".into(), None)]);
    };
    let mut metadata_roundtrips = 0;
    while pending_output_names(state) > 0 && metadata_roundtrips < 3 {
        queue
            .roundtrip(state)
            .map_err(|error| format!("output metadata roundtrip failed: {error}"))?;
        metadata_roundtrips += 1;
    }
    if pending_output_names(state) > 0 {
        return Err("compositor did not complete wl_output name metadata".into());
    }
    let outputs: Vec<wl_output::WlOutput> = state.output_state.outputs().collect();
    let selected: Vec<(String, wl_output::WlOutput)> = outputs
        .iter()
        .filter_map(|output| {
            let name = state.output_state.info(output)?.name?;
            requested.contains(&name).then(|| (name, output.clone()))
        })
        .collect();
    if selected.len() != requested.len() {
        let joined = requested.join(",");
        if !outputs.is_empty() && !outputs.iter().any(|output| output.version() >= 4) {
            return Err(format!(
                "compositor does not expose wl_output v4 names; cannot select '{joined}'"
            ));
        }
        return Err(format!("one or more Wayland outputs are unknown: '{joined}'"));
    }
    requested
        .into_iter()
        .map(|name| {
            let output = selected
                .iter()
                .find(|(candidate, _)| candidate == &name)
                .map(|(_, output)| output.clone())
                .ok_or_else(|| format!("unknown Wayland output '{name}'"))?;
            Ok((name, Some(output)))
        })
        .collect()
}

struct Buffers {
    pool: RawPool,
    handles: Vec<wl_buffer::WlBuffer>,
}

impl Buffers {
    fn allocate(shm: &Shm, qh: &QueueHandle<App>, video: VideoInfo) -> Result<Self, String> {
        let total = video
            .frame_bytes
            .checked_mul(BUFFER_COUNT)
            .ok_or_else(|| "wl_shm buffer size overflowed".to_string())?;
        let mut pool = RawPool::new(total, shm)
            .map_err(|error| format!("wl_shm pool allocation failed: {error}"))?;
        let width = i32::try_from(video.width).map_err(|_| "video width exceeds i32")?;
        let height = i32::try_from(video.height).map_err(|_| "video height exceeds i32")?;
        let stride = i32::try_from(video.stride).map_err(|_| "video stride exceeds i32")?;
        let mut handles = Vec::with_capacity(BUFFER_COUNT);
        for index in 0..BUFFER_COUNT {
            let offset = i32::try_from(index * video.frame_bytes)
                .map_err(|_| "wl_shm buffer offset exceeds i32")?;
            handles.push(pool.create_buffer(
                offset,
                width,
                height,
                stride,
                wl_shm::Format::Xrgb8888,
                index,
                qh,
            ));
        }
        Ok(Self { pool, handles })
    }
}

impl Drop for Buffers {
    fn drop(&mut self) {
        for handle in &self.handles {
            handle.destroy();
        }
    }
}

pub struct Wallpaper {
    buffers: Buffers,
    connection: Connection,
    queue: EventQueue<App>,
    state: App,
    video: VideoInfo,
}

impl Wallpaper {
    pub fn connect(outputs: Option<Vec<String>>, video: VideoInfo) -> Result<Self, String> {
        let connection = Connection::connect_to_env()
            .map_err(|error| format!("cannot connect to Wayland: {error}"))?;
        let (globals, mut queue) = registry_queue_init::<App>(&connection)
            .map_err(|error| format!("registry roundtrip failed: {error}"))?;
        let qh = queue.handle();
        let registry_state = RegistryState::new(&globals);
        let output_state = OutputState::new(&globals, &qh);
        let compositor = CompositorState::bind(&globals, &qh)
            .map_err(|_| "compositor lacks wl_compositor".to_string())?;
        let shm = Shm::bind(&globals, &qh)
            .map_err(|_| "compositor lacks wl_shm, layer-shell, or viewporter".to_string())?;
        let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals
            .bind(&qh, 1..=1, ())
            .map_err(|_| "compositor lacks wl_shm, layer-shell, or viewporter".to_string())?;
        let viewporter: wp_viewporter::WpViewporter = globals
            .bind(&qh, 1..=1, ())
            .map_err(|_| "compositor lacks wl_shm, layer-shell, or viewporter".to_string())?;
        let mut state = App {
            registry_state,
            output_state,
            shm,
            surfaces: Vec::new(),
            buffer_busy: [false; BUFFER_COUNT],
            video,
        };
        queue
            .roundtrip(&mut state)
            .map_err(|error| format!("registry roundtrip failed: {error}"))?;
        for (name, output) in resolve_targets(&mut state, &mut queue, outputs)? {
            let surface = compositor.create_surface(&qh);
            let layer_surface = layer_shell.get_layer_surface(
                &surface,
                output.as_ref(),
                zwlr_layer_shell_v1::Layer::Background,
                "skwd-paper-tinier".into(),
                &qh,
                name.clone(),
            );
            paper_runtime::layer_surface_defaults!(layer_surface);
            let viewport = viewporter.get_viewport(&surface, &qh, ());
            surface.commit();
            state.surfaces.push(SurfaceState {
                name,
                output,
                surface,
                layer_surface,
                viewport,
                output_width: 0,
                output_height: 0,
                configured: false,
                closed: false,
            });
        }
        while running()
            && state.surfaces.iter().any(|surface| !surface.closed && !surface.configured)
        {
            queue
                .blocking_dispatch(&mut state)
                .map_err(|error| format!("configure dispatch failed: {error}"))?;
        }
        if !running() || !state.surfaces.iter().any(|surface| !surface.closed && surface.configured)
        {
            return Err("Wayland surface closed before configuration".into());
        }
        let buffers = Buffers::allocate(&state.shm, &qh, video)?;
        Ok(Self { buffers, connection, queue, state, video })
    }

    pub fn output_sizes(&self) -> String {
        self.state
            .surfaces
            .iter()
            .filter(|surface| !surface.closed)
            .map(|surface| {
                format!("{}={}x{}", surface.name, surface.output_width, surface.output_height)
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn retain_outputs(&mut self, outputs: &[String]) -> Result<(), String> {
        for surface in self
            .state
            .surfaces
            .iter_mut()
            .filter(|surface| !outputs.iter().any(|output| output == &surface.name))
        {
            surface.viewport.destroy();
            surface.layer_surface.destroy();
            surface.surface.destroy();
            surface.closed = true;
        }
        self.connection.flush().map_err(|error| format!("Wayland flush failed: {error}"))
    }

    pub fn pixels_mut(&mut self, index: usize) -> &mut [u8] {
        let start = index * self.video.frame_bytes;
        &mut self.buffers.pool.mmap()[start..start + self.video.frame_bytes]
    }

    pub fn present(&mut self, index: usize) -> Result<(), String> {
        self.state.buffer_busy[index] = true;
        let width = i32::try_from(self.video.width).expect("validated video width fits i32");
        let height = i32::try_from(self.video.height).expect("validated video height fits i32");
        for target in self.state.surfaces.iter().filter(|surface| !surface.closed) {
            target.surface.attach(Some(&self.buffers.handles[index]), 0, 0);
            if target.surface.version() >= 4 {
                target.surface.damage_buffer(0, 0, width, height);
            } else {
                target.surface.damage(0, 0, i32::MAX, i32::MAX);
            }
            target.surface.commit();
        }
        match self.connection.flush() {
            Ok(()) => Ok(()),
            Err(wayland_client::backend::WaylandError::Io(error))
                if error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                Ok(())
            }
            Err(error) => Err(format!("Wayland flush failed: {error}")),
        }
    }

    pub fn wait_for_buffer(&mut self, index: usize) -> Result<bool, String> {
        while self.alive() && self.state.buffer_busy[index] {
            self.poll_dispatch(1000, None)?;
        }
        Ok(self.alive())
    }

    pub fn wait_until(&mut self, deadline: u64) -> Result<bool, String> {
        while self.alive() {
            self.dispatch()?;
            let now = crate::model::monotonic_ns()?;
            if now >= deadline {
                break;
            }
            let remaining = deadline - now;
            let milliseconds =
                i32::try_from(remaining.div_ceil(1_000_000).min(u64::try_from(i32::MAX).unwrap()))
                    .unwrap();
            self.poll_dispatch(milliseconds, None)?;
        }
        Ok(self.alive())
    }

    pub fn wait_idle(&mut self, wake_fd: libc::c_int) -> Result<(), String> {
        self.poll_dispatch(-1, Some(wake_fd))
    }

    pub fn alive(&self) -> bool {
        running() && self.state.surfaces.iter().any(|surface| !surface.closed)
    }

    fn dispatch(&mut self) -> Result<(), String> {
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|error| format!("Wayland dispatch failed: {error}"))?;
        Ok(())
    }

    fn poll_dispatch(
        &mut self,
        timeout_ms: i32,
        wake_fd: Option<libc::c_int>,
    ) -> Result<(), String> {
        self.dispatch()?;
        let Some(read_guard) = self.queue.prepare_read() else {
            return Ok(());
        };
        let mut events = libc::POLLIN;
        match self.connection.flush() {
            Ok(()) => {}
            Err(wayland_client::backend::WaylandError::Io(error))
                if error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                events |= libc::POLLOUT;
            }
            Err(error) => return Err(format!("Wayland flush failed: {error}")),
        }
        let mut descriptors = [
            libc::pollfd { fd: read_guard.connection_fd().as_raw_fd(), events, revents: 0 },
            libc::pollfd { fd: wake_fd.unwrap_or(-1), events: libc::POLLIN, revents: 0 },
        ];
        let count = if wake_fd.is_some() { 2 } else { 1 };
        let result =
            unsafe { libc::poll(descriptors.as_mut_ptr(), count as libc::nfds_t, timeout_ms) };
        if result < 0 {
            let error = std::io::Error::last_os_error();
            drop(read_guard);
            if error.kind() == std::io::ErrorKind::Interrupted {
                return Ok(());
            }
            return Err(format!("Wayland poll failed: {error}"));
        }
        let flush_again = descriptors[0].revents & libc::POLLOUT != 0;
        if descriptors[0].revents & libc::POLLIN != 0 {
            read_guard.read().map_err(|error| format!("Wayland read failed: {error}"))?;
            self.dispatch()?;
        } else {
            drop(read_guard);
        }
        if descriptors[..count].iter().any(|descriptor| {
            descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
        }) {
            return Err(if wake_fd.is_some() {
                "Wayland or control wake closed".into()
            } else {
                "Wayland connection closed".into()
            });
        }
        if flush_again {
            match self.connection.flush() {
                Ok(()) => {}
                Err(wayland_client::backend::WaylandError::Io(error))
                    if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(format!("Wayland flush failed: {error}")),
            }
        }
        Ok(())
    }
}

mod tests;
