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

pub(crate) fn dmabuf_video_stream(
    path: &str,
    width: u32,
    height: u32,
    fps: u32,
    socket: RawFd,
    transition_from: Option<&str>,
    shader: &str,
    duration_ms: u64,
    mute: bool,
    volume: u32,
    paused: bool,
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
    let mut transition = if let Some(from) = transition_from.filter(|from| *from != path) {
        let mut old_decoder = crate::decode::SwDecoder::open_threads(from, 1)?;
        let (old_width, old_height) = (old_decoder.width, old_decoder.height);
        let (old_frame, _) = old_decoder.next()?;
        let old_upload = renderer.create_upload_path(old_width, old_height)?;
        renderer.upload_nv12(
            &old_upload,
            old_frame.data(0),
            old_frame.stride(0),
            old_frame.data(1),
            old_frame.stride(1),
        )?;
        let old_uv = crate::fill::mode_uv(old_width, old_height, width, height);
        Some((old_upload, old_uv, None::<Instant>, 0u64))
    } else {
        None
    };
    let shader = selected_shader(shader);
    let sand = paper_shaders::sand_style_index(shader);
    let effect = sand.is_none().then(|| paper_shaders::effect_index(shader)).flatten();
    let duration = std::time::Duration::from_millis(duration_ms.max(100));
    let export_stream = (|| -> Result<_> {
        let exports = (0..3)
            .map(|_| renderer.create_stream_export(width, height))
            .collect::<Result<Vec<_>>>()?;
        let semaphores =
            (0..3).map(|_| renderer.create_external_semaphore()).collect::<Result<Vec<_>>>()?;
        let targets = exports
            .iter()
            .map(|export| renderer.create_export_rt(export))
            .collect::<Result<Vec<_>>>()?;
        Ok((exports, semaphores, targets))
    })();
    let (mut exports, semaphores, targets) = match export_stream {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!(%error, "skwd-wall-vk: external stream unavailable, using CPU frames");
            return video_stream(path, width, height, fps, write_header);
        }
    };
    let mut ctl = crate::ctl::Ctl::start(path, mute, volume, true);
    ctl.set_paused(paused);
    for (slot, export) in exports.iter().enumerate() {
        let init = packet(
            4,
            slot as u8,
            width,
            height,
            export.stride,
            export.offset,
            export.allocation_size,
        );
        send_packet(socket, &init, Some(export.fd)).context("send dmabuf slot")?;
        send_packet(socket, &packet(5, slot as u8, 0, 0, 0, 0, 0), Some(semaphores[slot].fd))
            .context("send dmabuf semaphore")?;
    }
    let uv = crate::fill::mode_uv(video_width, video_height, width, height);
    let mut free = [true; 3];
    let mut first_pts = None;
    let mut last_pts = None;
    let mut next_emit = 0.0;
    let mut started = Instant::now();
    let mut emitted = false;
    let mut previous_frame = None;
    let mut last_deadline = None;
    let frame_duration = std::time::Duration::from_secs_f64(frame_step);
    let mut transition_active = transition.is_some();
    let mut timeline_shift = std::time::Duration::ZERO;
    let mut emit_frame = |frame: &ffmpeg_the_third::frame::Video,
                          upload_frame: bool,
                          deadline: Instant,
                          emitted: &mut bool,
                          timeline_shift: &mut std::time::Duration|
     -> Result<bool> {
        let suspended =
            if *emitted { wait_stream_control(&mut ctl)? } else { std::time::Duration::ZERO };
        *timeline_shift += suspended;
        if let Some((_, _, Some(started), _)) = &mut transition {
            *started += suspended;
        }
        let mut deadline = deadline + *timeline_shift;
        while let Some(slot) = receive_ack(socket, false)? {
            if let Some(value) = free.get_mut(slot) {
                *value = true;
            }
        }
        let wait_started = Instant::now();
        while !free.iter().any(|value| *value) {
            if let Some(slot) = receive_ack(socket, true)?
                && let Some(value) = free.get_mut(slot)
            {
                *value = true;
            }
        }
        let now = Instant::now();
        let shift = crate::timing::stream_resume_shift(
            deadline,
            now,
            now.duration_since(wait_started),
            frame_duration,
        );
        *timeline_shift += shift;
        deadline += shift;
        if let Some((_, _, Some(started), _)) = &mut transition {
            *started += shift;
        }
        if *emitted
            && now
                .checked_duration_since(deadline)
                .is_some_and(|late| late.as_secs_f64() > frame_step * 1.5)
        {
            return Ok(transition.is_some());
        }
        if let Some(delay) = deadline.checked_duration_since(now) {
            std::thread::sleep(delay);
        }
        let slot = free.iter().position(|value| *value).unwrap();
        let source = if let Some(upload) = &upload {
            if upload_frame {
                renderer.upload_nv12(
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
        if let Some((old_upload, old_uv, transition_started, transition_frames)) = &mut transition {
            let started = *transition_started.get_or_insert_with(Instant::now);
            let elapsed = started.elapsed();
            let progress = if *transition_frames == 0 {
                0.0
            } else {
                (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
            };
            finish_transition = progress >= 1.0;
            *transition_frames += 1;
            let old_source = Src::Views(old_upload.luma_view, old_upload.chroma_view);
            match (progress, sand, effect) {
                (progress, _, _) if progress <= 0.0 => {
                    renderer.render_to(&targets[slot], &mut exports[slot], &old_source, *old_uv)?
                }
                (progress, _, _) if progress >= 1.0 => {
                    renderer.render_to(&targets[slot], &mut exports[slot], &source, uv)?
                }
                (_, Some(style), _) => renderer.render_sand_to(
                    &targets[slot],
                    &mut exports[slot],
                    &old_source,
                    *old_uv,
                    &source,
                    uv,
                    progress,
                    style,
                )?,
                (_, None, Some(effect)) => renderer.render_effect_to(
                    &targets[slot],
                    &mut exports[slot],
                    &old_source,
                    *old_uv,
                    &source,
                    uv,
                    progress,
                    effect,
                )?,
                (_, None, None) => renderer.render_fade_to(
                    &targets[slot],
                    &mut exports[slot],
                    &old_source,
                    *old_uv,
                    &source,
                    uv,
                    progress * progress * (3.0 - 2.0 * progress),
                )?,
            }
        } else {
            renderer.render_to(&targets[slot], &mut exports[slot], &source, uv)?;
        }
        renderer.wait_frame_complete()?;
        renderer.signal_external_semaphore(&semaphores[slot])?;
        send_packet(socket, &packet(2, slot as u8, 0, 0, 0, 0, 0), None)
            .context("send dmabuf frame")?;
        free[slot] = false;
        if !*emitted {
            paper_runtime::plasma::frame_ready()?;
        }
        *emitted = true;
        if finish_transition {
            if let Some((_, _, Some(started), frames)) = &transition {
                tracing::info!(
                    frames,
                    fps = *frames as f64 / started.elapsed().as_secs_f64().max(0.001),
                    "skwd-wall-vk: stream transition complete"
                );
            }
            transition = None;
        }
        Ok(transition.is_some())
    };
    loop {
        let (frame, pts) = decoder.next()?;
        if last_pts.is_some_and(|last| pts < last) {
            first_pts = None;
            next_emit = 0.0;
            started = Instant::now();
            emitted = false;
            previous_frame = None;
            last_deadline = None;
            timeline_shift = std::time::Duration::ZERO;
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
            let mut fill_deadline = previous_deadline + frame_duration;
            while transition_active && fill_deadline < deadline {
                transition_active =
                    emit_frame(previous, false, fill_deadline, &mut emitted, &mut timeline_shift)?;
                fill_deadline += frame_duration;
            }
        }
        transition_active = emit_frame(&frame, true, deadline, &mut emitted, &mut timeline_shift)?;
        last_deadline = Some(deadline);
        previous_frame = Some(frame);
    }
}
