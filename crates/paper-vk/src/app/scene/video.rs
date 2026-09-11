use super::{shared, shared_renderer, vk};
use crate::decode;
use anyhow::{Context, Result, anyhow};
use ffmpeg_the_third as ff;
use std::io::Write;

struct Timeline {
    pts: f64,
    due: f64,
    step: f64,
}

impl Timeline {
    fn new(pts: f64) -> Self {
        Self { pts, due: 0.0, step: 1.0 / 60.0 }
    }

    fn next(&mut self, pts: f64) -> f64 {
        let delta = pts - self.pts;
        if delta.is_finite() && delta > 0.0 {
            self.step = delta;
        }
        self.due += self.step;
        self.pts = pts;
        self.due
    }
}

pub(super) struct VideoTexture {
    decoder: decode::AnyDecoder,
    pending: Option<(ff::frame::Video, f64)>,
    timeline: Timeline,
    renderer: vk::Renderer,
    target: Option<vk::SceneTarget>,
    upload: Option<vk::UploadPath>,
    _file: tempfile::NamedTempFile,
}

impl VideoTexture {
    pub(super) fn open(
        sd: &shared::SharedDevice,
        payload: &[u8],
        clamp: bool,
        nearest: bool,
    ) -> Result<Self> {
        let mut file =
            tempfile::Builder::new().prefix("skwd-scene-video-").suffix(".mp4").tempfile()?;
        file.write_all(payload).context("write embedded scene video")?;
        let path = file.path().to_str().context("scene video path")?;
        let force_software = shared::software_decode_required(sd.software, sd.queue_sync);
        let mut decoder = decode::open_decoder(
            path,
            Some(sd.hwdev),
            sd.video_decode,
            sd.render_node.as_deref(),
            force_software,
        )
        .context("open embedded scene video")?;
        let (width, height) = decoder.dims();
        paper_scene::tex::PixelFormat::Rgba8
            .level_bytes(width, height)
            .ok_or_else(|| anyhow!("invalid scene video dimensions {width}x{height}"))?;
        let (frame, pts) = decoder.next().context("first embedded scene video frame")?;
        let renderer = shared_renderer(sd, width, height)?;
        let mut video = Self {
            decoder,
            pending: None,
            timeline: Timeline::new(pts),
            renderer,
            target: None,
            upload: None,
            _file: file,
        };
        video.target =
            Some(video.renderer.create_video_texture_target(width, height, clamp, nearest)?);
        if !matches!(video.decoder, decode::AnyDecoder::Vk(_)) {
            video.upload = Some(video.renderer.create_upload_path(width, height)?);
        }
        video.render(&frame)?;
        tracing::info!(width, height, "skwd-wall-vk: embedded scene video playing");
        Ok(video)
    }

    pub(super) fn target(&self) -> &vk::SceneTarget {
        self.target.as_ref().expect("initialized scene video target")
    }

    fn render(&mut self, frame: &ff::frame::Video) -> Result<()> {
        let source = if let Some(upload) = &self.upload {
            self.renderer.upload_nv12(
                upload,
                frame.data(0),
                frame.stride(0),
                frame.data(1),
                frame.stride(1),
            )?;
            vk::Src::Views(upload.luma_view, upload.chroma_view)
        } else {
            vk::Src::Avvk(frame)
        };
        self.renderer.render_video_texture(
            self.target.as_ref().expect("initialized scene video target"),
            &source,
        )?;
        self.renderer.wait_render()
    }

    pub(super) fn advance(&mut self, time: f64) -> Result<()> {
        let mut selected = None;
        for _ in 0..240 {
            if self.pending.is_none() {
                let (frame, pts) = self.decoder.next().context("advance embedded scene video")?;
                let due = self.timeline.next(pts);
                self.pending = Some((frame, due));
            }
            if self.pending.as_ref().is_some_and(|(_, due)| *due > time + 0.000001) {
                break;
            }
            selected = self.pending.take().map(|(frame, _)| frame);
        }
        if let Some(frame) = selected {
            self.render(&frame)?;
        }
        Ok(())
    }
}

impl Drop for VideoTexture {
    fn drop(&mut self) {
        let _ = self.renderer.wait_render();
        self.pending = None;
        if let Some(upload) = self.upload.take() {
            self.renderer.destroy_upload_path(upload);
        }
        if let Some(target) = self.target.take() {
            self.renderer.destroy_scene_target(target);
        }
    }
}

#[cfg(test)]
mod tests;
