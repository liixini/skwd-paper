use super::model::{App, SurfaceUi, Target};
use anyhow::{Result, anyhow};
use wayland_client::protocol::{wl_buffer, wl_output, wl_region, wl_shm};
use wayland_client::{Connection, Proxy};
use wayland_protocols::wp::content_type::v1::client::wp_content_type_v1;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1;
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

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
        idle: false,
        shm_formats: Vec::new(),
        probe_result: None,
        resized: false,
    };
    queue.roundtrip(&mut app)?;
    queue.roundtrip(&mut app)?;

    let compositor = app.compositor.clone().ok_or_else(|| anyhow!("no wl_compositor"))?;
    let shell = app
        .layer_shell
        .clone()
        .ok_or_else(|| anyhow!("no zwlr_layer_shell_v1 (wlr compositor required)"))?;
    let target = paper_control::OutputTarget::from_arg(target_output);
    let picked: Vec<(wl_output::WlOutput, String, i32, (u32, u32))> =
        app.outputs.iter().filter(|(_, n, ..)| target.matches(n)).cloned().collect();
    if picked.is_empty() {
        return Err(anyhow!("output '{target_output}' not found"));
    }

    let layer_pick = std::env::var("SKWD_VK_LAYER").ok().or_else(|| layer_arg.map(String::from));
    let vk_layer = match layer_pick.as_deref() {
        Some("top") => zwlr_layer_shell_v1::Layer::Top,
        Some("bottom") => zwlr_layer_shell_v1::Layer::Bottom,
        _ => zwlr_layer_shell_v1::Layer::Background,
    };
    let input_passthrough = input_passthrough();
    if input_passthrough {
        tracing::info!("skwd-wall-vk: KWin input passthrough enabled");
    }
    for (si, (output, name, scale, mode)) in picked.iter().enumerate() {
        let surface = compositor.create_surface(&qh, ());
        let layer = shell.get_layer_surface(
            &surface,
            Some(output),
            vk_layer,
            "skwd-wall-vk".into(),
            &qh,
            si,
        );
        layer.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right,
        );
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        surface.set_buffer_scale(*scale);
        let input_region = if input_passthrough {
            let region = compositor.create_region(&qh, ());
            surface.set_input_region(Some(&region));
            Some(region)
        } else {
            None
        };
        surface.commit();
        app.surfaces.push(SurfaceUi {
            name: name.clone(),
            surface,
            _input_region: input_region,
            _layer: layer,
            width: 0,
            height: 0,
            scale: *scale,
            mode: *mode,
            configured: false,
            closed: false,
            free_buffers: Vec::new(),
            last_flip_ns: 0,
            refresh_ns: 0,
            fps_limit: fps_limit(name),
            next_commit_ns: 0,
            presented: 0,
            discarded: 0,
            feedbacks: Vec::new(),
            viewport: None,
            _content_type: None,
        });
    }

    while !app.closed && app.surfaces.iter().any(|surf| !surf.configured && !surf.closed) {
        queue.blocking_dispatch(&mut app)?;
    }
    if app.closed || app.surfaces.iter().all(|surf| surf.closed || surf.width == 0) {
        return Err(anyhow!("layer surface closed or zero size"));
    }
    Ok(Target { conn, queue, app, ctl_fd: None })
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
            (si, bi),
        );
        params.destroy();
        self.conn.flush()?;
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

    pub fn reexec(source: &ReexecSource) -> Result<()> {
        tracing::info!("skwd-wall-vk: output resized, restarting renderer");
        let argv: Vec<String> = std::env::args().collect();
        let args: Vec<std::ffi::CString> = rebuild_argv(&argv, source)
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
                (si, slot),
            );
            buffers.push(buffer);
            ptrs.push(unsafe { (base as *mut u8).add(slot * slot_bytes) });
        }
        pool.destroy();
        unsafe { libc::close(fd) };
        self.conn.flush()?;
        Ok((buffers, ptrs, stride))
    }

    pub fn set_viewport_cover(&mut self, si: usize, video_w: u32, video_h: u32) -> Result<()> {
        let viewporter = self.app.viewporter.as_ref().ok_or_else(|| anyhow!("no wp_viewporter"))?;
        let qh = self.queue.handle();
        let surf = &mut self.app.surfaces[si];
        let viewport =
            surf.viewport.get_or_insert_with(|| viewporter.get_viewport(&surf.surface, &qh, ()));
        surf.surface.set_buffer_scale(1);
        let (x, y, width, height) =
            paper_geom::fill_crop_rect(video_w, video_h, surf.width, surf.height);
        viewport.set_source(f64::from(x), f64::from(y), f64::from(width), f64::from(height));
        viewport.set_destination(surf.width as i32, surf.height as i32);
        Ok(())
    }

    pub fn clear_viewport(&mut self, si: usize) {
        let surf = &mut self.app.surfaces[si];
        if let Some(viewport) = surf.viewport.take() {
            viewport.destroy();
            surf.surface.set_buffer_scale(surf.scale);
        }
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
            (si, bi),
        );
        params.destroy();
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
            (si, bi),
        );
        params.destroy();
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
        let surf = &self.app.surfaces[si];
        surf.surface.attach(Some(buffer), 0, 0);
        surf.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
    }

    pub fn request_presentation_feedback_at(&mut self, si: usize) {
        if let Some(pres) = self.app.presentation.as_ref() {
            let qh = self.queue.handle();
            pres.feedback(&self.app.surfaces[si].surface, &qh, si);
        }
    }

    pub fn supports_presentation_feedback(&self) -> bool {
        self.app.presentation.is_some()
    }

    pub fn commit_at(&mut self, si: usize) {
        self.app.surfaces[si].surface.commit();
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
        loop {
            let dispatched = self.queue.dispatch_pending(&mut self.app)?;
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
                ];
                let nfds = if self.ctl_fd.is_some() { 2 } else { 1 };
                let nready = unsafe { libc::poll(pfds.as_mut_ptr(), nfds, timeout_ms.max(1)) };
                if nready > 0 && pfds[0].revents & libc::POLLIN != 0 {
                    let _ = guard.read();
                } else {
                    drop(guard);
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
            self.conn.flush()?;
            let now = std::time::Instant::now();
            if now >= deadline || self.app.closed {
                return Ok(());
            }
            let timeout_ms = deadline.duration_since(now).as_millis().min(10_000) as i32;
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

    pub fn pump(&mut self) -> Result<()> {
        self.queue.dispatch_pending(&mut self.app)?;
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

pub(crate) fn rebuild_argv(args: &[String], source: &ReexecSource) -> Vec<String> {
    let mut args = args.to_vec();
    if args.get(1).is_some_and(|arg| arg == "--multi-json") {
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
