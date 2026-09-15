pub(crate) mod gpu;

use std::io::Write;
use std::os::fd::RawFd;
use std::time::Instant;

use anyhow::{Context, Result};

use crate::vk::Src;

fn wait_stream_control(ctl: &mut crate::ctl::Ctl) -> Result<std::time::Duration> {
    let _ = ctl.poll();
    if !ctl.paused {
        return Ok(std::time::Duration::ZERO);
    }
    let started = Instant::now();
    while ctl.paused {
        let Some(fd) = ctl.wake_fd() else {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = ctl.poll();
            continue;
        };
        let mut event = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let result = unsafe { libc::poll(&raw mut event, 1, 30_000) };
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error.into());
            }
        }
        let _ = ctl.poll();
        if unsafe { libc::getppid() } <= 1 {
            return Err(anyhow::anyhow!("stream parent exited"));
        }
    }
    Ok(started.elapsed())
}

pub(crate) fn parse_size(value: Option<&str>) -> (u32, u32) {
    value
        .and_then(|text| text.split_once('x'))
        .and_then(|(width, height)| Some((width.parse().ok()?, height.parse().ok()?)))
        .unwrap_or((640, 360))
}

fn load(path: &str) -> Result<(u32, u32, Vec<u8>)> {
    if let Ok(reader) =
        image::ImageReader::open(path).and_then(|reader| reader.with_guessed_format())
        && let Ok(image) = reader.decode()
    {
        let image = image.to_rgba8();
        return Ok((image.width(), image.height(), image.into_raw()));
    }
    let mut decoder = crate::decode::SwDecoder::open_threads(path, 1)?;
    let (source, _) = decoder.next_raw()?;
    let (width, height) = (source.width(), source.height());
    let mut rgba = ffmpeg_the_third::frame::Video::empty();
    let mut scaler = ffmpeg_the_third::software::scaling::Context::get(
        source.format(),
        width,
        height,
        ffmpeg_the_third::format::Pixel::RGBA,
        width,
        height,
        ffmpeg_the_third::software::scaling::Flags::BILINEAR,
    )
    .with_context(|| format!("create transition scaler for {path}"))?;
    scaler.run(&source, &mut rgba).with_context(|| format!("convert transition frame {path}"))?;
    let row = width as usize * 4;
    let stride = rgba.stride(0);
    let mut pixels = Vec::with_capacity(row * height as usize);
    for line in rgba.data(0).chunks(stride).take(height as usize) {
        pixels.extend_from_slice(&line[..row]);
    }
    Ok((width, height, pixels))
}

fn selected_shader(name: &str) -> &str {
    if name != "random" {
        return name;
    }
    let choices = paper_shaders::SAND_STYLES.len() + paper_shaders::EFFECTS.len();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    let index = nanos % choices.max(1);
    if index < paper_shaders::SAND_STYLES.len() {
        paper_shaders::SAND_STYLES[index]
    } else {
        paper_shaders::EFFECTS[index - paper_shaders::SAND_STYLES.len()].0
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn stream(
    from: &str,
    to: &str,
    shader: &str,
    width: u32,
    height: u32,
    duration_ms: u64,
    frame_ms: u64,
    write_header: bool,
    once: bool,
) -> Result<()> {
    let (width, height) = (width.max(16), height.max(16));
    let shared = crate::shared::create(std::ptr::null_mut()).context("preview Vulkan device")?;
    let mut renderer = crate::vk::Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        width,
        height,
    )
    .context("preview renderer")?;
    renderer.ensure_scene_pool(2)?;
    let (from_width, from_height, from_pixels) = load(from)?;
    let (to_width, to_height, to_pixels) = load(to)?;
    let from_texture = renderer.create_scene_texture(from_width, from_height, &from_pixels)?;
    let to_texture = renderer.create_scene_texture(to_width, to_height, &to_pixels)?;
    let mut export = renderer.create_export_image_opts(width, height, false)?;
    let target = if renderer.direct_render() {
        renderer.create_export_rt(&export)?
    } else {
        renderer.create_render_target(width, height)?
    };
    let readback = renderer.create_readback_buf(u64::from(width) * u64::from(height) * 4)?;
    let from_uv = crate::fill::mode_uv(from_width, from_height, width, height);
    let to_uv = crate::fill::mode_uv(to_width, to_height, width, height);
    let shader = selected_shader(shader);
    let sand = paper_shaders::sand_style_index(shader);
    let effect = sand.is_none().then(|| paper_shaders::effect_index(shader)).flatten();
    unsafe {
        libc::fcntl(libc::STDOUT_FILENO, libc::F_SETPIPE_SZ, 1024 * 1024);
    }
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    if write_header {
        output.write_all(b"SKWP")?;
        output.write_all(&width.to_le_bytes())?;
        output.write_all(&height.to_le_bytes())?;
    }
    output.flush()?;
    let duration_ms = duration_ms.max(100);
    let span = duration_ms + 600;
    let started = Instant::now();
    let frame_interval = std::time::Duration::from_millis(frame_ms.clamp(4, 200));
    let mut frames = 0u64;
    loop {
        let frame_deadline = Instant::now() + frame_interval;
        let elapsed = started.elapsed().as_millis() as u64;
        let reverse = (elapsed / span) % 2 == 1;
        let progress = ((elapsed % span) as f32 / duration_ms as f32).clamp(0.0, 1.0);
        let (a, a_uv, b, b_uv) = if reverse {
            (&to_texture, to_uv, &from_texture, from_uv)
        } else {
            (&from_texture, from_uv, &to_texture, to_uv)
        };
        let a = Src::Rgba(a.view);
        let b = Src::Rgba(b.view);
        match (sand, effect) {
            (Some(style), _) => renderer.render_sand_to(
                &target,
                &mut export,
                &a,
                a_uv,
                &b,
                b_uv,
                progress,
                style,
            )?,
            (None, Some(effect)) => renderer.render_effect_to(
                &target,
                &mut export,
                &a,
                a_uv,
                &b,
                b_uv,
                progress,
                effect,
            )?,
            (None, None) => {
                let mix = progress * progress * (3.0 - 2.0 * progress);
                renderer.render_fade_to(&target, &mut export, &a, a_uv, &b, b_uv, mix)?;
            }
        }
        renderer.read_export_to(&export, &readback, width, height)?;
        let bgra = unsafe {
            std::slice::from_raw_parts(readback.ptr, width as usize * height as usize * 4)
        };
        for (source, target) in bgra.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
            target[0] = source[2];
            target[1] = source[1];
            target[2] = source[0];
            target[3] = source[3];
        }
        if output.write_all(&rgba).is_err() || output.flush().is_err() {
            return Ok(());
        }
        frames += 1;
        if once && progress >= 1.0 {
            tracing::debug!(
                frames,
                fps = frames as f64 / started.elapsed().as_secs_f64().max(0.001),
                "skwd-wall-vk: preview transition complete"
            );
            return Ok(());
        }
        if unsafe { libc::getppid() } <= 1 {
            return Ok(());
        }
        if let Some(delay) = frame_deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(delay);
        }
    }
}

pub(crate) fn video_stream(
    path: &str,
    width: u32,
    height: u32,
    fps: u32,
    write_header: bool,
) -> Result<()> {
    let (width, height) = (width.max(16), height.max(16));
    let fps = fps.clamp(1, 240);
    let frame_step = 1.0 / f64::from(fps);
    let shared = crate::shared::create(std::ptr::null_mut()).context("video Vulkan device")?;
    let mut renderer = crate::vk::Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        width,
        height,
    )
    .context("video renderer")?;
    let force_software_decode =
        crate::shared::software_decode_required(shared.software, shared.queue_sync);
    let mut decoder = crate::decode::open_decoder(
        path,
        Some(shared.hwdev),
        shared.video_decode,
        shared.render_node.as_deref(),
        force_software_decode,
    )?;
    let hardware = matches!(decoder, crate::decode::AnyDecoder::Vk(_));
    let (video_width, video_height) = decoder.dims();
    let upload =
        (!hardware).then(|| renderer.create_upload_path(video_width, video_height)).transpose()?;
    let mut export = renderer.create_export_image_opts(width, height, false)?;
    let target = if renderer.direct_render() {
        renderer.create_export_rt(&export)?
    } else {
        renderer.create_render_target(width, height)?
    };
    let readback = renderer.create_readback_buf(u64::from(width) * u64::from(height) * 4)?;
    let uv = crate::fill::mode_uv(video_width, video_height, width, height);
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    if write_header {
        output.write_all(b"SKWP")?;
        output.write_all(&width.to_le_bytes())?;
        output.write_all(&height.to_le_bytes())?;
    }
    output.flush()?;
    let mut first_frame = true;
    let mut first_pts = None;
    let mut last_pts = None;
    let mut next_emit = 0.0;
    let mut started = Instant::now();
    loop {
        let (frame, pts) = decoder.next()?;
        if last_pts.is_some_and(|last| pts < last) {
            first_pts = None;
            next_emit = 0.0;
            started = Instant::now();
        }
        last_pts = Some(pts);
        let origin = *first_pts.get_or_insert(pts);
        let relative = (pts - origin).max(0.0);
        if relative + frame_step * 0.25 < next_emit {
            continue;
        }
        next_emit = relative + frame_step;
        let deadline = started + std::time::Duration::from_secs_f64(relative);
        if let Some(delay) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(delay);
        }
        let source = if let Some(upload) = &upload {
            renderer.upload_nv12(
                upload,
                frame.data(0),
                frame.stride(0),
                frame.data(1),
                frame.stride(1),
            )?;
            Src::Views(upload.luma_view, upload.chroma_view)
        } else {
            Src::Avvk(&frame)
        };
        renderer.render_to(&target, &mut export, &source, uv)?;
        renderer.read_export_to(&export, &readback, width, height)?;
        let bgra = unsafe {
            std::slice::from_raw_parts(readback.ptr, width as usize * height as usize * 4)
        };
        for (source, target) in bgra.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
            target[0] = source[2];
            target[1] = source[1];
            target[2] = source[0];
            target[3] = source[3];
        }
        if output.write_all(&rgba).is_err() || output.flush().is_err() {
            return Ok(());
        }
        if first_frame {
            paper_runtime::plasma::frame_ready()?;
            first_frame = false;
        }
        if unsafe { libc::getppid() } <= 1 {
            return Ok(());
        }
    }
}

pub(crate) fn packet(
    kind: u8,
    slot: u8,
    width: u32,
    height: u32,
    stride: u32,
    offset: u32,
    modifier: u64,
) -> [u8; 32] {
    let mut bytes =
        paper_runtime::plasma::packet(kind, slot, paper_runtime::plasma::stream_epoch());
    bytes[8..12].copy_from_slice(&width.to_le_bytes());
    bytes[12..16].copy_from_slice(&height.to_le_bytes());
    bytes[16..20].copy_from_slice(&stride.to_le_bytes());
    bytes[20..24].copy_from_slice(&offset.to_le_bytes());
    bytes[24..32].copy_from_slice(&modifier.to_le_bytes());
    bytes
}

pub(crate) fn send_packet(
    socket: RawFd,
    bytes: &[u8],
    pass_fd: Option<RawFd>,
) -> std::io::Result<()> {
    let mut iov = libc::iovec { iov_base: bytes.as_ptr().cast_mut().cast(), iov_len: bytes.len() };
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = std::ptr::from_mut(&mut iov);
    msg.msg_iovlen = 1;
    let mut control = [0u8; 64];
    if let Some(fd) = pass_fd {
        msg.msg_control = control.as_mut_ptr().cast();
        msg.msg_controllen =
            unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) as usize };
        let cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
        unsafe {
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as usize;
            std::ptr::write(libc::CMSG_DATA(cmsg).cast::<RawFd>(), fd);
        }
    }
    let sent = unsafe { libc::sendmsg(socket, &msg, libc::MSG_NOSIGNAL) };
    if sent == bytes.len() as isize {
        Ok(())
    } else if sent < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "short frame packet"))
    }
}

pub(crate) fn receive_ack(socket: RawFd, block: bool) -> std::io::Result<Option<usize>> {
    loop {
        let mut bytes = [0u8; 32];
        let flags = if block { 0 } else { libc::MSG_DONTWAIT };
        let len = unsafe { libc::recv(socket, bytes.as_mut_ptr().cast(), bytes.len(), flags) };
        if len == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "frame socket closed"));
        }
        if len < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if !block && error.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(None);
            }
            return Err(error);
        }
        if let Some(slot) = paper_runtime::plasma::acknowledged_slot(
            &bytes[..len as usize],
            paper_runtime::plasma::stream_epoch(),
        ) {
            return Ok(Some(slot));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamTarget {
    pub socket: RawFd,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub output: String,
    pub paused: bool,
}

impl StreamTarget {
    pub(crate) fn single(socket: RawFd, width: u32, height: u32, fps: u32, paused: bool) -> Self {
        Self { socket, width, height, fps, output: String::new(), paused }
    }
}

pub(crate) fn pacing_fps(targets: &[StreamTarget]) -> u32 {
    targets.iter().map(|target| target.fps).max().unwrap_or(30).clamp(1, 240)
}

pub(crate) fn apply_output_pauses(
    targets: &mut [StreamTarget],
    pauses: Vec<(String, bool)>,
) -> bool {
    for (output, paused) in pauses {
        for target in targets.iter_mut() {
            if target.output == output || target.output.is_empty() {
                target.paused = paused;
            }
        }
    }
    targets.iter().all(|target| target.paused)
}

pub(crate) fn wait_any_free(
    sockets: &[RawFd],
    free: &mut [[bool; 3]],
    active: &[bool],
) -> Result<()> {
    loop {
        if free.iter().zip(active).any(|(slots, active)| *active && slots.iter().any(|slot| *slot))
        {
            return Ok(());
        }
        let mut events: Vec<libc::pollfd> = sockets
            .iter()
            .map(|&fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 })
            .collect();
        let ready =
            unsafe { libc::poll(events.as_mut_ptr(), events.len() as libc::nfds_t, 30_000) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if ready == 0 {
            if unsafe { libc::getppid() } <= 1 {
                return Err(anyhow::anyhow!("stream parent exited"));
            }
            continue;
        }
        for (index, event) in events.iter().enumerate() {
            if event.revents == 0 {
                continue;
            }
            while let Some(slot) = receive_ack(sockets[index], false)? {
                if let Some(value) = free[index].get_mut(slot) {
                    *value = true;
                }
            }
        }
    }
}

struct Sink {
    target: StreamTarget,
    renderer: crate::vk::Renderer,
    exports: Vec<crate::vk::ExportImage>,
    semaphores: Vec<crate::vk::ExternalSemaphore>,
    targets: Vec<crate::vk::RenderTarget>,
    free: [bool; 3],
    uv: [f32; 4],
    old_uv: [f32; 4],
    step: f64,
    next_emit: f64,
    emitted: bool,
}

impl Sink {
    fn drain_acks(&mut self) -> Result<()> {
        while let Some(slot) = receive_ack(self.target.socket, false)? {
            if let Some(value) = self.free.get_mut(slot) {
                *value = true;
            }
        }
        Ok(())
    }

    fn announce(&self) -> Result<()> {
        for (slot, export) in self.exports.iter().enumerate() {
            let init = packet(
                4,
                slot as u8,
                self.target.width,
                self.target.height,
                export.stride,
                export.offset,
                export.allocation_size,
            );
            send_packet(self.target.socket, &init, Some(export.fd)).context("send dmabuf slot")?;
            send_packet(
                self.target.socket,
                &packet(5, slot as u8, 0, 0, 0, 0, 0),
                Some(self.semaphores[slot].fd),
            )
            .context("send dmabuf semaphore")?;
        }
        Ok(())
    }

    fn due(&mut self, relative: f64) -> bool {
        if self.emitted && relative + self.step * 0.25 < self.next_emit {
            return false;
        }
        self.next_emit = relative + self.step;
        true
    }

    fn publish(&mut self, slot: usize) -> Result<()> {
        self.renderer.wait_frame_complete()?;
        self.renderer.complete_external_signal(&self.semaphores[slot])?;
        send_packet(self.target.socket, &packet(2, slot as u8, 0, 0, 0, 0, 0), None)
            .context("send dmabuf frame")?;
        self.free[slot] = false;
        if !self.emitted {
            paper_runtime::plasma::frame_ready_on(self.target.socket)?;
        }
        self.emitted = true;
        Ok(())
    }
}

fn build_sink(
    shared: &crate::shared::SharedDevice,
    target: StreamTarget,
    video: (u32, u32),
    old: Option<(u32, u32)>,
) -> Result<Sink> {
    let renderer = crate::vk::Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        target.width,
        target.height,
    )
    .context("video renderer")?;
    let exports = (0..3)
        .map(|_| renderer.create_stream_export(target.width, target.height))
        .collect::<Result<Vec<_>>>()?;
    let semaphores =
        (0..3).map(|_| renderer.create_external_semaphore()).collect::<Result<Vec<_>>>()?;
    let targets = exports
        .iter()
        .map(|export| renderer.create_export_rt(export))
        .collect::<Result<Vec<_>>>()?;
    let uv = crate::fill::mode_uv(video.0, video.1, target.width, target.height);
    let old_uv = old
        .map(|(width, height)| crate::fill::mode_uv(width, height, target.width, target.height))
        .unwrap_or(uv);
    let step = 1.0 / f64::from(target.fps.clamp(1, 240));
    Ok(Sink {
        target,
        renderer,
        exports,
        semaphores,
        targets,
        free: [true; 3],
        uv,
        old_uv,
        step,
        next_emit: 0.0,
        emitted: false,
    })
}

struct Transition {
    upload: crate::vk::UploadPath,
    started: Option<Instant>,
    frames: u64,
}

struct VideoStreamer {
    sinks: Vec<Sink>,
    ctl: crate::ctl::Ctl,
    upload: Option<crate::vk::UploadPath>,
    transition: Option<Transition>,
    sand: Option<i32>,
    effect: Option<usize>,
    duration: std::time::Duration,
    frame_step: f64,
    frame_duration: std::time::Duration,
    timeline_shift: std::time::Duration,
    emitted: bool,
}

impl VideoStreamer {
    fn route_pauses(&mut self) {
        let pauses = self.ctl.take_output_pauses();
        if pauses.is_empty() {
            return;
        }
        let mut targets: Vec<StreamTarget> =
            self.sinks.iter().map(|sink| sink.target.clone()).collect();
        let all_paused = apply_output_pauses(&mut targets, pauses);
        for (sink, target) in self.sinks.iter_mut().zip(targets) {
            sink.target.paused = target.paused;
        }
        self.ctl.set_paused(all_paused);
    }

    fn shift_timeline(&mut self, by: std::time::Duration) {
        self.timeline_shift += by;
        if let Some(Transition { started: Some(started), .. }) = &mut self.transition {
            *started += by;
        }
    }

    fn wait_for_slot(&mut self) -> Result<()> {
        for sink in &mut self.sinks {
            sink.drain_acks()?;
        }
        let sockets: Vec<RawFd> = self.sinks.iter().map(|sink| sink.target.socket).collect();
        let active: Vec<bool> = self.sinks.iter().map(|sink| !sink.target.paused).collect();
        let mut free: Vec<[bool; 3]> = self.sinks.iter().map(|sink| sink.free).collect();
        wait_any_free(&sockets, &mut free, &active)?;
        for (sink, slots) in self.sinks.iter_mut().zip(free) {
            sink.free = slots;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        frame: &ffmpeg_the_third::frame::Video,
        decoder: &mut crate::decode::AnyDecoder,
        upload_frame: bool,
        deadline: Instant,
        relative: f64,
    ) -> Result<bool> {
        let suspended = if self.emitted {
            wait_stream_control(&mut self.ctl)?
        } else {
            let _ = self.ctl.poll();
            std::time::Duration::ZERO
        };
        self.shift_timeline(suspended);
        self.route_pauses();
        if self.ctl.paused {
            return Ok(self.transition.is_some());
        }
        let mut deadline = deadline + self.timeline_shift;
        let wait_started = Instant::now();
        self.wait_for_slot()?;
        let now = Instant::now();
        let shift = crate::timing::stream_resume_shift(
            deadline,
            now,
            now.duration_since(wait_started),
            self.frame_duration,
        );
        self.shift_timeline(shift);
        deadline += shift;
        if self.emitted
            && now
                .checked_duration_since(deadline)
                .is_some_and(|late| late.as_secs_f64() > self.frame_step * 1.5)
        {
            return Ok(self.transition.is_some());
        }
        let transferred = if upload_frame && let crate::decode::AnyDecoder::Vaapi(decoder) = decoder
        {
            Some(decoder.transfer_frame(frame)?)
        } else {
            None
        };
        let frame = transferred.as_ref().unwrap_or(frame);
        if let Some(delay) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(delay);
        }
        let source = if let Some(upload) = &self.upload {
            if upload_frame {
                self.sinks[0].renderer.upload_nv12(
                    upload,
                    frame.data(0),
                    frame.stride(0),
                    frame.data(1),
                    frame.stride(1),
                )?;
            }
            Src::Views(upload.luma_view, upload.chroma_view)
        } else {
            Src::Avvk(frame)
        };
        let mut finish_transition = false;
        let progress = self.transition.as_mut().map(|transition| {
            let started = *transition.started.get_or_insert_with(Instant::now);
            let progress = if transition.frames == 0 {
                0.0
            } else {
                (started.elapsed().as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
            };
            transition.frames += 1;
            progress
        });
        let old_source = self.transition.as_ref().map(|transition| {
            Src::Views(transition.upload.luma_view, transition.upload.chroma_view)
        });
        for sink in &mut self.sinks {
            if sink.target.paused || !sink.due(relative) {
                continue;
            }
            let Some(slot) = sink.free.iter().position(|value| *value) else {
                continue;
            };
            match (progress, old_source.as_ref()) {
                (Some(progress), Some(old_source)) => {
                    finish_transition = progress >= 1.0;
                    let target = &sink.targets[slot];
                    let export = &mut sink.exports[slot];
                    match (progress, self.sand, self.effect) {
                        (progress, _, _) if progress <= 0.0 => {
                            sink.renderer.render_to(target, export, old_source, sink.old_uv)?
                        }
                        (progress, _, _) if progress >= 1.0 => {
                            sink.renderer.render_to(target, export, &source, sink.uv)?
                        }
                        (_, Some(style), _) => sink.renderer.render_sand_to(
                            target,
                            export,
                            old_source,
                            sink.old_uv,
                            &source,
                            sink.uv,
                            progress,
                            style,
                        )?,
                        (_, None, Some(effect)) => sink.renderer.render_effect_to(
                            target,
                            export,
                            old_source,
                            sink.old_uv,
                            &source,
                            sink.uv,
                            progress,
                            effect,
                        )?,
                        (_, None, None) => sink.renderer.render_fade_to(
                            target,
                            export,
                            old_source,
                            sink.old_uv,
                            &source,
                            sink.uv,
                            progress * progress * (3.0 - 2.0 * progress),
                        )?,
                    }
                }
                _ => sink.renderer.render_to(
                    &sink.targets[slot],
                    &mut sink.exports[slot],
                    &source,
                    sink.uv,
                )?,
            }
            sink.publish(slot)?;
            self.emitted = true;
        }
        if finish_transition {
            if let Some(Transition { started: Some(started), frames, .. }) = &self.transition {
                tracing::info!(
                    frames,
                    fps = *frames as f64 / started.elapsed().as_secs_f64().max(0.001),
                    "skwd-wall-vk: stream transition complete"
                );
            }
            self.transition = None;
        }
        Ok(self.transition.is_some())
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dmabuf_video_stream(
    path: &str,
    targets: Vec<StreamTarget>,
    transition_from: Option<&str>,
    shader: &str,
    duration_ms: u64,
    mute: bool,
    volume: u32,
    write_header: bool,
) -> Result<()> {
    let targets: Vec<StreamTarget> = targets
        .into_iter()
        .map(|target| StreamTarget {
            width: target.width.max(16),
            height: target.height.max(16),
            ..target
        })
        .collect();
    anyhow::ensure!(!targets.is_empty(), "video stream needs at least one target");
    let fps = pacing_fps(&targets);
    let frame_step = 1.0 / f64::from(fps);
    let shared = crate::shared::create(std::ptr::null_mut()).context("video Vulkan device")?;
    let force_software_decode =
        crate::shared::software_decode_required(shared.software, shared.queue_sync);
    let mut decoder = crate::decode::open_decoder(
        path,
        Some(shared.hwdev),
        shared.video_decode,
        shared.render_node.as_deref(),
        force_software_decode,
    )?;
    let hardware = matches!(decoder, crate::decode::AnyDecoder::Vk(_));
    let (video_width, video_height) = decoder.dims();
    let mut old_decoder = transition_from
        .filter(|from| *from != path)
        .map(|from| crate::decode::SwDecoder::open_threads(from, 1))
        .transpose()?;
    let old_dims = old_decoder.as_ref().map(|old| (old.width, old.height));
    let single = targets.len() == 1;
    let built = targets
        .iter()
        .cloned()
        .map(|target| build_sink(&shared, target, (video_width, video_height), old_dims))
        .collect::<Result<Vec<_>>>();
    let sinks = match built {
        Ok(sinks) => sinks,
        Err(error) if single => {
            tracing::warn!(%error, "skwd-wall-vk: external stream unavailable, using CPU frames");
            let target = &targets[0];
            return video_stream(path, target.width, target.height, target.fps, write_header);
        }
        Err(error) => return Err(error),
    };
    let upload = (!hardware)
        .then(|| sinks[0].renderer.create_upload_path(video_width, video_height))
        .transpose()?;
    let transition = match old_decoder.as_mut() {
        Some(old) => {
            let (old_frame, _) = old.next()?;
            let old_upload = sinks[0].renderer.create_upload_path(old.width, old.height)?;
            sinks[0].renderer.upload_nv12(
                &old_upload,
                old_frame.data(0),
                old_frame.stride(0),
                old_frame.data(1),
                old_frame.stride(1),
            )?;
            Some(Transition { upload: old_upload, started: None, frames: 0 })
        }
        None => None,
    };
    let shader = selected_shader(shader);
    let sand = paper_shaders::sand_style_index(shader);
    let effect = sand.is_none().then(|| paper_shaders::effect_index(shader)).flatten();
    let mut ctl = crate::ctl::Ctl::start(path, mute, volume, true);
    ctl.route_output_pauses();
    ctl.set_paused(targets.iter().all(|target| target.paused));
    for sink in &sinks {
        sink.announce()?;
    }
    let mut streamer = VideoStreamer {
        sinks,
        ctl,
        upload,
        transition,
        sand,
        effect,
        duration: std::time::Duration::from_millis(duration_ms.max(100)),
        frame_step,
        frame_duration: std::time::Duration::from_secs_f64(frame_step),
        timeline_shift: std::time::Duration::ZERO,
        emitted: false,
    };
    let mut first_pts = None;
    let mut last_pts = None;
    let mut next_emit = 0.0;
    let mut started = Instant::now();
    let mut previous_frame = None;
    let mut last_deadline = None;
    let mut transition_active = streamer.transition.is_some();
    loop {
        let (frame, pts) = match &mut decoder {
            crate::decode::AnyDecoder::Vaapi(decoder) => decoder.next_hw_frame()?,
            _ => decoder.next()?,
        };
        if last_pts.is_some_and(|last| pts < last) {
            first_pts = None;
            next_emit = 0.0;
            started = Instant::now();
            streamer.emitted = false;
            for sink in &mut streamer.sinks {
                sink.emitted = false;
                sink.next_emit = 0.0;
            }
            previous_frame = None;
            last_deadline = None;
            streamer.timeline_shift = std::time::Duration::ZERO;
        }
        last_pts = Some(pts);
        let origin = *first_pts.get_or_insert(pts);
        let relative = (pts - origin).max(0.0);
        if relative + frame_step * 0.25 < next_emit {
            continue;
        }
        next_emit = relative + frame_step;
        let deadline = started + std::time::Duration::from_secs_f64(relative);
        if let Some(previous_deadline) = last_deadline
            && let Some(previous) = previous_frame.as_ref()
        {
            let mut fill_deadline: Instant = previous_deadline + streamer.frame_duration;
            while transition_active && fill_deadline < deadline {
                let fill_relative = fill_deadline.duration_since(started).as_secs_f64();
                transition_active =
                    streamer.emit(previous, &mut decoder, false, fill_deadline, fill_relative)?;
                fill_deadline += streamer.frame_duration;
            }
        }
        transition_active = streamer.emit(&frame, &mut decoder, true, deadline, relative)?;
        last_deadline = Some(deadline);
        previous_frame = Some(frame);
    }
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
