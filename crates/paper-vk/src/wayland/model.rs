use wayland_client::protocol::{wl_compositor, wl_output, wl_region, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, EventQueue};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use wayland_protocols::wp::content_type::v1::client::{
    wp_content_type_manager_v1::WpContentTypeManagerV1, wp_content_type_v1::WpContentTypeV1,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1;
use wayland_protocols::wp::presentation_time::client::wp_presentation::WpPresentation;
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
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
    pub surface: wl_surface::WlSurface,
    pub(super) _input_region: Option<wl_region::WlRegion>,
    pub(super) _layer: ZwlrLayerSurfaceV1,
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
    pub(super) content_type_manager: Option<WpContentTypeManagerV1>,
    pub(super) shm: Option<wl_shm::WlShm>,
    pub(super) outputs: Vec<(wl_output::WlOutput, String, i32, (u32, u32))>,
    pub surfaces: Vec<SurfaceUi>,
    pub closed: bool,
    pub frame_fired: bool,
    pub frame_ts_ms: u32,
    pub(super) presentation: Option<WpPresentation>,
    pub(super) idle_notifier: Option<ExtIdleNotifierV1>,
    pub(super) seat: Option<wl_seat::WlSeat>,
    pub idle: bool,
    pub shm_formats: Vec<wl_shm::Format>,
    pub probe_result: Option<bool>,
    pub resized: bool,
}

pub struct Target {
    pub layer: wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer,
    pub conn: Connection,
    pub queue: EventQueue<App>,
    pub app: App,
    pub ctl_fd: Option<std::os::fd::RawFd>,
}

#[cfg(test)]
mod tests;
