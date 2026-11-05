use super::mirror::{dmabuf_layout, shm_fourcc};
use super::model::{App, BufferGenerations, SurfaceUi, Target};
use anyhow::{Result, anyhow};
use paper_geom::FillMode;
use paper_runtime::mirror::{Layout, Memory, Plane};
use wayland_client::protocol::{wl_buffer, wl_region, wl_shm};
use wayland_client::{Connection, Proxy};
use wayland_protocols::wp::content_type::v1::client::wp_content_type_v1;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1;
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

const MULTI_JSON_ARG: &str = "--multi-json";
const SCENE_ARG: &str = "--scene";
const OUTPUT_SEPARATOR: &str = ",";
static RETAINED_OUTPUTS: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

fn fps_limit(name: &str) -> u32 {
    std::env::var("SKWD_PAPER_OUTPUT_FPS")
        .ok()
        .and_then(|value| {
            value.split(';').find_map(|entry| {
                let (output, fps) = entry.split_once('=')?;
                if output == name { fps.parse().ok() } else { None }
            })
        })
        .unwrap_or(0)
}

fn commit_due(next_commit_ns: &mut u64, fps_limit: u32, now_ns: u64) -> bool {
    if fps_limit == 0 {
        return true;
    }
    let step = 1_000_000_000 / u64::from(fps_limit);
    if *next_commit_ns == 0 {
        *next_commit_ns = now_ns.saturating_add(step);
        return true;
    }
    if now_ns.saturating_add(500_000) < *next_commit_ns {
        return false;
    }
    if now_ns > next_commit_ns.saturating_add(step.saturating_mul(4)) {
        *next_commit_ns = now_ns.saturating_add(step);
    } else {
        while *next_commit_ns <= now_ns.saturating_add(500_000) {
            *next_commit_ns = next_commit_ns.saturating_add(step);
        }
    }
    true
}

fn return_free_buffer(free_buffers: &mut Vec<usize>, bi: usize) {
    if !free_buffers.contains(&bi) {
        free_buffers.push(bi);
    }
}

fn all_surfaces_presented_after(presented: impl IntoIterator<Item = (u64, u64)>) -> bool {
    let mut presented = presented.into_iter().peekable();
    presented.peek().is_some() && presented.all(|(current, checkpoint)| current > checkpoint)
}

fn viewport_crop(
    mode: FillMode,
    source: (u32, u32),
    size: (u32, u32),
    position: Option<(i32, i32)>,
    bounds: Option<(i32, i32, u32, u32)>,
) -> (u32, u32, u32, u32) {
    match (mode, bounds, position) {
        (FillMode::Span, Some(bounds), Some((x, y))) => {
            paper_geom::span_crop_rect(source.0, source.1, bounds, (x, y, size.0, size.1))
        }
        _ => paper_geom::fill_crop_rect(source.0, source.1, size.0, size.1),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBufferWait {
    Ready(usize),
    Closed,
    Unavailable,
}

pub fn setup(target_output: &str, layer_arg: Option<&str>) -> Result<Target> {
    let conn = Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    let display = conn.display();
    display.get_registry(&qh, ());

    let mut app = App {
        compositor: None,
        layer_shell: None,
        dmabuf: None,
        dmabuf_formats: Vec::new(),
        dmabuf_feedback_table: Vec::new(),
        dmabuf_feedback_indices: Vec::new(),
        dmabuf_feedback_target: None,
        dmabuf_feedback_scanout: false,
        dmabuf_feedback_best: Vec::new(),
        dmabuf_feedback_all: Vec::new(),
        dmabuf_feedback_score: 0,
        viewporter: None,
        xdg_output_manager: None,
        content_type_manager: None,
        shm: None,
        outputs: Vec::new(),
        surfaces: Vec::new(),
        closed: false,
        frame_fired: false,
        frame_ts_ms: 0,
        presentation: None,
        idle_notifier: None,
        seat: None,
        pointer: None,
        pointer_enabled: false,
        pointer_capable: false,
        pointer_focus: None,
        mouse: super::pointer::Mouse::default(),
        idle: false,
        shm_formats: Vec::new(),
        probe_result: None,
        resized: false,
        mirror_buffers: super::mirror::Buffers::default(),
        buffer_generations: BufferGenerations::default(),
        pointer_surfaces: Vec::new(),
    };
    queue.roundtrip(&mut app)?;
    if let Some(manager) = &app.xdg_output_manager {
        for (index, info) in app.outputs.iter_mut().enumerate() {
            info.xdg_output = Some(manager.get_xdg_output(&info.output, &qh, index));
        }
    }
    queue.roundtrip(&mut app)?;

    let compositor = app.compositor.clone().ok_or_else(|| anyhow!("no wl_compositor"))?;
    let shell = app
        .layer_shell
        .clone()
        .ok_or_else(|| anyhow!("no zwlr_layer_shell_v1 (wlr compositor required)"))?;
    let target = paper_control::OutputTarget::from_arg(target_output);
    let picked =
        app.outputs.iter().filter(|info| target.matches(&info.name)).cloned().collect::<Vec<_>>();
    if picked.is_empty() {
        return Err(anyhow!("output '{target_output}' not found"));
    }

    let layer_pick = std::env::var("SKWD_VK_LAYER").ok().or_else(|| layer_arg.map(String::from));
    let vk_layer = parse_layer(layer_pick.as_deref())?;
    let input_passthrough = input_passthrough()
        || matches!(
            vk_layer,
            zwlr_layer_shell_v1::Layer::Top | zwlr_layer_shell_v1::Layer::Overlay
        );
    if input_passthrough {
        tracing::info!("skwd-wall-vk: input passthrough enabled");
    }
    for (si, info) in picked.iter().enumerate() {
        let surface = compositor.create_surface(&qh, ());
        let layer =
            shell.get_layer_surface(&surface, Some(&info.output), vk_layer, namespace(), &qh, si);
        fill_output(&layer);
        surface.set_buffer_scale(info.scale);
        let input_region = if input_passthrough {
            let region = compositor.create_region(&qh, ());
            surface.set_input_region(Some(&region));
            Some(region)
        } else {
            None
        };
        surface.commit();
        app.surfaces.push(SurfaceUi {
            name: info.name.clone(),
            output: info.output.clone(),
            position: info.position(),
            surface,
            _input_region: input_region,
            layer,
            width: 0,
            height: 0,
            scale: info.scale,
            mode: info.mode,
            configured: false,
            closed: false,
            free_buffers: Vec::new(),
            last_flip_ns: 0,
            refresh_ns: 0,
            fps_limit: fps_limit(&info.name),
            next_commit_ns: 0,
            presented: 0,
            discarded: 0,
            feedbacks: Vec::new(),
            viewport: None,
            _content_type: None,
            source: None,
            attached: None,
            mirror: None,
        });
    }

    while !app.closed && app.surfaces.iter().any(|surf| !surf.configured && !surf.closed) {
        queue.blocking_dispatch(&mut app)?;
    }
    if app.closed || app.surfaces.iter().all(|surf| surf.closed || surf.width == 0) {
        return Err(anyhow!("layer surface closed or zero size"));
    }
    Ok(Target {
        conn,
        queue,
        app,
        layer: vk_layer,
        ctl_fd: None,
        hidden: std::env::var_os(paper_control::PREPARE_HIDDEN_ENV).is_some(),
    })
}

fn parse_layer(value: Option<&str>) -> Result<zwlr_layer_shell_v1::Layer> {
    use zwlr_layer_shell_v1::Layer;
    match value.unwrap_or("bottom") {
        "background" => Ok(Layer::Background),
        "bottom" => Ok(Layer::Bottom),
        "top" => Ok(Layer::Top),
        "overlay" => Ok(Layer::Overlay),
        other => {
            Err(anyhow!("unknown layer {other:?}; expected background, bottom, top, or overlay"))
        }
    }
}

pub(super) fn fill_output(layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1) {
    layer.set_anchor(
        zwlr_layer_surface_v1::Anchor::Top
            | zwlr_layer_surface_v1::Anchor::Bottom
            | zwlr_layer_surface_v1::Anchor::Left
            | zwlr_layer_surface_v1::Anchor::Right,
    );
    layer.set_exclusive_zone(-1);
    layer.set_size(0, 0);
}

pub(super) fn namespace() -> String {
    std::env::var("SKWD_PAPER_NAMESPACE").unwrap_or_else(|_| "skwd-wall-vk".into())
}

fn input_passthrough() -> bool {
    input_passthrough_for(
        std::env::var("SKWD_VK_INPUT").ok().as_deref(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        std::env::var("XDG_SESSION_DESKTOP").ok().as_deref(),
        std::env::var("DESKTOP_SESSION").ok().as_deref(),
        std::env::var("KDE_FULL_SESSION").ok().as_deref(),
    )
}

pub(crate) fn is_kwin_session() -> bool {
    input_passthrough_for(
        None,
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        std::env::var("XDG_SESSION_DESKTOP").ok().as_deref(),
        std::env::var("DESKTOP_SESSION").ok().as_deref(),
        std::env::var("KDE_FULL_SESSION").ok().as_deref(),
    )
}

fn input_passthrough_for(
    override_mode: Option<&str>,
    current_desktop: Option<&str>,
    session_desktop: Option<&str>,
    desktop_session: Option<&str>,
    kde_full_session: Option<&str>,
) -> bool {
    match override_mode.map(str::to_ascii_lowercase).as_deref() {
        Some("passthrough") => return true,
        Some("interactive") => return false,
        _ => {}
    }
    kde_full_session.is_some_and(session_flag_enabled)
        || [current_desktop, session_desktop, desktop_session]
            .into_iter()
            .flatten()
            .any(desktop_is_kwin)
}

fn session_flag_enabled(value: &str) -> bool {
    !matches!(value.to_ascii_lowercase().as_str(), "" | "0" | "false" | "no")
}

fn desktop_is_kwin(value: &str) -> bool {
    value
        .split([':', ';', ','])
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .any(|part| matches!(part.as_str(), "kde" | "plasma" | "kwin"))
}

impl Target {
    pub fn span_bounds(&self) -> Option<(i32, i32, u32, u32)> {
        let surfaces = self.app.surfaces.iter().filter(|surface| {
            surface.configured && !surface.closed && surface.width > 0 && surface.height > 0
        });
        let mut outputs = Vec::new();
        for surface in surfaces {
            let (x, y) = surface.position?;
            outputs.push((x, y, surface.width, surface.height));
        }
        paper_geom::desktop_bounds(&outputs)
    }

    pub fn span_size(&self) -> Option<(u32, u32)> {
        let (_, _, width, height) = self.span_bounds()?;
        let scale = self
            .app
            .surfaces
            .iter()
            .filter(|surface| {
                surface.configured && !surface.closed && surface.width > 0 && surface.height > 0
            })
            .map(|surface| surface.scale.max(1) as u32)
            .max()?;
        Some((width.saturating_mul(scale), height.saturating_mul(scale)))
    }

    pub fn set_content_type(&mut self, kind: wp_content_type_v1::Type) {
        let Some(manager) = &self.app.content_type_manager else {
            return;
        };
        let qh = self.queue.handle();
        for surface in &mut self.app.surfaces {
            let content_type = manager.get_surface_content_type(&surface.surface, &qh, ());
            content_type.set_content_type(kind);
            surface._content_type = Some(content_type);
        }
    }

    pub fn display_ptr(&self) -> *mut std::ffi::c_void {
        self.conn.backend().display_ptr().cast()
    }

    pub fn arm_idle(&mut self, timeout_secs: u32) {
        let (Some(notifier), Some(seat)) = (&self.app.idle_notifier, &self.app.seat) else {
            tracing::info!("skwd-wall-vk: idle pause unavailable (no ext-idle-notify or wl_seat)");
            return;
        };
        let qh = self.queue.handle();
        notifier.get_idle_notification(timeout_secs.saturating_mul(1000), seat, &qh, ());
        tracing::info!("skwd-wall-vk: idle pause armed at {timeout_secs}s");
    }

    pub fn surface_count(&self) -> usize {
        self.app.surfaces.len()
    }

    pub fn size_at(&self, si: usize) -> (u32, u32) {
        let surf = &self.app.surfaces[si];
        let (mut mw, mut mh) = surf.mode;
        if (mw > mh) != (surf.width > surf.height) {
            std::mem::swap(&mut mw, &mut mh);
        }
        if mw > 0 && mh > 0 {
            return (mw, mh);
        }
        let sc = surf.scale.max(1) as u32;
        (surf.width * sc, surf.height * sc)
    }

    pub fn supports_viewporter(&self) -> bool {
        self.app.viewporter.is_some()
    }

    pub fn set_viewport_dst(&mut self, si: usize) -> Result<()> {
        let viewporter = self.app.viewporter.as_ref().ok_or_else(|| anyhow!("no wp_viewporter"))?;
        let qh = self.queue.handle();
        let surf = &mut self.app.surfaces[si];
        let viewport =
            surf.viewport.get_or_insert_with(|| viewporter.get_viewport(&surf.surface, &qh, ()));
        surf.surface.set_buffer_scale(1);
        viewport.set_source(-1.0, -1.0, -1.0, -1.0);
        viewport.set_destination(surf.width as i32, surf.height as i32);
        surf.source = None;
        Ok(())
    }

    pub fn surface_ptr_at(&self, si: usize) -> *mut std::ffi::c_void {
        self.app.surfaces[si].surface.id().as_ptr().cast()
    }

    pub fn create_dmabuf_buffer(
        &mut self,
        fd: std::os::fd::RawFd,
        width: u32,
        height: u32,
        offset: u32,
        stride: u32,
        modifier: Option<u64>,
        si: usize,
        bi: usize,
    ) -> Result<wl_buffer::WlBuffer> {
        let dmabuf = self.app.dmabuf.as_ref().ok_or_else(|| anyhow!("no zwp_linux_dmabuf_v1"))?;
        let qh = self.queue.handle();
        let params = dmabuf.create_params(&qh, ());
        let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
        let modifier = modifier.unwrap_or_else(|| {
            crate::dmabuf::preferred_modifier(&self.app.dmabuf_formats, crate::dmabuf::FOURCC_XR24)
        });
        params.add(borrowed, 0, offset, stride, (modifier >> 32) as u32, modifier as u32);
        let buffer = params.create_immed(
            width as i32,
            height as i32,
            0x34325258,
            zwp_linux_buffer_params_v1::Flags::empty(),
            &qh,
            (si, bi, self.app.buffer_generations.allocation()),
        );
        params.destroy();
        let plane = Plane { stride, offset };
        self.app.mirror_buffers.record(
            &buffer,
            &[fd],
            dmabuf_layout(width, height, crate::dmabuf::FOURCC_XR24, modifier, &[plane]),
        );
        if let Err(error) = self.conn.flush() {
            self.retire_scene_buffer(&buffer);
            return Err(error.into());
        }
        Ok(buffer)
    }

    pub fn take_resized(&mut self) -> Result<bool> {
        if !self.app.resized {
            return Ok(false);
        }
        loop {
            self.app.resized = false;
            self.dispatch_until(std::time::Instant::now() + std::time::Duration::from_millis(400))?;
            if !self.app.resized {
                return Ok(true);
            }
        }
    }

    pub fn stage_scene_buffers(&mut self, staging: bool) {
        self.app.buffer_generations.stage(staging);
    }

    pub fn rotate_scene_buffers(&mut self, buffers: &[Vec<wl_buffer::WlBuffer>]) -> u64 {
        self.app.end_mirrors_for_resize();
        let outstanding = buffers
            .iter()
            .zip(&self.app.surfaces)
            .map(|(ring, surface)| {
                if surface.closed {
                    0
                } else {
                    ring.len().saturating_sub(surface.free_buffers.len())
                }
            })
            .sum();
        self.app.buffer_generations.rotate(outstanding)
    }

    pub fn scene_buffers_released(&self, generation: u64) -> bool {
        self.app.buffer_generations.retired(generation)
    }

    pub fn retire_scene_buffer(&mut self, buffer: &wl_buffer::WlBuffer) {
        self.app.mirror_buffers.retire(buffer);
        buffer.destroy();
    }

    pub fn reexec(source: &ReexecSource) -> Result<()> {
        tracing::info!("skwd-wall-vk: output resized, restarting renderer");
        let argv: Vec<String> = std::env::args().collect();
        let mut rebuilt = rebuild_argv(&argv, source);
        let retained = RETAINED_OUTPUTS.lock().expect("retained outputs lock");
        if let Some(outputs) = retained.as_ref() {
            retain_output_args(&mut rebuilt, outputs)?;
        }
        drop(retained);
        let args: Vec<std::ffi::CString> = rebuilt
            .into_iter()
            .map(std::ffi::CString::new)
            .collect::<std::result::Result<_, _>>()
            .map_err(|_| anyhow!("argv contains NUL"))?;
        Err(anyhow!("reexec failed: {}", crate::sandbox::reexec(args)))
    }

    pub fn probe_dmabuf_import(
        &mut self,
        fd: std::os::fd::RawFd,
        width: u32,
        height: u32,
        offset: u32,
        stride: u32,
        modifier: Option<u64>,
    ) -> Result<bool> {
        let modifier = modifier.unwrap_or_else(|| {
            crate::dmabuf::preferred_modifier(&self.app.dmabuf_formats, crate::dmabuf::FOURCC_XR24)
        });
        self.probe_dmabuf_planes(fd, width, height, 0x34325258, modifier, &[(offset, stride)])
    }

    #[allow(clippy::too_many_arguments)]
    pub fn probe_nv12_import(
        &mut self,
        fd: std::os::fd::RawFd,
        width: u32,
        height: u32,
        plane0_offset: u32,
        plane0_stride: u32,
        plane1_offset: u32,
        plane1_stride: u32,
        modifier: u64,
    ) -> Result<bool> {
        self.probe_dmabuf_planes(
            fd,
            width,
            height,
            0x3231_564e,
            modifier,
            &[(plane0_offset, plane0_stride), (plane1_offset, plane1_stride)],
        )
    }

    fn probe_dmabuf_planes(
        &mut self,
        fd: std::os::fd::RawFd,
        width: u32,
        height: u32,
        format: u32,
        modifier: u64,
        planes: &[(u32, u32)],
    ) -> Result<bool> {
        let Some(dmabuf) = self.app.dmabuf.as_ref() else {
            return Ok(false);
        };
        let qh = self.queue.handle();
        let params = dmabuf.create_params(&qh, ());
        let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
        let (hi, lo) = ((modifier >> 32) as u32, modifier as u32);
        for (plane, &(offset, stride)) in planes.iter().enumerate() {
            params.add(borrowed, plane as u32, offset, stride, hi, lo);
        }
        self.app.probe_result = None;
        params.create(
            width as i32,
            height as i32,
            format,
            zwp_linux_buffer_params_v1::Flags::empty(),
        );
        self.conn.flush()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while self.app.probe_result.is_none()
            && !self.app.closed
            && std::time::Instant::now() < deadline
        {
            self.queue.blocking_dispatch(&mut self.app)?;
        }
        params.destroy();
        self.conn.flush()?;
        Ok(self.app.probe_result.take().unwrap_or(false))
    }

    pub fn create_shm_ring(
        &mut self,
        si: usize,
        width: u32,
        height: u32,
        count: usize,
    ) -> Result<(Vec<wl_buffer::WlBuffer>, Vec<*mut u8>, u32)> {
        self.create_shm_ring_fmt(si, width, height, count, wl_shm::Format::Xrgb8888, 4)
    }

    pub fn create_shm_ring_fmt(
        &mut self,
        si: usize,
        width: u32,
        height: u32,
        count: usize,
        format: wl_shm::Format,
        bpp: u32,
    ) -> Result<(Vec<wl_buffer::WlBuffer>, Vec<*mut u8>, u32)> {
        let shm = self.app.shm.as_ref().ok_or_else(|| anyhow!("no wl_shm"))?;
        let qh = self.queue.handle();
        let stride = width * bpp;
        let slot_bytes = (stride * height) as usize;
        let total = slot_bytes * count;
        let fd = unsafe { libc::memfd_create(c"skwd-vk-shm".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(anyhow!("memfd_create failed"));
        }
        if unsafe { libc::ftruncate(fd, total as i64) } < 0 {
            unsafe { libc::close(fd) };
            return Err(anyhow!("ftruncate failed"));
        }
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                total,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Err(anyhow!("mmap failed"));
        }
        let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
        let pool = shm.create_pool(borrowed, total as i32, &qh, ());
        let mut buffers = Vec::with_capacity(count);
        let mut ptrs = Vec::with_capacity(count);
        for slot in 0..count {
            let buffer = pool.create_buffer(
                (slot * slot_bytes) as i32,
                width as i32,
                height as i32,
                stride as i32,
                format,
                &qh,
                (si, slot, self.app.buffer_generations.allocation()),
            );
            self.app.mirror_buffers.record(
                &buffer,
                &[fd],
                Layout {
                    width,
                    height,
                    fourcc: shm_fourcc(format),
                    modifier: 0,
                    memory: Memory::Shm,
                    planes: [
                        Plane { stride, offset: (slot * slot_bytes) as u32 },
                        Plane::default(),
                    ],
                    plane_count: 1,
                },
            );
            buffers.push(buffer);
            ptrs.push(unsafe { (base as *mut u8).add(slot * slot_bytes) });
        }
        pool.destroy();
        unsafe { libc::close(fd) };
        if let Err(error) = self.conn.flush() {
            for buffer in &buffers {
                self.retire_scene_buffer(buffer);
            }
            unsafe { libc::munmap(base, total) };
            return Err(error.into());
        }
        Ok((buffers, ptrs, stride))
    }

    pub fn set_viewport_cover(&mut self, si: usize, video_w: u32, video_h: u32) -> Result<()> {
        let bounds = self.span_bounds();
        let viewporter = self.app.viewporter.as_ref().ok_or_else(|| anyhow!("no wp_viewporter"))?;
        let qh = self.queue.handle();
        let surf = &mut self.app.surfaces[si];
        let viewport =
            surf.viewport.get_or_insert_with(|| viewporter.get_viewport(&surf.surface, &qh, ()));
        surf.surface.set_buffer_scale(1);
        let (x, y, width, height) = viewport_crop(
            crate::fill::fill_mode(),
            (video_w, video_h),
            (surf.width, surf.height),
            surf.position,
            bounds,
        );
        viewport.set_source(f64::from(x), f64::from(y), f64::from(width), f64::from(height));
        viewport.set_destination(surf.width as i32, surf.height as i32);
        surf.source = Some([x, y, width, height]);
        Ok(())
    }

    pub fn clear_viewport(&mut self, si: usize) {
        let surf = &mut self.app.surfaces[si];
        if let Some(viewport) = surf.viewport.take() {
            viewport.destroy();
            surf.surface.set_buffer_scale(surf.scale);
        }
        surf.source = None;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_nv12_buffer(
        &mut self,
        fd: std::os::fd::RawFd,
        width: u32,
        height: u32,
        plane0_offset: u32,
        plane0_stride: u32,
        plane1_offset: u32,
        plane1_stride: u32,
        modifier: u64,
        si: usize,
        bi: usize,
    ) -> Result<wl_buffer::WlBuffer> {
        let dmabuf = self.app.dmabuf.as_ref().ok_or_else(|| anyhow!("no zwp_linux_dmabuf_v1"))?;
        let qh = self.queue.handle();
        let params = dmabuf.create_params(&qh, ());
        let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
        let (hi, lo) = ((modifier >> 32) as u32, modifier as u32);
        params.add(borrowed, 0, plane0_offset, plane0_stride, hi, lo);
        params.add(borrowed, 1, plane1_offset, plane1_stride, hi, lo);
        let buffer = params.create_immed(
            width as i32,
            height as i32,
            0x3231_564e,
            zwp_linux_buffer_params_v1::Flags::empty(),
            &qh,
            (si, bi, self.app.buffer_generations.current()),
        );
        params.destroy();
        let planes = [
            Plane { stride: plane0_stride, offset: plane0_offset },
            Plane { stride: plane1_stride, offset: plane1_offset },
        ];
        self.app.mirror_buffers.record(
            &buffer,
            &[fd, fd],
            dmabuf_layout(width, height, crate::dmabuf::FOURCC_NV12, modifier, &planes),
        );
        self.conn.flush()?;
        Ok(buffer)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_nv12_buffer_planes(
        &mut self,
        plane0_fd: std::os::fd::RawFd,
        plane0_offset: u32,
        plane0_stride: u32,
        plane0_modifier: u64,
        plane1_fd: std::os::fd::RawFd,
        plane1_offset: u32,
        plane1_stride: u32,
        plane1_modifier: u64,
        width: u32,
        height: u32,
        si: usize,
        bi: usize,
    ) -> Result<wl_buffer::WlBuffer> {
        let dmabuf = self.app.dmabuf.as_ref().ok_or_else(|| anyhow!("no zwp_linux_dmabuf_v1"))?;
        let qh = self.queue.handle();
        let params = dmabuf.create_params(&qh, ());
        let plane0 = unsafe { std::os::fd::BorrowedFd::borrow_raw(plane0_fd) };
        let plane1 = unsafe { std::os::fd::BorrowedFd::borrow_raw(plane1_fd) };
        let (plane0_hi, plane0_lo) = ((plane0_modifier >> 32) as u32, plane0_modifier as u32);
        let (plane1_hi, plane1_lo) = ((plane1_modifier >> 32) as u32, plane1_modifier as u32);
        params.add(plane0, 0, plane0_offset, plane0_stride, plane0_hi, plane0_lo);
        params.add(plane1, 1, plane1_offset, plane1_stride, plane1_hi, plane1_lo);
        let buffer = params.create_immed(
            width as i32,
            height as i32,
            0x3231_564e,
            zwp_linux_buffer_params_v1::Flags::empty(),
            &qh,
            (si, bi, self.app.buffer_generations.current()),
        );
        params.destroy();
        if plane0_modifier == plane1_modifier {
            let planes = [
                Plane { stride: plane0_stride, offset: plane0_offset },
                Plane { stride: plane1_stride, offset: plane1_offset },
            ];
            self.app.mirror_buffers.record(
                &buffer,
                &[plane0_fd, plane1_fd],
                dmabuf_layout(width, height, crate::dmabuf::FOURCC_NV12, plane0_modifier, &planes),
            );
        }
        self.conn.flush()?;
        Ok(buffer)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn probe_nv12_buffer_planes(
        &mut self,
        plane0_fd: std::os::fd::RawFd,
        plane0_offset: u32,
        plane0_stride: u32,
        plane0_modifier: u64,
        plane1_fd: std::os::fd::RawFd,
        plane1_offset: u32,
        plane1_stride: u32,
        plane1_modifier: u64,
        width: u32,
        height: u32,
    ) -> Result<bool> {
        let Some(dmabuf) = self.app.dmabuf.as_ref() else {
            return Ok(false);
        };
        let qh = self.queue.handle();
        let params = dmabuf.create_params(&qh, ());
        let plane0 = unsafe { std::os::fd::BorrowedFd::borrow_raw(plane0_fd) };
        let plane1 = unsafe { std::os::fd::BorrowedFd::borrow_raw(plane1_fd) };
        let (plane0_hi, plane0_lo) = ((plane0_modifier >> 32) as u32, plane0_modifier as u32);
        let (plane1_hi, plane1_lo) = ((plane1_modifier >> 32) as u32, plane1_modifier as u32);
        params.add(plane0, 0, plane0_offset, plane0_stride, plane0_hi, plane0_lo);
        params.add(plane1, 1, plane1_offset, plane1_stride, plane1_hi, plane1_lo);
        self.app.probe_result = None;
        params.create(
            width as i32,
            height as i32,
            0x3231_564e,
            zwp_linux_buffer_params_v1::Flags::empty(),
        );
        self.conn.flush()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while self.app.probe_result.is_none()
            && !self.app.closed
            && std::time::Instant::now() < deadline
        {
            self.queue.blocking_dispatch(&mut self.app)?;
        }
        params.destroy();
        self.conn.flush()?;
        Ok(self.app.probe_result.take().unwrap_or(false))
    }

    pub fn attach_at(&mut self, si: usize, buffer: &wl_buffer::WlBuffer) {
        let surf = &mut self.app.surfaces[si];
        if !self.hidden {
            surf.surface.attach(Some(buffer), 0, 0);
        }
        surf.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
        surf.attached = Some(buffer.clone());
        if surf.mirror.is_some() {
            self.app.publish_mirror(si, buffer);
        }
    }

    pub fn request_presentation_feedback_at(&mut self, si: usize) {
        if self.hidden {
            return;
        }
        if let Some(pres) = self.app.presentation.as_ref() {
            let qh = self.queue.handle();
            pres.feedback(&self.app.surfaces[si].surface, &qh, si);
        }
    }

    pub fn supports_presentation_feedback(&self) -> bool {
        self.app.presentation.is_some()
    }

    pub fn commit_at(&mut self, si: usize) {
        if !self.hidden {
            self.app.surfaces[si].surface.commit();
        }
    }

    pub fn reveal_prepared(&mut self) -> Result<bool> {
        if !std::mem::take(&mut self.hidden) {
            return Ok(false);
        }
        for index in 0..self.app.surfaces.len() {
            if let Some(buffer) = self.app.surfaces[index].attached.clone() {
                self.attach_at(index, &buffer);
                self.request_presentation_feedback_at(index);
                self.commit_at(index);
            }
        }
        self.flush()?;
        self.queue.roundtrip(&mut self.app)?;
        Ok(true)
    }

    pub fn set_transition_fps(&mut self, transition: Option<u32>, playback: &[u32]) {
        for (surface, playback) in self.app.surfaces.iter_mut().zip(playback) {
            let fps = transition.map_or(*playback, |cap| {
                let refresh = if surface.refresh_ns > 0 {
                    (1_000_000_000 + surface.refresh_ns / 2) / surface.refresh_ns
                } else {
                    60
                };
                if cap == 0 { refresh } else { cap.min(refresh) }.max(1)
            });
            if surface.fps_limit != fps {
                surface.fps_limit = fps;
                surface.next_commit_ns = 0;
            }
        }
    }

    pub fn commit_due_at(&mut self, si: usize, now_ns: u64) -> bool {
        let surface = &mut self.app.surfaces[si];
        commit_due(&mut surface.next_commit_ns, surface.fps_limit, now_ns)
    }

    /// Return a ring slot acquired for a frame that output cadence throttling
    /// decided not to commit. With no compositor attachment there will be no
    /// `wl_buffer.release` event to return it for us.
    pub fn return_uncommitted_buffer_at(&mut self, si: usize, bi: usize) {
        let surface = &mut self.app.surfaces[si];
        if !surface.closed {
            return_free_buffer(&mut surface.free_buffers, bi);
        }
    }

    pub fn retain_reexec_outputs(outputs: &[String]) {
        let mut retained = RETAINED_OUTPUTS.lock().expect("retained outputs lock");
        if let Some(previous) = retained.as_mut() {
            previous.retain(|output| outputs.contains(output));
        } else {
            *retained = Some(outputs.to_vec());
        }
    }

    pub fn retain_outputs(&mut self, outputs: &[String]) -> Result<()> {
        for surface in &mut self.app.surfaces {
            if surface.closed || outputs.contains(&surface.name) {
                continue;
            }
            surface.surface.attach(None, 0, 0);
            surface.surface.commit();
            surface.layer.destroy();
            surface.closed = true;
            surface.attached = None;
        }
        self.app.closed = self.app.surfaces.iter().all(|surface| surface.closed);
        self.conn.flush()?;
        self.queue.roundtrip(&mut self.app)?;
        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        self.conn.flush()?;
        Ok(())
    }

    pub fn commit_plain_at(&mut self, si: usize) -> Result<()> {
        self.commit_at(si);
        self.flush()
    }

    pub fn try_free_group_buffer_in(
        &mut self,
        group: &[usize],
        range: std::ops::Range<usize>,
    ) -> Result<GroupBufferWait> {
        self.queue.dispatch_pending(&mut self.app)?;
        self.settle_mirrors();
        Ok(self.take_free_group_buffer_in(group, range))
    }

    fn take_free_group_buffer_in(
        &mut self,
        group: &[usize],
        range: std::ops::Range<usize>,
    ) -> GroupBufferWait {
        let Some(first_live) = group.iter().copied().find(|&si| !self.app.surfaces[si].closed)
        else {
            return GroupBufferWait::Closed;
        };
        let pick = self.app.surfaces[first_live].free_buffers.iter().rev().copied().find(|bi| {
            range.contains(bi)
                && group.iter().all(|&si| {
                    let surf = &self.app.surfaces[si];
                    surf.closed || surf.free_buffers.contains(bi)
                })
        });
        let Some(bi) = pick else {
            return GroupBufferWait::Unavailable;
        };
        for &si in group {
            let surf = &mut self.app.surfaces[si];
            if !surf.closed {
                surf.free_buffers.retain(|&idx| idx != bi);
            }
        }
        GroupBufferWait::Ready(bi)
    }

    pub fn take_free_buffer_at(&mut self, si: usize) -> Option<usize> {
        (!self.app.surfaces[si].closed).then(|| self.app.surfaces[si].free_buffers.pop()).flatten()
    }

    fn wait_feedback_until(
        &mut self,
        deadline: std::time::Instant,
        ready: impl Fn(&App) -> bool,
    ) -> Result<bool> {
        loop {
            self.queue.dispatch_pending(&mut self.app)?;
            self.conn.flush()?;
            if ready(&self.app) {
                return Ok(true);
            }
            if self.app.closed || std::time::Instant::now() >= deadline {
                return Ok(false);
            }
            if let Some(guard) = self.conn.prepare_read() {
                use std::os::fd::AsRawFd;
                let mut pfd = libc::pollfd {
                    fd: guard.connection_fd().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let nready = unsafe { libc::poll(&mut pfd, 1, 20) };
                if nready > 0 {
                    guard.read().map_err(|err| anyhow!("wayland read: {err}"))?;
                } else {
                    drop(guard);
                }
            }
        }
    }

    pub fn presentation_checkpoint(&self) -> Vec<u64> {
        self.app.surfaces.iter().map(|surface| surface.presented).collect()
    }

    pub fn wait_presentation_after(
        &mut self,
        checkpoint: &[u64],
        deadline: std::time::Instant,
    ) -> Result<Option<bool>> {
        if !self.supports_presentation_feedback() {
            return Ok(None);
        }
        if checkpoint.len() != self.app.surfaces.len() {
            return Err(anyhow!("presentation checkpoint does not match the current outputs"));
        }
        self.wait_feedback_until(deadline, |app| {
            all_surfaces_presented_after(
                app.surfaces
                    .iter()
                    .zip(checkpoint)
                    .filter(|(surface, _)| !surface.closed)
                    .map(|(surface, checkpoint)| (surface.presented, *checkpoint)),
            )
        })
        .map(Some)
    }

    pub fn request_frame_at(&mut self, si: usize) {
        let qh = self.queue.handle();
        self.app.frame_fired = false;
        self.app.surfaces[si].surface.frame(&qh, ());
    }

    pub fn wait_frame(&mut self, deadline: std::time::Instant) -> Result<()> {
        loop {
            self.queue.dispatch_pending(&mut self.app)?;
            self.settle_mirrors();
            self.conn.flush()?;
            if self.app.frame_fired || self.app.closed {
                return Ok(());
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Ok(());
            }
            let timeout_ms = deadline.duration_since(now).as_millis().min(50) as i32;
            if let Some(guard) = self.conn.prepare_read() {
                use std::os::fd::AsRawFd;
                let mut pfd = libc::pollfd {
                    fd: guard.connection_fd().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let nready = unsafe { libc::poll(&mut pfd, 1, timeout_ms.max(1)) };
                if nready > 0 {
                    let _ = guard.read();
                } else {
                    drop(guard);
                }
            }
        }
    }

    pub fn dispatch_wait_events(&mut self, deadline: std::time::Instant) -> Result<()> {
        self.dispatch_wait_events_with_fd(deadline, None)
    }

    pub fn dispatch_wait_events_with_fd(
        &mut self,
        deadline: std::time::Instant,
        wake_fd: Option<i32>,
    ) -> Result<()> {
        loop {
            let dispatched = self.queue.dispatch_pending(&mut self.app)?;
            self.settle_mirrors();
            self.conn.flush()?;
            if dispatched > 0 || self.app.closed {
                return Ok(());
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Ok(());
            }
            let timeout_ms = deadline.duration_since(now).as_millis().min(30_000) as i32;
            if let Some(guard) = self.conn.prepare_read() {
                use std::os::fd::AsRawFd;
                let mut pfds = [
                    libc::pollfd {
                        fd: guard.connection_fd().as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: self.ctl_fd.unwrap_or(-1),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd { fd: wake_fd.unwrap_or(-1), events: libc::POLLIN, revents: 0 },
                ];
                let mut mirrors = self.mirror_pollfds();
                let nready = poll_with(&mut pfds, &mut mirrors, timeout_ms.max(1));
                if nready > 0 && pfds[0].revents & libc::POLLIN != 0 {
                    let _ = guard.read();
                } else {
                    drop(guard);
                }
                if nready > 0
                    && (pfds[2].revents != 0 || mirrors.iter().any(|pfd| pfd.revents != 0))
                {
                    self.settle_mirrors();
                    return Ok(());
                }
                if nready > 0 && pfds[1].revents & libc::POLLIN != 0 {
                    let mut buf = [0u8; 64];
                    while unsafe { libc::read(pfds[1].fd, buf.as_mut_ptr().cast(), buf.len()) } > 0
                    {
                    }
                    self.queue.dispatch_pending(&mut self.app)?;
                    return Ok(());
                }
            }
        }
    }

    pub fn dispatch_until(&mut self, deadline: std::time::Instant) -> Result<()> {
        loop {
            self.queue.dispatch_pending(&mut self.app)?;
            self.settle_mirrors();
            self.conn.flush()?;
            let now = std::time::Instant::now();
            if now >= deadline || self.app.closed {
                return Ok(());
            }
            let timeout_ms = deadline.duration_since(now).as_millis().min(10_000) as i32;
            if let Some(guard) = self.conn.prepare_read() {
                use std::os::fd::AsRawFd;
                let mut pfd = [libc::pollfd {
                    fd: guard.connection_fd().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                }];
                let mut mirrors = self.mirror_pollfds();
                let nready = poll_with(&mut pfd, &mut mirrors, timeout_ms.max(1));
                if nready > 0 && pfd[0].revents != 0 {
                    let _ = guard.read();
                } else {
                    drop(guard);
                }
            }
        }
    }

    fn settle_mirrors(&mut self) {
        self.app.apply_mirror_requests();
        self.app.service_mirrors();
    }

    fn mirror_pollfds(&self) -> Vec<libc::pollfd> {
        self.app
            .mirror_fds()
            .map(|fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 })
            .collect()
    }

    pub fn pump(&mut self) -> Result<()> {
        self.queue.dispatch_pending(&mut self.app)?;
        self.settle_mirrors();
        self.conn.flush()?;
        let Some(guard) = self.conn.prepare_read() else {
            return Ok(());
        };
        use std::os::fd::AsRawFd;
        let mut pfds = [
            libc::pollfd {
                fd: guard.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd { fd: self.ctl_fd.unwrap_or(-1), events: libc::POLLIN, revents: 0 },
        ];
        let nfds = if self.ctl_fd.is_some() { 2 } else { 1 };
        let nready = unsafe { libc::poll(pfds.as_mut_ptr(), nfds, 0) };
        if nready < 0 {
            drop(guard);
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error.into());
            }
            return Ok(());
        }
        if pfds[0].revents & libc::POLLIN != 0 {
            guard.read().map_err(|err| anyhow!("wayland read: {err}"))?;
            self.queue.dispatch_pending(&mut self.app)?;
        } else {
            drop(guard);
        }
        if nfds == 2 && pfds[1].revents & libc::POLLIN != 0 {
            let mut buf = [0u8; 64];
            while unsafe { libc::read(pfds[1].fd, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
        }
        Ok(())
    }
}

fn poll_with(fixed: &mut [libc::pollfd], extra: &mut [libc::pollfd], timeout_ms: i32) -> i32 {
    if extra.is_empty() {
        return unsafe { libc::poll(fixed.as_mut_ptr(), fixed.len() as libc::nfds_t, timeout_ms) };
    }
    let mut all: Vec<libc::pollfd> = fixed.iter().chain(extra.iter()).copied().collect();
    let nready = unsafe { libc::poll(all.as_mut_ptr(), all.len() as libc::nfds_t, timeout_ms) };
    for (slot, polled) in fixed.iter_mut().chain(extra.iter_mut()).zip(&all) {
        slot.revents = polled.revents;
    }
    nready
}

impl wayland_client::Dispatch<wl_region::WlRegion, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_region::WlRegion,
        _: wl_region::Event,
        _: &(),
        _: &Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

pub enum ReexecSource<'a> {
    Video(&'a str),
    Scene { dir: &'a str, properties: Option<&'a str> },
}

fn retain_output_args(args: &mut [String], outputs: &[String]) -> Result<()> {
    if args.get(1).is_some_and(|arg| arg == MULTI_JSON_ARG)
        && let Some(manifest) = args.get_mut(2)
    {
        let mut entries: Vec<paper_control::MultiVideoEntry> = serde_json::from_str(manifest)?;
        entries.retain(|entry| outputs.contains(&entry.output));
        *manifest = serde_json::to_string(&entries)?;
    } else if args.iter().any(|arg| arg == SCENE_ARG)
        && let Some(target) = args.get_mut(1)
    {
        *target = outputs.join(OUTPUT_SEPARATOR);
    }
    Ok(())
}

pub(crate) fn rebuild_argv(args: &[String], source: &ReexecSource) -> Vec<String> {
    let mut args = args.to_vec();
    if args.get(1).is_some_and(|arg| arg == MULTI_JSON_ARG) {
        return args;
    }
    match source {
        ReexecSource::Video(path) => {
            let canonical =
                args.len() > 2 && !args[1].starts_with('-') && !args[2].starts_with('-');
            if canonical {
                args[2] = (*path).to_string();
            }
        }
        ReexecSource::Scene { dir, properties } => {
            if let Some(pos) = args.iter().position(|arg| arg == "--scene")
                && pos + 1 < args.len()
            {
                args[pos + 1] = (*dir).to_string();
            }
            if let Some(pos) = args.iter().position(|arg| arg == "--scene-properties") {
                let end = (pos + 2).min(args.len());
                args.drain(pos..end);
            }
            if let Some(properties) = properties {
                args.push("--scene-properties".to_string());
                args.push((*properties).to_string());
            }
        }
    }
    if let Some(pos) = args.iter().position(|arg| arg == "--transition-from") {
        let end = (pos + 2).min(args.len());
        args.drain(pos..end);
    }
    args
}

#[cfg(test)]
mod tests;
