use super::model::{App, Target};
use anyhow::{Context, Result};
use paper_control::WidgetScene;
use paper_runtime::minute::MinuteTimer;
use paper_runtime::shm::SharedMemory;
use paper_widgets::{Lettering, Moment, Scene, Tones, place};
use std::os::fd::AsFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use wayland_client::protocol::{
    wl_buffer::{self, WlBuffer},
    wl_shm,
    wl_shm_pool::WlShmPool,
    wl_subcompositor::{self, WlSubcompositor},
    wl_subsurface::{self, WlSubsurface},
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

const BYTES_PER_PIXEL: u32 = 4;
const ALPHA_CHANNEL: usize = 3;

pub(super) struct Widgets {
    pub(super) scene: WidgetScene,
    lettering: Lettering,
    timer: MinuteTimer,
    panels: Vec<Panel>,
    retired: Vec<Buffer>,
}

struct Panel {
    parent: WlSurface,
    widget: usize,
    surface: WlSurface,
    subsurface: WlSubsurface,
    viewport: WpViewport,
    size: (u32, u32, i32),
    position: Option<[i32; 2]>,
    buffer: Option<Buffer>,
}

struct Buffer {
    _memory: SharedMemory,
    pool: WlShmPool,
    handle: WlBuffer,
    released: Arc<AtomicBool>,
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.viewport.destroy();
        self.subsurface.destroy();
        self.surface.destroy();
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        self.handle.destroy();
        self.pool.destroy();
    }
}

impl Widgets {
    fn new(scene: WidgetScene) -> Result<Self> {
        scene.validate().context("widget scene")?;
        let font = match &scene.font {
            Some(path) => paper_widgets::read_font(path).context("widget font")?,
            None => paper_widgets::FONT.to_vec(),
        };
        Ok(Self {
            scene,
            lettering: Lettering::open(font).context("widget font")?,
            timer: MinuteTimer::new().context("widget timer")?,
            panels: Vec::new(),
            retired: Vec::new(),
        })
    }

    pub(super) fn timer_fd(&self) -> i32 {
        self.timer.raw()
    }

    fn panel(&mut self, app: &App, parent: &WlSurface, widget: usize, qh: &QueueHandle<App>) {
        let Some(subcompositor) = &app.subcompositor else { return };
        let Some(compositor) = &app.compositor else { return };
        let Some(viewporter) = &app.viewporter else { return };
        let surface = compositor.create_surface(qh, ());
        let subsurface = subcompositor.get_subsurface(&surface, parent, qh, ());
        subsurface.set_desync();
        let region = compositor.create_region(qh, ());
        surface.set_input_region(Some(&region));
        region.destroy();
        let viewport = viewporter.get_viewport(&surface, qh, ());
        self.panels.push(Panel {
            parent: parent.clone(),
            widget,
            surface,
            subsurface,
            viewport,
            size: (0, 0, 0),
            position: None,
            buffer: None,
        });
    }

    fn paint(&mut self, app: &App, index: usize, qh: &QueueHandle<App>) -> Result<()> {
        let panel = &mut self.panels[index];
        let Some(surf) = app.surfaces.iter().find(|surf| surf.surface == panel.parent) else {
            return Ok(());
        };
        let Some(shm) = &app.shm else { return Ok(()) };
        let widget = &self.scene.widgets[panel.widget];
        let logical = [surf.width, surf.height];
        let physical = logical.map(|side| side.saturating_mul(surf.scale.max(1) as u32));
        let frame = paper_widgets::frame(widget.face);
        let Some(place) = place(logical, physical, frame, widget.pin, widget.height) else {
            return Ok(());
        };
        let time = paper_runtime::clock::local_now().context("widget clock")?;
        let moment = Moment::civil(time.hour, time.minute, time.weekday, time.day, time.month);
        let ground = paper_widgets::luminance(self.scene.palette.shade);
        let scene = Scene {
            lettering: &self.lettering,
            tones: Tones::over(&self.scene.palette, ground),
            moment,
            labels: &self.scene.labels,
        };
        let canvas = scene.paint(widget.face, place, None);
        let size = (canvas.width as u32, canvas.height as u32);
        let stride = size.0.checked_mul(BYTES_PER_PIXEL).context("widget stride")?;
        let bytes = stride.checked_mul(size.1).context("widget bytes")? as usize;
        let mut memory = SharedMemory::new(bytes)?;
        let format = if app.shm_formats.contains(&wl_shm::Format::Abgr8888) {
            wl_shm::Format::Abgr8888
        } else {
            wl_shm::Format::Argb8888
        };
        let raw = canvas.pixels.as_flattened();
        if format == wl_shm::Format::Abgr8888 {
            memory.bytes().copy_from_slice(raw);
        } else {
            for (dst, src) in memory
                .bytes()
                .chunks_exact_mut(BYTES_PER_PIXEL as usize)
                .zip(raw.chunks_exact(BYTES_PER_PIXEL as usize))
            {
                dst.copy_from_slice(&[src[2], src[1], src[0], src[ALPHA_CHANNEL]]);
            }
        }
        let pool = shm.create_pool(memory.as_fd(), bytes as i32, qh, ());
        let released = Arc::new(AtomicBool::new(false));
        let buffer = pool.create_buffer(
            0,
            size.0 as i32,
            size.1 as i32,
            stride as i32,
            format,
            qh,
            Arc::clone(&released),
        );
        let moved = panel.position != Some(place.position);
        if moved {
            panel.subsurface.set_position(place.position[0], place.position[1]);
            panel.position = Some(place.position);
        }
        panel.viewport.set_destination(place.frame[0], place.frame[1]);
        panel.surface.attach(Some(&buffer), 0, 0);
        panel.surface.damage_buffer(0, 0, size.0 as i32, size.1 as i32);
        panel.surface.commit();
        memory.drop_local_pages();
        if moved {
            surf.surface.commit();
        }
        if let Some(old) =
            panel.buffer.replace(Buffer { _memory: memory, pool, handle: buffer, released })
        {
            self.retired.push(old);
        }
        panel.size = (surf.width, surf.height, surf.scale);
        Ok(())
    }

    fn follow(&mut self, app: &App, qh: &QueueHandle<App>, repaint: bool) {
        self.panels.retain(|panel| {
            app.surfaces.iter().any(|surf| surf.surface == panel.parent && !surf.closed)
        });
        for surf in app.surfaces.iter().filter(|surf| surf.configured && !surf.closed) {
            for widget in 0..self.scene.widgets.len() {
                if self.scene.widgets[widget].shown_on(&surf.name)
                    && !self
                        .panels
                        .iter()
                        .any(|panel| panel.parent == surf.surface && panel.widget == widget)
                {
                    self.panel(app, &surf.surface, widget, qh);
                }
            }
        }
        for index in 0..self.panels.len() {
            let panel = &self.panels[index];
            let changed = app
                .surfaces
                .iter()
                .find(|surf| surf.surface == panel.parent)
                .is_some_and(|surf| panel.size != (surf.width, surf.height, surf.scale));
            if (repaint || changed)
                && let Err(error) = self.paint(app, index, qh)
            {
                tracing::warn!(%error, "widgets: dynamic panel failed");
            }
        }
        self.retired.retain(|buffer| !buffer.released.load(Ordering::Acquire));
    }
}

impl Target {
    pub fn sync_widgets(&mut self, ctl: &mut crate::ctl::Ctl) {
        if let Some(scene) = ctl.take_widgets() {
            self.set_widgets(scene);
        }
    }

    pub(super) fn retire_widget_outputs(&mut self, outputs: &[String]) {
        let Some(widgets) = self.app.widgets.as_mut() else { return };
        widgets.panels.retain(|panel| {
            self.app
                .surfaces
                .iter()
                .any(|surf| surf.surface == panel.parent && outputs.contains(&surf.name))
        });
    }

    pub fn set_widgets(&mut self, scene: WidgetScene) {
        if self.app.widgets.as_ref().is_some_and(|current| current.scene == scene) {
            return;
        }
        if scene.widgets.is_empty() {
            self.app.widgets = None;
            return;
        }
        match Widgets::new(scene) {
            Ok(widgets) => self.app.widgets = Some(widgets),
            Err(error) => {
                tracing::warn!(%error, "widgets: dynamic scene rejected");
                return;
            }
        }
        self.follow_widgets(true);
    }

    pub(super) fn follow_widgets(&mut self, repaint: bool) {
        let Some(mut widgets) = self.app.widgets.take() else { return };
        let qh = self.queue.handle();
        widgets.follow(&self.app, &qh, repaint);
        self.app.widgets = Some(widgets);
    }

    pub(super) fn widget_tick(&mut self) {
        let Some(widgets) = &self.app.widgets else { return };
        match widgets.timer.fire() {
            Ok(true) => {
                paper_runtime::clock::reload_zone();
                self.follow_widgets(true);
            }
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, "widgets: timer failed"),
        }
    }
}

impl Dispatch<wl_subcompositor::WlSubcompositor, ()> for App {
    fn event(
        _: &mut Self,
        _: &WlSubcompositor,
        _: wl_subcompositor::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_subsurface::WlSubsurface, ()> for App {
    fn event(
        _: &mut Self,
        _: &WlSubsurface,
        _: wl_subsurface::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, Arc<AtomicBool>> for App {
    fn event(
        _: &mut Self,
        _: &WlBuffer,
        event: wl_buffer::Event,
        released: &Arc<AtomicBool>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            released.store(true, Ordering::Release);
        }
    }
}
