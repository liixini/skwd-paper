use super::model::{App, Target};
use anyhow::{Context, Result, anyhow};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use wayland_client::protocol::{wl_buffer, wl_pointer, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

const NIRI_SOCKET: &str = "NIRI_SOCKET";
const POINTER_NAMESPACE_SUFFIX: &str = "-pointer";
const CLEAR_PIXEL_BYTES: i32 = 4;
const CLEAR_PIXEL_NAME: &std::ffi::CStr = c"skwd-vk-pointer";

pub(super) struct PointerSurface {
    output: usize,
    surface: wl_surface::WlSurface,
    layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    viewport: WpViewport,
    buffer: wl_buffer::WlBuffer,
}

impl Drop for PointerSurface {
    fn drop(&mut self) {
        self.viewport.destroy();
        self.layer.destroy();
        self.surface.destroy();
        self.buffer.destroy();
    }
}

#[derive(Clone, Copy)]
pub(super) struct PointerLayer(usize);

fn clear_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<App>,
    index: usize,
) -> Result<wl_buffer::WlBuffer> {
    let fd = unsafe { libc::memfd_create(CLEAR_PIXEL_NAME.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(anyhow!("memfd_create failed"));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    if unsafe { libc::ftruncate(fd.as_raw_fd(), i64::from(CLEAR_PIXEL_BYTES)) } != 0 {
        return Err(anyhow!("ftruncate failed"));
    }
    let pool = shm.create_pool(fd.as_fd(), CLEAR_PIXEL_BYTES, qh, ());
    let buffer = pool.create_buffer(
        0,
        1,
        1,
        CLEAR_PIXEL_BYTES,
        wl_shm::Format::Argb8888,
        qh,
        PointerLayer(index),
    );
    pool.destroy();
    Ok(buffer)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Mouse {
    pub surface: Option<usize>,
    pub position: [f32; 2],
    pub buttons: [bool; 3],
    pub revision: u64,
}

impl App {
    fn bind_pointer(&mut self, qh: &QueueHandle<Self>) {
        if self.pointer_enabled && self.pointer_capable && self.pointer.is_none() {
            self.pointer = self.seat.as_ref().map(|seat| seat.get_pointer(qh, ()));
        }
    }

    fn pointer_motion(&mut self, x: f64, y: f64) {
        let Some(index) = self.pointer_focus else { return };
        let surface = &self.surfaces[index];
        if !x.is_finite() || !y.is_finite() || surface.width == 0 || surface.height == 0 {
            return;
        }
        self.mouse.surface = Some(index);
        self.mouse.position = [
            (x as f32 / surface.width as f32).clamp(0.0, 1.0),
            (y as f32 / surface.height as f32).clamp(0.0, 1.0),
        ];
        self.mouse.revision = self.mouse.revision.wrapping_add(1);
    }

    fn pointer_leave(&mut self) {
        self.pointer_focus = None;
        self.mouse.position = [-1.0; 2];
        self.mouse.buttons = [false; 3];
        self.mouse.revision = self.mouse.revision.wrapping_add(1);
    }
}

impl Target {
    pub fn enable_pointer(&mut self) {
        self.app.pointer_enabled = true;
        self.app.bind_pointer(&self.queue.handle());
    }

    pub fn sync_desktop_pointer(&mut self, reads_pointer: bool) {
        if !reads_pointer {
            self.app.pointer_surfaces.clear();
            return;
        }
        if std::env::var_os(NIRI_SOCKET).is_none()
            || self.layer != zwlr_layer_shell_v1::Layer::Background
            || !self.app.pointer_surfaces.is_empty()
        {
            return;
        }
        match self.app.pointer_surfaces(&self.queue.handle()) {
            Ok(count) => tracing::info!("skwd-wall-vk: pointer surfaces on {count} output(s)"),
            Err(error) => tracing::warn!("skwd-wall-vk: pointer surfaces unavailable: {error:#}"),
        }
    }
}

impl App {
    fn pointer_surfaces(&mut self, qh: &QueueHandle<Self>) -> Result<usize> {
        let compositor = self.compositor.clone().context("no wl_compositor")?;
        let shell = self.layer_shell.clone().context("no zwlr_layer_shell_v1")?;
        let viewporter = self.viewporter.clone().context("no wp_viewporter")?;
        let shm = self.shm.clone().context("no wl_shm")?;
        let namespace = format!("{}{POINTER_NAMESPACE_SUFFIX}", super::target::namespace());
        for (index, ui) in self.surfaces.iter().enumerate() {
            if ui.closed {
                continue;
            }
            let buffer = clear_buffer(&shm, qh, index)?;
            let surface = compositor.create_surface(qh, ());
            let layer = shell.get_layer_surface(
                &surface,
                Some(&ui.output),
                zwlr_layer_shell_v1::Layer::Background,
                namespace.clone(),
                qh,
                PointerLayer(index),
            );
            super::target::fill_output(&layer);
            surface.commit();
            let viewport = viewporter.get_viewport(&surface, qh, ());
            self.pointer_surfaces.push(PointerSurface {
                output: index,
                surface,
                layer,
                viewport,
                buffer,
            });
        }
        Ok(self.pointer_surfaces.len())
    }

    fn pointer_output(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.surfaces.iter().position(|ui| ui.surface == *surface).or_else(|| {
            self.pointer_surfaces.iter().find(|pad| pad.surface == *surface).map(|pad| pad.output)
        })
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, PointerLayer> for App {
    fn event(
        state: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        PointerLayer(index): &PointerLayer,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let zwlr_layer_surface_v1::Event::Configure { serial, width, height } = event else {
            if matches!(event, zwlr_layer_surface_v1::Event::Closed) {
                state.pointer_surfaces.retain(|pad| pad.output != *index);
            }
            return;
        };
        layer.ack_configure(serial);
        let Some(pad) = state.pointer_surfaces.iter().find(|pad| pad.output == *index) else {
            return;
        };
        if width > 0 && height > 0 {
            pad.viewport.set_destination(width as i32, height as i32);
        }
        pad.surface.attach(Some(&pad.buffer), 0, 0);
        pad.surface.commit();
    }
}

impl Dispatch<wl_buffer::WlBuffer, PointerLayer> for App {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &PointerLayer,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(capabilities) } = event {
            state.pointer_capable = capabilities.contains(wl_seat::Capability::Pointer);
            if !state.pointer_capable {
                if let Some(pointer) = state.pointer.take()
                    && pointer.version() >= 3
                {
                    pointer.release();
                }
                state.pointer_leave();
            }
            state.bind_pointer(qh);
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { surface, surface_x, surface_y, .. } => {
                state.pointer_focus = state.pointer_output(&surface);
                state.pointer_motion(surface_x, surface_y);
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                state.pointer_motion(surface_x, surface_y)
            }
            wl_pointer::Event::Leave { .. } => state.pointer_leave(),
            wl_pointer::Event::Button { button, state: WEnum::Value(button_state), .. } => {
                if state.pointer_focus.is_some() && (0x110..=0x112).contains(&button) {
                    state.mouse.buttons[(button - 0x110) as usize] =
                        button_state == wl_pointer::ButtonState::Pressed;
                    state.mouse.revision = state.mouse.revision.wrapping_add(1);
                }
            }
            _ => {}
        }
    }
}
