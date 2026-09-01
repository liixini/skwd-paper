use super::buffer_set::BufferSet;
use crate::fill_mode::FillMode;
use paper_control::{OutputTarget, StillCommand};
use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    registry::RegistryState,
    shm::{Shm, slot::SlotPool},
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wayland_client::{
    QueueHandle,
    protocol::{wl_output::WlOutput, wl_surface::WlSurface},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::ZwlrLayerShellV1, zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
};

pub(super) struct ReadyBuffer {
    pub(super) buffer: BufferSet,
    pub(super) for_w: u32,
    pub(super) for_h: u32,
}

pub(super) struct SlideAnim {
    pub(super) buffer: BufferSet,
    pub(super) w: u32,
    pub(super) h: u32,
    pub(super) dir_up: bool,
    pub(super) started: Instant,
    pub(super) duration: Duration,
    pub(super) target: String,
}

pub(super) struct App {
    pub(super) registry_state: RegistryState,
    pub(super) output_state: OutputState,
    pub(super) compositor_state: CompositorState,
    pub(super) shm: Shm,
    pub(super) layer_shell: ZwlrLayerShellV1,
    pub(super) viewporter: WpViewporter,
    pub(super) qh: QueueHandle<App>,
    pub(super) target: OutputTarget,
    pub(super) path: String,
    pub(super) raw_pixels: Vec<u8>,
    pub(super) raw_w: u32,
    pub(super) raw_h: u32,
    pub(super) fill_mode: FillMode,
    pub(super) buffers: HashMap<(u32, u32), BufferSet>,
    pub(super) surfaces: Vec<SurfaceState>,
    pub(super) ready_signaled: bool,
    pub(super) persist: bool,
    pub(super) pending_cmd: Arc<Mutex<Option<StillCommand>>>,
    pub(super) namespace: String,
    pub(super) layer: paper_control::Layer,
    pub(super) blur: f32,
    pub(super) dim: u32,
    pub(super) preloaded: HashMap<String, ReadyBuffer>,
    pub(super) slide: Option<SlideAnim>,
    pub(super) pending_preload: Vec<String>,
    pub(super) retired: Vec<BufferSet>,
}

pub(super) struct SurfaceState {
    pub(super) output: WlOutput,
    pub(super) output_name: String,
    pub(super) surface: WlSurface,
    pub(super) layer: ZwlrLayerSurfaceV1,
    pub(super) viewport: WpViewport,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) scale: i32,
    pub(super) pos: (i32, i32),
    pub(super) attached: bool,
    pub(super) span_pool: Option<SlotPool>,
    pub(super) span_keepalive: Option<smithay_client_toolkit::shm::slot::Buffer>,
}
