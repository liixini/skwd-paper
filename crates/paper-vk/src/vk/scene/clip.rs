use super::{LayerPush, PuppetDraw, QUAD_VERTEX_COUNT, SceneBlend, SceneTarget, SceneTexture};
use crate::vk::Renderer;
use crate::vk::pipelines::ClipPipelines;
use anyhow::Result;
use ash::vk;
use paper_scene::puppet::masks::{Clip, Command, Plan};
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

pub(in crate::vk) const MASK_FORMAT: vk::Format = vk::Format::R8_UNORM;
const CLIP_INVERT: u32 = 32;
const KEPT_CLEAR: [f32; 4] = [0.0; 4];
const INVERTED_CLEAR: [f32; 4] = [1.0; 4];
const FOLD_PUSH: LayerPush = LayerPush {
    rect: [0.5, 0.5, 1.0, 1.0],
    uv: [0.0, 0.0, 1.0, 1.0],
    tint: [1.0; 4],
    canvas: [1.0, 1.0],
    angle: 0.0,
    pad: 0.0,
    projection: [[0.0; 4]; 3],
    uv_axes: [0.0; 4],
};

pub(super) type Shared = Rc<RefCell<PuppetClip>>;

pub struct PuppetClip {
    plan: Plan,
    sources: Vec<ClipSource>,
    planes: PlaneHandles,
    bounds: Vec<Option<Area>>,
    skipped: Cell<bool>,
}

impl PuppetClip {
    pub fn new(plan: Plan, sources: Vec<ClipSource>, planes: &ClipPlanes) -> Self {
        let bounds = vec![None; sources.len()];
        Self { plan, sources, planes: planes.handles(), bounds, skipped: Cell::new(false) }
    }

    pub fn indices(&self) -> &[u16] {
        &self.plan.indices
    }

    pub(super) fn drawn(&self) -> u32 {
        self.plan.indices.len() as u32
    }

    pub(super) fn update_bounds(&mut self, positions: impl Fn(u16) -> Option<[f32; 2]>) {
        for step in &self.plan.commands {
            let Command::Clip(cut) = step else { continue };
            let span = &self.plan.indices[cut.base.start as usize..cut.base.end as usize];
            self.bounds[cut.mask] = span.iter().filter_map(|index| positions(*index)).fold(
                None,
                |area: Option<Area>, [x, y]| {
                    Some(area.map_or(Area { left: x, top: y, right: x, bottom: y }, |area| {
                        area.include(x, y)
                    }))
                },
            );
        }
    }
}

#[derive(Clone, Copy)]
pub struct ClipSource {
    pub set: vk::DescriptorSet,
    pub additive: bool,
    pub inverted: bool,
}

impl ClipSource {
    fn clear(self) -> [f32; 4] {
        if self.inverted { INVERTED_CLEAR } else { KEPT_CLEAR }
    }
}

struct Plane {
    target: SceneTarget,
    slot: SceneTexture,
    load: vk::RenderPass,
}

#[derive(Clone, Copy)]
struct PlaneHandle {
    framebuffer: vk::Framebuffer,
    clear: vk::RenderPass,
    load: vk::RenderPass,
    set: vk::DescriptorSet,
}

#[derive(Clone, Copy)]
struct PlaneHandles {
    extent: vk::Extent2D,
    main: PlaneHandle,
    intermediate: Option<PlaneHandle>,
}

impl Plane {
    fn handle(&self) -> PlaneHandle {
        PlaneHandle {
            framebuffer: self.target.framebuffer,
            clear: self.target.render_pass,
            load: self.load,
            set: self.slot.set,
        }
    }
}

pub struct ClipPlanes {
    extent: vk::Extent2D,
    main: Plane,
    intermediate: Option<Plane>,
}

impl ClipPlanes {
    pub fn allocation_bytes(&self) -> u64 {
        self.planes().map(|plane| plane.target.allocation_bytes).sum()
    }

    fn planes(&self) -> impl Iterator<Item = &Plane> {
        std::iter::once(&self.main).chain(&self.intermediate)
    }

    fn handles(&self) -> PlaneHandles {
        PlaneHandles {
            extent: self.extent,
            main: self.main.handle(),
            intermediate: self.intermediate.as_ref().map(Plane::handle),
        }
    }
}

pub(super) struct ClipDraw<'a> {
    pub target: &'a SceneTarget,
    pub set: vk::DescriptorSet,
    pub push: &'a LayerPush,
    pub blend: SceneBlend,
    pub pipeline: vk::Pipeline,
}

struct Ready {
    pipelines: ClipPipelines,
    restart: vk::RenderPass,
}

impl super::SceneMesh {
    pub fn set_clip(&mut self, clip: PuppetClip) {
        self.drawn = clip.drawn();
        self.clip = Some(Rc::new(RefCell::new(clip)));
    }

    pub fn clipped(&self) -> bool {
        self.clip.is_some()
    }

    pub fn update_clip(&mut self, plan: Plan) {
        let Some(clip) = &self.clip else { return };
        let mut clip = clip.borrow_mut();
        clip.plan = plan;
        self.drawn = clip.drawn();
    }
}

impl Renderer {
    pub fn create_clip_planes(&mut self, extent: vk::Extent2D, nested: bool) -> Result<ClipPlanes> {
        let format = self.scene_format();
        for samples in [vk::SampleCountFlags::TYPE_1, self.scene_sample_count()] {
            self.ensure_clip_pipelines(samples)?;
            self.scene_pass_append_for(format, samples)?;
        }
        let main = self.clip_plane(extent)?;
        let intermediate = nested.then(|| self.clip_plane(extent)).transpose()?;
        Ok(ClipPlanes { extent, main, intermediate })
    }

    pub fn destroy_clip_planes(&self, planes: ClipPlanes) {
        let ClipPlanes { main, intermediate, .. } = planes;
        for Plane { target, slot, .. } in std::iter::once(main).chain(intermediate) {
            self.destroy_scene_texture(slot);
            self.destroy_scene_target(target);
        }
    }

    fn clip_plane(&mut self, extent: vk::Extent2D) -> Result<Plane> {
        let target = self.create_scene_target_opts(super::SceneTargetSpec {
            depth_attachment: false,
            extent,
            format: MASK_FORMAT,
            repeat: false,
            samples: vk::SampleCountFlags::TYPE_1,
        })?;
        let slot = self.create_view_slot()?;
        self.point_slot_at(&slot, target.view);
        let load = self
            .depth_passes_for(
                super::SceneConfig { format: MASK_FORMAT, depth: None },
                vk::SampleCountFlags::TYPE_1,
            )?
            .append;
        Ok(Plane { target, slot, load })
    }

    pub(super) fn draw_clipped(&self, draw: &ClipDraw<'_>, mesh: PuppetDraw, clip: &PuppetClip) {
        let ready = self.clip_ready(draw.target, clip);
        if ready.is_none() && !clip.skipped.replace(true) {
            tracing::warn!("skwd-wall-vk: puppet clipping masks skipped for this target");
        }
        for step in &clip.plan.commands {
            match (step, &ready) {
                (Command::Draw(range), _) => self.draw_range(mesh, range),
                (Command::Clip(cut), Some(ready)) if !cut.clipped.is_empty() => {
                    self.draw_cut(draw, mesh, clip, cut, ready);
                }
                (Command::Clip(_), _) => {}
            }
        }
    }

    fn clip_ready(&self, target: &SceneTarget, clip: &PuppetClip) -> Option<Ready> {
        let fits = target.extent.width <= clip.planes.extent.width
            && target.extent.height <= clip.planes.extent.height;
        let slot = usize::from(target.samples != vk::SampleCountFlags::TYPE_1);
        let pipelines = self.clip_pipelines[slot].filter(|_| fits)?;
        let restart = match &target.promoted {
            Some(depth) => depth.color_append,
            None => self
                .format_passes_append
                .iter()
                .find(|(key, _)| *key == (target.format, target.samples))
                .map(|(_, pass)| *pass)?,
        };
        Some(Ready { pipelines, restart })
    }

    fn draw_cut(
        &self,
        draw: &ClipDraw<'_>,
        mesh: PuppetDraw,
        clip: &PuppetClip,
        cut: &Clip,
        ready: &Ready,
    ) {
        let passes: Vec<_> = cut
            .ancestors
            .iter()
            .map(|(mask, base)| (*mask, base))
            .chain([(cut.mask, &cut.base)])
            .collect();
        let Some(area) = clip_area(draw, clip, passes.iter().map(|(mask, _)| *mask)) else {
            return;
        };
        let full = vk::Rect2D { offset: vk::Offset2D::default(), extent: draw.target.extent };
        let layout = self.pipeline_layout_clip;
        let PlaneHandles { main, intermediate, .. } = clip.planes;
        unsafe { self.device.cmd_end_render_pass(self.cmd) };
        for (index, (mask, base)) in passes.into_iter().enumerate() {
            let source = clip.sources[mask];
            let plane = if index == 0 { main } else { intermediate.expect("nested clip plane") };
            let begin = PlaneBegin { framebuffer: plane.framebuffer, pass: plane.clear, area };
            self.begin_plane(draw.target, begin, Some(source.clear()));
            let mut push = *draw.push;
            push.pad = (draw.push.pad as u32 | (u32::from(source.inverted) * CLIP_INVERT)) as f32;
            self.bind_clip(ready.pipelines.mask, layout, &[draw.set, source.set], &push);
            self.draw_range(mesh, base);
            unsafe { self.device.cmd_end_render_pass(self.cmd) };
            if index > 0 {
                let begin = PlaneBegin { framebuffer: main.framebuffer, pass: main.load, area };
                self.begin_plane(draw.target, begin, None);
                self.bind_clip(ready.pipelines.fold, layout, &[plane.set], &FOLD_PUSH);
                unsafe {
                    self.device.cmd_draw(self.cmd, QUAD_VERTEX_COUNT, 1, 0, 0);
                    self.device.cmd_end_render_pass(self.cmd);
                }
            }
        }
        let begin =
            PlaneBegin { framebuffer: draw.target.framebuffer, pass: ready.restart, area: full };
        self.begin_plane(draw.target, begin, None);
        let blend = if clip.sources[cut.mask].additive { SceneBlend::Add } else { draw.blend };
        let clipped = ready.pipelines.clipped_for(mesh.cull)[blend as usize];
        self.bind_clip(clipped, layout, &[draw.set, main.set], draw.push);
        unsafe { self.device.cmd_set_scissor(self.cmd, 0, &[area]) };
        self.draw_range(mesh, &cut.clipped);
        unsafe { self.device.cmd_set_scissor(self.cmd, 0, &[full]) };
        self.bind_clip(draw.pipeline, self.pipeline_layout_layer, &[draw.set], draw.push);
    }

    fn begin_plane(&self, target: &SceneTarget, begin: PlaneBegin, clear: Option<[f32; 4]>) {
        let values =
            clear.map(|color| vk::ClearValue { color: vk::ClearColorValue { float32: color } });
        unsafe {
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(begin.pass)
                    .framebuffer(begin.framebuffer)
                    .render_area(begin.area)
                    .clear_values(values.as_slice()),
                vk::SubpassContents::INLINE,
            );
            self.device.cmd_set_viewport(
                self.cmd,
                0,
                &[target.draw_viewport(false, crate::vk::effect::EffectCoordinates::Canvas)],
            );
            self.device.cmd_set_scissor(self.cmd, 0, &[begin.area]);
        }
    }

    fn bind_clip(
        &self,
        pipeline: vk::Pipeline,
        layout: vk::PipelineLayout,
        sets: &[vk::DescriptorSet],
        push: &LayerPush,
    ) {
        unsafe {
            self.device.cmd_bind_pipeline(self.cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
            self.device.cmd_bind_descriptor_sets(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                layout,
                0,
                sets,
                &[],
            );
            self.device.cmd_push_constants(
                self.cmd,
                layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                std::slice::from_raw_parts((&raw const *push).cast(), size_of::<LayerPush>()),
            );
        }
    }

    pub(super) fn draw_range(&self, mesh: PuppetDraw, range: &Range<u32>) {
        unsafe {
            self.device.cmd_bind_vertex_buffers(self.cmd, 0, &[mesh.buffer], &[0]);
            self.device.cmd_bind_index_buffer(
                self.cmd,
                mesh.buffer,
                mesh.index_offset,
                mesh.index_type,
            );
            self.device.cmd_draw_indexed(self.cmd, range.end - range.start, 1, range.start, 0, 0);
        }
    }
}

struct PlaneBegin {
    framebuffer: vk::Framebuffer,
    pass: vk::RenderPass,
    area: vk::Rect2D,
}

#[derive(Clone, Copy)]
struct Area {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Area {
    fn include(self, x: f32, y: f32) -> Self {
        Self {
            left: self.left.min(x),
            top: self.top.min(y),
            right: self.right.max(x),
            bottom: self.bottom.max(y),
        }
    }

    fn intersect(self, other: Self) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }
}

fn clip_area(
    draw: &ClipDraw<'_>,
    clip: &PuppetClip,
    masks: impl Iterator<Item = usize>,
) -> Option<vk::Rect2D> {
    let extent = draw.target.extent;
    let viewport = draw.target.draw_viewport(false, crate::vk::effect::EffectCoordinates::Canvas);
    let full =
        Area { left: 0.0, top: 0.0, right: extent.width as f32, bottom: extent.height as f32 };
    let area = masks
        .filter(|mask| !clip.sources[*mask].inverted)
        .filter_map(|mask| {
            clip.bounds[mask].and_then(|bounds| screen_area(draw.push, &viewport, bounds))
        })
        .fold(full, Area::intersect);
    let (left, top) = (area.left.floor().max(0.0) as u32, area.top.floor().max(0.0) as u32);
    let (right, bottom) = (area.right.ceil() as u32, area.bottom.ceil() as u32);
    (right > left && bottom > top).then(|| vk::Rect2D {
        offset: vk::Offset2D { x: left as i32, y: top as i32 },
        extent: vk::Extent2D { width: right - left, height: bottom - top },
    })
}

fn screen_area(push: &LayerPush, viewport: &vk::Viewport, bounds: Area) -> Option<Area> {
    let Area { left, top, right, bottom } = bounds;
    let mut area: Option<Area> = None;
    for (x, y) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
        let [nx, ny] = ndc(push, x, y)?;
        let px = viewport.x + (nx + 1.0) * 0.5 * viewport.width;
        let py = viewport.y + (ny + 1.0) * 0.5 * viewport.height;
        let point = Area { left: px, top: py, right: px, bottom: py };
        area = Some(area.map_or(point, |area| area.include(px, py)));
    }
    area
}

fn ndc(push: &LayerPush, x: f32, y: f32) -> Option<[f32; 2]> {
    let [row_x, row_y, row_w] = push.projection;
    if row_w.iter().any(|value| *value != 0.0) {
        let project = |[along_x, along_y, _, offset]: [f32; _]| along_x * x + along_y * y + offset;
        let w = project(row_w);
        return (w > f32::EPSILON).then(|| [project(row_x) / w, project(row_y) / w]);
    }
    let [center_x, center_y, width, height] = push.rect;
    let [canvas_w, canvas_h] = push.canvas;
    let (sin, cos) = push.angle.sin_cos();
    let (scaled_x, scaled_y) = (x * width, y * height);
    let rotated = [scaled_x * cos - scaled_y * sin, scaled_x * sin + scaled_y * cos];
    Some([
        (center_x + rotated[0]) / canvas_w * 2.0 - 1.0,
        (center_y + rotated[1]) / canvas_h * 2.0 - 1.0,
    ])
}

#[cfg(test)]
#[path = "clip_tests.rs"]
mod tests;
