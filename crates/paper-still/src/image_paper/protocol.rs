use super::model::App;
use crate::fill_mode::FillMode;
use smithay_client_toolkit::{
    compositor::CompositorHandler,
    delegate_shm,
    output::{OutputHandler, OutputState},
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    protocol::{wl_output::WlOutput, wl_surface::WlSurface},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::ZwlrLayerShellV1, zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
};

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &WlSurface,
        new_scale: i32,
    ) {
        let mut changed = None;
        for (idx, surf) in self.surfaces.iter_mut().enumerate() {
            if &surf.surface == surface && surf.scale != new_scale {
                surf.scale = new_scale;
                changed = Some(idx);
            }
        }
        if let Some(idx) = changed {
            self.retire_current();
            self.attach_to(idx);
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: wayland_client::protocol::wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: u32) {
        self.slide_tick();
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: WlOutput) {
        self.maybe_create_surface(output);
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {
        if self.fill_mode == FillMode::Span && self.refresh_span_positions() {
            self.attach_all_span();
        }
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: WlOutput) {
        self.surfaces.retain(|surf| surf.output != output);
        if self.fill_mode == FillMode::Span {
            self.attach_all_span();
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

impl wayland_client::Dispatch<ZwlrLayerSurfaceV1, ()> for App {
    fn event(
        state: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Event;
        match event {
            Event::Configure { serial, width, height } => {
                layer.ack_configure(serial);
                let Some(idx) = state.surfaces.iter().position(|surf| &surf.layer == layer) else {
                    return;
                };
                if width > 0 && height > 0 {
                    state.surfaces[idx].width = width;
                    state.surfaces[idx].height = height;
                }
                if state.fill_mode == FillMode::Span {
                    state.refresh_span_positions();
                    state.attach_all_span();
                } else {
                    state.attach_to(idx);
                }
            }
            Event::Closed => {
                state.surfaces.retain(|surf| &surf.layer != layer);
                if state.surfaces.is_empty() {
                    std::process::exit(0);
                }
            }
            _ => {}
        }
    }
}

paper_runtime::impl_empty_dispatch!(App, ZwlrLayerShellV1, WpViewporter, WpViewport);
