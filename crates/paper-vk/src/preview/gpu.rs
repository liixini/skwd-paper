use super::*;

pub(crate) fn stream(preview: &Preview, socket: RawFd) -> Result<()> {
    let (width, height) = preview.extent();
    let (shared, mut renderer) = preview.renderer()?;
    let pair = Pair::load(&mut renderer, preview)?;
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
    let duration = std::time::Duration::from_millis(preview.duration_ms.max(MIN_DURATION_MS));
    let interval = preview.frame_interval;
    let started = Instant::now();
    let mut frames = 0;
    let mut consumer_wait = std::time::Duration::ZERO;
    let mut render_time = std::time::Duration::ZERO;
    let mut free = [true; 3];
    loop {
        let wait_started = Instant::now();
        let deadline = Instant::now() + interval;
        drain_acks(socket, &mut free)?;
        if !wait_any_free(
            &[socket],
            std::slice::from_mut(&mut free),
            &[true],
            &[],
            Some(started + duration),
        )? {
            consumer_wait += wait_started.elapsed();
            break;
        }
        let slot = free.iter().position(|slot| *slot).unwrap();
        consumer_wait += wait_started.elapsed();
        let render_started = Instant::now();
        let progress = if frames == 0 {
            0.0
        } else {
            (started.elapsed().as_secs_f32() / duration.as_secs_f32()).min(1.0)
        };
        pair.render(&mut renderer, &targets[slot], &mut exports[slot], progress, false)?;
        renderer.wait_render()?;
        render_time += render_started.elapsed();
        renderer.complete_external_signal(&semaphores[slot])?;
        send_packet(socket, &packet(2, slot as u8, 0, 0, 0, 0, 0), None)?;
        free[slot] = false;
        frames += 1;
        if progress >= 1.0 {
            break;
        }
        if let Some(delay) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(delay);
        }
    }
    crate::shared::wait_idle(&shared.device)?;
    tracing::info!(
        frames,
        fps = frames as f64 / started.elapsed().as_secs_f64(),
        consumer_wait_ms = consumer_wait.as_millis(),
        render_ms = render_time.as_millis(),
        "skwd-wall-vk: GPU transition complete"
    );
    Ok(())
}
