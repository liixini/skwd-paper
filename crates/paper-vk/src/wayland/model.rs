use std::collections::HashMap;
use wayland_client::protocol::{
    wl_compositor, wl_output, wl_pointer, wl_region, wl_seat, wl_shm, wl_subcompositor, wl_surface,
};
use wayland_client::{Connection, EventQueue, Proxy};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use wayland_protocols::wp::content_type::v1::client::{
    wp_content_type_manager_v1::WpContentTypeManagerV1, wp_content_type_v1::WpContentTypeV1,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1;
use wayland_protocols::wp::presentation_time::client::wp_presentation::WpPresentation;
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1, zxdg_output_v1::ZxdgOutputV1,
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::ZwlrLayerShellV1, zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
};

static COMPOSITOR_DRM_DEVICE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn parse_drm_device(bytes: &[u8]) -> Option<libc::dev_t> {
    let bytes: [u8; std::mem::size_of::<libc::dev_t>()] = bytes.try_into().ok()?;
    Some(libc::dev_t::from_ne_bytes(bytes))
}

pub(crate) fn set_compositor_drm_device(device: libc::dev_t) {
    COMPOSITOR_DRM_DEVICE.store(device, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn compositor_drm_device() -> Option<libc::dev_t> {
    let device = COMPOSITOR_DRM_DEVICE.load(std::sync::atomic::Ordering::Relaxed);
    (device != 0).then_some(device as libc::dev_t)
}

pub struct SurfaceUi {
    pub name: String,
    pub output: wl_output::WlOutput,
    pub position: Option<(i32, i32)>,
    pub surface: wl_surface::WlSurface,
    pub(super) _input_region: Option<wl_region::WlRegion>,
    pub(super) layer: ZwlrLayerSurfaceV1,
    pub width: u32,
    pub height: u32,
    pub scale: i32,
    pub mode: (u32, u32),
    pub configured: bool,
    pub closed: bool,
    pub free_buffers: Vec<usize>,
    pub last_flip_ns: u64,
    pub refresh_ns: u32,
    pub fps_limit: u32,
    pub next_commit_ns: u64,
    pub presented: u64,
    pub discarded: u64,
    pub feedbacks: Vec<u64>,
    pub viewport: Option<WpViewport>,
    pub(super) _content_type: Option<WpContentTypeV1>,
    pub(crate) source: Option<[u32; 4]>,
    pub(super) attached: Option<wayland_client::protocol::wl_buffer::WlBuffer>,
    pub(super) mirror: Option<super::mirror::SurfaceMirror>,
}

#[derive(Clone)]
pub(super) struct OutputInfo {
    pub output: wl_output::WlOutput,
    pub name: String,
    pub scale: i32,
    pub mode: (u32, u32),
    pub geometry_position: Option<(i32, i32)>,
    pub logical_position: Option<(i32, i32)>,
    pub xdg_output: Option<ZxdgOutputV1>,
}

impl OutputInfo {
    pub fn position(&self) -> Option<(i32, i32)> {
        self.logical_position.or(self.geometry_position)
    }
}

pub struct App {
    pub(super) compositor: Option<wl_compositor::WlCompositor>,
    pub(super) layer_shell: Option<ZwlrLayerShellV1>,
    pub(super) dmabuf: Option<ZwpLinuxDmabufV1>,
    pub dmabuf_formats: Vec<(u32, u64)>,
    pub(super) dmabuf_feedback_table: Vec<(u32, u64)>,
    pub(super) dmabuf_feedback_indices: Vec<u16>,
    pub(super) dmabuf_feedback_target: Option<libc::dev_t>,
    pub(super) dmabuf_feedback_scanout: bool,
    pub(super) dmabuf_feedback_best: Vec<(u32, u64)>,
    pub(super) dmabuf_feedback_all: Vec<(u32, u64)>,
    pub(super) dmabuf_feedback_score: u8,
    pub(super) viewporter: Option<WpViewporter>,
    pub(super) xdg_output_manager: Option<ZxdgOutputManagerV1>,
    pub(super) content_type_manager: Option<WpContentTypeManagerV1>,
    pub(super) shm: Option<wl_shm::WlShm>,
    pub(super) subcompositor: Option<wl_subcompositor::WlSubcompositor>,
    pub(super) widgets: Option<super::widgets::Widgets>,
    pub(super) outputs: Vec<OutputInfo>,
    pub surfaces: Vec<SurfaceUi>,
    pub closed: bool,
    pub frame_fired: bool,
    pub frame_ts_ms: u32,
    pub(super) presentation: Option<WpPresentation>,
    pub(super) idle_notifier: Option<ExtIdleNotifierV1>,
    pub(super) seat: Option<wl_seat::WlSeat>,
    pub(super) pointer: Option<wl_pointer::WlPointer>,
    pub(super) pointer_enabled: bool,
    pub(super) pointer_capable: bool,
    pub(super) pointer_focus: Option<usize>,
    pub mouse: super::pointer::Mouse,
    pub idle: bool,
    pub shm_formats: Vec<wl_shm::Format>,
    pub probe_result: Option<bool>,
    pub resized: bool,
    pub(super) mirror_buffers: super::mirror::Buffers,
    pub(super) buffer_generations: BufferGenerations,
    pub(super) pointer_surfaces: Vec<super::pointer::PointerSurface>,
}

#[derive(Default)]
pub(super) struct BufferGenerations {
    current: u64,
    staging: bool,
    pending: HashMap<u64, usize>,
}

impl BufferGenerations {
    pub(super) fn current(&self) -> u64 {
        self.current
    }

    pub(super) fn allocation(&self) -> u64 {
        self.current.checked_add(u64::from(self.staging)).expect("buffer generation exhausted")
    }

    pub(super) fn stage(&mut self, staging: bool) {
        self.staging = staging;
    }

    pub(super) fn rotate(&mut self, outstanding: usize) -> u64 {
        let old = self.current;
        if outstanding > 0 {
            self.pending.insert(old, outstanding);
        }
        self.current = self.current.checked_add(1).expect("buffer generation exhausted");
        old
    }

    pub(super) fn release(&mut self, generation: u64) -> bool {
        if generation == self.current {
            return true;
        }
        if let Some(count) = self.pending.get_mut(&generation) {
            *count -= 1;
            if *count == 0 {
                self.pending.remove(&generation);
            }
        }
        false
    }

    pub(super) fn retired(&self, generation: u64) -> bool {
        !self.pending.contains_key(&generation)
    }
}

impl App {
    pub(super) fn set_output_position(
        &mut self,
        output: &wl_output::WlOutput,
        position: (i32, i32),
        logical: bool,
    ) {
        let Some(info) = self.outputs.iter_mut().find(|info| info.output.id() == output.id())
        else {
            return;
        };
        if logical {
            info.logical_position = Some(position);
        } else {
            info.geometry_position = Some(position);
        }
        let position = info.position();
        for surface in self.surfaces.iter_mut().filter(|surface| surface.output.id() == output.id())
        {
            if surface.position != position {
                surface.position = position;
                if surface.configured && !surface.closed {
                    self.resized = true;
                }
            }
        }
    }
}

pub struct Target {
    pub hidden: bool,
    pub layer: wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer,
    pub conn: Connection,
    pub queue: EventQueue<App>,
    pub app: App,
    pub ctl_fd: Option<std::os::fd::RawFd>,
}

#[cfg(test)]
mod tests;
