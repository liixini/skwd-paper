use super::{shared, shared_renderer, vk};
use crate::app::shared_support;
use crate::decode;
use anyhow::{Context, Result, anyhow, bail};
use ffmpeg_the_third as ff;
use paper_scene::model::{Video, VideoRange};
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::sync::Arc;

const DEFAULT_FRAME_STEP: f64 = 1.0 / 60.0;
const FRAME_EPSILON: f64 = 0.000_001;
const MAX_ADVANCE_FRAMES: usize = 240;

struct Playback {
    duration: f64,
    elapsed: f64,
    rate: f64,
    playing: bool,
    looping: bool,
    last_time: f64,
    ended: u64,
}

impl Playback {
    fn new(duration: f64) -> Self {
        Self {
            duration,
            elapsed: 0.0,
            rate: 1.0,
            playing: true,
            looping: true,
            last_time: 0.0,
            ended: 0,
        }
    }

    fn position(&self) -> f64 {
        if self.looping && self.duration > 0.0 {
            self.elapsed.rem_euclid(self.duration)
        } else {
            self.elapsed.max(0.0)
        }
    }

    fn advance(&mut self, time: f64) {
        let delta = (time - self.last_time).max(0.0);
        self.last_time = time;
        if !self.playing || self.rate == 0.0 {
            return;
        }
        let next = self.elapsed + delta * self.rate;
        if !next.is_finite() {
            self.playing = false;
            return;
        }
        if self.duration > 0.0 {
            if self.looping {
                let crossed = ((next / self.duration).floor()
                    - (self.elapsed / self.duration).floor())
                .abs() as u64;
                self.ended = self.ended.saturating_add(crossed);
            } else if next >= self.duration || next < 0.0 {
                self.elapsed = next.clamp(0.0, self.duration);
                self.playing = false;
                self.ended = self.ended.saturating_add(1);
                return;
            }
        }
        self.elapsed = next;
    }
}

const VIDEO_SUFFIX: &str = ".mp4";
const VIDEO_DEVICE_UNAVAILABLE: &str = "video hardware device unavailable";

pub(super) struct VideoDevice(shared::SharedDevice);

impl VideoDevice {
    pub(super) fn new(shared: &shared::SharedDevice) -> Result<Self> {
        let hwdev = unsafe { ff::ffi::av_buffer_ref(shared.hwdev) };
        anyhow::ensure!(!hwdev.is_null(), VIDEO_DEVICE_UNAVAILABLE);
        Ok(Self(shared::SharedDevice {
            entry: shared.entry.clone(),
            instance: shared.instance.clone(),
            phys: shared.phys,
            device: shared.device.clone(),
            gfx_family: shared.gfx_family,
            queue: shared.queue,
            hwdev,
            queue_sync: shared.queue_sync,
            software: shared.software,
            foreign_queue: shared.foreign_queue,
            video_decode: shared.video_decode,
            render_node: shared.render_node.clone(),
            decode_fingerprint: shared.decode_fingerprint.clone(),
            decode_device: shared.decode_device.clone(),
        }))
    }

    pub(super) fn shared(&self) -> &shared::SharedDevice {
        &self.0
    }
}

impl Drop for VideoDevice {
    fn drop(&mut self) {
        unsafe { ff::ffi::av_buffer_unref(&raw mut self.0.hwdev) };
    }
}

struct Timeline {
    pts: f64,
    due: f64,
    step: f64,
}

impl Timeline {
    fn new(pts: f64) -> Self {
        Self { pts, due: 0.0, step: DEFAULT_FRAME_STEP }
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
    playback: Playback,
    origin: f64,
    ahead: ahead::Ahead,
    pending: Option<(decode::RenderFrame, f64)>,
    timeline: Timeline,
    renderer: vk::Renderer,
    target: Option<vk::SceneTarget>,
    slot: Option<shared_support::FrameSlot>,
    shown: Option<decode::RenderFrame>,
    generation: u64,
    _source: Source,
}

enum Source {
    Scratch(tempfile::NamedTempFile),
    #[cfg(target_os = "linux")]
    Package(Arc<std::fs::File>),
}

pub(super) struct Package(Arc<std::fs::File>);

impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Package {}

impl std::hash::Hash for Package {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

#[derive(PartialEq, Eq, Hash)]
pub(super) enum VideoKey {
    Bytes(Arc<[u8]>),
    Range(Package, decode::Clip),
}

impl VideoKey {
    pub(super) fn of(video: &Video) -> Self {
        match video {
            Video::Bytes(payload) => Self::Bytes(payload.clone()),
            Video::Range(range) => Self::Range(Package(range.file.clone()), clip(range)),
        }
    }
}

fn clip(range: &VideoRange) -> decode::Clip {
    decode::Clip { offset: range.offset, length: range.length }
}

fn scratch(sd: &shared::SharedDevice, payload: &[u8]) -> Result<(decode::AnyDecoder, Source)> {
    let mut file = paper_runtime::scratch::file(paper_runtime::scratch::VIDEO, VIDEO_SUFFIX)?;
    file.write_all(payload).context("write embedded scene video")?;
    let path = file.path().to_str().context("scene video path")?;
    #[cfg(not(target_os = "linux"))]
    let decoder = sd.open_decoder(path)?;
    #[cfg(target_os = "linux")]
    let decoder = crate::health::open(
        sd,
        &crate::health::Media::Scratch { path, payload },
        shared::software_decode_required(sd.software, sd.queue_sync),
    )?;
    Ok((decoder, Source::Scratch(file)))
}

fn package(sd: &shared::SharedDevice, range: &VideoRange) -> Result<(decode::AnyDecoder, Source)> {
    #[cfg(target_os = "linux")]
    {
        let force_software = shared::software_decode_required(sd.software, sd.queue_sync);
        let logical = format!("{}{VIDEO_SUFFIX}", paper_runtime::scratch::VIDEO);
        let media = crate::health::Media::Package {
            file: &range.file,
            logical: &logical,
            clip: clip(range),
        };
        let error = match crate::health::open(sd, &media, force_software) {
            Ok(decoder) => return Ok((decoder, Source::Package(range.file.clone()))),
            Err(error) => error,
        };
        let VideoRange { offset, length, .. } = range;
        tracing::info!("skwd-wall-vk: video range {offset}+{length} unreadable ({error:#})");
    }
    scratch(sd, &buffer(range)?)
}

fn buffer(range: &VideoRange) -> Result<Vec<u8>> {
    let length = usize::try_from(range.length)?;
    if length > paper_scene::tex::MAX_MIP_BYTES {
        bail!("scene video of {length} bytes exceeds the buffered limit");
    }
    let mut payload = vec![0; length];
    range.file.read_exact_at(&mut payload, range.offset).context("read scene video range")?;
    Ok(payload)
}

impl VideoTexture {
    pub(super) fn open(
        sd: &shared::SharedDevice,
        video: &Video,
        clamp: bool,
        nearest: bool,
    ) -> Result<Self> {
        let (decoder, source) = match video {
            Video::Bytes(payload) => scratch(sd, payload),
            Video::Range(range) => package(sd, range),
        }
        .context("open embedded scene video")?;
        let (width, height) = decoder.dims();
        paper_scene::tex::PixelFormat::Rgba8
            .level_bytes(width, height)
            .ok_or_else(|| anyhow!("invalid scene video dimensions {width}x{height}"))?;
        let duration = decoder.duration();
        let mut ahead = ahead::Ahead::start(decoder)?;
        let (frame, pts) = ahead.next().context("first embedded scene video frame")?;
        let renderer = shared_renderer(sd, width, height)?;
        let mut video = Self {
            playback: Playback::new(duration),
            origin: pts,
            ahead,
            pending: None,
            timeline: Timeline::new(pts),
            renderer,
            target: None,
            slot: None,
            shown: None,
            generation: 0,
            _source: source,
        };
        video.target =
            Some(video.renderer.create_video_texture_target(width, height, clamp, nearest)?);
        video.render(frame)?;
        tracing::info!(width, height, "skwd-wall-vk: embedded scene video playing");
        Ok(video)
    }

    pub(super) fn target(&self) -> &vk::SceneTarget {
        self.target.as_ref().expect("initialized scene video target")
    }

    pub(super) fn view_slot(&self, renderer: &mut vk::Renderer) -> Result<vk::SceneTexture> {
        let target = self.target();
        let mut slot = renderer.create_view_slot()?;
        slot.extent = target.extent;
        slot.view = target.view;
        slot.sampler = target.sampler;
        renderer.point_slot_at(&slot, target.view);
        Ok(slot)
    }

    fn render(&mut self, frame: decode::RenderFrame) -> Result<()> {
        self.renderer.wait_render()?;
        if shared_support::needs_frame_slot(&frame) {
            self.generation += 1;
            shared_support::ensure_frame_slot(
                &mut self.slot,
                &self.renderer,
                &frame,
                self.generation as f64,
            )?;
            if frame.mapped_frame().is_some()
                && !self.slot.as_ref().is_some_and(shared_support::FrameSlot::imported)
            {
                self.ahead.transfer_frames();
            }
        }
        let rendered = self.renderer.render_video_texture(
            self.target.as_ref().expect("initialized scene video target"),
            &shared_support::src_of(&frame, self.slot.as_ref()),
        );
        self.shown = Some(frame);
        rendered
    }

    pub(super) fn playing(&self) -> bool {
        self.playback.playing && self.playback.rate != 0.0
    }

    pub(super) fn start_at(&mut self, time: f64) {
        self.playback.last_time = time;
    }

    pub(super) fn state(&self, object: usize) -> paper_scene::script::VideoState {
        paper_scene::script::VideoState {
            object,
            duration: self.playback.duration,
            position: self.playback.position(),
            rate: self.playback.rate,
            playing: self.playback.playing,
            looping: self.playback.looping,
            ended: self.playback.ended,
        }
    }

    pub(super) fn command(&mut self, op: paper_scene::script::VideoOp) -> Result<()> {
        use paper_scene::script::VideoOp;
        match op {
            VideoOp::Play => self.playback.playing = true,
            VideoOp::Pause => self.playback.playing = false,
            VideoOp::Stop => {
                self.playback.playing = false;
                self.seek(0.0)?;
            }
            VideoOp::Seek(position) => self.seek(position)?,
            VideoOp::Rate(rate) => self.playback.rate = rate,
            VideoOp::Loop(looping) => self.playback.looping = looping,
        }
        Ok(())
    }

    fn seek(&mut self, position: f64) -> Result<()> {
        let duration = self.playback.duration;
        let position =
            if duration > 0.0 { position.clamp(0.0, duration) } else { position.max(0.0) };
        let mut decoder = self.ahead.stop()?;
        let sought = decoder.seek(position + self.origin);
        self.ahead = ahead::Ahead::start(decoder)?;
        sought?;
        let (frame, pts) = self.ahead.next()?;
        self.timeline.pts = pts;
        self.timeline.due = pts - self.origin;
        self.pending = None;
        self.playback.elapsed = position;
        self.render(frame)?;
        self.decode_until(position)
    }

    fn decode_until(&mut self, time: f64) -> Result<()> {
        let mut selected = None;
        for _ in 0..MAX_ADVANCE_FRAMES {
            if self.pending.is_none() {
                let (frame, pts) = self.ahead.next().context("advance embedded scene video")?;
                if !self.playback.looping && pts < self.timeline.pts {
                    break;
                }
                let due = self.timeline.next(pts);
                self.pending = Some((frame, due));
            }
            if self.pending.as_ref().is_some_and(|(_, due)| *due > time + FRAME_EPSILON) {
                break;
            }
            selected = self.pending.take().map(|(frame, _)| frame);
        }
        if let Some(frame) = selected {
            self.render(frame)?;
        }
        Ok(())
    }

    pub(super) fn advance(&mut self, time: f64) -> Result<()> {
        let before = self.playback.elapsed;
        self.playback.advance(time);
        let next = self.playback.elapsed;
        if next == before {
            return Ok(());
        }
        if next < before
            || next - self.timeline.due > self.timeline.step * MAX_ADVANCE_FRAMES as f64
        {
            return self.seek(self.playback.position());
        }
        self.decode_until(next)
    }
}

impl Drop for VideoTexture {
    fn drop(&mut self) {
        let _ = self.renderer.wait_render();
        self.pending = None;
        if let Some(slot) = self.slot.take() {
            slot.destroy(&self.renderer);
        }
        self.shown = None;
        if let Some(target) = self.target.take() {
            self.renderer.destroy_scene_target(target);
        }
    }
}

mod ahead;
#[cfg(test)]
mod tests;
