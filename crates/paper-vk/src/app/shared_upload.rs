use super::readiness::signal_ready;
use super::shared_support::{FrameSlot, draw_source, run_shared_vsync};
use crate::fill::mode_uv;
use crate::shared;
use crate::timing::Pacer;
use crate::{ctl, decode, vk, wayland};
use anyhow::{Context, Result};
use std::time::{Duration, Instant};

pub(super) fn run_shared(
    target: &mut wayland::Target,
    video: &str,
    mute: bool,
    volume: u32,
) -> Result<()> {
    let sd = shared::create(target.display_ptr()).context("shared device")?;
    if !sd.queue_sync {
        tracing::info!(
            "skwd-wall-vk: FFmpeg queue callbacks unavailable, using separate-device upload"
        );
        return super::upload::run_upload(target, video, mute, volume);
    }
    tracing::info!("skwd-wall-vk: shared VkDevice accepted by ffmpeg");
    let mut dec = decode::open_decoder(
        video,
        Some(sd.hwdev),
        sd.video_decode,
        sd.render_node.as_deref(),
        shared::software_decode_required(sd.software, sd.queue_sync),
    )
    .context("decoder on shared device")?;
    if dec.still() {
        return Err(anyhow::anyhow!("still sources are rendered by skwd-wall-still"));
    }
    let (video_w, video_h) = dec.dims();
    let (w0, h0) = target.size_at(0);
    let mut renderer = vk::Renderer::new_shared(
        (
            sd.entry.clone(),
            sd.instance.clone(),
            sd.phys,
            sd.device.clone(),
            sd.gfx_family,
            sd.queue,
        ),
        target.display_ptr(),
        target.surface_ptr_at(0),
        w0,
        h0,
    )
    .context("renderer on shared device")?;
    tracing::info!("skwd-wall-vk: path = shared-device (zero-copy)");

    let uv = mode_uv(video_w, video_h, renderer.extent.width, renderer.extent.height);
    let mut ctl = ctl::Ctl::start(video, mute, volume, true);
    target.ctl_fd = ctl.wake_fd();
    if std::env::var("SKWD_VK_VSYNC").as_deref() != Ok("0") {
        return run_shared_vsync(target, dec, &mut renderer, uv, ctl);
    }
    let mut pacer = Pacer::new();
    let mut frames: u64 = 0;
    let mut slot: Option<FrameSlot> = None;
    let mut last_presented: Option<ffmpeg_the_third::frame::Video> = None;
    let mut report = Instant::now();
    loop {
        target.pump()?;
        if target.app.closed {
            return Ok(());
        }
        poll_ignoring_swap(&mut ctl);
        crate::freeze::write_last_requested(&mut ctl, last_presented.as_ref())?;
        if ctl.paused && !ctl.freeze_pending() {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let (frame, pts) = dec.next_render()?;
        let now = Instant::now();
        let due = pacer.due(now, pts);
        if due > now {
            std::thread::sleep(due - now);
        }
        let draw_start = Instant::now();
        maybe_draw(&mut renderer, &mut slot, &frame, pts, uv)?;
        last_presented = Some(decode::retain_frame(frame.video())?);
        report_timing(frames, draw_start, due);
        hash_frame(&frame, frames, pts);
        drop(frame);
        frames += 1;
        if frames == 1 {
            tracing::info!("skwd-wall-vk: first zero-copy frame presented");
            signal_ready();
        }
        if frames.is_multiple_of(120) {
            tracing::info!(
                "skwd-wall-vk: {frames} frames, {:.1} fps",
                120.0 / report.elapsed().as_secs_f64().max(0.001)
            );
            report = Instant::now();
        }
    }
}

pub(super) fn poll_ignoring_swap(ctl: &mut ctl::Ctl) {
    if let Some(req) = ctl.poll() {
        tracing::info!(
            "skwd-wall-vk: swap to {} ignored on debug path SKWD_VK_PATH=shared",
            req.to
        );
    }
}

pub(super) fn maybe_draw(
    renderer: &mut vk::Renderer,
    slot: &mut Option<FrameSlot>,
    frame: &decode::RenderFrame,
    pts: f64,
    uv: [f32; 4],
) -> Result<()> {
    if std::env::var("SKWD_VK_NODRAW").is_err() {
        return draw_source(renderer, slot, frame, pts, uv);
    }
    Ok(())
}

pub(super) fn report_timing(frames: u64, draw_start: Instant, due: Instant) {
    if std::env::var("SKWD_VK_TIMING").is_err() {
        return;
    }
    let draw_ms = draw_start.elapsed().as_secs_f64() * 1000.0;
    let late_ms = draw_start.duration_since(due).as_secs_f64() * 1000.0;
    if draw_ms > 12.0 || late_ms > 12.0 {
        tracing::info!("timing frame {frames} draw={draw_ms:.1}ms late={late_ms:.1}ms");
    }
}

pub(super) fn hash_frame(frame: &ffmpeg_the_third::frame::Video, frames: u64, pts: f64) {
    if std::env::var("SKWD_VK_HASH").is_err() {
        return;
    }
    let mut sw = ffmpeg_the_third::frame::Video::empty();
    let rc = unsafe {
        ffmpeg_the_third::ffi::av_hwframe_transfer_data(sw.as_mut_ptr(), frame.as_ptr(), 0)
    };
    if rc >= 0 {
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in sw.data(0) {
            hash = (hash ^ *byte as u64).wrapping_mul(0x100000001b3);
        }
        tracing::info!("hash {frames} {hash:016x} pts={pts:.4}");
    } else {
        tracing::info!("hash {frames} transfer_failed({rc}) pts={pts:.4}");
    }
}
