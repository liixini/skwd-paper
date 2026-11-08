use super::decode::decode_image;
use crate::fill_mode::{FillMode, apply_fill_mode};
use anyhow::{Context, Result};
use paper_control::{Depth, DrawingManifest, StillCommand, WidgetScene};
use paper_runtime::minute::MinuteTimer;
use paper_widgets::{Frame, Lettering, Moment, Outline, Place, Scene, Tones, ground, place};
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;

const FRAME_MAGIC: &[u8; 4] = b"SKWP";
const CHANNELS: usize = 4;
const ALPHA: usize = CHANNELS - 1;
const INPUT_CHUNK: usize = 4096;
const INPUT_LIMIT: usize = 1024 * 1024;

pub struct StreamSpec {
    pub path: String,
    pub outputs: Vec<StreamOutput>,
    pub fill: FillMode,
    pub blur: f32,
    pub dim: u32,
    pub header: bool,
}

pub struct StreamOutput {
    pub size: [u32; 2],
    pub logical: [u32; 2],
    pub name: String,
    pub fd: Option<i32>,
}

struct Output {
    spec: StreamOutput,
    base: Vec<u8>,
    sink: Box<dyn Write>,
}

struct WidgetLayer {
    face: paper_control::Face,
    place: Place,
    tones: Tones,
    cut: Option<Vec<u8>>,
}

struct ActiveScene {
    scene: WidgetScene,
    lettering: Lettering,
    layers: Vec<Vec<WidgetLayer>>,
    timer: Option<MinuteTimer>,
}

#[expect(
    clippy::too_many_lines,
    reason = "Owns the still frame stream protocol and its event loop"
)]
pub fn stream(spec: StreamSpec) -> Result<()> {
    let (source_width, source_height, source) = decode_image(&spec.path, spec.blur, spec.dim)?;
    let mut outputs = Vec::with_capacity(spec.outputs.len());
    for output in spec.outputs {
        let (_, _, base) = apply_fill_mode(
            source_width,
            source_height,
            &source,
            output.size[0],
            output.size[1],
            spec.fill,
        );
        let sink: Box<dyn Write> = match output.fd {
            Some(fd) if fd >= 0 => Box::new(paper_runtime::plasma::frame_pipe(fd)?),
            _ => Box::new(std::io::stdout()),
        };
        outputs.push(Output { spec: output, base, sink });
    }
    drop(source);
    for output in &mut outputs {
        write_frame(
            &mut *output.sink,
            output.spec.size[0],
            output.spec.size[1],
            &output.base,
            spec.header,
        )?;
    }
    paper_runtime::plasma::frame_ready()?;
    let mut active: Option<ActiveScene> = None;
    let mut pending = Vec::new();
    let mut stdin = std::fs::File::from(std::io::stdin().as_fd().try_clone_to_owned()?);
    loop {
        let mut poll = [
            libc::pollfd { fd: stdin.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd {
                fd: active
                    .as_ref()
                    .and_then(|scene| scene.timer.as_ref())
                    .map_or(-1, MinuteTimer::raw),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        if unsafe { libc::poll(poll.as_mut_ptr(), poll.len() as libc::nfds_t, -1) } < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if poll[0].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            let mut chunk = [0; INPUT_CHUNK];
            let count = stdin.read(&mut chunk)?;
            if count == 0 {
                return Ok(());
            }
            pending.extend_from_slice(&chunk[..count]);
            while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                if end > INPUT_LIMIT {
                    pending.drain(..=end);
                    tracing::warn!("widgets: input line too large");
                    continue;
                }
                let line: Vec<_> = pending.drain(..=end).collect();
                let Ok(command) = serde_json::from_slice::<StillCommand>(&line) else {
                    tracing::warn!("widgets: invalid stream command");
                    continue;
                };
                let Some(scene) = command.widgets else { continue };
                if active.as_ref().is_some_and(|current| current.scene == scene) {
                    continue;
                }
                match ActiveScene::new(
                    scene,
                    &outputs,
                    &spec.path,
                    [source_width, source_height],
                    spec.fill,
                ) {
                    Ok(next) => {
                        active = next;
                        repaint(&mut outputs, active.as_ref(), false)?;
                    }
                    Err(error) => tracing::warn!(%error, "widgets: scene rejected"),
                }
            }
            if pending.len() > INPUT_LIMIT {
                pending.clear();
                tracing::warn!("widgets: input line too large");
            }
        }
        if poll[1].revents & libc::POLLIN != 0
            && let Some(scene) = active.as_ref()
            && let Some(timer) = scene.timer.as_ref()
            && timer.fire()?
        {
            paper_runtime::clock::reload_zone();
            repaint(&mut outputs, active.as_ref(), true)?;
            scene.lettering.sweep();
        }
    }
}

impl ActiveScene {
    fn new(
        scene: WidgetScene,
        outputs: &[Output],
        path: &str,
        source: [u32; 2],
        fill: FillMode,
    ) -> Result<Option<Self>> {
        scene.validate().context("widget scene")?;
        if scene.widgets.is_empty() {
            return Ok(None);
        }
        let font = match &scene.font {
            Some(path) => paper_widgets::read_font(path)
                .with_context(|| format!("widget font {}", path.display()))?,
            None => paper_widgets::FONT.to_vec(),
        };
        let lettering = Lettering::open(font).context("widget font is not usable")?;
        let outline = scene
            .outlines
            .iter()
            .find(|manifest| DrawingManifest::describes(manifest, Path::new(path)))
            .and_then(|manifest| Outline::load(manifest, Path::new(path)).ok());
        let mut layers = Vec::with_capacity(outputs.len());
        for output in outputs {
            let mut visible = Vec::new();
            for widget in &scene.widgets {
                if !widget.shown_on(&output.spec.name) {
                    continue;
                }
                let Some(place) = place(
                    output.spec.logical,
                    output.spec.size,
                    paper_widgets::frame(widget.face),
                    widget.pin,
                    widget.height,
                ) else {
                    continue;
                };
                let area = [
                    place.origin[0] as usize,
                    place.origin[1] as usize,
                    place.size[0],
                    place.size[1],
                ];
                let tones = Tones::over(
                    &scene.palette,
                    ground(&output.base, output.spec.size[0] as usize, area),
                );
                let cut = if widget.depth == Depth::Behind && fill != FillMode::Span {
                    outline.as_ref().and_then(|outline| {
                        outline.crop(
                            Frame { image: source, buffer: output.spec.size, mode: fill },
                            place.origin,
                            place.size,
                        )
                    })
                } else {
                    None
                };
                visible.push(WidgetLayer { face: widget.face, place, tones, cut });
            }
            layers.push(visible);
        }
        let timer =
            layers.iter().any(|layers| !layers.is_empty()).then(MinuteTimer::new).transpose()?;
        Ok(Some(Self { scene, lettering, layers, timer }))
    }
}

fn repaint(outputs: &mut [Output], active: Option<&ActiveScene>, clock_only: bool) -> Result<()> {
    let moment = paper_runtime::clock::local_now()
        .map(|time| Moment::civil(time.hour, time.minute, time.weekday, time.day, time.month));
    for (index, output) in outputs.iter_mut().enumerate() {
        if clock_only && active.is_none_or(|scene| scene.layers[index].is_empty()) {
            continue;
        }
        let mut frame = output.base.clone();
        if let (Some(active), Some(moment)) = (active, moment) {
            for layer in &active.layers[index] {
                let scene = Scene {
                    lettering: &active.lettering,
                    tones: layer.tones,
                    moment,
                    labels: &active.scene.labels,
                };
                let canvas = scene.paint(layer.face, layer.place, layer.cut.as_deref());
                blend(&mut frame, output.spec.size[0] as usize, layer.place.origin, &canvas);
            }
        }
        write_frame(&mut *output.sink, output.spec.size[0], output.spec.size[1], &frame, false)?;
    }
    Ok(())
}

fn blend(frame: &mut [u8], width: usize, origin: [u32; 2], canvas: &paper_widgets::Canvas) {
    let height = frame.len() / CHANNELS / width;
    let rows = canvas.height.min(height.saturating_sub(origin[1] as usize));
    let columns = canvas.width.min(width.saturating_sub(origin[0] as usize));
    for row in 0..rows {
        for column in 0..columns {
            let pixel = canvas.pixels[row * canvas.width + column];
            let at = ((origin[1] as usize + row) * width + origin[0] as usize + column) * CHANNELS;
            let keep = u16::from(u8::MAX - pixel[ALPHA]);
            for channel in 0..ALPHA {
                frame[at + channel] = (u16::from(pixel[channel])
                    + u16::from(frame[at + channel]) * keep / u16::from(u8::MAX))
                .min(u16::from(u8::MAX)) as u8;
            }
            frame[at + ALPHA] = (u16::from(pixel[ALPHA])
                + u16::from(frame[at + ALPHA]) * keep / u16::from(u8::MAX))
            .min(u16::from(u8::MAX)) as u8;
        }
    }
}

pub(super) fn write_frame(
    sink: &mut dyn Write,
    width: u32,
    height: u32,
    frame: &[u8],
    header: bool,
) -> Result<()> {
    let mut output = std::io::BufWriter::new(sink);
    if header {
        output.write_all(FRAME_MAGIC)?;
        output.write_all(&width.to_le_bytes())?;
        output.write_all(&height.to_le_bytes())?;
    }
    output.write_all(frame)?;
    output.flush()?;
    Ok(())
}

#[cfg(test)]
#[path = "stream/tests.rs"]
mod tests;
