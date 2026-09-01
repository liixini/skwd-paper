use super::readiness::signal_ready;
use super::shared_upload::poll_ignoring_swap;
use crate::timing::{VideoClock, due};
use crate::{ctl, decode, vk, wayland};
use anyhow::{Context, Result};
use std::time::{Duration, Instant};

pub(super) struct SendFrame(pub(super) decode::RenderFrame, pub(super) f64);

pub(super) struct FadeState {
    pub(super) rx: std::sync::mpsc::Receiver<SendFrame>,
    pub(super) abort: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) worker: Option<std::thread::JoinHandle<()>>,
    pub(super) cur: (decode::RenderFrame, f64),
    pub(super) queued: Option<(decode::RenderFrame, f64)>,
    pub(super) pts0: f64,
    pub(super) speed: f64,
    pub(super) t0: Instant,
    pub(super) dur_ms: u64,
    pub(super) uvs: Vec<[f32; 4]>,
    pub(super) style: Option<i32>,
    pub(super) effect: Option<usize>,
    pub(super) still_b: bool,
    pub(super) first_frame: bool,
    pub(super) path: String,
}

impl FadeState {
    pub(super) fn cancel(mut self) {
        self.abort.store(true, std::sync::atomic::Ordering::Relaxed);
        drop(self.rx);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    pub(super) fn progress(&mut self) -> f32 {
        if self.first_frame {
            return 0.0;
        }
        let elapsed = self.t0.elapsed().as_secs_f64();
        loop {
            if self.queued.is_none() {
                self.queued = self.rx.try_recv().ok().map(|SendFrame(nf, np)| (nf, np));
            }
            let Some((_, np)) = &self.queued else { break };
            let np = *np;
            if np < self.pts0 {
                self.pts0 = np - elapsed * self.speed;
            }
            if due(elapsed, self.speed, np, self.pts0) {
                self.cur = self.queued.take().unwrap();
            } else {
                break;
            }
        }
        if self.dur_ms == 0 {
            return 1.0;
        }
        (self.t0.elapsed().as_millis() as f32 / self.dur_ms as f32).clamp(0.0, 1.0)
    }

    pub(super) fn note_committed(&mut self) {
        if self.first_frame {
            self.t0 = Instant::now();
            self.pts0 = self.cur.1;
        }
        self.first_frame = false;
    }
}
unsafe impl Send for SendFrame {}

pub(super) fn run_shared_vsync(
    target: &mut wayland::Target,
    dec: decode::AnyDecoder,
    renderer: &mut vk::Renderer,
    uv: [f32; 4],
    mut ctl: ctl::Ctl,
) -> Result<()> {
    let speed = env_speed();
    tracing::info!(
        "skwd-wall-vk: vsync-locked presentation, display-clock advance, speed {speed}x"
    );
    let (tx, rx) = std::sync::mpsc::sync_channel::<SendFrame>(1);
    decode_thread(dec, tx, "decode thread");
    let SendFrame(frame, pts) = rx.recv().context("first frame")?;
    let mut vclock = VideoClock::new(pts);
    let mut refresh_period = 1.0 / 144.0;
    let mut current = (frame, pts);
    let mut slot: Option<FrameSlot> = None;
    let mut pending: Option<(decode::RenderFrame, f64)> = None;
    let mut draws: u64 = 0;
    let mut advanced: u64 = 0;
    let mut report = Instant::now();
    let timing = std::env::var("SKWD_VK_TIMING").is_ok();
    let mut idle_paused = false;
    let mut last_draw: Option<Instant> = None;
    let mut intervals: Vec<f64> = Vec::with_capacity(720);
    loop {
        target.pump()?;
        if target.app.closed {
            return Ok(());
        }
        poll_ignoring_swap(&mut ctl);
        crate::freeze::write_last_requested(&mut ctl, Some(current.0.video()))?;
        let idle_should_pause = target.app.idle && draws > 0;
        if !idle_should_pause && idle_paused {
            idle_paused = false;
            if let Some(audio) = &mut ctl.audio {
                audio.set_pause(ctl.paused);
            }
        }
        if (ctl.paused && !ctl.freeze_pending()) || idle_should_pause {
            if idle_should_pause && !idle_paused {
                idle_paused = true;
                if let Some(audio) = &mut ctl.audio {
                    audio.set_pause(true);
                }
            }
            target.dispatch_wait_events(Instant::now() + Duration::from_secs(30))?;
            continue;
        }
        vclock.tick(refresh_period, speed);
        advanced += pump_frames(&rx, &mut pending, &mut current, &mut vclock)?;
        draw_source(renderer, &mut slot, &current.0, current.1, uv)?;
        draws += 1;
        timing_tick(&mut refresh_period, &mut last_draw, &mut intervals, timing);
        if draws == 1 {
            tracing::info!("skwd-wall-vk: first vsync-locked frame presented");
            signal_ready();
        }
        if draws.is_multiple_of(720) {
            let el = report.elapsed().as_secs_f64().max(0.001);
            tracing::info!(
                "skwd-wall-vk: {:.1} draws/s, {:.1} video fps",
                720.0 / el,
                advanced as f64 / el
            );
            advanced = 0;
            report = Instant::now();
        }
    }
}

pub(super) fn fade_fps_cap() -> u32 {
    std::env::var("SKWD_PAPER_SAND_FPS").ok().and_then(|val| val.parse().ok()).unwrap_or(0)
}

pub(super) fn env_speed() -> f64 {
    std::env::var("SKWD_VK_SPEED")
        .ok()
        .and_then(|val| val.parse().ok())
        .filter(|val: &f64| *val > 0.0)
        .unwrap_or(1.0)
}

pub(super) fn decode_thread(
    mut dec: decode::AnyDecoder,
    tx: std::sync::mpsc::SyncSender<SendFrame>,
    tag: &'static str,
) {
    std::thread::spawn(move || {
        loop {
            match dec.next_render() {
                Ok((frame, pts)) => {
                    if tx.send(SendFrame(frame, pts)).is_err() {
                        return;
                    }
                }
                Err(err) => {
                    tracing::info!("skwd-wall-vk: {tag}: {err:?}");
                    return;
                }
            }
        }
    });
}

enum FrameBacking {
    Upload(vk::UploadPath),
    Imported(vk::FrameImages),
}

pub(super) struct FrameSlot {
    backing: FrameBacking,
    pub(super) pts: f64,
    pub(super) dims: (u32, u32),
}

impl FrameSlot {
    pub(super) fn destroy(self, renderer: &vk::Renderer) {
        match self.backing {
            FrameBacking::Upload(upload) => renderer.destroy_upload_path(upload),
            FrameBacking::Imported(frame) => renderer.destroy_frame(frame),
        }
    }

    fn source(&self) -> vk::Src<'_> {
        match &self.backing {
            FrameBacking::Upload(upload) => vk::Src::Views(upload.luma_view, upload.chroma_view),
            FrameBacking::Imported(frame) => vk::Src::Imported(frame),
        }
    }
}

pub(super) fn is_vulkan_frame(frame: &ffmpeg_the_third::frame::Video) -> bool {
    frame.format() == ffmpeg_the_third::format::Pixel::VULKAN
}

pub(super) fn is_vaapi_frame(frame: &ffmpeg_the_third::frame::Video) -> bool {
    frame.format() == ffmpeg_the_third::format::Pixel::VAAPI
}

pub(super) fn is_cpu_frame(frame: &ffmpeg_the_third::frame::Video) -> bool {
    !is_vulkan_frame(frame) && !is_vaapi_frame(frame)
}

pub(super) fn needs_frame_slot(frame: &ffmpeg_the_third::frame::Video) -> bool {
    !is_vulkan_frame(frame)
}

pub(super) fn src_of<'a>(
    frame: &'a decode::RenderFrame,
    slot: Option<&'a FrameSlot>,
) -> vk::Src<'a> {
    if needs_frame_slot(frame)
        && let Some(slot) = slot
    {
        return slot.source();
    }
    vk::Src::Avvk(frame)
}

fn try_import_vaapi(
    rend: &vk::Renderer,
    frame: &decode::RenderFrame,
    dims: (u32, u32),
) -> Result<vk::FrameImages> {
    let mapped = frame.mapped_frame().context("VAAPI frame has no DRM-PRIME mapping")?;
    mapped.wait_ready()?;
    rend.import_nv12(dims.0, dims.1, &mapped.luma, &mapped.chroma)
        .context("import VAAPI DRM-PRIME frame")
}

fn upload_cpu(
    slot: &mut Option<FrameSlot>,
    rend: &vk::Renderer,
    frame: &ffmpeg_the_third::frame::Video,
    pts: f64,
) -> Result<()> {
    let dims = (frame.width(), frame.height());
    if !slot
        .as_ref()
        .is_some_and(|slot| slot.dims == dims && matches!(slot.backing, FrameBacking::Upload(_)))
    {
        if let Some(stale) = slot.take() {
            stale.destroy(rend);
        }
        *slot = Some(FrameSlot {
            backing: FrameBacking::Upload(rend.create_upload_path(dims.0, dims.1)?),
            pts: f64::NEG_INFINITY,
            dims,
        });
    }
    let Some(FrameSlot { backing: FrameBacking::Upload(upload), pts: uploaded_pts, .. }) = slot
    else {
        return Err(anyhow::anyhow!("upload slot unavailable"));
    };
    if *uploaded_pts != pts {
        rend.upload_nv12(upload, frame.data(0), frame.stride(0), frame.data(1), frame.stride(1))?;
        *uploaded_pts = pts;
    }
    Ok(())
}

pub(super) fn ensure_frame_slot(
    slot: &mut Option<FrameSlot>,
    rend: &vk::Renderer,
    frame: &decode::RenderFrame,
    pts: f64,
) -> Result<()> {
    if !is_vaapi_frame(frame) {
        return upload_cpu(slot, rend, frame, pts);
    }
    let dims = (frame.width(), frame.height());
    if slot
        .as_ref()
        .is_some_and(|slot| slot.dims == dims && matches!(slot.backing, FrameBacking::Upload(_)))
    {
        let transferred = decode::transfer_hardware_nv12(frame.video())?;
        return upload_cpu(slot, rend, &transferred, pts);
    }
    if slot.as_ref().is_some_and(|slot| slot.pts == pts && slot.dims == dims) {
        return Ok(());
    }
    if let Some(stale) = slot.take() {
        stale.destroy(rend);
    }
    match try_import_vaapi(rend, frame, dims) {
        Ok(imported) => {
            static LOGGED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::info!(
                    target: "skwd_wall::video_path",
                    "skwd-wall-vk: path = VAAPI DRM-PRIME -> Vulkan import"
                );
            }
            *slot = Some(FrameSlot { backing: FrameBacking::Imported(imported), pts, dims });
            Ok(())
        }
        Err(error) => {
            tracing::info!(
                target: "skwd_wall::video_path",
                "skwd-wall-vk: VAAPI DRM-PRIME import unavailable ({error:#}), using CPU transfer"
            );
            let transferred = decode::transfer_hardware_nv12(frame.video())?;
            upload_cpu(slot, rend, &transferred, pts)
        }
    }
}

pub(super) fn draw_source(
    renderer: &mut vk::Renderer,
    slot: &mut Option<FrameSlot>,
    frame: &decode::RenderFrame,
    pts: f64,
    uv: [f32; 4],
) -> Result<()> {
    if is_vulkan_frame(frame) {
        return renderer.draw_avvk(frame.video(), uv);
    }
    ensure_frame_slot(slot, renderer, frame, pts)?;
    match &slot.as_ref().context("frame slot")?.backing {
        FrameBacking::Upload(upload) => {
            renderer.draw_views(upload.luma_view, upload.chroma_view, uv, false)
        }
        FrameBacking::Imported(frame) => renderer.draw(frame, uv),
    }
}

pub(super) fn copy_exports_to_shm(
    renderers: &mut [vk::Renderer],
    exports: &[Vec<vk::ExportImage>],
    chosen: &[Option<usize>],
    ridx: &[usize],
    rings: &[(Vec<*mut u8>, u32)],
    rbs: &[vk::ReadbackBuf],
    distinct: &[(u32, u32)],
    dims: &[(u32, u32)],
) -> Result<()> {
    for ri in 0..distinct.len() {
        let Some(bi) = chosen[ri] else {
            continue;
        };
        let (w, h) = distinct[ri];
        renderers[ri].read_export_to(&exports[ri][bi], &rbs[ri], w, h)?;
    }
    for (si, &(w, h)) in dims.iter().enumerate() {
        let ri = ridx[si];
        let Some(bi) = chosen[ri] else {
            continue;
        };
        let (ptrs, dst_stride) = &rings[si];
        let src_stride = (w * 4) as usize;
        let dst_stride = *dst_stride as usize;
        unsafe {
            let src = rbs[ri].ptr;
            let dst = ptrs[bi];
            for y in 0..h as usize {
                std::ptr::copy_nonoverlapping(
                    src.add(y * src_stride),
                    dst.add(y * dst_stride),
                    src_stride,
                );
            }
        }
    }
    Ok(())
}

pub(super) fn pump_frames(
    rx: &std::sync::mpsc::Receiver<SendFrame>,
    pending: &mut Option<(decode::RenderFrame, f64)>,
    current: &mut (decode::RenderFrame, f64),
    vclock: &mut VideoClock,
) -> Result<u64> {
    let mut advanced: u64 = 0;
    for _ in 0..4 {
        if pending.is_none() {
            match rx.try_recv() {
                Ok(SendFrame(frame, pts)) => *pending = Some((frame, pts)),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(anyhow::anyhow!("decode thread exited"));
                }
            }
        }
        match pending.take() {
            Some((frame, pts)) if vclock.should_show(pts) => {
                *current = (frame, pts);
                advanced += 1;
            }
            other => {
                *pending = other;
                break;
            }
        }
    }
    Ok(advanced)
}

pub(super) fn timing_tick(
    refresh_period: &mut f64,
    last_draw: &mut Option<Instant>,
    intervals: &mut Vec<f64>,
    timing: bool,
) {
    let now = Instant::now();
    if let Some(prev) = *last_draw {
        let secs = now.duration_since(prev).as_secs_f64();
        if (secs - *refresh_period).abs() < *refresh_period * 0.2 {
            *refresh_period = *refresh_period * 0.995 + secs * 0.005;
        }
        if timing {
            intervals.push(secs * 1000.0);
        }
    }
    *last_draw = Some(now);
    if timing && intervals.len() >= 720 {
        intervals.sort_by(f64::total_cmp);
        let iv: &[f64] = &intervals[..];
        let pct = |q: f64| iv[((iv.len() - 1) as f64 * q) as usize];
        tracing::info!(
            "interval ms min={:.2} p10={:.2} p50={:.2} p90={:.2} p99={:.2} max={:.2} period={:.4}",
            iv[0],
            pct(0.10),
            pct(0.50),
            pct(0.90),
            pct(0.99),
            iv[iv.len() - 1],
            *refresh_period * 1000.0
        );
        intervals.clear();
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;

    fn frame(pts: f64) -> (decode::RenderFrame, f64) {
        (decode::RenderFrame::plain(ffmpeg_the_third::frame::Video::empty()), pts)
    }

    fn state(staged_for: Duration) -> FadeState {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        tx.send(SendFrame(frame(1.0).0, 1.0)).unwrap();
        FadeState {
            rx,
            abort: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            worker: None,
            cur: frame(0.0),
            queued: None,
            pts0: 0.0,
            speed: 1.0,
            t0: Instant::now() - staged_for,
            dur_ms: 600,
            uvs: Vec::new(),
            style: None,
            effect: None,
            still_b: false,
            first_frame: true,
            path: String::new(),
        }
    }

    #[test]
    fn staged_time_does_not_advance_first_transition_frame() {
        let mut fade = state(Duration::from_secs(2));
        assert_eq!(fade.progress(), 0.0);
        assert_eq!(fade.cur.1, 0.0);
        assert!(fade.queued.is_none());
    }

    #[test]
    fn first_commit_rebases_transition_and_video_clocks() {
        let mut fade = state(Duration::from_secs(2));
        fade.note_committed();
        assert!(!fade.first_frame);
        assert_eq!(fade.pts0, fade.cur.1);
        assert!(fade.t0.elapsed() < Duration::from_millis(100));
        assert!(fade.progress() < 0.2);
        assert_eq!(fade.cur.1, 0.0);
    }
}
