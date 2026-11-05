use super::model::{App, OutputInfo};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_output, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::{self, ExtIdleNotifierV1},
};
use wayland_protocols::wp::content_type::v1::client::{
    wp_content_type_manager_v1::{self, WpContentTypeManagerV1},
    wp_content_type_v1::{self, WpContentTypeV1},
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1::{self, ZwpLinuxBufferParamsV1},
    zwp_linux_dmabuf_feedback_v1::{self, ZwpLinuxDmabufFeedbackV1},
    zwp_linux_dmabuf_v1::{self, ZwpLinuxDmabufV1},
};
use wayland_protocols::wp::presentation_time::client::{
    wp_presentation::{self, WpPresentation},
    wp_presentation_feedback::{self, WpPresentationFeedback},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::{self, WpViewport},
    wp_viewporter::{self, WpViewporter},
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::{self, ZxdgOutputManagerV1},
    zxdg_output_v1::{self, ZxdgOutputV1},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};

const XDG_OUTPUT_MANAGER: &str = "zxdg_output_manager_v1";
const XDG_OUTPUT_VERSION: u32 = 3;

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            if std::env::var("SKWD_VK_GLOBALS").is_ok() {
                tracing::info!("global: {interface} v{version}");
            }
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind::<wl_compositor::WlCompositor, _, _>(
                        name,
                        version.min(4),
                        qh,
                        (),
                    ));
                }
                "zwlr_layer_shell_v1" => {
                    state.layer_shell =
                        Some(registry.bind::<ZwlrLayerShellV1, _, _>(name, version.min(4), qh, ()));
                }
                "wp_viewporter" => {
                    state.viewporter = Some(registry.bind::<WpViewporter, _, _>(name, 1, qh, ()));
                }
                XDG_OUTPUT_MANAGER => {
                    state.xdg_output_manager = Some(registry.bind::<ZxdgOutputManagerV1, _, _>(
                        name,
                        version.min(XDG_OUTPUT_VERSION),
                        qh,
                        (),
                    ));
                }
                "wp_content_type_manager_v1" => {
                    state.content_type_manager =
                        Some(registry.bind::<WpContentTypeManagerV1, _, _>(name, 1, qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind::<wl_shm::WlShm, _, _>(name, 1, qh, ()));
                }
                "wp_presentation" => {
                    state.presentation =
                        Some(registry.bind::<WpPresentation, _, _>(name, version.min(1), qh, ()));
                }
                "zwp_linux_dmabuf_v1" => {
                    // v3 carries the modifier events the presentation code reads; the extra v4
                    // binding only reports the preferred DRM device.
                    state.dmabuf =
                        Some(registry.bind::<ZwpLinuxDmabufV1, _, _>(name, version.min(3), qh, ()));
                    if version >= 4 {
                        let feedback_factory =
                            registry.bind::<ZwpLinuxDmabufV1, _, _>(name, 4, qh, ());
                        feedback_factory.get_default_feedback(qh, ());
                        feedback_factory.destroy();
                    }
                }
                "wl_output" => {
                    let out =
                        registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, ());
                    state.outputs.push(OutputInfo {
                        output: out,
                        name: String::new(),
                        scale: 1,
                        mode: (0, 0),
                        geometry_position: None,
                        logical_position: None,
                        xdg_output: None,
                    });
                }
                "ext_idle_notifier_v1" => {
                    state.idle_notifier =
                        Some(registry.bind::<ExtIdleNotifierV1, _, _>(name, 1, qh, ()));
                }
                "wl_seat" if state.seat.is_none() => {
                    state.seat =
                        Some(registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(5), qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for App {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Name { name } => {
                if let Some(info) =
                    state.outputs.iter_mut().find(|info| info.output.id() == output.id())
                {
                    info.name = name;
                }
            }
            wl_output::Event::Scale { factor } => {
                if let Some(info) =
                    state.outputs.iter_mut().find(|info| info.output.id() == output.id())
                {
                    info.scale = factor.max(1);
                    for surface in
                        state.surfaces.iter_mut().filter(|surface| surface.output == *output)
                    {
                        if surface.scale != info.scale {
                            surface.scale = info.scale;
                            state.resized |= surface.configured && !surface.closed;
                        }
                    }
                }
            }
            wl_output::Event::Mode { flags, width, height, .. } => {
                if let wayland_client::WEnum::Value(fl) = flags
                    && fl.contains(wl_output::Mode::Current)
                    && width > 0
                    && height > 0
                    && let Some(info) =
                        state.outputs.iter_mut().find(|info| info.output.id() == output.id())
                {
                    info.mode = (width as u32, height as u32);
                    for surface in
                        state.surfaces.iter_mut().filter(|surface| surface.output == *output)
                    {
                        if surface.mode != info.mode {
                            surface.mode = info.mode;
                            state.resized |= surface.configured && !surface.closed;
                        }
                    }
                }
            }
            wl_output::Event::Geometry { x, y, .. } => {
                state.set_output_position(output, (x, y), false)
            }
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZxdgOutputManagerV1,
        _: zxdg_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgOutputV1, usize> for App {
    fn event(
        state: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(info) = state.outputs.get(*index) else {
            return;
        };
        let output = info.output.clone();
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                state.set_output_position(&output, (x, y), true);
            }
            zxdg_output_v1::Event::Name { name } => {
                if state.outputs[*index].name.is_empty() {
                    state.outputs[*index].name = name;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpLinuxDmabufV1, ()> for App {
    fn event(
        state: &mut Self,
        _: &ZwpLinuxDmabufV1,
        event: zwp_linux_dmabuf_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_linux_dmabuf_v1::Event::Modifier { format, modifier_hi, modifier_lo } = event {
            if !state.dmabuf_feedback_best.is_empty() {
                return;
            }
            let modifier = (u64::from(modifier_hi) << 32) | u64::from(modifier_lo);
            state.dmabuf_formats.push((format, modifier));
            if format == 0x3231_564e {
                tracing::info!("compositor accepts NV12 dmabuf, modifier {modifier:#018x}");
            }
        }
    }
}

impl Dispatch<ZwpLinuxDmabufFeedbackV1, ()> for App {
    fn event(
        state: &mut Self,
        feedback: &ZwpLinuxDmabufFeedbackV1,
        event: zwp_linux_dmabuf_feedback_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_linux_dmabuf_feedback_v1::Event::FormatTable { fd, size } => {
                state.dmabuf_feedback_table = read_dmabuf_format_table(&fd, size as usize);
            }
            zwp_linux_dmabuf_feedback_v1::Event::MainDevice { device } => {
                if let Some(device) = super::model::parse_drm_device(&device) {
                    super::model::set_compositor_drm_device(device);
                    tracing::info!(
                        major = libc::major(device),
                        minor = libc::minor(device),
                        "skwd-wall-vk: compositor DRM device feedback"
                    );
                } else {
                    tracing::warn!(
                        bytes = device.len(),
                        "skwd-wall-vk: compositor sent invalid DRM device feedback"
                    );
                }
            }
            zwp_linux_dmabuf_feedback_v1::Event::TrancheTargetDevice { device } => {
                state.dmabuf_feedback_target = super::model::parse_drm_device(&device);
            }
            zwp_linux_dmabuf_feedback_v1::Event::TrancheFormats { indices } => {
                state.dmabuf_feedback_indices = indices
                    .chunks_exact(2)
                    .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
                    .collect();
            }
            zwp_linux_dmabuf_feedback_v1::Event::TrancheFlags { flags } => {
                state.dmabuf_feedback_scanout = matches!(
                    flags,
                    wayland_client::WEnum::Value(value)
                        if value.contains(zwp_linux_dmabuf_feedback_v1::TrancheFlags::Scanout)
                );
            }
            zwp_linux_dmabuf_feedback_v1::Event::TrancheDone => {
                let main = super::model::compositor_drm_device();
                let score = u8::from(main.is_some() && state.dmabuf_feedback_target == main)
                    + 2 * u8::from(state.dmabuf_feedback_scanout);
                let formats = state
                    .dmabuf_feedback_indices
                    .iter()
                    .filter_map(|&index| {
                        state.dmabuf_feedback_table.get(usize::from(index)).copied()
                    })
                    .collect::<Vec<_>>();
                for format in &formats {
                    if !state.dmabuf_feedback_all.contains(format) {
                        state.dmabuf_feedback_all.push(*format);
                    }
                }
                if !formats.is_empty()
                    && (state.dmabuf_feedback_best.is_empty()
                        || score > state.dmabuf_feedback_score)
                {
                    state.dmabuf_feedback_best = formats;
                    state.dmabuf_feedback_score = score;
                }
                state.dmabuf_feedback_indices.clear();
                state.dmabuf_feedback_target = None;
                state.dmabuf_feedback_scanout = false;
            }
            zwp_linux_dmabuf_feedback_v1::Event::Done => {
                if !state.dmabuf_feedback_best.is_empty() {
                    let mut formats = state.dmabuf_feedback_best.clone();
                    for format in &state.dmabuf_feedback_all {
                        if !formats.contains(format) {
                            formats.push(*format);
                        }
                    }
                    state.dmabuf_formats = formats;
                    tracing::info!(
                        "skwd-wall-vk: using {} preferred DMA-BUF format/modifier pairs",
                        state.dmabuf_formats.len()
                    );
                }
                feedback.destroy();
            }
            _ => {}
        }
    }
}

fn read_dmabuf_format_table(fd: &std::os::fd::OwnedFd, size: usize) -> Vec<(u32, u64)> {
    use std::os::fd::AsRawFd;

    if size == 0 || !size.is_multiple_of(16) {
        return Vec::new();
    }
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            fd.as_raw_fd(),
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        return Vec::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), size) };
    let formats = parse_dmabuf_format_table(bytes);
    unsafe {
        libc::munmap(ptr, size);
    }
    formats
}

fn parse_dmabuf_format_table(bytes: &[u8]) -> Vec<(u32, u64)> {
    bytes
        .chunks_exact(16)
        .map(|entry| {
            let format = u32::from_ne_bytes(entry[0..4].try_into().unwrap());
            let modifier = u64::from_ne_bytes(entry[8..16].try_into().unwrap());
            (format, modifier)
        })
        .collect()
}

impl Dispatch<WpContentTypeManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpContentTypeManagerV1,
        _: wp_content_type_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpContentTypeV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpContentTypeV1,
        _: wp_content_type_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpLinuxBufferParamsV1, ()> for App {
    fn event(
        state: &mut Self,
        _: &ZwpLinuxBufferParamsV1,
        event: zwp_linux_buffer_params_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_linux_buffer_params_v1::Event::Created { buffer } => {
                buffer.destroy();
                state.probe_result = Some(true);
            }
            zwp_linux_buffer_params_v1::Event::Failed => {
                state.probe_result = Some(false);
            }
            _ => {}
        }
    }

    wayland_client::event_created_child!(App, ZwpLinuxBufferParamsV1, [
        zwp_linux_buffer_params_v1::EVT_CREATED_OPCODE => (wl_buffer::WlBuffer, (usize::MAX, usize::MAX, u64::MAX)),
    ]);
}

impl Dispatch<WpPresentation, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpPresentation,
        _: wp_presentation::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpPresentationFeedback, usize> for App {
    fn event(
        state: &mut Self,
        _: &WpPresentationFeedback,
        event: wp_presentation_feedback::Event,
        si: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(surf) = state.surfaces.get_mut(*si) else {
            return;
        };
        match event {
            wp_presentation_feedback::Event::Presented {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
                refresh,
                ..
            } => {
                let sec = ((tv_sec_hi as u64) << 32) | tv_sec_lo as u64;
                let ns = sec * 1_000_000_000 + tv_nsec as u64;
                surf.last_flip_ns = ns;
                if refresh > 0 {
                    surf.refresh_ns = refresh;
                }
                surf.presented += 1;
                if surf.feedbacks.len() >= 64 {
                    surf.feedbacks.drain(..32);
                }
                surf.feedbacks.push(ns);
            }
            wp_presentation_feedback::Event::Discarded => {
                surf.discarded += 1;
                if surf.feedbacks.len() >= 64 {
                    surf.feedbacks.drain(..32);
                }
                surf.feedbacks.push(0);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { callback_data } = event {
            state.frame_fired = true;
            state.frame_ts_ms = callback_data;
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, (usize, usize, u64)> for App {
    fn event(
        state: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        (si, bi, generation): &(usize, usize, u64),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event
            && state.buffer_generations.release(*generation)
            && let Some(surf) = state.surfaces.get_mut(*si)
        {
            surf.give_back(*bi);
        }
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwlrLayerShellV1,
        _: zwlr_layer_shell_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, usize> for App {
    fn event(
        state: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        si: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                layer.ack_configure(serial);
                if let Some(surf) = state.surfaces.get_mut(*si) {
                    if width > 0 && height > 0 {
                        if surf.configured && (surf.width != width || surf.height != height) {
                            state.resized = true;
                        }
                        surf.width = width;
                        surf.height = height;
                    }
                    surf.configured = true;
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                if let Some(surf) = state.surfaces.get_mut(*si) {
                    surf.closed = true;
                    tracing::info!(
                        "skwd-wall-vk: output {} closed, dropping its surface",
                        surf.name
                    );
                }
                if state.surfaces.iter().all(|surf| surf.closed) {
                    state.closed = true;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WpViewporter, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_shm::WlShm,
        event: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_shm::Event::Format { format: wayland_client::WEnum::Value(format) } = event {
            if std::env::var("SKWD_VK_GLOBALS").is_ok() {
                tracing::info!("shm format: {format:?}");
            }
            state.shm_formats.push(format);
        }
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpViewport, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ExtIdleNotifierV1,
        _: ext_idle_notifier_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for App {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => {
                state.idle = true;
                tracing::info!("skwd-wall-vk: session idle, pausing playback");
            }
            ext_idle_notification_v1::Event::Resumed => {
                state.idle = false;
                tracing::info!("skwd-wall-vk: session active, resuming playback");
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
