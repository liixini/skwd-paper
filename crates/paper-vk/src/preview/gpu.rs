use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn stream(
    from: &str,
    to: &str,
    shader: &str,
    width: u32,
    height: u32,
    duration_ms: u64,
    frame_ms: u64,
    socket: RawFd,
) -> Result<()> {
    let (width, height) = (width.max(16), height.max(16));
    let shared = crate::shared::create(std::ptr::null_mut()).context("transition Vulkan device")?;
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
    )?;
    renderer.ensure_scene_pool(2)?;
    let (aw, ah, pixels) = load(from)?;
    let a = renderer.create_scene_texture(aw, ah, &pixels)?;
    let (bw, bh, pixels) = load(to)?;
    let b = renderer.create_scene_texture(bw, bh, &pixels)?;
    let auv = crate::fill::mode_uv(aw, ah, width, height);
    let buv = crate::fill::mode_uv(bw, bh, width, height);
    let mut exports =
        (0..3).map(|_| renderer.create_stream_export(width, height)).collect::<Result<Vec<_>>>()?;
    let semaphores =
        (0..3).map(|_| renderer.create_external_semaphore()).collect::<Result<Vec<_>>>()?;
    let targets = exports
        .iter()
        .map(|export| renderer.create_export_rt(export))
        .collect::<Result<Vec<_>>>()?;
    for (slot, export) in exports.iter().enumerate() {
        send_packet(
            socket,
            &packet(4, slot as u8, width, height, 0, 0, export.allocation_size),
            Some(export.fd),
        )?;
        send_packet(socket, &packet(5, slot as u8, 0, 0, 0, 0, 0), Some(semaphores[slot].fd))?;
    }
    let shader = selected_shader(shader);
    let sand = paper_shaders::sand_style_index(shader);
    let effect = sand.is_none().then(|| paper_shaders::effect_index(shader)).flatten();
    let duration = std::time::Duration::from_millis(duration_ms.max(100));
    let interval = std::time::Duration::from_millis(frame_ms.clamp(4, 200));
    let started = Instant::now();
    let mut frames = 0;
    let mut consumer_wait = std::time::Duration::ZERO;
    let mut render_time = std::time::Duration::ZERO;
    let mut free = [true; 3];
    loop {
        let wait_started = Instant::now();
        let deadline = Instant::now() + interval;
        while let Some(slot) = receive_ack(socket, false)? {
            free[slot] = true;
        }
        while !free.iter().any(|slot| *slot) {
            if let Some(slot) = receive_ack(socket, true)? {
                free[slot] = true;
            }
        }
        let slot = free.iter().position(|slot| *slot).unwrap();
        consumer_wait += wait_started.elapsed();
        let render_started = Instant::now();
        let progress = if frames == 0 {
            0.0
        } else {
            (started.elapsed().as_secs_f32() / duration.as_secs_f32()).min(1.0)
        };
        let a = Src::Rgba(a.view);
        let b = Src::Rgba(b.view);
        match (sand, effect) {
            (Some(style), _) => renderer.render_sand_to(
                &targets[slot],
                &mut exports[slot],
                &a,
                auv,
                &b,
                buv,
                progress,
                style,
            )?,
            (_, Some(effect)) => renderer.render_effect_to(
                &targets[slot],
                &mut exports[slot],
                &a,
                auv,
                &b,
                buv,
                progress,
                effect,
            )?,
            _ => renderer.render_fade_to(
                &targets[slot],
                &mut exports[slot],
                &a,
                auv,
                &b,
                buv,
                progress * progress * (3.0 - 2.0 * progress),
            )?,
        }
        renderer.wait_frame_complete()?;
        render_time += render_started.elapsed();
        renderer.signal_external_semaphore(&semaphores[slot])?;
        send_packet(socket, &packet(2, slot as u8, 0, 0, 0, 0, 0), None)?;
        free[slot] = false;
        frames += 1;
        if progress >= 1.0 {
            unsafe {
                shared.device.device_wait_idle()?;
            }
            tracing::info!(
                frames,
                fps = frames as f64 / started.elapsed().as_secs_f64(),
                consumer_wait_ms = consumer_wait.as_millis(),
                render_ms = render_time.as_millis(),
                "skwd-wall-vk: GPU transition complete"
            );
            return Ok(());
        }
        if let Some(delay) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(delay);
        }
    }
}
