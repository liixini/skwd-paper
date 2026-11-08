use anyhow::{Context, Result};
use paper_control::WidgetScene;
use paper_runtime::minute::MinuteTimer;
use paper_widgets::{Lettering, Moment, Scene, Tones};

mod snapshot;

use snapshot::{Snapshot, Snapshots};

use super::StreamTarget;

const OPAQUE_TINT: [f32; 4] = [1.0; 4];
use crate::vk::{Renderer, SceneBlend, SceneQuad};

#[derive(Default)]
pub(crate) struct Widgets {
    scene: Option<WidgetScene>,
    snapshots: Option<Snapshots>,
    lettering: Option<Lettering>,
    timer: Option<MinuteTimer>,
    pub(crate) pending: bool,
}

impl Widgets {
    pub(crate) fn stream(socket: std::os::fd::RawFd) -> Self {
        let capable =
            snapshot::enabled(socket, std::env::var(snapshot::CAPABILITY).ok().as_deref());
        Self { snapshots: capable.then(Snapshots::default), ..Self::default() }
    }

    pub(crate) fn publish(&mut self, target: &StreamTarget, slot: usize) -> Result<()> {
        if let Some(snapshots) = &mut self.snapshots {
            snapshots.publish(target, slot)?;
        }
        Ok(())
    }

    pub(crate) fn wake_fd(&self) -> Option<i32> {
        self.timer.as_ref().map(MinuteTimer::raw)
    }

    #[expect(clippy::too_many_lines, reason = "stream widget scene transition")]
    pub(crate) fn sync(
        &mut self,
        incoming: Option<&WidgetScene>,
        target: &StreamTarget,
        renderer: &mut Renderer,
    ) -> Result<()> {
        let changed = incoming.is_some_and(|scene| self.scene.as_ref() != Some(scene));
        let tick = self.timer.as_ref().map(MinuteTimer::fire).transpose()?.unwrap_or(false);
        if !changed && !tick {
            return Ok(());
        }
        if let Some(scene) = incoming.filter(|_| changed) {
            scene.validate().context("stream widget scene")?;
            let visible = scene.widgets.iter().any(|widget| widget.shown_on(&target.connector));
            self.lettering = if visible {
                let font = scene.font.as_ref().map_or_else(
                    || Ok(paper_widgets::FONT.to_vec()),
                    |path| paper_widgets::read_font(path),
                )?;
                Some(Lettering::open(font).context("stream widget font")?)
            } else {
                None
            };
            self.timer = visible.then(MinuteTimer::new).transpose()?;
            self.scene = Some(scene.clone());
        }
        if tick {
            paper_runtime::clock::reload_zone();
        }
        let mut textures = Vec::new();
        let mut quads = Vec::new();
        if let (Some(scene), Some(lettering)) = (&self.scene, &self.lettering) {
            let time = paper_runtime::clock::local_now().context("stream widget clock")?;
            let paint = Scene {
                lettering,
                tones: Tones::over(&scene.palette, paper_widgets::luminance(scene.palette.shade)),
                moment: Moment::civil(time.hour, time.minute, time.weekday, time.day, time.month),
                labels: &scene.labels,
            };
            let placed: Vec<_> = scene
                .widgets
                .iter()
                .filter(|widget| widget.shown_on(&target.connector))
                .filter_map(|widget| {
                    paper_widgets::place(
                        target.logical,
                        [target.width, target.height],
                        paper_widgets::frame(widget.face),
                        widget.pin,
                        widget.height,
                    )
                    .map(|place| (widget.face, place))
                })
                .collect();
            if let Some(snapshots) = self.snapshots.as_mut().filter(|snapshots| !snapshots.fallback)
            {
                match Snapshot::paint(&paint, &placed, [target.width, target.height]) {
                    Ok(snapshot) => {
                        snapshots.replace(snapshot);
                        lettering.sweep();
                        self.pending = true;
                        return Ok(());
                    }
                    Err(error) => {
                        tracing::warn!(%error, "stream widgets: gpu fallback");
                        snapshots.fallback = true;
                    }
                }
            }
            for (face, place) in placed {
                let canvas = paint.paint(face, place, None);
                let mut texture = renderer.create_scene_texture(
                    canvas.width as u32,
                    canvas.height as u32,
                    canvas.pixels.as_flattened(),
                )?;
                texture.premultiplied = true;
                quads.push(SceneQuad {
                    rect: [
                        place.origin[0] as f32 + canvas.width as f32 / 2.0,
                        place.origin[1] as f32 + canvas.height as f32 / 2.0,
                        canvas.width as f32,
                        canvas.height as f32,
                    ],
                    uv: [0.0, 0.0, 1.0, 1.0],
                    uv_axes: None,
                    uv_padding: false,
                    tint: OPAQUE_TINT,
                    angle: 0.0,
                    projection: None,
                    order_bias: 0,
                    texture: textures.len(),
                    blend: SceneBlend::Alpha,
                });
                textures.push(texture);
            }
            lettering.sweep();
        }
        if let Some(snapshots) = &mut self.snapshots {
            snapshots.replace(None);
            if !snapshots.fallback {
                self.pending = true;
                return Ok(());
            }
        }
        renderer.set_overlay(textures, quads)?;
        self.pending = true;
        Ok(())
    }
}

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod tests;
