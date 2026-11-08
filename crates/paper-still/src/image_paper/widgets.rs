use super::buffer_set::BufferSet;
use super::model::App;
use super::shm_pixels::pack_pixels;
use crate::fill_mode::FillMode;
use anyhow::{Context, Result};
use paper_control::{Depth, DrawingManifest, WidgetScene};
use paper_runtime::minute::MinuteTimer;
use paper_widgets::{Frame, Lettering, Moment, Outline, Place, Scene, Tones, place};
use std::cell::OnceCell;
use std::path::Path;
use std::time::SystemTime;
use wayland_client::protocol::{wl_subsurface::WlSubsurface, wl_surface::WlSurface};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

pub(super) struct Widgets {
    scene: WidgetScene,
    lettering: Lettering,
    timer: MinuteTimer,
    panels: Vec<Panel>,
}

struct Panel {
    parent: WlSurface,
    widget: usize,
    surface: WlSurface,
    subsurface: WlSubsurface,
    viewport: WpViewport,
    buffer: Option<BufferSet>,
    seen: Option<View>,
    look: Option<Look>,
    cut: Option<(CutKey, Option<Vec<u8>>)>,
    placed_at: Option<[i32; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Hidden,
    Shown(u64),
}

#[derive(Clone, Debug, PartialEq)]
struct CutKey {
    path: String,
    stamp: Option<(u64, SystemTime)>,
    frame: Frame,
    origin: [u32; 2],
    size: [usize; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Same,
    Repaint,
    Recut,
    Panels,
    Rebuild,
}

impl Change {
    fn between(old: &WidgetScene, new: &WidgetScene) -> Self {
        if old == new {
            Self::Same
        } else if old.font != new.font {
            Self::Rebuild
        } else if old.widgets != new.widgets {
            Self::Panels
        } else if old.outlines != new.outlines {
            Self::Recut
        } else {
            Self::Repaint
        }
    }
}

struct Look {
    tones: Tones,
    place: Place,
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.viewport.destroy();
        self.subsurface.destroy();
        self.surface.destroy();
    }
}

impl Widgets {
    pub(super) fn timer_fd(&self) -> i32 {
        self.timer.raw()
    }

    fn retune(&mut self, scene: WidgetScene, change: Change) {
        if change == Change::Panels {
            self.panels.clear();
        }
        for panel in &mut self.panels {
            panel.seen = None;
            if change == Change::Recut {
                panel.cut = None;
            }
        }
        self.scene = scene;
    }
}

impl App {
    pub(super) fn set_widgets(&mut self, scene: WidgetScene) {
        let change = self
            .widgets
            .as_ref()
            .map_or(Change::Rebuild, |current| Change::between(&current.scene, &scene));
        if change == Change::Same {
            return;
        }
        if scene.widgets.is_empty() {
            self.widgets = None;
            return;
        }
        if self.subcompositor.is_none() {
            tracing::warn!("widgets: compositor has no subsurfaces");
            return;
        }
        if change != Change::Rebuild
            && scene.validate().is_ok()
            && let Some(current) = self.widgets.as_mut()
        {
            current.retune(scene, change);
            return;
        }
        match build(scene) {
            Ok(widgets) => {
                tracing::info!(count = widgets.scene.widgets.len(), "widgets: scene set");
                self.widgets = Some(widgets);
            }
            Err(error) => tracing::warn!(error = %format!("{error:#}"), "widgets: scene rejected"),
        }
    }

    pub(super) fn widget_tick(&mut self) {
        let Some(widgets) = self.widgets.as_ref() else { return };
        match widgets.timer.fire() {
            Ok(true) => {
                paper_runtime::clock::reload_zone();
                self.repaint_widgets(|_| true);
                if let Some(widgets) = &self.widgets {
                    widgets.lettering.sweep();
                }
            }
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, "widgets: timer failed"),
        }
    }

    pub(super) fn follow_widgets(&mut self) {
        let Some(mut widgets) = self.widgets.take() else { return };
        widgets
            .panels
            .retain(|panel| self.surfaces.iter().any(|surf| surf.surface == panel.parent));
        for surf in &self.surfaces {
            for (widget, placed) in widgets.scene.widgets.iter().enumerate() {
                if !placed.shown_on(&surf.output_name) {
                    continue;
                }
                let exists = widgets
                    .panels
                    .iter()
                    .any(|panel| panel.parent == surf.surface && panel.widget == widget);
                if !exists && let Some(panel) = self.panel(&surf.surface, widget) {
                    widgets.panels.push(panel);
                }
            }
        }
        let mut dirty = vec![false; widgets.panels.len()];
        let outline = OnceCell::new();
        for (index, flag) in dirty.iter_mut().enumerate() {
            let state = self.surface_state(&widgets.panels[index].parent);
            if widgets.panels[index].seen == Some(state) {
                continue;
            }
            widgets.panels[index].seen = Some(state);
            *flag = true;
            if let Some(look) = self.look(&widgets, index) {
                self.refresh_cut(&mut widgets, index, &look.place, &outline);
                widgets.panels[index].look = Some(look);
                continue;
            }
            let panel = &mut widgets.panels[index];
            panel.look = None;
            panel.surface.attach(None, 0, 0);
            panel.surface.commit();
            if let Some(buffer) = panel.buffer.take() {
                self.retired.push(buffer);
            }
        }
        self.widgets = Some(widgets);
        if dirty.contains(&true) {
            self.repaint_widgets(|index| dirty[index]);
        }
    }

    fn surface_state(&self, parent: &WlSurface) -> View {
        self.surfaces
            .iter()
            .find(|surf| &surf.surface == parent)
            .filter(|surf| self.slide.is_none() && surf.shown.is_some())
            .map_or(View::Hidden, |surf| View::Shown(surf.shown_at))
    }

    fn repaint_widgets(&mut self, wanted: impl Fn(usize) -> bool) {
        let Some(mut widgets) = self.widgets.take() else { return };
        if let Some(moment) = paper_runtime::clock::local_now().map(moment) {
            for index in 0..widgets.panels.len() {
                if wanted(index) {
                    self.paint(&mut widgets, index, moment);
                }
            }
        }
        self.widgets = Some(widgets);
    }

    fn panel(&self, parent: &WlSurface, widget: usize) -> Option<Panel> {
        let subcompositor = self.subcompositor.as_ref()?;
        let (subsurface, surface) = subcompositor.create_subsurface(parent.clone(), &self.qh);
        subsurface.set_desync();
        let region = self.compositor_state.wl_compositor().create_region(&self.qh, ());
        surface.set_input_region(Some(&region));
        region.destroy();
        let viewport = self.viewporter.get_viewport(&surface, &self.qh, ());
        Some(Panel {
            parent: parent.clone(),
            widget,
            surface,
            subsurface,
            viewport,
            buffer: None,
            seen: None,
            look: None,
            cut: None,
            placed_at: None,
        })
    }

    fn look(&self, widgets: &Widgets, index: usize) -> Option<Look> {
        let panel = &widgets.panels[index];
        if self.surface_state(&panel.parent) == View::Hidden {
            return None;
        }
        let surf = self.surfaces.iter().find(|surf| surf.surface == panel.parent)?;
        let shown = surf.shown.as_ref()?;
        let widget = &widgets.scene.widgets[panel.widget];
        let buffer = [shown.layout.width, shown.layout.height];
        let frame = paper_widgets::frame(widget.face);
        let place = place([surf.width, surf.height], buffer, frame, widget.pin, widget.height)?;
        let area = [place.origin[0], place.origin[1], place.size[0] as u32, place.size[1] as u32];
        let ground = match shown.region(area) {
            Ok(pixels) => {
                let whole = [0, 0, place.size[0], place.size[1]];
                paper_widgets::ground(pixels.as_raw(), pixels.width() as usize, whole)
            }
            Err(error) => {
                tracing::warn!(error = %format!("{error:#}"), "widgets: backdrop unreadable");
                return None;
            }
        };
        let tones = Tones::over(&widgets.scene.palette, ground);
        Some(Look { tones, place })
    }

    fn refresh_cut(
        &self,
        widgets: &mut Widgets,
        index: usize,
        place: &Place,
        outline: &OnceCell<Option<Outline>>,
    ) {
        let widget = &widgets.scene.widgets[widgets.panels[index].widget];
        let behind = widget.depth == Depth::Behind && self.fill_mode != FillMode::Span;
        let shown = Path::new(&self.path);
        let manifest = behind
            .then(|| {
                widgets
                    .scene
                    .outlines
                    .iter()
                    .find(|manifest| DrawingManifest::describes(manifest, shown))
            })
            .flatten()
            .cloned();
        let Some(manifest) = manifest else {
            widgets.panels[index].cut = None;
            return;
        };
        let parent = &widgets.panels[index].parent;
        let shown = self
            .surfaces
            .iter()
            .find(|surf| &surf.surface == parent)
            .and_then(|surf| surf.shown.as_ref());
        let Some(shown) = shown else {
            widgets.panels[index].cut = None;
            return;
        };
        let buffer = [shown.layout.width, shown.layout.height];
        let frame = Frame { image: [self.raw_w, self.raw_h], buffer, mode: self.fill_mode };
        let stamp = std::fs::metadata(&manifest)
            .and_then(|metadata| Ok((metadata.len(), metadata.modified()?)))
            .ok();
        let key = CutKey {
            path: self.path.clone(),
            stamp,
            frame,
            origin: place.origin,
            size: place.size,
        };
        if widgets.panels[index].cut.as_ref().is_some_and(|(cached, _)| *cached == key) {
            return;
        }
        let loaded = outline.get_or_init(|| {
            Outline::load(&manifest, Path::new(&self.path))
                .inspect_err(
                    |error| tracing::warn!(%error, "widgets: no outline, drawing in front"),
                )
                .ok()
        });
        let cut = loaded.as_ref().and_then(|outline| outline.crop(frame, place.origin, place.size));
        widgets.panels[index].cut = Some((key, cut));
    }

    fn paint(&mut self, widgets: &mut Widgets, index: usize, moment: Moment) {
        let panel = &mut widgets.panels[index];
        let Some(look) = panel.look.as_ref() else { return };
        let place = look.place;
        let face = widgets.scene.widgets[panel.widget].face;
        let labels = &widgets.scene.labels;
        let scene = Scene { lettering: &widgets.lettering, tones: look.tones, moment, labels };
        let cut = panel.cut.as_ref().and_then(|(_, cut)| cut.as_deref());
        let canvas = scene.paint(face, place, cut);
        let flat = canvas.pixels.as_flattened();
        let size = (canvas.width as u32, canvas.height as u32);
        let reused = panel
            .buffer
            .as_mut()
            .filter(|buffer| buffer.dimensions() == size)
            .and_then(|buffer| buffer.write(|bytes, format| pack_pixels(bytes, flat, format)));
        if reused.is_none() {
            let fill = |bytes: &mut [u8], format| pack_pixels(bytes, flat, format);
            match BufferSet::new(&self.shm, &self.qh, size.0, size.1, 1, fill) {
                Ok(buffer) => {
                    if let Some(old) = panel.buffer.replace(buffer) {
                        self.retired.push(old);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "widgets: buffer failed");
                    return;
                }
            }
        }
        let Some(buffer) = panel.buffer.as_mut() else { return };
        let moved = panel.placed_at != Some(place.position);
        if moved {
            panel.subsurface.set_position(place.position[0], place.position[1]);
            panel.placed_at = Some(place.position);
        }
        panel.viewport.set_destination(place.frame[0], place.frame[1]);
        buffer.attach_to(0, &panel.surface);
        panel.surface.damage_buffer(0, 0, size.0 as i32, size.1 as i32);
        panel.surface.commit();
        buffer.drop_local_pages();
        let parent = self.surfaces.iter().find(|surf| surf.surface == panel.parent);
        if moved && let Some(parent) = parent {
            parent.surface.commit();
        }
    }
}

fn moment(time: paper_runtime::clock::LocalTime) -> Moment {
    Moment::civil(time.hour, time.minute, time.weekday, time.day, time.month)
}

fn build(scene: WidgetScene) -> Result<Widgets> {
    scene.validate().context("widget scene")?;
    let font = match &scene.font {
        Some(path) => paper_widgets::read_font(path)
            .with_context(|| format!("widget font {}", path.display()))?,
        None => paper_widgets::FONT.to_vec(),
    };
    let lettering = Lettering::open(font).context("widget font is not a usable font")?;
    let timer = MinuteTimer::new().context("widget timer")?;
    Ok(Widgets { scene, lettering, timer, panels: Vec::new() })
}

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod tests;
