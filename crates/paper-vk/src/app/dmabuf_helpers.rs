use super::shared_support::{FadeState, FrameSlot, SendFrame, decode_thread, env_speed, src_of};
use crate::decode::open_decoder;
use crate::fill::mode_uv;
use crate::timing::{frame_target, loop_frame_target, pace_stalled};
use crate::{ctl, decode, vk, wayland};
use anyhow::{Context, Result};
use std::time::Instant;

pub(super) fn open_shared_decoder(
    pattern: &Option<String>,
    video: &str,
    shared: &crate::shared::SharedDevice,
    force_software: bool,
) -> Result<Option<decode::AnyDecoder>> {
    if pattern.is_some() {
        return Ok(None);
    }
    Ok(Some(open_decoder(
        video,
        Some(shared.hwdev),
        shared.video_decode,
        shared.render_node.as_deref(),
        force_software,
    )?))
}

pub(super) fn init_free_buffers(target: &mut wayland::Target, n_exports: usize) {
    for surf in &mut target.app.surfaces {
        surf.free_buffers = (0..n_exports).collect();
    }
}

pub(super) fn spawn_decode(
    dec: Option<decode::AnyDecoder>,
    tx: std::sync::mpsc::SyncSender<SendFrame>,
) {
    match dec {
        Some(dec) => decode_thread(dec, tx, "decode thread"),
        None => drop(tx),
    }
}

pub(super) struct PendingSwap {
    req: ctl::SwapReq,
    rx: std::sync::mpsc::Receiver<SendFrame>,
    abort: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    still: std::sync::Arc<std::sync::atomic::AtomicBool>,
    started: Instant,
}

pub(super) struct RgbaStillSource {
    pub(super) texture: vk::SceneTexture,
    pub(super) uvs: Vec<[f32; 4]>,
}

pub(super) fn load_rgba_still(
    renderer: &mut vk::Renderer,
    path: &str,
    dims: &[(u32, u32)],
) -> Result<RgbaStillSource> {
    // Ordinary video presenters do not otherwise allocate scene descriptors.
    // Reserve enough slots for startup endpoints and later warm swaps before
    // creating the first RGBA texture.
    renderer.ensure_scene_pool(16)?;
    let image = image::ImageReader::open(path)
        .with_context(|| format!("open transition still {path}"))?
        .with_guessed_format()?
        .decode()
        .with_context(|| format!("decode transition still {path}"))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    let texture = renderer.create_scene_texture(width, height, image.as_raw())?;
    let uvs = dims.iter().map(|&(w, h)| mode_uv(width, height, w, h)).collect();
    Ok(RgbaStillSource { texture, uvs })
}

impl PendingSwap {
    pub(super) fn cancel(mut self) {
        self.abort.store(true, std::sync::atomic::Ordering::Relaxed);
        drop(self.rx);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(super) fn begin_swap(
    req: &ctl::SwapReq,
    hwdev: *mut ffmpeg_the_third::ffi::AVBufferRef,
    vulkan_decode: bool,
    render_node: Option<std::path::PathBuf>,
    force_software: bool,
) -> PendingSwap {
    let abort = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let still = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::sync_channel::<SendFrame>(1);
    struct HwPtr(*mut ffmpeg_the_third::ffi::AVBufferRef);
    unsafe impl Send for HwPtr {}
    let hw = HwPtr(hwdev);
    let path = req.to.clone();
    let abort_thread = abort.clone();
    let still_thread = still.clone();
    let worker = std::thread::spawn(move || {
        let hw = hw;
        let mut dec = match open_decoder(
            &path,
            Some(hw.0),
            vulkan_decode,
            render_node.as_deref(),
            force_software,
        ) {
            Ok(decoder) => decoder,
            Err(error) => {
                tracing::info!("skwd-wall-vk: swap open {path} failed: {error:?}");
                return;
            }
        };
        still_thread.store(dec.still(), std::sync::atomic::Ordering::Relaxed);
        dec.install_abort(abort_thread);
        loop {
            match dec.next_render() {
                Ok((frame, pts)) => {
                    if tx.send(SendFrame(frame, pts)).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    tracing::info!("skwd-wall-vk: swap decode thread: {error:?}");
                    return;
                }
            }
        }
    });
    PendingSwap {
        req: ctl::SwapReq {
            to: req.to.clone(),
            duration_ms: req.duration_ms,
            shader: req.shader.clone(),
            properties: req.properties.clone(),
        },
        rx,
        abort,
        worker: Some(worker),
        still,
        started: Instant::now(),
    }
}

pub(super) fn wait_free_buffers(
    target: &mut wayland::Target,
    chosen: &mut [Option<usize>],
    groups: &[Vec<usize>],
    n_exports: usize,
) -> Result<bool> {
    chosen.fill(None);
    for (ri, slot) in chosen.iter_mut().enumerate() {
        *slot = match target.try_free_group_buffer_in(&groups[ri], 0..n_exports)? {
            wayland::GroupBufferWait::Ready(bi) => Some(bi),
            wayland::GroupBufferWait::Closed => None,
            wayland::GroupBufferWait::Unavailable => {
                for (returned_ri, returned) in chosen[..ri].iter_mut().enumerate() {
                    if let Some(bi) = returned.take() {
                        for &si in &groups[returned_ri] {
                            target.return_uncommitted_buffer_at(si, bi);
                        }
                    }
                }
                return Ok(true);
            }
        };
    }
    Ok(false)
}

pub(super) fn retarget_anchor(
    target: &wayland::Target,
    anchor: &mut usize,
    base: &mut Option<(u64, f64)>,
    wall_anchor: &mut Option<(u64, f64)>,
    scheduled_q: &mut std::collections::VecDeque<u64>,
    flip_hist: &mut Vec<u64>,
) {
    let live_anchor = target
        .app
        .surfaces
        .iter()
        .enumerate()
        .filter(|(_, surface)| !surface.closed)
        .max_by_key(|(_, surface)| surface.fps_limit)
        .map_or(0, |(index, _)| index);
    if live_anchor == *anchor {
        return;
    }
    *anchor = live_anchor;
    *base = None;
    *wall_anchor = None;
    scheduled_q.clear();
    flip_hist.clear();
}

pub(super) fn render_surfaces(
    renderers: &mut [vk::Renderer],
    rts: &[Vec<vk::RenderTarget>],
    exports: &mut [Vec<vk::ExportImage>],
    chosen: &[Option<usize>],
    distinct: &[(u32, u32)],
    pattern: &Option<String>,
    frame: &decode::RenderFrame,
    uvs: &[[f32; 4]],
    fade: Option<&FadeState>,
    fade_mix: Option<f32>,
    frames: u64,
    sw_a: Option<&FrameSlot>,
    sw_b: Option<&FrameSlot>,
    rgba_a: Option<&RgbaStillSource>,
    rgba_b: Option<&RgbaStillSource>,
) -> Result<()> {
    for ri in 0..renderers.len() {
        let Some(bi) = chosen[ri] else {
            continue;
        };
        let (w, h) = distinct[ri];
        let rt = &rts[ri][if exports[ri][bi].direct_render { bi } else { 0 }];
        match pattern.as_deref() {
            None => render_video(
                &mut renderers[ri],
                rt,
                &mut exports[ri][bi],
                frame,
                uvs[ri],
                fade,
                fade_mix,
                ri,
                sw_a,
                sw_b,
                rgba_a,
                rgba_b,
            )?,
            Some(mode) => {
                render_pattern(&mut renderers[ri], rt, &mut exports[ri][bi], mode, w, h, frames)?
            }
        }
    }
    Ok(())
}

pub(super) fn step_ns(pts: f64, prev_pts: f64, speed: f64, prev_step_ns: u64) -> u64 {
    if pts > prev_pts && prev_pts.is_finite() {
        return ((pts - prev_pts) / speed * 1e9) as u64;
    }
    prev_step_ns
}

pub(super) fn wait_render_all(
    renderers: &mut [vk::Renderer],
    pattern: &Option<String>,
) -> Result<()> {
    if pattern.is_some() {
        return Ok(());
    }
    for rend in renderers {
        rend.wait_render()?;
    }
    Ok(())
}

pub(super) fn present_all(
    target: &mut wayland::Target,
    buffers: &[Vec<wayland_client::protocol::wl_buffer::WlBuffer>],
    chosen: &[Option<usize>],
    ridx: &[usize],
    n_surf: usize,
) -> Result<bool> {
    let now_ns = monotonic_ns();
    let mut committed = false;
    for (ri, selected) in chosen.iter().enumerate() {
        let Some(bi) = *selected else {
            continue;
        };
        let surfaces: Vec<usize> =
            (0..n_surf).filter(|&si| !target.app.surfaces[si].closed && ridx[si] == ri).collect();
        let due = surfaces.iter().copied().any(|si| target.commit_due_at(si, now_ns));
        if due {
            // `bi` was acquired as one shared group slot. Attach it to the
            // entire group so every compositor release converges on the same
            // free slot; splitting a shared slot by per-output cadence can
            // permanently fragment a two-deep ring.
            for si in surfaces {
                target.attach_at(si, &buffers[si][bi]);
                target.request_presentation_feedback_at(si);
                target.commit_at(si);
                committed = true;
            }
        } else {
            for si in surfaces {
                target.return_uncommitted_buffer_at(si, bi);
            }
        }
    }
    target.flush()?;
    Ok(committed)
}

pub(super) fn monotonic_ns() -> u64 {
    let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) };
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}

pub(super) fn complete_swap(
    fade_mix: Option<f32>,
    fade: &mut Option<FadeState>,
    renderers: &mut [vk::Renderer],
    rx: &mut std::sync::mpsc::Receiver<SendFrame>,
    pending: &mut Option<(decode::RenderFrame, f64)>,
    uvs: &mut Vec<[f32; 4]>,
    base: &mut Option<(u64, f64)>,
    wall_anchor: &mut Option<(u64, f64)>,
    scheduled_q: &mut std::collections::VecDeque<u64>,
    flip_hist: &mut Vec<u64>,
    prev_pts: &mut f64,
    prev_commit_ns: &mut u64,
    prev_step_ns: &mut u64,
    sw_a: &mut Option<FrameSlot>,
    sw_b: &mut Option<FrameSlot>,
    still: &mut bool,
) {
    let Some(mix) = fade_mix else {
        return;
    };
    if mix < 1.0 {
        return;
    }
    let done = fade.take().unwrap();
    *still = done.still_b;
    for rend in renderers.iter_mut() {
        let _ = rend.retire_plane_views();
    }
    if let Some(sl) = sw_a.take() {
        sl.destroy(&renderers[0]);
    }
    *sw_a = sw_b.take();
    *rx = done.rx;
    *pending = Some(done.cur);
    *uvs = done.uvs;
    *base = None;
    *wall_anchor = None;
    scheduled_q.clear();
    flip_hist.clear();
    *prev_pts = f64::NEG_INFINITY;
    *prev_commit_ns = 0;
    *prev_step_ns = 0;
    tracing::info!("skwd-wall-vk: swap complete");
}

pub(super) fn dedup_dims(dims: &[(u32, u32)]) -> (Vec<(u32, u32)>, Vec<usize>) {
    let mut distinct: Vec<(u32, u32)> = Vec::new();
    let ridx: Vec<usize> = dims
        .iter()
        .map(|dim| {
            if let Some(idx) = distinct.iter().position(|known| known == dim) {
                idx
            } else {
                distinct.push(*dim);
                distinct.len() - 1
            }
        })
        .collect();
    (distinct, ridx)
}

pub(super) fn shared_render_dims(
    dims: &[(u32, u32)],
    source: Option<(u32, u32)>,
    mode: Option<&str>,
    supported: bool,
    automatic: bool,
) -> Vec<(u32, u32)> {
    if !supported || dims.len() < 2 {
        return dims.to_vec();
    }
    let largest = dims.iter().copied().max_by_key(|(w, h)| u64::from(*w) * u64::from(*h));
    let extent = match mode {
        Some("source") => source,
        Some("max") => largest,
        None | Some("auto") if automatic && matching_aspect_ratios(dims, source) => {
            match (source, largest) {
                (Some(source), Some(largest))
                    if u64::from(source.0) * u64::from(source.1)
                        <= u64::from(largest.0) * u64::from(largest.1) =>
                {
                    Some(source)
                }
                (_, largest) => largest,
            }
        }
        _ => None,
    };
    extent.map_or_else(|| dims.to_vec(), |extent| vec![extent; dims.len()])
}

pub(super) fn xr_render_dims(
    dims: &[(u32, u32)],
    source: Option<(u32, u32)>,
    mode: Option<&str>,
    supported: bool,
) -> Vec<(u32, u32)> {
    let uniform = dims.windows(2).all(|pair| pair[0] == pair[1]);
    shared_render_dims(dims, source, mode, supported, uniform)
}

fn matching_aspect_ratios(dims: &[(u32, u32)], source: Option<(u32, u32)>) -> bool {
    let Some(source) = source else {
        return false;
    };
    dims.iter().copied().all(|dim| aspect_ratio_matches(source, dim))
}

fn aspect_ratio_matches(a: (u32, u32), b: (u32, u32)) -> bool {
    if a.0 == 0 || a.1 == 0 || b.0 == 0 || b.1 == 0 {
        return false;
    }
    let lhs = u128::from(a.0) * u128::from(b.1);
    let rhs = u128::from(b.0) * u128::from(a.1);
    lhs.abs_diff(rhs) * 200 <= lhs.max(rhs)
}

pub(super) fn create_buffers(
    target: &mut wayland::Target,
    dims: &[(u32, u32)],
    exports: &[impl AsRef<[vk::ExportImage]>],
    ridx: &[usize],
) -> Result<Vec<Vec<wayland_client::protocol::wl_buffer::WlBuffer>>> {
    let mut buffers = Vec::with_capacity(dims.len());
    for (si, &(w, h)) in dims.iter().enumerate() {
        let ring = exports[ridx[si]]
            .as_ref()
            .iter()
            .enumerate()
            .map(|(bi, exp)| {
                target.create_dmabuf_buffer(
                    exp.fd,
                    w,
                    h,
                    exp.offset,
                    exp.stride,
                    exp.modifier,
                    si,
                    bi,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        buffers.push(ring);
    }
    Ok(buffers)
}

#[cfg(test)]
mod tests;

pub(super) fn pattern_check(
    pattern: &Option<String>,
    exports: &[Vec<vk::ExportImage>],
) -> Result<()> {
    let Some(mode) = pattern else {
        return Ok(());
    };
    tracing::info!("skwd-wall-vk: PATTERN MODE ({mode}) - moving bar, no video");
    if mode == "cpu" && exports.iter().flatten().any(|exp| exp.mapped.is_none()) {
        return Err(anyhow::anyhow!("export memory not host-mappable, cpu pattern unavailable"));
    }
    Ok(())
}

pub(super) fn start_swap(
    req: &ctl::SwapReq,
    shared: &crate::shared::SharedDevice,
    force_software: bool,
    dims: &[(u32, u32)],
    ctl: &mut ctl::Ctl,
) -> Option<FadeState> {
    let pending = begin_swap(
        req,
        shared.hwdev,
        shared.video_decode,
        shared.render_node.clone(),
        force_software,
    );
    let received = pending.rx.recv_timeout(std::time::Duration::from_secs(3));
    let Ok(frame) = received else {
        tracing::info!("skwd-wall-vk: swap: no frame from {} within 3s, keeping current", req.to);
        pending.cancel();
        return None;
    };
    Some(finish_swap(pending, frame, dims, ctl))
}

pub(super) fn poll_pending_swap(
    pending: &mut Option<PendingSwap>,
    dims: &[(u32, u32)],
    ctl: &mut ctl::Ctl,
) -> Option<FadeState> {
    let received = match pending.as_ref()?.rx.try_recv() {
        Ok(frame) => Some(Ok(frame)),
        Err(std::sync::mpsc::TryRecvError::Empty)
            if pending.as_ref()?.started.elapsed() < std::time::Duration::from_secs(3) =>
        {
            None
        }
        Err(error) => Some(Err(error)),
    };
    match received? {
        Ok(frame) => Some(finish_swap(pending.take().unwrap(), frame, dims, ctl)),
        Err(error) => {
            let failed = pending.take().unwrap();
            tracing::info!("skwd-wall-vk: swap staging failed for {}: {error}", failed.req.to);
            failed.cancel();
            None
        }
    }
}

fn finish_swap(
    mut pending: PendingSwap,
    SendFrame(frame, pts): SendFrame,
    dims: &[(u32, u32)],
    ctl: &mut ctl::Ctl,
) -> FadeState {
    let (bw, bh) = (frame.width(), frame.height());
    let uvs_b: Vec<[f32; 4]> = dims.iter().map(|&(w, h)| mode_uv(bw, bh, w, h)).collect();
    ctl.swap_audio(&pending.req.to, true);
    let shader = pending.req.shader.as_deref().map(|name| {
        if name == "random" {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            let sand = paper_shaders::SAND_STYLES.len();
            let pick = nanos as usize % (sand + paper_shaders::EFFECTS.len());
            if pick < sand {
                paper_shaders::SAND_STYLES[pick]
            } else {
                paper_shaders::EFFECTS[pick - sand].0
            }
        } else {
            name
        }
    });
    let style = shader.and_then(paper_shaders::sand_style_index);
    let effect = if style.is_none() { shader.and_then(paper_shaders::effect_index) } else { None };
    tracing::info!(
        "skwd-wall-vk: swap to {} ({}ms, style {:?}, effect {:?})",
        pending.req.to,
        pending.req.duration_ms,
        style,
        effect.map(|fx| paper_shaders::EFFECTS[fx].0)
    );
    FadeState {
        rx: pending.rx,
        abort: pending.abort,
        worker: pending.worker.take(),
        cur: (frame, pts),
        queued: None,
        pts0: pts,
        speed: env_speed(),
        t0: Instant::now(),
        dur_ms: pending.req.duration_ms,
        uvs: uvs_b,
        style,
        effect,
        still_b: pending.still.load(std::sync::atomic::Ordering::Relaxed),
        first_frame: true,
        path: pending.req.to,
    }
}

pub(super) fn next_pending(
    rx: &std::sync::mpsc::Receiver<SendFrame>,
    pattern: &Option<String>,
    frames: u64,
) -> Result<(decode::RenderFrame, f64)> {
    if pattern.is_some() {
        return Ok((
            decode::RenderFrame::plain(ffmpeg_the_third::frame::Video::empty()),
            frames as f64 / 24.0,
        ));
    }
    let SendFrame(frame, pts) = rx.recv().context("decode thread gone")?;
    Ok((frame, pts))
}

pub(super) fn promote_transition_target(
    fade: &mut Option<FadeState>,
    pending: &mut Option<(decode::RenderFrame, f64)>,
) -> bool {
    let Some(target) = fade.as_mut() else {
        return false;
    };
    *pending = Some((target.cur.0.clone(), target.cur.1));
    target.dur_ms = 0;
    target.first_frame = false;
    true
}

pub(super) fn prepare_transition_pipelines(
    renderers: &mut [vk::Renderer],
    fade: &FadeState,
    rgba: bool,
) -> Result<()> {
    for renderer in renderers {
        if let Some(effect) = fade.effect {
            renderer.effect_pipeline(effect)?;
        } else {
            renderer.ensure_transition_pipelines()?;
        }
        if rgba {
            renderer.ensure_rgba_pipelines()?;
        }
    }
    Ok(())
}

pub(super) fn content_hash(frame: &ffmpeg_the_third::frame::Video) -> Option<u64> {
    if !super::shared_support::is_cpu_frame(frame) {
        return None;
    }
    let mut hh: u64 = 0xcbf29ce484222325;
    for plane in 0..2usize {
        if plane >= unsafe { (*frame.as_ptr()).data.len() }
            || unsafe { (*frame.as_ptr()).data[plane].is_null() }
        {
            break;
        }
        let data = frame.data(plane);
        let mut chunks = data.chunks_exact(8);
        for chunk in &mut chunks {
            let word = u64::from_le_bytes(chunk.try_into().unwrap());
            hh = (hh ^ word).wrapping_mul(0x100000001b3);
        }
        for byte in chunks.remainder() {
            hh = (hh ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    Some(hh)
}

pub(super) fn dec_hash(
    pattern: &Option<String>,
    hash_mode: bool,
    frame: &decode::RenderFrame,
    frames: u64,
    pts: f64,
) {
    if pattern.is_some() || !hash_mode {
        return;
    }
    let mut sw = ffmpeg_the_third::frame::Video::empty();
    let rc = unsafe {
        ffmpeg_the_third::ffi::av_hwframe_transfer_data(sw.as_mut_ptr(), frame.as_ptr(), 0)
    };
    if rc < 0 {
        return;
    }
    let mut hh: u64 = 0xcbf29ce484222325;
    for byte in sw.data(0) {
        hh = (hh ^ *byte as u64).wrapping_mul(0x100000001b3);
    }
    tracing::info!("dechash {frames} {hh:016x} pts={pts:.4}");
}

pub(super) fn render_video(
    renderer: &mut vk::Renderer,
    rt: &vk::RenderTarget,
    export: &mut vk::ExportImage,
    frame: &decode::RenderFrame,
    uv: [f32; 4],
    fade: Option<&FadeState>,
    fade_mix: Option<f32>,
    si: usize,
    sw_a: Option<&FrameSlot>,
    sw_b: Option<&FrameSlot>,
    rgba_a: Option<&RgbaStillSource>,
    rgba_b: Option<&RgbaStillSource>,
) -> Result<()> {
    let fallback_a = src_of(frame, sw_a);
    let src_a = rgba_a.map_or(fallback_a, |still| vk::Src::Rgba(still.texture.view));
    let uv_a = rgba_a.map_or(uv, |still| still.uvs[si]);
    let (Some(fade), Some(t)) = (fade, fade_mix) else {
        return renderer.render_to(rt, export, &src_a, uv_a);
    };
    let fallback_b = src_of(&fade.cur.0, sw_b);
    let src_b = rgba_b.map_or(fallback_b, |still| vk::Src::Rgba(still.texture.view));
    let uv_b = rgba_b.map_or(fade.uvs[si], |still| still.uvs[si]);
    if t <= 0.0 {
        return renderer.render_to(rt, export, &src_a, uv_a);
    }
    if t >= 1.0 {
        return renderer.render_to(rt, export, &src_b, uv_b);
    }
    match (fade.style, fade.effect) {
        (Some(style), _) => {
            renderer.render_sand_to(rt, export, &src_a, uv_a, &src_b, uv_b, t, style)
        }
        (None, Some(fx)) => {
            renderer.render_effect_to(rt, export, &src_a, uv_a, &src_b, uv_b, t, fx)
        }
        (None, None) => {
            let mix = t * t * (3.0 - 2.0 * t);
            renderer.render_fade_to(rt, export, &src_a, uv_a, &src_b, uv_b, mix)
        }
    }
}

pub(super) fn render_pattern(
    renderer: &mut vk::Renderer,
    rt: &vk::RenderTarget,
    export: &mut vk::ExportImage,
    mode: &str,
    w: u32,
    h: u32,
    frames: u64,
) -> Result<()> {
    let bar = (frames as u32 * 12) % (w - 60);
    if mode == "gpu" {
        return renderer.render_pattern_to(rt, export, bar);
    }
    let bar = bar as usize;
    let ptr = export.mapped.unwrap();
    let stride = export.stride as usize;
    let offset = export.offset as usize;
    unsafe {
        for y in 0..h as usize {
            let row = ptr.add(offset + y * stride) as *mut u32;
            for x in 0..w as usize {
                let val = if x >= bar && x < bar + 60 { 0xFFFF_FFFF } else { 0xFF0D_0D0D };
                row.add(x).write(val);
            }
        }
    }
    Ok(())
}

pub(super) fn commit_at(
    s0: &mut wayland::SurfaceUi,
    now: u64,
    pts: f64,
    prev_pts: f64,
    speed: f64,
    base: &mut Option<(u64, f64)>,
    wall_anchor: &mut Option<(u64, f64)>,
    scheduled_q: &mut std::collections::VecDeque<u64>,
    flip_hist: &mut Vec<u64>,
    prev_commit_ns: u64,
    prev_step_ns: u64,
) -> u64 {
    let refresh = if s0.refresh_ns > 0 { s0.refresh_ns as u64 } else { 6_944_444 };
    absorb_feedback(s0, scheduled_q, flip_hist, base);
    if base.is_none() && s0.last_flip_ns != 0 {
        *base = Some((s0.last_flip_ns, pts));
    }
    let Some((bf, bp)) = base.as_mut() else {
        scheduled_q.push_back(0);
        if pts < prev_pts {
            *wall_anchor = Some(((prev_commit_ns + prev_step_ns).max(now), pts));
        }
        let (t0, pts0) = wall_anchor.get_or_insert((now, pts));
        let target = (*t0 as f64 + (pts - *pts0) / speed * 1e9) as i64;
        if pace_stalled(target, now, refresh) {
            *t0 = now;
            *pts0 = pts;
            return now;
        }
        return target.max(now as i64) as u64;
    };
    if pts < prev_pts {
        *bf = loop_frame_target(prev_commit_ns, prev_step_ns, now, refresh);
        *bp = pts;
    }
    let mut tf = frame_target(*bf, *bp, pts, speed, refresh);
    if pace_stalled(tf as i64, now, refresh) {
        *bf = now;
        *bp = pts;
        tf = now;
    }
    scheduled_q.push_back(tf);
    (tf.saturating_sub(refresh) + 1_500_000).max(now)
}

pub(super) fn absorb_feedback(
    s0: &mut wayland::SurfaceUi,
    scheduled_q: &mut std::collections::VecDeque<u64>,
    flip_hist: &mut Vec<u64>,
    base: &mut Option<(u64, f64)>,
) {
    for flip in s0.feedbacks.drain(..) {
        let sched = scheduled_q.pop_front();
        if flip == 0 {
            continue;
        }
        flip_hist.push(flip);
        if let Some(sched) = sched
            && sched != 0
        {
            let err = flip as i64 - sched as i64;
            let step = ((err as f64) * 0.08).clamp(-600_000.0, 600_000.0) as i64;
            if let Some((bf, _)) = base.as_mut() {
                *bf = (*bf as i64 + step).max(0) as u64;
            }
        }
    }
}

pub(super) fn report_flips(
    s0: &wayland::SurfaceUi,
    flip_hist: &mut Vec<u64>,
    frames: u64,
    report: &mut Instant,
) {
    let refresh = if s0.refresh_ns > 0 { s0.refresh_ns as u64 } else { 6_944_444 };
    let mut hist = std::collections::BTreeMap::new();
    for win in flip_hist.windows(2) {
        let slots = ((win[1] - win[0]) as f64 / refresh as f64).round() as i64;
        *hist.entry(slots).or_insert(0u32) += 1;
    }
    tracing::info!(
        "skwd-wall-vk: {frames} frames, {:.1} video fps, presented={} discarded={} flip-slots={:?}",
        240.0 / report.elapsed().as_secs_f64().max(0.001),
        s0.presented,
        s0.discarded,
        hist
    );
    flip_hist.clear();
    *report = Instant::now();
}
