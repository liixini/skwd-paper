use super::readiness::{PresentationReadiness, signal_ready};
use crate::fill::mode_uv;
use crate::timing::Pacer;
use crate::{ctl, decode, vk, wayland};
use anyhow::{Context, Result};
use std::time::{Duration, Instant};

pub(super) fn run_upload(
    target: &mut wayland::Target,
    video: &str,
    mute: bool,
    volume: u32,
) -> Result<()> {
    let mut dec = open_source(video).context("upload decoder")?;
    let (video_w, video_h) = dec.dims();
    tracing::info!("skwd-wall-vk: separate-device decode {video_w}x{video_h}");

    let dmabuf_path = std::env::var("SKWD_VK_PATH").as_deref() == Ok("dmabuf")
        && matches!(dec, decode::AnyDecoder::Vk(_));
    let (mut renderers, mut uvs, mut uploads) = build_render(target, &dec, dmabuf_path)?;
    let mut ctl = ctl::Ctl::start(video, mute, volume, true);
    let mut sw = ffmpeg_the_third::frame::Video::empty();
    let mut pacer = Pacer::new();
    let mut readiness = PresentationReadiness::startup();
    let mut last_presented = None;

    loop {
        target.pump()?;
        if target.app.closed || target.app.surfaces.iter().any(|surf| surf.closed) {
            break;
        }
        let was_paused = ctl.paused;
        if let Some(req) = ctl.poll()
            && swap_upload(
                &req.to,
                &mut dec,
                &mut renderers,
                &mut uvs,
                &mut uploads,
                &mut pacer,
                &mut ctl,
            )?
        {
            readiness.arm_swap();
        }
        crate::freeze::write_last_requested(&mut ctl, last_presented.as_ref())?;
        if ctl.paused && !ctl.freeze_pending() {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        if was_paused {
            pacer = Pacer::new();
        }
        let pts = decode_frame(
            &mut dec,
            &mut sw,
            &mut last_presented,
            uploads.as_deref(),
            &mut renderers,
            &uvs,
        )?;
        let now = Instant::now();
        let due = pacer.due(now, pts);
        if due > now {
            std::thread::sleep(due - now);
        }
        draw_uploads(uploads.as_deref(), &mut renderers, &uvs)?;
        if readiness.take_if_committed(true) {
            tracing::info!("skwd-wall-vk: upload-path source frame presented");
            signal_ready();
        }
    }
    Ok(())
}

type RenderSet = (Vec<vk::Renderer>, Vec<[f32; 4]>, Option<Vec<vk::UploadPath>>);

fn open_source(video: &str) -> Result<decode::AnyDecoder> {
    let dec = decode::open_decoder(video, None, true, None, false)?;
    if dec.still() {
        return Err(anyhow::anyhow!("still sources are rendered by skwd-wall-still"));
    }
    Ok(dec)
}

fn build_render(
    target: &wayland::Target,
    dec: &decode::AnyDecoder,
    dmabuf_path: bool,
) -> Result<RenderSet> {
    let (video_w, video_h) = dec.dims();
    let n_surf = if dmabuf_path { 1 } else { target.surface_count() };
    let mut renderers: Vec<vk::Renderer> = (0..n_surf)
        .map(|si| {
            let (w, h) = target.size_at(si);
            vk::Renderer::new(target.display_ptr(), target.surface_ptr_at(si), w, h)
        })
        .collect::<Result<_>>()
        .context("vulkan renderer")?;
    tracing::info!("skwd-wall-vk: {} renderer(s) up", renderers.len());

    let uvs: Vec<[f32; 4]> = renderers
        .iter()
        .map(|rend| mode_uv(video_w, video_h, rend.extent.width, rend.extent.height))
        .collect();
    let uploads: Option<Vec<vk::UploadPath>> = if dmabuf_path {
        None
    } else {
        Some(
            renderers
                .iter_mut()
                .map(|rend| rend.create_upload_path(video_w, video_h))
                .collect::<Result<_>>()?,
        )
    };
    tracing::info!(
        "skwd-wall-vk: path = {}",
        if dmabuf_path { "dmabuf (zero-copy)" } else { "transfer+upload" }
    );
    Ok((renderers, uvs, uploads))
}

fn swap_upload(
    to: &str,
    dec: &mut decode::AnyDecoder,
    renderers: &mut [vk::Renderer],
    uvs: &mut Vec<[f32; 4]>,
    uploads: &mut Option<Vec<vk::UploadPath>>,
    pacer: &mut Pacer,
    ctl: &mut ctl::Ctl,
) -> Result<bool> {
    let nd = match open_source(to) {
        Ok(nd) => nd,
        Err(err) => {
            tracing::info!("skwd-wall-vk: swap open {to} failed: {err:?}");
            return Ok(false);
        }
    };
    if uploads.is_none() && !matches!(nd, decode::AnyDecoder::Vk(_)) {
        tracing::info!("skwd-wall-vk: swap {to} needs an upload path, keeping the current source");
        return Ok(false);
    }
    *dec = nd;
    let (video_w, video_h) = dec.dims();
    *uvs = renderers
        .iter()
        .map(|rend| mode_uv(video_w, video_h, rend.extent.width, rend.extent.height))
        .collect();
    if let Some(old_ups) = uploads.take() {
        for (rend, up) in renderers.iter().zip(old_ups) {
            rend.destroy_upload_path(up);
        }
        *uploads = Some(
            renderers
                .iter_mut()
                .map(|rend| rend.create_upload_path(video_w, video_h))
                .collect::<Result<_>>()?,
        );
    }
    *pacer = Pacer::new();
    ctl.swap_audio(to, true);
    tracing::info!("skwd-wall-vk: swapped to {to} (cut, upload path)");
    Ok(true)
}

fn decode_frame(
    dec: &mut decode::AnyDecoder,
    sw: &mut ffmpeg_the_third::frame::Video,
    last_presented: &mut Option<ffmpeg_the_third::frame::Video>,
    uploads: Option<&[vk::UploadPath]>,
    renderers: &mut [vk::Renderer],
    uvs: &[[f32; 4]],
) -> Result<f64> {
    let Some(ups) = uploads else {
        let decode::AnyDecoder::Vk(dec) = dec else {
            return Err(anyhow::anyhow!("SKWD_VK_PATH=dmabuf requires Vulkan decode"));
        };
        let (mapped, pts) = dec.next_frame_retained(last_presented)?;
        let r0 = &mut renderers[0];
        let imgs = r0.import_nv12(dec.width, dec.height, &mapped.luma, &mapped.chroma)?;
        r0.draw(&imgs, uvs[0])?;
        r0.destroy_frame(imgs);
        drop(mapped);
        return Ok(pts);
    };
    let pts = dec.next_cpu(sw)?;
    *last_presented = Some(sw.clone());
    let (lp, cp) = (sw.stride(0), sw.stride(1));
    for (rend, up) in renderers.iter_mut().zip(ups) {
        rend.upload_nv12(up, sw.data(0), lp, sw.data(1), cp)?;
    }
    Ok(pts)
}

fn draw_uploads(
    uploads: Option<&[vk::UploadPath]>,
    renderers: &mut [vk::Renderer],
    uvs: &[[f32; 4]],
) -> Result<()> {
    let Some(ups) = uploads else {
        return Ok(());
    };
    for (si, (rend, up)) in renderers.iter_mut().zip(ups).enumerate() {
        rend.draw_views(up.luma_view, up.chroma_view, uvs[si], false)?;
    }
    Ok(())
}
