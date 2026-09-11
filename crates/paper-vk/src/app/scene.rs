mod video;

use super::dmabuf_helpers::{create_buffers, init_free_buffers, monotonic_ns};
use super::model::StartFade;
use super::readiness::{signal_ready, signal_swap_failure};
use crate::dmabuf::{DRM_MOD_INVALID, DRM_MOD_LINEAR, xr24_export_modifiers};
use crate::fill::{fill_mode, mode_uv};
use crate::{ctl, decode, shared, vk, wayland};
use anyhow::{Context, Result, anyhow};
use paper_geom::FillMode;
use paper_scene::effect_lifetime::{self as lifetime, Target as FxTargetId, TargetPlan};
use paper_scene::effects::{CompositeBuffer, EffectBind};
use paper_scene::model::SceneModel;
use paper_scene::scene_targets::{
    LayerTargetNode, PassiveLayerTarget, SceneTargetPlan, plan_scene_targets,
};
use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::os::fd::RawFd;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
struct SceneFadeStep {
    mix: f32,
    render_transition: bool,
    finish_after_commit: bool,
}

fn scene_fade_step(ratio: Option<f32>, first_frame: bool) -> SceneFadeStep {
    let Some(ratio) = ratio else {
        return SceneFadeStep { mix: 1.0, render_transition: false, finish_after_commit: false };
    };
    if first_frame {
        return SceneFadeStep { mix: 0.0, render_transition: true, finish_after_commit: false };
    }
    let mix = ratio.clamp(0.0, 1.0);
    SceneFadeStep { mix, render_transition: true, finish_after_commit: mix >= 1.0 }
}

fn scene_swap_duration(duration_ms: u64) -> Option<u64> {
    (duration_ms > 0).then(|| duration_ms.clamp(80, 4000))
}

struct LayerFx {
    layer_id: String,
    quad: usize,
    scene_order: usize,
    slot: usize,
    pipelines: Vec<vk::EffectPipeline>,
    ping: FxTargetStorage,
    pong: FxTargetStorage,
    base_quad: vk::SceneQuad,
    verts: vk::QuadBuffer,
    inputs: Vec<Vec<usize>>,
    fbos: Vec<(String, FxTargetStorage)>,
    fbo_owners: Vec<Option<usize>>,
    remap: Vec<std::cell::Cell<usize>>,
    swaps: Vec<(Option<usize>, usize, usize)>,
    pass_targets: Vec<Option<String>>,
    binds: Vec<Vec<(usize, EffectBind)>>,
    owner: Vec<usize>,
    passes: Vec<paper_scene::effects::PassMeta>,
    output: Option<FxTargetId>,
    source_dynamic: bool,
    source_clock: bool,
    dependency_dynamic: bool,
    retain_targets: bool,
    snapshot: bool,
    passthrough: bool,
    snapshot_slot: Option<usize>,
    crop: Option<FxTargetStorage>,
    crop_quad: Option<vk::SceneQuad>,
    fallback_tint: [f32; 4],
    sampled_targets: BTreeSet<CompositeBuffer>,
    uniforms: std::collections::BTreeMap<String, Vec<f32>>,
}

struct LiveText {
    slot: usize,
    prepared: paper_scene::text::Prepared,
    shown: String,
}

struct LayerAnimation {
    quad: usize,
    frames: Vec<paper_scene::model::SpriteFrame>,
    total: f32,
}

fn frame_uv(frames: &[paper_scene::model::SpriteFrame], total: f32, time: f32) -> [f32; 4] {
    let mut cursor = if total > 0.0 { time.rem_euclid(total) } else { 0.0 };
    for frame in frames {
        if cursor < frame.time {
            return frame.uv;
        }
        cursor -= frame.time;
    }
    frames.last().map_or([0.0, 0.0, 1.0, 1.0], |frame| frame.uv)
}

struct PuppetGroup {
    puppet: paper_scene::model::Puppet,
    mesh: vk::SceneMesh,
    target: vk::SceneTarget,
    texture: usize,
    layer: usize,
}

struct BaseLayerTarget {
    layer_id: String,
    view: ash::vk::ImageView,
    sampler: ash::vk::Sampler,
    extent: ash::vk::Extent2D,
    dynamic: bool,
}

impl PuppetGroup {
    fn animated(&self) -> bool {
        self.puppet.layers.iter().any(|layer| {
            self.puppet.mesh.animations.iter().any(|animation| animation.id == layer.id)
        })
    }
}

enum FxTargetStorage {
    Owned(vk::SceneTarget),
    Shared(usize),
    Released,
}

impl FxTargetStorage {
    fn get<'a>(&'a self, shared: &'a [vk::SceneTarget]) -> &'a vk::SceneTarget {
        match self {
            Self::Owned(target) => target,
            Self::Shared(index) => &shared[*index],
            Self::Released => panic!("released effect target accessed"),
        }
    }

    fn owned(&self) -> Option<&vk::SceneTarget> {
        match self {
            Self::Owned(target) => Some(target),
            Self::Shared(_) | Self::Released => None,
        }
    }

    fn take_owned(&mut self) -> Option<vk::SceneTarget> {
        match std::mem::replace(self, Self::Released) {
            Self::Owned(target) => Some(target),
            storage @ (Self::Shared(_) | Self::Released) => {
                *self = storage;
                None
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FxTargetClass {
    width: u32,
    height: u32,
    repeat: bool,
    format: i32,
}

impl FxTargetClass {
    fn of(target: &vk::SceneTarget) -> Self {
        Self {
            width: target.extent.width,
            height: target.extent.height,
            repeat: target.repeat,
            format: target.format.as_raw(),
        }
    }
}

fn vk_format(format: paper_scene::effects::FboFormat) -> ash::vk::Format {
    use paper_scene::effects::FboFormat;
    match format {
        FboFormat::Rgba8 => ash::vk::Format::R8G8B8A8_UNORM,
        FboFormat::R8 => ash::vk::Format::R8_UNORM,
        FboFormat::Rg8 => ash::vk::Format::R8G8_UNORM,
        FboFormat::R16f => ash::vk::Format::R16_SFLOAT,
        FboFormat::Rg16f => ash::vk::Format::R16G16_SFLOAT,
        FboFormat::Rgba16f => ash::vk::Format::R16G16B16A16_SFLOAT,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct EffectTargetBytes {
    retained: u64,
    transient: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct EffectBakeReport {
    baked_layers: usize,
    retained_bytes: u64,
    transient_bytes: u64,
    released_transient_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct EffectAliasReport {
    planned_layers: usize,
    fallback_layers: usize,
    logical_scratch_targets: usize,
    physical_scratch_targets: usize,
    scratch_bytes: u64,
    released_bytes: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ScratchLayout {
    assignments: Vec<Vec<usize>>,
    physical_classes: Vec<FxTargetClass>,
}

fn plan_scratch_layout(layers: &[Vec<FxTargetClass>]) -> ScratchLayout {
    let mut slots_by_class: HashMap<FxTargetClass, Vec<usize>> = HashMap::new();
    let mut layout = ScratchLayout::default();
    for layer in layers {
        let mut used_by_class: HashMap<FxTargetClass, usize> = HashMap::new();
        let mut assignments = Vec::with_capacity(layer.len());
        for class in layer {
            let ordinal = used_by_class.entry(*class).or_default();
            let slots = slots_by_class.entry(*class).or_default();
            let slot = if let Some(slot) = slots.get(*ordinal) {
                *slot
            } else {
                let slot = layout.physical_classes.len();
                layout.physical_classes.push(*class);
                slots.push(slot);
                slot
            };
            *ordinal += 1;
            assignments.push(slot);
        }
        layout.assignments.push(assignments);
    }
    layout
}

fn classify_effect_target_bytes(
    allocations: &[u64],
    output: FxTargetId,
) -> Option<EffectTargetBytes> {
    let retained = *allocations.get(output.index())?;
    Some(EffectTargetBytes {
        retained,
        transient: allocations.iter().copied().sum::<u64>().saturating_sub(retained),
    })
}

fn scratch_candidate(
    plan: &TargetPlan,
    sampled: &BTreeSet<CompositeBuffer>,
    target: FxTargetId,
) -> bool {
    let scene_sampled = match target {
        FxTargetId::Ping => sampled.contains(&CompositeBuffer::A),
        FxTargetId::Pong => sampled.contains(&CompositeBuffer::B),
        FxTargetId::Fbo(_) => false,
    };
    plan.lifetimes[target.index()].scratch_eligible() && !scene_sampled
}

impl LayerFx {
    fn target_bytes(&self) -> Option<EffectTargetBytes> {
        let mut allocations = Vec::with_capacity(self.fbos.len() + 2);
        allocations
            .extend([self.ping.owned()?.allocation_bytes, self.pong.owned()?.allocation_bytes]);
        allocations.extend(
            self.fbos
                .iter()
                .map(|(_, target)| target.owned().map(|target| target.allocation_bytes))
                .collect::<Option<Vec<_>>>()?,
        );
        if let Some(crop) = &self.crop {
            allocations.push(crop.owned()?.allocation_bytes);
        }
        classify_effect_target_bytes(&allocations, self.output?)
    }

    fn fbo_names(&self) -> Vec<String> {
        self.fbos.iter().map(|(name, _)| name.clone()).collect()
    }

    fn target<'a>(&'a self, id: FxTargetId, shared: &'a [vk::SceneTarget]) -> &'a vk::SceneTarget {
        self.target_storage(id).get(shared)
    }

    fn target_storage(&self, id: FxTargetId) -> &FxTargetStorage {
        match id {
            FxTargetId::Ping => &self.ping,
            FxTargetId::Pong => &self.pong,
            FxTargetId::Fbo(index) => &self.fbos[index].1,
        }
    }

    fn target_storage_mut(&mut self, id: FxTargetId) -> &mut FxTargetStorage {
        match id {
            FxTargetId::Ping => &mut self.ping,
            FxTargetId::Pong => &mut self.pong,
            FxTargetId::Fbo(index) => &mut self.fbos[index].1,
        }
    }

    fn target_ids(&self) -> impl Iterator<Item = FxTargetId> + '_ {
        [FxTargetId::Ping, FxTargetId::Pong]
            .into_iter()
            .chain((0..self.fbos.len()).map(FxTargetId::Fbo))
    }

    fn plan(&self) -> Result<TargetPlan, lifetime::TargetPlanError> {
        lifetime::plan_targets_with(
            &self.fbo_names(),
            &self.fbo_owners,
            &self.swaps,
            &self.pass_targets,
            &self.binds,
            &self.owner,
        )
    }

    fn resolve_fbo(&self, owner: usize, name: &str) -> Option<usize> {
        lifetime::resolve_fbo_scoped(
            self.fbos.iter().map(|(fbo, _)| fbo.as_str()),
            &self.fbo_owners,
            owner,
            name,
        )
        .map(|logical| self.remap[logical].get())
    }

    fn apply_swaps(&self, after: Option<usize>) {
        for &(when, a, b) in &self.swaps {
            if when == after && a != b && a < self.remap.len() && b < self.remap.len() {
                let (physical_a, physical_b) = (self.remap[a].get(), self.remap[b].get());
                self.remap[a].set(physical_b);
                self.remap[b].set(physical_a);
            }
        }
    }

    fn intrinsic_dynamic(&self) -> bool {
        self.source_dynamic
            || self.passes.iter().any(paper_scene::effects::PassMeta::time_dependent)
            || self
                .plan()
                .map(|plan| plan.lifetimes.iter().any(|lifetime| lifetime.loop_carried))
                .unwrap_or(true)
    }

    fn frame_state_dependent(&self) -> bool {
        self.source_clock || self.intrinsic_dynamic() || self.dependency_dynamic
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct TextureKey {
    pixels: paper_scene::tex::Pixels,
    clamp: bool,
    nearest: bool,
}

impl TextureKey {
    fn take(texture: &mut paper_scene::model::Texture) -> Self {
        Self {
            pixels: std::mem::take(&mut texture.pixels),
            clamp: texture.clamp,
            nearest: texture.nearest,
        }
    }

    fn clear() -> Self {
        Self {
            pixels: paper_scene::tex::Pixels::rgba(1, 1, vec![0, 0, 0, 0]),
            clamp: true,
            nearest: false,
        }
    }
}

struct TextureInterner<'a> {
    device: &'a shared::SharedDevice,
    videos: Vec<video::VideoTexture>,
    video_slots: HashMap<(std::sync::Arc<[u8]>, bool, bool), usize>,
    slots: HashMap<TextureKey, usize>,
    reused: usize,
    reused_bytes: usize,
}

impl TextureInterner<'_> {
    fn intern_key(
        &mut self,
        renderer: &mut vk::Renderer,
        textures: &mut Vec<vk::SceneTexture>,
        key: TextureKey,
    ) -> std::result::Result<usize, (anyhow::Error, TextureKey)> {
        if let Some(&slot) = self.slots.get(&key) {
            self.reused += 1;
            self.reused_bytes += key.pixels.bytes();
            return Ok(slot);
        }
        let uploaded =
            match renderer.create_scene_texture_pixels(&key.pixels, key.clamp, key.nearest, true) {
                Ok(uploaded) => uploaded,
                Err(err) => return Err((err, key)),
            };
        let slot = textures.len();
        textures.push(uploaded);
        self.slots.insert(key, slot);
        Ok(slot)
    }

    fn intern_texture(
        &mut self,
        renderer: &mut vk::Renderer,
        textures: &mut Vec<vk::SceneTexture>,
        texture: &mut paper_scene::model::Texture,
    ) -> Result<usize> {
        if let Some(payload) = &texture.video {
            let key = (payload.clone(), texture.clamp, texture.nearest);
            if let Some(&slot) = self.video_slots.get(&key) {
                return Ok(slot);
            }
            let video =
                video::VideoTexture::open(self.device, payload, texture.clamp, texture.nearest)?;
            let mut texture_slot = renderer.create_view_slot()?;
            texture_slot.view = video.target().view;
            texture_slot.sampler = video.target().sampler;
            renderer.point_slot_at(&texture_slot, video.target().view);
            let slot = textures.len();
            textures.push(texture_slot);
            self.videos.push(video);
            self.video_slots.insert(key, slot);
            return Ok(slot);
        }
        let key = TextureKey::take(texture);
        match self.intern_key(renderer, textures, key) {
            Ok(slot) => Ok(slot),
            Err((err, key)) => {
                texture.pixels = key.pixels;
                Err(err)
            }
        }
    }

    fn intern_rgba(
        &mut self,
        renderer: &mut vk::Renderer,
        textures: &mut Vec<vk::SceneTexture>,
        key: TextureKey,
    ) -> Result<usize> {
        self.intern_key(renderer, textures, key).map_err(|(err, _)| err)
    }
}

fn size4(extent: ash::vk::Extent2D) -> [f32; 4] {
    let (w, h) = (extent.width as f32, extent.height as f32);
    [w, h, w, h]
}

struct ParticleEngine {
    pipeline: vk::EffectPipeline,
    meta: paper_scene::effects::PassMeta,
    vertices: vk::DynBuffer,
    indices: vk::DynBuffer,
    capacity: usize,
    extra_textures: Vec<usize>,
    render_var1: [f32; 4],
    texture_size: [f32; 4],
    scratch: Vec<f32>,
}

struct ParticleGroup {
    system: paper_scene::particles::ParticleSystem,
    sim: paper_scene::particles::Sim,
    texture: usize,
    ratio: f32,
    frames: Vec<paper_scene::model::SpriteFrame>,
    scene_order: usize,
    engine: Option<ParticleEngine>,
}

struct OrderedParticleQuad {
    scene_order: usize,
    quad: vk::SceneQuad,
}

struct OrderedParticleDraw {
    scene_order: usize,
    group: usize,
    index_count: u32,
}

type Mat4 = [f32; 16];

fn mat4_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            out[col * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[col * 4 + k]).sum();
        }
    }
    out
}

fn clip_y(d3d_clip: bool) -> f32 {
    if d3d_clip { 1.0 } else { -1.0 }
}

fn scene_ortho(canvas: (f32, f32), d3d_clip: bool) -> Mat4 {
    let (w, h) = (canvas.0.max(1.0), canvas.1.max(1.0));
    let ys = clip_y(d3d_clip);
    [2.0 / w, 0.0, 0.0, 0.0, 0.0, ys * 2.0 / h, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -ys, 0.5, 1.0]
}

pub(crate) const PARTICLE_EYE_FACTOR: f32 = 0.45;

pub(crate) fn particle_eye_distance(canvas_height: f32) -> f32 {
    PARTICLE_EYE_FACTOR * canvas_height.max(1.0)
}

fn scene_perspective(canvas: (f32, f32), d3d_clip: bool) -> Mat4 {
    let (w, h) = (canvas.0.max(1.0), canvas.1.max(1.0));
    let (near, far) = (1.0f32, 100_000.0f32);
    let eye_distance = particle_eye_distance(h);
    let f = eye_distance / (h * 0.5);
    let a = far / (near - far);
    let b = near * far / (near - far);
    let ys = clip_y(d3d_clip);
    let projection: Mat4 =
        [f / (w / h), 0.0, 0.0, 0.0, 0.0, ys * f, 0.0, 0.0, 0.0, 0.0, a, -1.0, 0.0, 0.0, b, 0.0];
    let view: Mat4 = [
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        -w * 0.5,
        -h * 0.5,
        -eye_distance,
        1.0,
    ];
    mat4_mul(&projection, &view)
}

fn layer_model_matrix(layer: &paper_scene::model::Layer, canvas: (f32, f32)) -> (Mat4, Mat4) {
    let (ox, oy, oz) = (layer.center.0, canvas.1 - layer.center.1, layer.depth);
    let (kx, ky) = (layer.scale.0, layer.scale.1);
    let (n, c) = (-layer.angle).sin_cos();
    let inv = |s: f32| if s.abs() < 1e-6 { 0.0 } else { 1.0 / s };
    let (ix, iy) = (inv(kx), inv(ky));
    let model =
        [kx * c, kx * n, 0.0, 0.0, -ky * n, ky * c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, ox, oy, oz, 1.0];
    let inverse = [
        c * ix,
        -n * iy,
        0.0,
        0.0,
        n * ix,
        c * iy,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        -(c * ox + n * oy) * ix,
        -(-n * ox + c * oy) * iy,
        -oz,
        1.0,
    ];
    (model, inverse)
}

fn layer_uniforms(
    layer: &paper_scene::model::Layer,
    canvas: (f32, f32),
    lights: ([f32; 3], [f32; 3]),
) -> std::collections::BTreeMap<String, Vec<f32>> {
    let (matrix, inverse) = layer_model_matrix(layer, canvas);
    std::collections::BTreeMap::from([
        ("g_LayerModelMatrix".to_string(), matrix.to_vec()),
        ("g_ModelMatrix".to_string(), matrix.to_vec()),
        ("g_ModelMatrixInverse".to_string(), inverse.to_vec()),
        ("g_LightAmbientColor".to_string(), lights.0.to_vec()),
        ("g_LightSkylightColor".to_string(), lights.1.to_vec()),
    ])
}

fn model_matrix(origin: (f32, f32, f32), scale: [f32; 3], angle: f32) -> Mat4 {
    let (sin, cos) = angle.sin_cos();
    [
        cos * scale[0],
        sin * scale[0],
        0.0,
        0.0,
        -sin * scale[1],
        cos * scale[1],
        0.0,
        0.0,
        0.0,
        0.0,
        scale[2],
        0.0,
        origin.0,
        origin.1,
        origin.2,
        1.0,
    ]
}

fn model_inverse(origin: (f32, f32, f32), scale: [f32; 3], angle: f32) -> Mat4 {
    let inv = |s: f32| if s.abs() < 1e-6 { 0.0 } else { 1.0 / s };
    let (sx, sy, sz) = (inv(scale[0]), inv(scale[1]), inv(scale[2]));
    let (sin, cos) = angle.sin_cos();
    let (a, b, c, d) = (cos * sx, -sin * sy, sin * sx, cos * sy);
    [
        a,
        b,
        0.0,
        0.0,
        c,
        d,
        0.0,
        0.0,
        0.0,
        0.0,
        sz,
        0.0,
        -(a * origin.0 + c * origin.1),
        -(b * origin.0 + d * origin.1),
        -origin.2 * sz,
        1.0,
    ]
}

fn sprite_grid(texture: &paper_scene::model::Texture) -> [f32; 4] {
    let (w, h) = (texture.width.max(1) as f32, texture.height.max(1) as f32);
    if texture.frames.len() > 1 {
        let uv = texture.frames[0].uv;
        let cols = (1.0 / uv[2].max(1e-6)).round().max(1.0);
        let rows = (1.0 / uv[3].max(1e-6)).round().max(1.0);
        let ratio = (h / rows) / (w / cols);
        [1.0 / cols, 1.0 / rows, texture.frames.len() as f32, ratio]
    } else {
        [0.0, 0.0, 0.0, h / w]
    }
}

fn particle_batches<'a>(
    groups: &'a [ParticleGroup],
    image_orders: &[usize],
    particle_quads: &[OrderedParticleQuad],
    draws: &[OrderedParticleDraw],
    before: Option<usize>,
    grab: Option<&'a vk::SceneTarget>,
) -> Vec<vk::ParticleDraw<'a>> {
    let mut out: Vec<vk::ParticleDraw<'a>> = draws
        .iter()
        .filter(|draw| before.is_none_or(|limit| draw.scene_order < limit))
        .filter_map(|draw| {
            let group = groups.get(draw.group)?;
            let engine = group.engine.as_ref()?;
            let after = image_orders
                .iter()
                .filter(|order| {
                    before.is_none_or(|limit| **order < limit) && **order <= draw.scene_order
                })
                .count()
                + particle_quads
                    .iter()
                    .filter(|quad| {
                        before.is_none_or(|limit| quad.scene_order < limit)
                            && quad.scene_order <= draw.scene_order
                    })
                    .count();
            Some(vk::ParticleDraw {
                after,
                pipeline: &engine.pipeline,
                vertices: engine.vertices.buffer,
                indices: engine.indices.buffer,
                index_count: draw.index_count,
                grab: group.system.grab_slot.and(grab),
            })
        })
        .collect();
    out.sort_by_key(|batch| batch.after);
    out
}

struct FrameSample {
    uv: [f32; 4],
    ratio: f32,
    extra: f32,
    next: Option<([f32; 4], f32, f32)>,
}

fn frame_of(group: &ParticleGroup, index: usize) -> ([f32; 4], f32, f32) {
    let frame = group.frames[index];
    let ratio =
        if frame.uv[3] > 0.0 { frame.uv[2] / frame.uv[3] * group.ratio } else { group.ratio };
    let extra = if frame.rotated { -std::f32::consts::FRAC_PI_2 } else { 0.0 };
    (frame.uv, ratio, extra)
}

fn particle_frame(
    group: &ParticleGroup,
    particle: &paper_scene::particles::Particle,
) -> FrameSample {
    if group.frames.is_empty() {
        return FrameSample {
            uv: [0.0, 0.0, 1.0, 1.0],
            ratio: group.ratio,
            extra: 0.0,
            next: None,
        };
    }
    let count = group.frames.len();
    match group.system.animation {
        paper_scene::particles::Animation::RandomFrame => {
            let seed = particle.phase / std::f32::consts::TAU;
            let (uv, ratio, extra) =
                frame_of(group, ((seed * count as f32) as usize).min(count - 1));
            FrameSample { uv, ratio, extra, next: None }
        }
        paper_scene::particles::Animation::Sequence => {
            let life = (particle.age / particle.lifetime).clamp(0.0, 1.0);
            let pos = (life * group.system.sequence_multiplier).fract() * count as f32;
            let index = (pos as usize).min(count - 1);
            let blend = pos - index as f32;
            let (uv, ratio, extra) = frame_of(group, index);
            let next = (blend > 0.01).then(|| {
                let (uv2, _, extra2) = frame_of(group, (index + 1) % count);
                (uv2, extra2, blend)
            });
            FrameSample { uv, ratio, extra, next }
        }
    }
}

enum FadeFrom {
    Nv12(vk::UploadPath),
    Scene(vk::SceneTarget),
}

struct FadeSource {
    source: FadeFrom,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransitionStyle {
    Sand(i32),
    Effect(usize),
    Fade,
}

struct Group {
    renderer: vk::Renderer,
    target: vk::SceneTarget,
    scene_snapshot: Option<vk::SceneTarget>,
    scene_grab: Option<vk::SceneTarget>,
    frozen: bool,
    from: Option<FadeSource>,
    quads: Vec<vk::SceneQuad>,
    quad_scene_order: Vec<usize>,
    textures: Vec<vk::SceneTexture>,
    videos: Vec<video::VideoTexture>,
    passive_video_targets: Vec<(usize, vk::SceneQuad)>,
    base_scene_targets: Vec<BaseLayerTarget>,
    passive_scene_targets: Vec<vk::SceneTarget>,
    fx: Vec<LayerFx>,
    baked_fx_outputs: Vec<vk::SceneTarget>,
    puppets: Vec<PuppetGroup>,
    fx_scratch: Vec<vk::SceneTarget>,
    ndc_quad: Option<vk::QuadBuffer>,
    ndc_quad_d3d: Option<vk::QuadBuffer>,
    particles: Vec<ParticleGroup>,
    animations: Vec<LayerAnimation>,
    audio: Option<paper_audio::Analyser>,
    audio_fx: Vec<usize>,
    bands: paper_audio::Bands,
    live_text: Vec<LiveText>,
    live_text_due: f32,
    effects_animated: bool,
    scene_uniforms: std::collections::BTreeMap<String, Vec<f32>>,
    fixed_daytime: Option<f32>,
    canvas: (f32, f32),
    clear: [f32; 3],
}

struct Presenter {
    rts: Vec<vk::RenderTarget>,
    exports: Vec<vk::ExportImage>,
    stream_semaphores: Vec<vk::ExternalSemaphore>,
    renderer: vk::Renderer,
    width: u32,
    height: u32,
    direct_scene_copy: bool,
}

impl Presenter {
    fn grow_scene_pool(
        &mut self,
        target: &mut wayland::Target,
        buffers: &mut [Vec<wayland_client::protocol::wl_buffer::WlBuffer>],
    ) -> Result<usize> {
        let selected = self.exports[0]
            .modifier
            .filter(|modifier| *modifier != DRM_MOD_LINEAR && *modifier != DRM_MOD_INVALID);
        let export = self.renderer.create_xr24_export(
            self.width,
            self.height,
            selected.as_slice(),
            selected.is_none().then_some(self.exports[0].modifier).flatten(),
        )?;
        if !target.probe_dmabuf_import(
            export.fd,
            self.width,
            self.height,
            export.offset,
            export.stride,
            export.modifier,
        )? {
            return Err(anyhow!("compositor rejected additional scene pool buffer"));
        }
        let rt = if export.direct_render && !self.rts.is_empty() {
            Some(self.renderer.create_export_rt(&export)?)
        } else {
            None
        };
        let bi = self.exports.len();
        for (si, ring) in buffers.iter_mut().enumerate() {
            ring.push(target.create_dmabuf_buffer(
                export.fd,
                self.width,
                self.height,
                export.offset,
                export.stride,
                export.modifier,
                si,
                bi,
            )?);
            target.app.surfaces[si].free_buffers.push(bi);
        }
        self.exports.push(export);
        if let Some(rt) = rt {
            self.rts.push(rt);
        }
        tracing::info!(
            buffers = self.exports.len(),
            export_mib = self.exports.iter().map(|export| export.allocation_size).sum::<u64>()
                as f64
                / 1_048_576.0,
            "skwd-wall-vk: expanded shared scene presentation pool"
        );
        Ok(bi)
    }

    fn ensure_render_targets(&mut self) -> Result<()> {
        if !self.rts.is_empty() {
            return Ok(());
        }
        self.rts = if self.exports[0].direct_render {
            self.exports
                .iter()
                .map(|export| self.renderer.create_export_rt(export))
                .collect::<Result<_>>()?
        } else {
            vec![self.renderer.create_render_target(self.width, self.height)?]
        };
        Ok(())
    }

    fn present(&mut self, target: &vk::SceneTarget, buffer_index: usize) -> Result<()> {
        let uv = mode_uv(target.extent.width, target.extent.height, self.width, self.height);
        if self.direct_scene_copy
            && target.extent.width == self.width
            && target.extent.height == self.height
            && uv == [1.0, 1.0, 0.0, 0.0]
        {
            if !self.rts.is_empty() {
                self.renderer.wait_render()?;
                self.rts.clear();
            }
            let export = &mut self.exports[buffer_index];
            return self.renderer.blit_scene_to_export(target, export);
        }
        self.ensure_render_targets()?;
        let rt_index = if self.exports[buffer_index].direct_render { buffer_index } else { 0 };
        let export = &mut self.exports[buffer_index];
        self.renderer.render_to(&self.rts[rt_index], export, &vk::Src::Rgba(target.view), uv)
    }

    fn fade(
        &mut self,
        group: &Group,
        buffer_index: usize,
        progress: f32,
        style: TransitionStyle,
    ) -> Result<()> {
        let Some(source) = &group.from else {
            return self.present(&group.target, buffer_index);
        };
        if progress <= 0.0 {
            return match &source.source {
                FadeFrom::Scene(target) => self.present(target, buffer_index),
                FadeFrom::Nv12(up) => {
                    self.ensure_render_targets()?;
                    let rt_index =
                        if self.exports[buffer_index].direct_render { buffer_index } else { 0 };
                    let export = &mut self.exports[buffer_index];
                    let from = vk::Src::Views(up.luma_view, up.chroma_view);
                    let from_uv = mode_uv(source.width, source.height, self.width, self.height);
                    self.renderer.render_to(&self.rts[rt_index], export, &from, from_uv)
                }
            };
        }
        if progress >= 1.0 {
            return self.present(&group.target, buffer_index);
        }
        self.ensure_render_targets()?;
        let rt_index = if self.exports[buffer_index].direct_render { buffer_index } else { 0 };
        let export = &mut self.exports[buffer_index];
        let from = match &source.source {
            FadeFrom::Nv12(up) => vk::Src::Views(up.luma_view, up.chroma_view),
            FadeFrom::Scene(rt) => vk::Src::Rgba(rt.view),
        };
        let from_uv = mode_uv(source.width, source.height, self.width, self.height);
        let to_uv =
            mode_uv(group.target.extent.width, group.target.extent.height, self.width, self.height);
        let to = vk::Src::Rgba(group.target.view);
        match style {
            TransitionStyle::Sand(style) => self.renderer.render_sand_to(
                &self.rts[rt_index],
                export,
                &from,
                from_uv,
                &to,
                to_uv,
                progress,
                style,
            ),
            TransitionStyle::Effect(effect) => self.renderer.render_effect_to(
                &self.rts[rt_index],
                export,
                &from,
                from_uv,
                &to,
                to_uv,
                progress,
                effect,
            ),
            TransitionStyle::Fade => {
                let mix = progress * progress * (3.0 - 2.0 * progress);
                self.renderer.render_fade_to(
                    &self.rts[rt_index],
                    export,
                    &from,
                    from_uv,
                    &to,
                    to_uv,
                    mix,
                )
            }
        }
    }

    fn read_export_to(&mut self, buffer_index: usize, rb: &vk::ReadbackBuf) -> Result<()> {
        self.renderer.read_export_to(&self.exports[buffer_index], rb, self.width, self.height)
    }

    fn wait_render(&self) -> Result<()> {
        self.renderer.wait_render()
    }
}

impl Group {
    fn scene_target_plan(
        &self,
        include_clocks: bool,
    ) -> Result<SceneTargetPlan, paper_scene::scene_targets::SceneTargetPlanError> {
        let local_targets: Vec<Vec<String>> = self.fx.iter().map(LayerFx::fbo_names).collect();
        let nodes: Vec<LayerTargetNode<'_>> = self
            .fx
            .iter()
            .enumerate()
            .map(|(index, fx)| LayerTargetNode {
                id: &fx.layer_id,
                scene_order: fx.scene_order,
                local_targets: &local_targets[index],
                binds: &fx.binds,
                dynamic: fx.intrinsic_dynamic() || (include_clocks && fx.source_clock),
                passthrough: fx.passthrough,
                prefix_dynamic: self
                    .particles
                    .iter()
                    .any(|particle| particle.scene_order < fx.scene_order)
                    || self.puppets.iter().any(|puppet| {
                        self.quad_scene_order[puppet.layer] < fx.scene_order && puppet.animated()
                    }),
            })
            .collect();
        let passive: Vec<PassiveLayerTarget<'_>> = self
            .base_scene_targets
            .iter()
            .map(|layer| PassiveLayerTarget { id: &layer.layer_id, dynamic: layer.dynamic })
            .collect();
        plan_scene_targets(&nodes, &passive)
    }

    fn apply_scene_target_plan(&mut self, plan: SceneTargetPlan) -> Result<()> {
        for index in 0..self.fx.len() {
            self.fx[index].snapshot = plan.snapshots[index];
            self.fx[index].dependency_dynamic = plan.dynamic[index];
            self.fx[index].retain_targets = plan.retain[index];
            self.fx[index].sampled_targets.clone_from(&plan.sampled[index]);
        }
        let mut slots: Vec<Option<LayerFx>> =
            std::mem::take(&mut self.fx).into_iter().map(Some).collect();
        self.fx = plan
            .order
            .iter()
            .map(|&index| slots[index].take().expect("scene target order is a permutation"))
            .collect();
        if plan.shadow_targets > 0 && self.scene_snapshot.is_none() {
            self.scene_snapshot = Some(
                self.renderer
                    .create_scene_target(self.target.extent.width, self.target.extent.height)
                    .context("scene-so-far shadow target")?,
            );
        }
        if let Some(snapshot) = &self.scene_snapshot {
            for slot in self.fx.iter().filter_map(|fx| fx.snapshot_slot) {
                self.renderer.point_slot_at(&self.textures[slot], snapshot.view);
            }
        }
        if self.scene_grab.is_none()
            && self.particles.iter().any(|group| group.system.grab_slot.is_some())
        {
            self.scene_grab = Some(
                self.renderer
                    .create_scene_target(self.target.extent.width, self.target.extent.height)
                    .context("scene grab target")?,
            );
        }
        let shadow_bytes = self.scene_snapshot.as_ref().map_or(0, |target| target.allocation_bytes)
            + self.scene_grab.as_ref().map_or(0, |target| target.allocation_bytes);
        let passive_target_bytes =
            self.passive_scene_targets.iter().map(|target| target.allocation_bytes).sum::<u64>();
        tracing::info!(
            effect_layers = self.fx.len(),
            dependencies = plan.dependencies.len(),
            snapshot_consumers = plan.snapshots.iter().filter(|needed| **needed).count(),
            sampled_layer_targets = plan.sampled.iter().map(BTreeSet::len).sum::<usize>(),
            shadow_targets = plan.shadow_targets,
            shadow_bytes,
            shadow_mib = shadow_bytes as f64 / 1_048_576.0,
            passive_targets = self.passive_scene_targets.len(),
            passive_target_bytes,
            passive_target_mib = passive_target_bytes as f64 / 1_048_576.0,
            "skwd-wall-vk: planned scene-wide render targets"
        );
        Ok(())
    }

    fn configure_scene_targets(&mut self, strict: bool) -> Result<()> {
        loop {
            match self.scene_target_plan(true) {
                Ok(plan) => {
                    let animation = self.scene_target_plan(false)?;
                    self.effects_animated = animation.dynamic.iter().any(|dynamic| *dynamic);
                    return self.apply_scene_target_plan(plan);
                }
                Err(error) if strict => {
                    return Err(anyhow!("[native-scene-gap:scene-render-targets] {error}"));
                }
                Err(error) => {
                    let mut affected = error.consumers();
                    affected.sort_unstable();
                    affected.dedup();
                    tracing::warn!(
                        detail = %error,
                        affected = ?affected,
                        "skwd-wall-vk: skipping unsafe scene render-target effect chain"
                    );
                    for index in affected.into_iter().rev() {
                        if index < self.fx.len() {
                            let fx = self.fx.remove(index);
                            self.quads[fx.quad].tint = fx.fallback_tint;
                            self.destroy_layer_fx(fx);
                        }
                    }
                }
            }
        }
    }

    fn frame_clock(&self, time: f32, dt: f32) -> paper_scene::effects::FrameClock {
        match self.fixed_daytime {
            Some(daytime) => paper_scene::effects::FrameClock { time, dt, daytime },
            None => paper_scene::effects::FrameClock::now(time, dt),
        }
    }

    fn read_canvas(&mut self) -> Result<(u32, u32, Vec<u8>)> {
        self.renderer.read_scene_target(&self.target)
    }

    fn advance_layer_animations(&mut self, time: f32) {
        for animation in &self.animations {
            let uv = frame_uv(&animation.frames, animation.total, time);
            match self.fx.iter_mut().find(|fx| fx.quad == animation.quad) {
                Some(fx) if !fx.passthrough => fx.base_quad.uv = uv,
                Some(_) => {}
                None => self.quads[animation.quad].uv = uv,
            }
        }
    }

    fn animated(&self) -> bool {
        !self.videos.is_empty()
            || !self.animations.is_empty()
            || !self.particles.is_empty()
            || self.puppets.iter().any(PuppetGroup::animated)
            || self.fx.iter().flat_map(|fx| &fx.passes).any(|pass| pass.time_dependent())
            || self.effects_animated
            || self.audio.is_some()
    }

    fn live_text_wait(&self, time: f32) -> Option<f32> {
        (!self.live_text.is_empty()).then(|| (self.live_text_due - time).max(0.0))
    }

    fn advance_live_text(&mut self, time: f32) -> Result<()> {
        if self.live_text.is_empty() || time < self.live_text_due {
            return Ok(());
        }
        let Some(now) = paper_scene::dynamic_text::local_now() else {
            self.live_text_due = time + 60.0;
            return Ok(());
        };
        let cadence = self
            .live_text
            .iter()
            .map(|entry| entry.prepared.cadence())
            .min()
            .unwrap_or(paper_scene::dynamic_text::Cadence::Day);
        self.live_text_due = time + paper_scene::dynamic_text::seconds_until_next(cadence, now);
        self.refresh_live_text(now)
    }

    fn refresh_live_text(&mut self, now: paper_scene::dynamic_text::LocalTime) -> Result<()> {
        for index in 0..self.live_text.len() {
            let wanted = self.live_text[index].prepared.value(now);
            if wanted == self.live_text[index].shown {
                continue;
            }
            let Some(texture) = self.live_text[index].prepared.rasterize(&wanted) else {
                continue;
            };
            let replacement =
                self.renderer.create_scene_texture_pixels(&texture.pixels, true, false, false)?;
            let slot = self.live_text[index].slot;
            if slot >= self.textures.len() {
                self.renderer.destroy_scene_texture(replacement);
                continue;
            }
            let previous = std::mem::replace(&mut self.textures[slot], replacement);
            self.renderer.destroy_scene_texture(previous);
            self.live_text[index].shown = wanted;
        }
        Ok(())
    }

    fn advance_audio(&mut self, dt: f32) {
        let Some(analyser) = self.audio.as_mut() else {
            return;
        };
        analyser.fill(&mut self.bands, dt);
        write_bands(&mut self.scene_uniforms, &self.bands);
        for index in &self.audio_fx {
            if let Some(fx) = self.fx.get_mut(*index) {
                write_bands(&mut fx.uniforms, &self.bands);
            }
        }
    }

    fn particle_quads(
        &mut self,
        time: f32,
        dt: f32,
    ) -> (Vec<OrderedParticleQuad>, Vec<OrderedParticleDraw>) {
        let mut out = Vec::new();
        let mut draws = Vec::new();
        let clock = self.frame_clock(time, dt);
        let screen = (self.target.extent.width, self.target.extent.height);
        let canvas = self.canvas;
        for (group_index, group) in self.particles.iter_mut().enumerate() {
            group.sim.step(&group.system, dt);
            if let Some(engine) = group.engine.as_mut() {
                let system = &group.system;
                let count = paper_scene::particles::pack_sprites(
                    system,
                    &group.sim,
                    group.frames.len(),
                    &mut engine.scratch,
                );
                let count = count.min(engine.capacity);
                if count == 0 {
                    continue;
                }
                let floats = count
                    * paper_scene::particles::SPRITE_VERTICES
                    * paper_scene::particles::SPRITE_FLOATS_PER_VERTEX;
                let bytes: Vec<u8> =
                    engine.scratch[..floats].iter().flat_map(|value| value.to_le_bytes()).collect();
                self.renderer.write_dyn_buffer(&engine.vertices, &bytes);
                let d3d_clip = engine.pipeline.hlsl && vk::d3d_clip_enabled();
                let view_projection = if system.perspective {
                    scene_perspective(canvas, d3d_clip)
                } else {
                    scene_ortho(canvas, d3d_clip)
                };
                let model = model_matrix(system.origin, system.draw_scale3(), system.angle);
                let mut overrides = std::collections::BTreeMap::new();
                overrides.insert(
                    "g_ModelViewProjectionMatrix".to_string(),
                    mat4_mul(&view_projection, &model).to_vec(),
                );
                overrides.insert("g_ModelMatrix".to_string(), model.to_vec());
                overrides.insert(
                    "g_ModelMatrixInverse".to_string(),
                    model_inverse(system.origin, system.draw_scale3(), system.angle).to_vec(),
                );
                overrides.insert("g_OrientationUp".to_string(), vec![0.0, 1.0, 0.0]);
                overrides.insert("g_OrientationRight".to_string(), vec![1.0, 0.0, 0.0]);
                overrides.insert("g_OrientationForward".to_string(), vec![0.0, 0.0, 1.0]);
                overrides.insert("g_ViewUp".to_string(), vec![0.0, 1.0, 0.0]);
                overrides.insert("g_ViewRight".to_string(), vec![1.0, 0.0, 0.0]);
                overrides.insert(
                    "g_EyePosition".to_string(),
                    vec![canvas.0 * 0.5, canvas.1 * 0.5, particle_eye_distance(canvas.1)],
                );
                overrides.insert(
                    "g_RenderVar0".to_string(),
                    vec![
                        system.trail.length,
                        system.trail.max_length,
                        system.trail.min_length,
                        0.0,
                    ],
                );
                overrides.insert("g_RenderVar1".to_string(), engine.render_var1.to_vec());
                for (name, value) in &self.scene_uniforms {
                    overrides.entry(name.clone()).or_insert_with(|| value.clone());
                }
                let base = &self.textures[group.texture];
                let mut views = vec![(base.view, base.sampler)];
                let mut sizes = vec![Some(engine.texture_size)];
                for &slot in &engine.extra_textures {
                    let texture = &self.textures[slot];
                    views.push((texture.view, texture.sampler));
                    sizes.push(None);
                }
                if let (Some(slot), Some(grab)) = (system.grab_slot, self.scene_grab.as_ref()) {
                    if views.len() <= slot {
                        views.resize(slot + 1, views[0]);
                        sizes.resize(slot + 1, None);
                    }
                    views[slot] = (grab.view, grab.sampler);
                    sizes[slot] = Some(size4(grab.extent));
                }
                let uniforms = engine.meta.uniform_bytes_with(
                    clock, None, screen.0, screen.1, screen, &sizes, &overrides, d3d_clip,
                );
                self.renderer.write_effect_inputs(&engine.pipeline, &views, &uniforms);
                draws.push(OrderedParticleDraw {
                    scene_order: group.scene_order,
                    group: group_index,
                    index_count: (count * paper_scene::particles::SPRITE_INDICES) as u32,
                });
                continue;
            }
            let system = &group.system;
            let blend = if system.additive { vk::SceneBlend::Add } else { vk::SceneBlend::Alpha };
            if system.renderer == paper_scene::particles::Renderer::Ribbon {
                for (particle, trail) in group.sim.particles.iter().zip(group.sim.history.iter()) {
                    if particle.alpha <= 0.002 || particle.size <= 0.01 {
                        continue;
                    }
                    let sample = particle_frame(group, particle);
                    let (uv, extra) = (sample.uv, sample.extra);
                    let width =
                        particle.size * paper_scene::particles::SPRITE_EXTENT * system.draw_scale();
                    for (index, pair) in trail.windows(2).enumerate() {
                        let (ax, ay) = (
                            system.origin.0 + pair[0][0] * system.draw_scale(),
                            system.origin.1 + pair[0][1] * system.draw_scale(),
                        );
                        let (bx, by) = (
                            system.origin.0 + pair[1][0] * system.draw_scale(),
                            system.origin.1 + pair[1][1] * system.draw_scale(),
                        );
                        let (dx, dy) = (bx - ax, by - ay);
                        let span = dx.hypot(dy);
                        if span < 0.5 {
                            continue;
                        }
                        let taper = if system.trail.fade_alpha {
                            1.0 - index as f32 / paper_scene::particles::TRAIL_POINTS.max(1) as f32
                        } else {
                            1.0
                        };
                        out.push(OrderedParticleQuad {
                            scene_order: group.scene_order,
                            quad: vk::SceneQuad {
                                rect: [
                                    (ax + bx) * 0.5,
                                    self.canvas.1 - (ay + by) * 0.5,
                                    width,
                                    span,
                                ],
                                uv,
                                tint: [
                                    particle.color[0] * system.tint[0],
                                    particle.color[1] * system.tint[1],
                                    particle.color[2] * system.tint[2],
                                    particle.alpha * taper,
                                ],
                                angle: (-dx).atan2(dy) + extra,
                                texture: group.texture,
                                blend,
                            },
                        });
                    }
                }
                continue;
            }
            for particle in &group.sim.particles {
                if particle.alpha <= 0.002 || particle.size <= 0.01 {
                    continue;
                }
                let sample = particle_frame(group, particle);
                let size =
                    particle.size * paper_scene::particles::SPRITE_EXTENT * system.draw_scale();
                let x = system.origin.0 + particle.pos[0] * system.draw_scale();
                let y = system.origin.1 + particle.pos[1] * system.draw_scale();
                let (width, height, angle) =
                    if system.renderer == paper_scene::particles::Renderer::Trail {
                        let (vx, vy) = (particle.vel[0], particle.vel[1]);
                        let speed = vx.hypot(vy);
                        if speed < 1.0e-4 {
                            (size, size, particle.angle)
                        } else {
                            let stretch = (speed * system.trail.length)
                                .clamp(system.trail.min_length, system.trail.max_length);
                            (size, size * stretch * sample.ratio, (-vx).atan2(-vy))
                        }
                    } else {
                        (size, size, particle.angle)
                    };
                let tint = [
                    particle.color[0] * system.tint[0],
                    particle.color[1] * system.tint[1],
                    particle.color[2] * system.tint[2],
                    particle.alpha,
                ];
                let rect = [x, self.canvas.1 - y, width, height];
                let fade = sample.next.map_or(0.0, |(_, _, blend)| blend);
                out.push(OrderedParticleQuad {
                    scene_order: group.scene_order,
                    quad: vk::SceneQuad {
                        rect,
                        uv: sample.uv,
                        tint: [tint[0], tint[1], tint[2], tint[3] * (1.0 - fade)],
                        angle: angle + sample.extra,
                        texture: group.texture,
                        blend,
                    },
                });
                if let Some((uv, extra, mix)) = sample.next {
                    out.push(OrderedParticleQuad {
                        scene_order: group.scene_order,
                        quad: vk::SceneQuad {
                            rect,
                            uv,
                            tint: [tint[0], tint[1], tint[2], tint[3] * mix],
                            angle: angle + extra,
                            texture: group.texture,
                            blend,
                        },
                    });
                }
            }
        }
        (out, draws)
    }

    fn puppet_frame(&mut self, time: f32, batched: bool) -> Result<()> {
        for puppet in &mut self.puppets {
            let positions =
                paper_scene::puppet::skin(&puppet.puppet.mesh, &puppet.puppet.layers, time);
            self.renderer.update_scene_mesh(&puppet.mesh, &positions, puppet.puppet.size)?;
            let texture = self
                .textures
                .get(puppet.texture)
                .ok_or_else(|| anyhow!("puppet texture slot {} missing", puppet.texture))?;
            if batched {
                self.renderer.record_scene_mesh(&puppet.target, texture, &puppet.mesh);
            } else {
                self.renderer.render_scene_mesh(&puppet.target, texture, &puppet.mesh)?;
            }
        }
        Ok(())
    }

    fn compose(&mut self, time: f32, dt: f32) -> Result<()> {
        if self.frozen {
            return Ok(());
        }
        let logical_canvas = [self.canvas.0.max(1.0), self.canvas.1.max(1.0)];
        let clock = self.frame_clock(time, dt);
        let screen = (self.target.extent.width, self.target.extent.height);
        for video in &mut self.videos {
            video.advance(f64::from(time))?;
        }
        for (index, quad) in &self.passive_video_targets {
            self.renderer.render_scene(
                &self.passive_scene_targets[*index],
                [0.0; 4],
                std::slice::from_ref(quad),
                &self.textures,
            )?;
        }
        self.advance_layer_animations(time);
        self.advance_live_text(time)?;
        self.advance_audio(dt);
        let debug_fx = std::env::var("SKWD_VK_FX_DEBUG").is_ok();
        let batched = !debug_fx && (!self.fx.is_empty() || !self.puppets.is_empty());
        if batched {
            self.renderer.begin_scene_batch()?;
        }
        self.puppet_frame(time, batched)?;
        let (particle_quads, particle_draws) = self.particle_quads(time, dt);
        if self.fx.is_empty() {
            let clear = [self.clear[0], self.clear[1], self.clear[2], 1.0];
            let quads =
                ordered_scene_quads(&self.quads, &self.quad_scene_order, &particle_quads, None);
            let batches = particle_batches(
                &self.particles,
                &self.quad_scene_order,
                &particle_quads,
                &particle_draws,
                None,
                self.scene_grab.as_ref(),
            );
            if batched {
                self.renderer.record_scene_with_canvas(
                    &self.target,
                    logical_canvas,
                    clear,
                    &quads,
                    &self.textures,
                    &batches,
                );
                return self.renderer.submit_scene_batch();
            }
            return self.renderer.render_scene_with_canvas(
                &self.target,
                logical_canvas,
                clear,
                &quads,
                &self.textures,
                &batches,
            );
        }
        let ndc_quad = self.ndc_quad.as_ref().context("ndc quad missing")?;
        let ndc_quad_d3d = self.ndc_quad_d3d.as_ref().context("ndc quad missing")?;
        let fx_scratch = &self.fx_scratch;
        let mut scene_targets = HashMap::new();
        for layer in &self.base_scene_targets {
            let sample = (layer.view, layer.sampler, layer.extent);
            scene_targets.insert((layer.layer_id.clone(), CompositeBuffer::A), sample);
            scene_targets.insert((layer.layer_id.clone(), CompositeBuffer::B), sample);
        }
        for fx_index in 0..self.fx.len() {
            let (needs_snapshot, scene_order) = {
                let fx = &self.fx[fx_index];
                (fx.snapshot, fx.scene_order)
            };
            let snapshot = self
                .scene_snapshot
                .as_ref()
                .map(|target| (target.view, target.sampler, target.extent));
            if needs_snapshot {
                let target = self
                    .scene_snapshot
                    .as_ref()
                    .context("scene target plan requires a shadow snapshot")?;
                let prefix = ordered_scene_quads(
                    &self.quads,
                    &self.quad_scene_order,
                    &particle_quads,
                    Some(scene_order),
                );
                let batches = particle_batches(
                    &self.particles,
                    &self.quad_scene_order,
                    &particle_quads,
                    &particle_draws,
                    Some(scene_order),
                    self.scene_grab.as_ref(),
                );
                let clear = [self.clear[0], self.clear[1], self.clear[2], 1.0];
                if debug_fx {
                    self.renderer.render_scene_with_canvas(
                        target,
                        logical_canvas,
                        clear,
                        &prefix,
                        &self.textures,
                        &batches,
                    )?;
                } else {
                    self.renderer.record_scene_with_canvas(
                        target,
                        logical_canvas,
                        clear,
                        &prefix,
                        &self.textures,
                        &batches,
                    );
                }
            }
            let fx = &mut self.fx[fx_index];
            let ping = fx.ping.get(fx_scratch);
            let pong = fx.pong.get(fx_scratch);
            if let (Some(crop), Some(crop_quad)) = (&fx.crop, &fx.crop_quad) {
                let crop = crop.get(fx_scratch);
                let quad = std::slice::from_ref(crop_quad);
                if debug_fx {
                    self.renderer.render_scene(crop, [0.0; 4], quad, &self.textures)?;
                } else {
                    self.renderer.record_scene(crop, [0.0; 4], quad, &self.textures);
                }
            }
            if debug_fx {
                self.renderer.render_scene(
                    pong,
                    [0.0, 0.0, 0.0, 0.0],
                    std::slice::from_ref(&fx.base_quad),
                    &self.textures,
                )?;
            } else {
                self.renderer.record_scene(
                    pong,
                    [0.0, 0.0, 0.0, 0.0],
                    std::slice::from_ref(&fx.base_quad),
                    &self.textures,
                );
            }
            if debug_fx && let Ok((w, h, px)) = self.renderer.read_scene_target(pong) {
                let n = (px.len() / 4).max(1);
                let sum: u64 = px
                    .chunks_exact(4)
                    .map(|p| u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2]))
                    .sum();
                let mid = ((h / 2) * w + w / 2) as usize * 4;
                let alpha: u64 = px.chunks_exact(4).map(|p| u64::from(p[3])).sum();
                tracing::info!(
                    "FXDBG primed {w}x{h} mean={} a={} mid={:?}",
                    sum / (3 * n as u64),
                    alpha / n as u64,
                    &px[mid..mid + 4]
                );
            }
            let mut previous = (pong.view, pong.sampler, pong.extent);
            let mut source = previous;
            let mut source_id = FxTargetId::Pong;
            let mut flip = true;
            let mut current_effect = fx.owner.first().copied().unwrap_or(0);
            fx.apply_swaps(None);
            for slot in 0..fx.pipelines.len() {
                let pipe = &fx.pipelines[slot];
                if fx.owner.get(slot).copied().unwrap_or(0) != current_effect {
                    current_effect = fx.owner.get(slot).copied().unwrap_or(0);
                    previous = source;
                }
                let named_id = fx.pass_targets[slot]
                    .as_ref()
                    .and_then(|name| fx.resolve_fbo(fx.owner[slot], name))
                    .map(FxTargetId::Fbo);
                let named = named_id.map(|id| fx.target(id, fx_scratch));
                let mut views = vec![(source.0, source.1)];
                let mut sizes: Vec<Option<[f32; 4]>> = vec![Some(size4(source.2))];
                for &texture in &fx.inputs[slot] {
                    let texture = &self.textures[texture];
                    views.push((texture.view, texture.sampler));
                    sizes.push(None);
                }
                for (index, binding) in &fx.binds[slot] {
                    let bound = match binding {
                        EffectBind::Previous => Some(previous),
                        EffectBind::Named(name) => fx
                            .resolve_fbo(fx.owner[slot], name)
                            .map(|index| fx.target(FxTargetId::Fbo(index), fx_scratch))
                            .map(|rt| (rt.view, rt.sampler, rt.extent)),
                        EffectBind::LayerComposite { layer, buffer } => {
                            scene_targets.get(&(layer.clone(), *buffer)).copied()
                        }
                        EffectBind::SceneSoFar => snapshot,
                        EffectBind::SceneUnderLayer => fx.crop.as_ref().map(|storage| {
                            let crop = storage.get(fx_scratch);
                            (crop.view, crop.sampler, crop.extent)
                        }),
                    };
                    if let Some((view, sampler, extent)) = bound {
                        if *index >= views.len() {
                            views.resize(*index + 1, (view, sampler));
                            sizes.resize(*index + 1, None);
                        }
                        views[*index] = (view, sampler);
                        sizes[*index] = Some(size4(extent));
                    }
                }
                let target_id = match named_id {
                    Some(id) => id,
                    None => {
                        let candidate = if flip { FxTargetId::Ping } else { FxTargetId::Pong };
                        let candidate_target = fx.target(candidate, fx_scratch);
                        if views.iter().any(|(view, _)| *view == candidate_target.view) {
                            flip = !flip;
                            if flip { FxTargetId::Ping } else { FxTargetId::Pong }
                        } else {
                            candidate
                        }
                    }
                };
                let target = fx.target(target_id, fx_scratch);
                let meta = &fx.passes[slot];
                let quad_dims = named.is_none().then_some((ping.extent.width, ping.extent.height));
                let uniforms = meta.uniform_bytes_with(
                    clock,
                    quad_dims,
                    target.extent.width,
                    target.extent.height,
                    screen,
                    &sizes,
                    &fx.uniforms,
                    pipe.hlsl && vk::d3d_clip_enabled(),
                );
                if debug_fx {
                    let ids: Vec<String> = views
                        .iter()
                        .map(|(view, _)| {
                            if *view == ping.view {
                                "PING".into()
                            } else if *view == pong.view {
                                "PONG".into()
                            } else if let Some((name, _)) = fx
                                .fbos
                                .iter()
                                .find(|(_, storage)| storage.get(fx_scratch).view == *view)
                            {
                                name.clone()
                            } else {
                                "tex".into()
                            }
                        })
                        .collect();
                    let tgt = if target.view == ping.view {
                        "PING"
                    } else if target.view == pong.view {
                        "PONG"
                    } else {
                        "FBO"
                    };
                    tracing::info!(
                        "FXDBG pass{slot} {} hlsl={} views={ids:?} target={tgt}",
                        meta.name,
                        pipe.hlsl
                    );
                    if let Ok((_, _, px)) = self.renderer.read_scene_target(ping) {
                        let n = (px.len() / 4).max(1);
                        let alpha: u64 = px.chunks_exact(4).map(|p| u64::from(p[3])).sum();
                        tracing::info!("FXDBG   ping now a={}", alpha / n as u64);
                    }
                }
                let buffer = if named.is_none() && !meta.ndc() {
                    &fx.verts
                } else if pipe.hlsl && vk::d3d_clip_enabled() {
                    ndc_quad_d3d
                } else {
                    ndc_quad
                };
                if debug_fx {
                    self.renderer.run_effect_pass(pipe, buffer, target, &views, &uniforms)?;
                } else {
                    self.renderer.record_effect_pass(pipe, buffer, target, &views, &uniforms);
                }
                fx.apply_swaps(Some(slot));
                if debug_fx && let Ok((w, h, px)) = self.renderer.read_scene_target(target) {
                    let n = (px.len() / 4).max(1);
                    let sum: u64 = px
                        .chunks_exact(4)
                        .map(|p| u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2]))
                        .sum();
                    let mid = ((h / 2) * w + w / 2) as usize * 4;
                    let alpha: u64 = px.chunks_exact(4).map(|p| u64::from(p[3])).sum();
                    tracing::info!(
                        "FXDBG pass{slot} {w}x{h} mean={} a={} mid={:?}",
                        sum / (3 * n as u64),
                        alpha / n as u64,
                        &px[mid..mid + 4]
                    );
                }
                source = (target.view, target.sampler, target.extent);
                source_id = target_id;
                if named.is_none() {
                    flip = !flip;
                }
            }
            self.renderer.point_slot_at(&self.textures[fx.slot], source.0);
            self.quads[fx.quad].texture = fx.slot;
            fx.output = Some(source_id);
            for (buffer, target_id) in
                [(CompositeBuffer::A, FxTargetId::Ping), (CompositeBuffer::B, FxTargetId::Pong)]
            {
                let target = fx.target(target_id, fx_scratch);
                scene_targets.insert(
                    (fx.layer_id.clone(), buffer),
                    (target.view, target.sampler, target.extent),
                );
            }
        }
        let clear = [self.clear[0], self.clear[1], self.clear[2], 1.0];
        let quads = ordered_scene_quads(&self.quads, &self.quad_scene_order, &particle_quads, None);
        let batches = particle_batches(
            &self.particles,
            &self.quad_scene_order,
            &particle_quads,
            &particle_draws,
            None,
            self.scene_grab.as_ref(),
        );
        if debug_fx {
            for (i, q) in quads.iter().enumerate().take(6) {
                tracing::info!(
                    "QUAD{i} rect={:?} uv={:?} tint={:?} angle={} tex={} add={}",
                    q.rect,
                    q.uv,
                    q.tint,
                    q.angle,
                    q.texture,
                    q.blend == vk::SceneBlend::Add
                );
            }
            tracing::info!("textures={} quads={}", self.textures.len(), quads.len());
            self.renderer.render_scene_with_canvas(
                &self.target,
                logical_canvas,
                clear,
                &quads,
                &self.textures,
                &batches,
            )
        } else {
            self.renderer.record_scene_with_canvas(
                &self.target,
                logical_canvas,
                clear,
                &quads,
                &self.textures,
                &batches,
            );
            self.renderer.submit_scene_batch()
        }
    }

    fn drop_from(&mut self) {
        if let Some(source) = self.from.take() {
            match source.source {
                FadeFrom::Nv12(up) => self.renderer.destroy_upload_path(up),
                FadeFrom::Scene(rt) => self.renderer.destroy_scene_target(rt),
            }
        }
    }

    fn destroy_layer_fx(&self, fx: LayerFx) {
        for pipe in fx.pipelines {
            self.renderer.destroy_effect_pipeline(pipe);
        }
        for storage in [fx.ping, fx.pong]
            .into_iter()
            .chain(fx.crop)
            .chain(fx.fbos.into_iter().map(|(_, storage)| storage))
        {
            if let FxTargetStorage::Owned(target) = storage {
                self.renderer.destroy_scene_target(target);
            }
        }
        self.renderer.destroy_quad_buffer(fx.verts);
    }

    fn retain_layer_fx_output(&self, mut fx: LayerFx) -> vk::SceneTarget {
        let output = fx.output.expect("effect output validated before baking");
        for pipe in std::mem::take(&mut fx.pipelines) {
            self.renderer.destroy_effect_pipeline(pipe);
        }
        let retained = fx
            .target_storage_mut(output)
            .take_owned()
            .expect("static effect output must still own its target");
        self.destroy_layer_fx(fx);
        retained
    }

    fn bake_static_effects(&mut self) -> Result<EffectBakeReport> {
        for fx in &self.fx {
            fx.target_bytes().context("effect output missing before static bake")?;
        }

        let old_fx = std::mem::take(&mut self.fx);
        let mut live_fx = Vec::with_capacity(old_fx.len());
        let mut report = EffectBakeReport::default();
        for fx in old_fx {
            let bytes = fx.target_bytes().expect("effect output validated");
            if fx.passes.iter().any(paper_scene::effects::PassMeta::time_dependent)
                || fx.frame_state_dependent()
                || fx.retain_targets
            {
                report.retained_bytes += bytes.retained;
                report.transient_bytes += bytes.transient;
                live_fx.push(fx);
            } else {
                report.baked_layers += 1;
                report.retained_bytes += bytes.retained;
                report.released_transient_bytes += bytes.transient;
                let output = self.retain_layer_fx_output(fx);
                self.baked_fx_outputs.push(output);
            }
        }
        self.fx = live_fx;
        if self.fx.is_empty() {
            if let Some(quad) = self.ndc_quad.take() {
                self.renderer.destroy_quad_buffer(quad);
            }
            if let Some(quad) = self.ndc_quad_d3d.take() {
                self.renderer.destroy_quad_buffer(quad);
            }
        }
        Ok(report)
    }

    fn release_unused_scene_snapshot(&mut self) {
        if self.fx.iter().any(|fx| fx.snapshot) {
            return;
        }
        if let Some(target) = self.scene_snapshot.take() {
            self.renderer.destroy_scene_target(target);
        }
    }

    fn alias_dynamic_effect_scratch(&mut self) -> EffectAliasReport {
        debug_assert!(self.fx_scratch.is_empty());

        let mut report = EffectAliasReport::default();
        let mut plans: Vec<Option<TargetPlan>> = Vec::with_capacity(self.fx.len());
        let mut candidates: Vec<Vec<(FxTargetId, FxTargetClass)>> =
            Vec::with_capacity(self.fx.len());
        for (layer, fx) in self.fx.iter().enumerate() {
            let plan = match fx.plan() {
                Ok(plan) if Some(plan.output) == fx.output => plan,
                Ok(plan) => {
                    report.fallback_layers += 1;
                    tracing::warn!(
                        layer,
                        runtime_output = ?fx.output,
                        planned_output = ?plan.output,
                        "skwd-wall-vk: effect scratch alias plan disagreed with runtime routing"
                    );
                    plans.push(None);
                    candidates.push(Vec::new());
                    continue;
                }
                Err(error) => {
                    report.fallback_layers += 1;
                    tracing::warn!(
                        layer,
                        ?error,
                        "skwd-wall-vk: effect scratch alias plan rejected"
                    );
                    plans.push(None);
                    candidates.push(Vec::new());
                    continue;
                }
            };
            report.planned_layers += 1;
            let layer_candidates = fx
                .target_ids()
                .filter(|target| scratch_candidate(&plan, &fx.sampled_targets, *target))
                .map(|target| {
                    let class = FxTargetClass::of(
                        fx.target_storage(target)
                            .owned()
                            .expect("scratch aliasing runs before target sharing"),
                    );
                    (target, class)
                })
                .collect();
            plans.push(Some(plan));
            candidates.push(layer_candidates);
        }

        let classes: Vec<Vec<FxTargetClass>> = candidates
            .iter()
            .map(|layer| layer.iter().map(|(_, class)| *class).collect())
            .collect();
        let layout = plan_scratch_layout(&classes);
        report.logical_scratch_targets = candidates.iter().map(Vec::len).sum();
        report.physical_scratch_targets = layout.physical_classes.len();

        let mut physical: Vec<Option<vk::SceneTarget>> =
            (0..layout.physical_classes.len()).map(|_| None).collect();
        for (layer, (layer_candidates, assignments)) in
            candidates.into_iter().zip(layout.assignments).enumerate()
        {
            if plans[layer].is_none() {
                continue;
            }
            for ((target_id, class), slot) in layer_candidates.into_iter().zip(assignments) {
                debug_assert_eq!(layout.physical_classes[slot], class);
                let target = self.fx[layer]
                    .target_storage_mut(target_id)
                    .take_owned()
                    .expect("scratch candidate owns its image");
                if physical[slot].is_none() {
                    report.scratch_bytes += target.allocation_bytes;
                    physical[slot] = Some(target);
                } else {
                    report.released_bytes += target.allocation_bytes;
                    self.renderer.destroy_scene_target(target);
                }
                *self.fx[layer].target_storage_mut(target_id) = FxTargetStorage::Shared(slot);
            }
        }
        self.fx_scratch =
            physical.into_iter().map(|target| target.expect("scratch slot populated")).collect();
        report
    }

    fn effect_target_allocation_bytes(&self) -> u64 {
        let owned = self
            .fx
            .iter()
            .map(|fx| {
                fx.target_ids()
                    .filter_map(|target| fx.target_storage(target).owned())
                    .map(|target| target.allocation_bytes)
                    .sum::<u64>()
            })
            .sum::<u64>();
        owned
            + self.baked_fx_outputs.iter().map(|target| target.allocation_bytes).sum::<u64>()
            + self.fx_scratch.iter().map(|target| target.allocation_bytes).sum::<u64>()
    }

    fn release_composition_inputs(&mut self) {
        for fx in std::mem::take(&mut self.fx) {
            self.destroy_layer_fx(fx);
        }
        for target in std::mem::take(&mut self.baked_fx_outputs) {
            self.renderer.destroy_scene_target(target);
        }
        for puppet in std::mem::take(&mut self.puppets) {
            self.renderer.destroy_scene_mesh(puppet.mesh);
            self.renderer.destroy_scene_target(puppet.target);
        }
        for target in std::mem::take(&mut self.fx_scratch) {
            self.renderer.destroy_scene_target(target);
        }
        for target in std::mem::take(&mut self.passive_scene_targets) {
            self.renderer.destroy_scene_target(target);
        }
        self.base_scene_targets.clear();
        if let Some(quad) = self.ndc_quad.take() {
            self.renderer.destroy_quad_buffer(quad);
        }
        if let Some(quad) = self.ndc_quad_d3d.take() {
            self.renderer.destroy_quad_buffer(quad);
        }
        for texture in std::mem::take(&mut self.textures) {
            self.renderer.destroy_scene_texture(texture);
        }
        self.videos.clear();
        self.passive_video_targets.clear();
        self.quads.clear();
        self.quad_scene_order.clear();
        self.particles.clear();
        if let Some(target) = self.scene_snapshot.take() {
            self.renderer.destroy_scene_target(target);
        }
        if let Some(target) = self.scene_grab.take() {
            self.renderer.destroy_scene_target(target);
        }
    }

    fn destroy(mut self) {
        self.drop_from();
        self.release_composition_inputs();
        for group in std::mem::take(&mut self.particles) {
            if let Some(engine) = group.engine {
                self.renderer.destroy_effect_pipeline(engine.pipeline);
                self.renderer.destroy_dyn_buffer(engine.vertices);
                self.renderer.destroy_dyn_buffer(engine.indices);
            }
        }
        self.renderer.destroy_scene_target(self.target);
    }
}

fn quads(model: &SceneModel, layer_slots: &[usize]) -> Vec<vk::SceneQuad> {
    model
        .layers
        .iter()
        .enumerate()
        .map(|(idx, layer)| {
            let (uvw, uvh) = layer.texture.uv_scale();
            vk::SceneQuad {
                rect: [layer.center.0, layer.center.1, layer.size.0, layer.size.1],
                uv: [0.0, 0.0, uvw, uvh],
                tint: if !layer.passthrough && !layer.effects.is_empty() {
                    [1.0, 1.0, 1.0, if layer.visible { 1.0 } else { 0.0 }]
                } else {
                    layer_tint(layer)
                },
                angle: layer.angle,
                texture: layer_slots[idx],
                blend: match layer.color_blend {
                    6 => vk::SceneBlend::Screen,
                    8 => vk::SceneBlend::Add,
                    _ => vk::SceneBlend::Alpha,
                },
            }
        })
        .collect()
}

fn audio_enabled() -> bool {
    std::env::var("SKWD_VK_AUDIO").as_deref() != Ok("0")
}

fn start_audio(audio_fx: &[usize]) -> Option<paper_audio::Analyser> {
    if audio_fx.is_empty() {
        return None;
    }
    if !audio_enabled() {
        tracing::info!(
            "skwd-wall-vk: audio response disabled, {} layer(s) stay silent",
            audio_fx.len()
        );
        return None;
    }
    match paper_audio::Analyser::start() {
        Ok(analyser) => Some(analyser),
        Err(error) => {
            tracing::warn!("skwd-wall-vk: audio response unavailable: {error:#}");
            None
        }
    }
}

const AUDIO_LANES: [(usize, bool, &str); 6] = [
    (16, false, "g_AudioSpectrum16Left"),
    (16, true, "g_AudioSpectrum16Right"),
    (32, false, "g_AudioSpectrum32Left"),
    (32, true, "g_AudioSpectrum32Right"),
    (64, false, "g_AudioSpectrum64Left"),
    (64, true, "g_AudioSpectrum64Right"),
];

fn seed_bands(target: &mut std::collections::BTreeMap<String, Vec<f32>>) {
    for (count, _, name) in AUDIO_LANES {
        target.entry(name.to_string()).or_insert_with(|| vec![0.0; count]);
    }
}

fn write_bands(
    target: &mut std::collections::BTreeMap<String, Vec<f32>>,
    bands: &paper_audio::Bands,
) {
    for (count, right, name) in AUDIO_LANES {
        let (Some(values), Some(lane)) = (bands.slice(count, right), target.get_mut(name)) else {
            continue;
        };
        lane.clear();
        lane.extend_from_slice(values);
    }
}

fn ordered_scene_quads(
    image_quads: &[vk::SceneQuad],
    image_order: &[usize],
    particle_quads: &[OrderedParticleQuad],
    before: Option<usize>,
) -> Vec<vk::SceneQuad> {
    let mut ordered: Vec<(usize, usize, vk::SceneQuad)> = image_quads
        .iter()
        .zip(image_order)
        .enumerate()
        .filter(|(_, (_, order))| before.is_none_or(|limit| **order < limit))
        .map(|(serial, (quad, order))| (*order, serial, quad.clone()))
        .collect();
    let offset = ordered.len();
    ordered.extend(
        particle_quads
            .iter()
            .enumerate()
            .filter(|(_, entry)| before.is_none_or(|limit| entry.scene_order < limit))
            .map(|(serial, entry)| (entry.scene_order, offset + serial, entry.quad.clone())),
    );
    ordered.sort_by_key(|(order, serial, _)| (*order, *serial));
    ordered.into_iter().map(|(_, _, quad)| quad).collect()
}

fn layer_tint(layer: &paper_scene::model::Layer) -> [f32; 4] {
    [layer.color[0], layer.color[1], layer.color[2], if layer.visible { layer.alpha } else { 0.0 }]
}

fn fallback_tint(layer: &paper_scene::model::Layer) -> [f32; 4] {
    if layer.passthrough || (layer.solid && !layer.effects.is_empty()) {
        [1.0, 1.0, 1.0, 0.0]
    } else {
        layer_tint(layer)
    }
}

fn passthrough_uv(layer: &paper_scene::model::Layer, canvas: (f32, f32)) -> [f32; 4] {
    let (cw, ch) = (canvas.0.max(1.0), canvas.1.max(1.0));
    [
        (layer.center.0 - layer.size.0 * 0.5) / cw,
        (layer.center.1 - layer.size.1 * 0.5) / ch,
        layer.size.0 / cw,
        layer.size.1 / ch,
    ]
}

fn passive_target_quad(
    layer: &paper_scene::model::Layer,
    texture: usize,
    extent: ash::vk::Extent2D,
) -> vk::SceneQuad {
    let (uvw, uvh) = layer.texture.uv_scale();
    vk::SceneQuad {
        rect: [
            extent.width as f32 * 0.5,
            extent.height as f32 * 0.5,
            extent.width as f32,
            extent.height as f32,
        ],
        uv: [0.0, 0.0, uvw, uvh],
        tint: [1.0; 4],
        angle: 0.0,
        texture,
        blend: vk::SceneBlend::Copy,
    }
}

fn passive_provider_indices(
    layers: &[paper_scene::model::Layer],
    active_layers: &BTreeSet<usize>,
    referenced: &BTreeSet<String>,
) -> Vec<usize> {
    layers
        .iter()
        .enumerate()
        .filter(|(index, layer)| !active_layers.contains(index) && referenced.contains(&layer.id))
        .map(|(index, _)| index)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SceneDimensions {
    /// Wallpaper Engine coordinates used by layer and particle geometry.
    logical: [f32; 2],
    /// Physical pixels used by the bounded Vulkan composition target.
    raster: (u32, u32),
}

fn scene_dimensions_for(canvas: (f32, f32), limit: u32) -> SceneDimensions {
    scene_dimensions_for_outputs(canvas, limit, &[], FillMode::Center)
}

fn scene_dimensions_for_outputs(
    canvas: (f32, f32),
    limit: u32,
    outputs: &[(u32, u32)],
    mode: FillMode,
) -> SceneDimensions {
    let logical = [canvas.0.max(1.0), canvas.1.max(1.0)];
    let limit = limit.max(256);
    let raw_w = logical[0].round();
    let raw_h = logical[1].round();
    if mode == FillMode::Stretch {
        let output_w = outputs
            .iter()
            .filter(|(width, height)| *width > 0 && *height > 0)
            .map(|(width, _)| *width)
            .max();
        let output_h = outputs
            .iter()
            .filter(|(width, height)| *width > 0 && *height > 0)
            .map(|(_, height)| *height)
            .max();
        let raster_w = output_w
            .map_or(f64::from(raw_w), f64::from)
            .min(f64::from(raw_w))
            .min(f64::from(limit));
        let raster_h = output_h
            .map_or(f64::from(raw_h), f64::from)
            .min(f64::from(raw_h))
            .min(f64::from(limit));
        return SceneDimensions {
            logical,
            raster: ((raster_w.round() as u32).max(1), (raster_h.round() as u32).max(1)),
        };
    }
    let output_scale = match mode {
        FillMode::Center | FillMode::Tile => 1.0,
        FillMode::Stretch => unreachable!("stretch dimensions returned above"),
        FillMode::Fit => outputs
            .iter()
            .filter(|(width, height)| *width > 0 && *height > 0)
            .map(|(width, height)| {
                (f64::from(*width) / f64::from(raw_w)).min(f64::from(*height) / f64::from(raw_h))
            })
            .reduce(f64::max)
            .unwrap_or(1.0),
        FillMode::Fill | FillMode::Span => outputs
            .iter()
            .filter(|(width, height)| *width > 0 && *height > 0)
            .map(|(width, height)| {
                (f64::from(*width) / f64::from(raw_w)).max(f64::from(*height) / f64::from(raw_h))
            })
            .reduce(f64::max)
            .unwrap_or(1.0),
    };
    let shrink =
        (f64::from(limit) / f64::from(raw_w).max(f64::from(raw_h))).min(output_scale).min(1.0);
    let raster_w = ((f64::from(raw_w) * shrink).round() as u32).max(1);
    let raster_h = ((f64::from(raw_h) * shrink).round() as u32).max(1);
    SceneDimensions { logical, raster: (raster_w, raster_h) }
}

fn layer_effect_dimensions(
    layer: &paper_scene::model::Layer,
    dimensions: SceneDimensions,
) -> (u32, u32) {
    if layer.is_text {
        return bounded_effect_dimensions(
            f64::from(layer.texture.width),
            f64::from(layer.texture.height),
        );
    }
    effect_dimensions_for(layer.size, dimensions)
}

fn effect_dimensions_for(size: (f32, f32), dimensions: SceneDimensions) -> (u32, u32) {
    let scale_x = f64::from(dimensions.raster.0) / f64::from(dimensions.logical[0]);
    let scale_y = f64::from(dimensions.raster.1) / f64::from(dimensions.logical[1]);
    let raw_w = (f64::from(size.0.abs()) * scale_x).max(1.0);
    let raw_h = (f64::from(size.1.abs()) * scale_y).max(1.0);
    bounded_effect_dimensions(raw_w, raw_h)
}

fn bounded_effect_dimensions(raw_w: f64, raw_h: f64) -> (u32, u32) {
    let shrink = (2048.0 / raw_w.max(raw_h)).min(1.0);
    let width = ((raw_w * shrink).round() as u32).clamp(16, 2048);
    let height = ((raw_h * shrink).round() as u32).clamp(16, 2048);
    (width, height)
}

fn scene_dimensions(model: &SceneModel, outputs: &[(u32, u32)], mode: FillMode) -> SceneDimensions {
    let limit = std::env::var("SKWD_VK_SCENE_MAX")
        .ok()
        .and_then(|text| text.parse::<u32>().ok())
        .unwrap_or(4096)
        .max(256);
    let dimensions = scene_dimensions_for_outputs(model.canvas, limit, outputs, mode);
    if dimensions.raster.0 < dimensions.logical[0].round() as u32
        || dimensions.raster.1 < dimensions.logical[1].round() as u32
    {
        tracing::info!(
            "skwd-wall-vk: logical scene canvas {}x{} rasterized at {}x{}",
            dimensions.logical[0],
            dimensions.logical[1],
            dimensions.raster.0,
            dimensions.raster.1,
        );
    }
    dimensions
}

fn shared_renderer(sd: &shared::SharedDevice, width: u32, height: u32) -> Result<vk::Renderer> {
    vk::Renderer::new_shared_headless(
        (
            sd.entry.clone(),
            sd.instance.clone(),
            sd.phys,
            sd.device.clone(),
            sd.gfx_family,
            sd.queue,
        ),
        width,
        height,
    )
}

fn release_model_texture_payloads(model: &mut SceneModel) -> usize {
    let mut released = 0;
    for layer in &mut model.layers {
        released += std::mem::take(&mut layer.texture.pixels).bytes();
        released += layer.texture.video.take().map_or(0, |payload| payload.len());
        for effect in &mut layer.effects {
            for pass in &mut effect.passes {
                for texture in pass.textures.iter_mut().flatten() {
                    released += std::mem::take(&mut texture.pixels).bytes();
                    released += texture.video.take().map_or(0, |payload| payload.len());
                }
            }
        }
    }
    released
}

fn strict_scene_startup() -> bool {
    std::env::var_os("SKWD_VK_SCENE_STRICT").is_some_and(|value| value != "0")
}

fn validate_native_compatibility(pkg: &paper_scene::pkg::Package, strict: bool) -> Result<()> {
    let features = match paper_scene::scene::extract(pkg) {
        Ok(features) => features,
        Err(error) if strict => {
            return Err(anyhow!("[native-scene-gap:scene-analysis] {error}"));
        }
        Err(error) => {
            tracing::warn!(
                code = "scene-analysis",
                detail = %error,
                "skwd-wall-vk: native scene fidelity gap"
            );
            return Ok(());
        }
    };
    let compatibility = paper_scene::capability::assess_native(&features);
    for gap in &compatibility.gaps {
        tracing::warn!(
            code = gap.code(),
            detail = %gap.description(),
            "skwd-wall-vk: native scene fidelity gap"
        );
    }
    if strict && !compatibility.full_fidelity() {
        let details = compatibility
            .gaps
            .iter()
            .map(|gap| format!("[native-scene-gap:{}] {}", gap.code(), gap.description()))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(anyhow!("strict native scene startup rejected compatibility gaps: {details}"));
    }
    Ok(())
}

fn validate_scene_skips(strict: bool, skipped: &[String]) -> Result<()> {
    if !strict || skipped.is_empty() {
        return Ok(());
    }
    let details = skipped.iter().take(4).cloned().collect::<Vec<_>>().join("; ");
    Err(anyhow!(
        "strict native scene startup rejected {} skipped element(s): {details}",
        skipped.len()
    ))
}

fn validate_pipeline_skip(strict: bool, layer: &str, reason: &str) -> Result<()> {
    if strict {
        Err(anyhow!("strict native scene startup rejected effect pipeline on {layer}: {reason}"))
    } else {
        Ok(())
    }
}

fn create_puppets(
    renderer: &mut vk::Renderer,
    model: &mut SceneModel,
    layer_slots: &mut [usize],
    textures: &mut Vec<vk::SceneTexture>,
    dimensions: SceneDimensions,
) -> Result<Vec<PuppetGroup>> {
    let mut outputs = Vec::new();
    for (index, layer) in model.layers.iter_mut().enumerate() {
        if !layer.visible {
            continue;
        }
        let Some(puppet) = layer.puppet.take() else {
            continue;
        };
        let (width, height) = layer_effect_dimensions(layer, dimensions);
        let texture = layer_slots[index];
        if textures.get(texture).is_none() {
            return Err(anyhow!("puppet atlas slot missing for {}", layer.name));
        }
        let mesh = renderer
            .create_scene_mesh(&puppet.mesh, puppet.size)
            .with_context(|| format!("puppet mesh for {}", layer.name))?;
        let output = if layer.texture.clamp {
            renderer.create_scene_target(width, height)
        } else {
            renderer.create_scene_target_repeat(width, height)
        };
        let output = match output {
            Ok(output) => output,
            Err(err) => {
                renderer.destroy_scene_mesh(mesh);
                return Err(err).with_context(|| format!("puppet target for {}", layer.name));
            }
        };
        let slot = match renderer.create_view_slot() {
            Ok(slot) => slot,
            Err(err) => {
                renderer.destroy_scene_target(output);
                return Err(err).with_context(|| format!("puppet slot for {}", layer.name));
            }
        };
        renderer.point_slot_at(&slot, output.view);
        layer_slots[index] = textures.len();
        textures.push(slot);
        layer.texture.img_width = layer.texture.width;
        layer.texture.img_height = layer.texture.height;
        tracing::info!(
            layer = %layer.name,
            vertices = puppet.mesh.vertices.len(),
            indices = puppet.mesh.indices.len(),
            "skwd-wall-vk: assembled puppet mesh"
        );
        outputs.push(PuppetGroup { puppet, mesh, target: output, texture, layer: index });
    }
    Ok(outputs)
}

fn build_group(
    sd: &shared::SharedDevice,
    model: &mut SceneModel,
    strict: bool,
    outputs: &[(u32, u32)],
    mode: FillMode,
) -> Result<Group> {
    let dimensions = scene_dimensions(model, outputs, mode);
    let (canvas_w, canvas_h) = dimensions.raster;
    let mut renderer =
        shared_renderer(sd, canvas_w, canvas_h).context("shared scene composition renderer")?;
    let fx_limit = std::env::var("SKWD_VK_SCENE_FX")
        .ok()
        .and_then(|text| text.parse::<usize>().ok())
        .unwrap_or(8);
    let pass_cap = std::env::var("SKWD_VK_FX_PASSES")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(usize::MAX);
    let fx_layers =
        model.layers.iter().filter(|layer| layer.visible && !layer.effects.is_empty()).count();
    let fx_textures: usize = model
        .layers
        .iter()
        .filter(|layer| layer.visible)
        .flat_map(|layer| &layer.effects)
        .flat_map(|effect| &effect.passes)
        .map(|pass| {
            pass.textures.len().max(pass.fragment.samplers.len().max(pass.vertex.samplers.len()))
        })
        .sum();
    let fx_fbos: usize = model
        .layers
        .iter()
        .filter(|layer| layer.visible)
        .flat_map(|layer| &layer.effects)
        .map(|effect| effect.fbos.len())
        .sum();
    let sets =
        model.layers.len() + fx_layers * 4 + fx_textures + fx_fbos + model.particles.len() + 16;
    renderer.ensure_scene_pool(sets.max(1) as u32)?;
    let target = renderer.create_scene_target(canvas_w, canvas_h).context("scene target")?;
    let mut textures = Vec::with_capacity(model.layers.len());
    let mut texture_interner = TextureInterner {
        device: sd,
        videos: Vec::new(),
        video_slots: HashMap::new(),
        slots: HashMap::new(),
        reused: 0,
        reused_bytes: 0,
    };
    let mut layer_slots = Vec::with_capacity(model.layers.len());
    let mut hidden_target_texture = None;
    for layer in &mut model.layers {
        if !layer.visible {
            let slot = if let Some(slot) = hidden_target_texture {
                slot
            } else {
                let slot = texture_interner
                    .intern_rgba(&mut renderer, &mut textures, TextureKey::clear())
                    .context("transparent hidden-layer render target")?;
                hidden_target_texture = Some(slot);
                slot
            };
            layer_slots.push(slot);
            continue;
        }
        layer_slots.push(
            texture_interner
                .intern_texture(&mut renderer, &mut textures, &mut layer.texture)
                .with_context(|| format!("texture for layer {}", layer.name))?,
        );
    }
    let puppets =
        create_puppets(&mut renderer, model, &mut layer_slots, &mut textures, dimensions)?;
    let mut quads = quads(model, &layer_slots);
    let animations: Vec<LayerAnimation> = model
        .layers
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| {
            let frames = layer.texture.atlas_frames()?.to_vec();
            let total = frames.iter().map(|frame| frame.time).sum();
            Some(LayerAnimation { quad: index, frames, total })
        })
        .collect();
    for animation in &animations {
        quads[animation.quad].uv = animation.frames[0].uv;
    }
    if !animations.is_empty() {
        tracing::info!("skwd-wall-vk: animated image layers {}", animations.len());
    }
    let quad_scene_order = model.layers.iter().map(|layer| layer.scene_order).collect();
    let mut fx = Vec::new();
    let canvas = model.canvas;
    let mut dynamic_chains = 0usize;
    let mut admitted_bytes = 0u64;
    let byte_limit = std::env::var("SKWD_VK_SCENE_FX_BYTES")
        .ok()
        .and_then(|text| text.parse::<u64>().ok())
        .unwrap_or(512)
        .saturating_mul(1024 * 1024);
    let scene_lights = (model.ambient, model.skylight);
    for (index, layer) in model.layers.iter_mut().enumerate() {
        if !layer.visible || layer.effects.is_empty() {
            continue;
        }

        let (fx_w, fx_h) = layer_effect_dimensions(layer, dimensions);
        let mut pipelines = Vec::new();
        let mut inputs = Vec::new();
        let mut metas = Vec::new();
        let mut pass_targets = Vec::new();
        let mut pass_binds = Vec::new();
        let mut pass_owner: Vec<usize> = Vec::new();
        let mut fbos: Vec<(String, vk::SceneTarget)> = Vec::new();
        let mut fbo_owners: Vec<Option<usize>> = Vec::new();
        let mut swaps: Vec<(Option<usize>, usize, usize)> = Vec::new();
        let mut failed = None;
        for (effect_index, effect) in layer.effects.iter_mut().enumerate() {
            if pipelines.len() >= pass_cap {
                break;
            }
            for fbo in &effect.fbos {
                let owner = fbo.unique.then_some(effect_index);
                if fbos
                    .iter()
                    .zip(&fbo_owners)
                    .any(|((existing, _), known)| existing == &fbo.name && *known == owner)
                {
                    continue;
                }
                let (sw, sh) = fbo.extent((fx_w, fx_h));
                let repeat = fbo.repeat.unwrap_or(!layer.texture.clamp);
                match renderer.create_scene_target_fmt(sw, sh, repeat, vk_format(fbo.format)) {
                    Ok(rt) => {
                        if let Err(err) = renderer.render_scene(&rt, fbo.clear, &[], &[]) {
                            failed = Some(format!("{}: {err:#}", fbo.name));
                            renderer.destroy_scene_target(rt);
                            break;
                        }
                        fbos.push((fbo.name.clone(), rt));
                        fbo_owners.push(owner);
                    }
                    Err(err) => {
                        failed = Some(format!("{}: {err:#}", fbo.name));
                        break;
                    }
                }
            }
            if failed.is_some() {
                break;
            }
            let pass_offset = pipelines.len();
            for swap in &effect.swaps {
                let resolve = |name: &str| {
                    lifetime::resolve_fbo_scoped(
                        fbos.iter().map(|(fbo, _)| fbo.as_str()),
                        &fbo_owners,
                        effect_index,
                        name,
                    )
                };
                if let (Some(a), Some(b)) = (resolve(&swap.source), resolve(&swap.target)) {
                    let after = match swap.after_pass {
                        Some(local) => Some(pass_offset + local),
                        None => pass_offset.checked_sub(1),
                    };
                    swaps.push((after, a, b));
                }
            }
            for pass in &mut effect.passes {
                if pipelines.len() >= pass_cap {
                    break;
                }
                let meta = paper_scene::effects::PassMeta::of(pass);
                let write_format = pass
                    .target
                    .as_deref()
                    .and_then(|name| {
                        lifetime::resolve_fbo_scoped(
                            fbos.iter().map(|(fbo, _)| fbo.as_str()),
                            &fbo_owners,
                            effect_index,
                            name,
                        )
                    })
                    .map_or(ash::vk::Format::R8G8B8A8_UNORM, |index| fbos[index].1.format);
                match renderer.create_effect_pipeline_for(
                    &pass.vertex,
                    &pass.fragment,
                    pass.hlsl.as_ref(),
                    &pass.name,
                    write_format,
                ) {
                    Ok(pipe) => {
                        let mut slot_textures = Vec::new();
                        let wanted = pipe.sampler_count.saturating_sub(1) as usize;
                        for slot in 1..pass.textures.len().max(wanted + 1) {
                            let uploaded = match pass
                                .textures
                                .get_mut(slot)
                                .and_then(Option::as_mut)
                            {
                                Some(texture) => texture_interner
                                    .intern_texture(&mut renderer, &mut textures, texture)
                                    .map_err(|err| {
                                        tracing::warn!(
                                            "skwd-wall-vk: effect texture upload failed: {err:#}"
                                        );
                                    })
                                    .ok(),
                                None => None,
                            };
                            let uploaded = match uploaded {
                                Some(texture) => Some(texture),
                                None if slot <= wanted => texture_interner
                                    .intern_rgba(&mut renderer, &mut textures, TextureKey::clear())
                                    .ok(),
                                None => None,
                            };
                            let Some(uploaded) = uploaded else {
                                break;
                            };
                            slot_textures.push(uploaded);
                        }
                        metas.push(meta);
                        inputs.push(slot_textures);
                        pass_targets.push(pass.target.clone());
                        pass_binds.push(pass.binds.clone());
                        pass_owner.push(effect_index);
                        pipelines.push(pipe);
                    }
                    Err(err) => {
                        failed = Some(format!("{}: {err:#}", pass.name));
                        break;
                    }
                }
            }
            if failed.is_some() {
                break;
            }
        }
        if let Some(reason) = failed {
            tracing::info!("skwd-wall-vk: effect skipped on {} ({reason})", layer.name);
            for pipe in pipelines {
                renderer.destroy_effect_pipeline(pipe);
            }
            for (_, rt) in fbos {
                renderer.destroy_scene_target(rt);
            }
            quads[index].tint = fallback_tint(layer);
            validate_pipeline_skip(strict, &layer.name, &reason)?;
            continue;
        }
        if pipelines.is_empty() {
            quads[index].tint = fallback_tint(layer);
            continue;
        }
        let intrinsic_dynamic = metas.iter().any(paper_scene::effects::PassMeta::time_dependent)
            || lifetime::plan_targets(
                &fbos.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
                &pass_targets,
                &pass_binds,
                &pass_owner,
            )
            .map(|plan| plan.lifetimes.iter().any(|lifetime| lifetime.loop_carried))
            .unwrap_or(true);
        let chain_bytes = u64::from(fx_w) * u64::from(fx_h) * 4 * 2
            + fbos.iter().map(|(_, target)| target.allocation_bytes).sum::<u64>();
        let over_dynamic_cap = intrinsic_dynamic && dynamic_chains >= fx_limit;
        let over_bytes = admitted_bytes + chain_bytes > byte_limit;
        if over_dynamic_cap || over_bytes {
            tracing::info!(
                "skwd-wall-vk: effect chain dropped on {} (dynamic={} chains={} bytes={} budget={})",
                layer.name,
                intrinsic_dynamic,
                dynamic_chains,
                admitted_bytes + chain_bytes,
                byte_limit
            );
            for pipe in pipelines {
                renderer.destroy_effect_pipeline(pipe);
            }
            for (_, rt) in fbos {
                renderer.destroy_scene_target(rt);
            }
            quads[index].tint = fallback_tint(layer);
            continue;
        }
        if intrinsic_dynamic {
            dynamic_chains += 1;
        }
        admitted_bytes += chain_bytes;
        let verts = renderer.create_quad_buffer(fx_w, fx_h)?;
        let (ping, pong) = if layer.texture.clamp {
            (renderer.create_scene_target(fx_w, fx_h)?, renderer.create_scene_target(fx_w, fx_h)?)
        } else {
            (
                renderer.create_scene_target_repeat(fx_w, fx_h)?,
                renderer.create_scene_target_repeat(fx_w, fx_h)?,
            )
        };
        renderer.render_scene(&ping, [0.0; 4], &[], &[])?;
        let (uvw, uvh) = layer.texture.uv_scale();
        let needs_crop = pass_binds
            .iter()
            .flatten()
            .any(|(_, bind)| matches!(bind, EffectBind::SceneUnderLayer));
        let snapshot_slot = if layer.passthrough || needs_crop {
            let slot_texture = renderer.create_view_slot()?;
            textures.push(slot_texture);
            Some(textures.len() - 1)
        } else {
            None
        };
        let atlas = layer.texture.atlas_frames().map(|frames| frames[0].uv);
        let (base_texture, base_uv) = match snapshot_slot {
            Some(slot) if layer.passthrough => (slot, passthrough_uv(layer, canvas)),
            _ => (layer_slots[index], atlas.unwrap_or([0.0, 0.0, uvw, uvh])),
        };
        let full_rect = [fx_w as f32 / 2.0, fx_h as f32 / 2.0, fx_w as f32, fx_h as f32];
        let base_quad = vk::SceneQuad {
            rect: full_rect,
            uv: base_uv,
            tint: if layer.passthrough {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [layer.color[0], layer.color[1], layer.color[2], layer.alpha]
            },
            angle: 0.0,
            texture: base_texture,
            blend: vk::SceneBlend::Copy,
        };
        let crop = if needs_crop {
            let target = if layer.texture.clamp {
                renderer.create_scene_target(fx_w, fx_h)?
            } else {
                renderer.create_scene_target_repeat(fx_w, fx_h)?
            };
            renderer.render_scene(&target, [0.0; 4], &[], &[])?;
            Some(FxTargetStorage::Owned(target))
        } else {
            None
        };
        let crop_quad = snapshot_slot.filter(|_| needs_crop).map(|slot| vk::SceneQuad {
            rect: full_rect,
            uv: passthrough_uv(layer, canvas),
            tint: [1.0, 1.0, 1.0, 1.0],
            angle: 0.0,
            texture: slot,
            blend: vk::SceneBlend::Copy,
        });
        let slot_texture = renderer.create_view_slot()?;
        let slot = textures.len();
        textures.push(slot_texture);
        let source_dynamic = layer.texture.video.is_some()
            || inputs.iter().flatten().any(|slot| {
                texture_interner.video_slots.values().any(|video_slot| video_slot == slot)
            })
            || atlas.is_some()
            || puppets.iter().any(|puppet| puppet.layer == index && puppet.animated());
        fx.push(LayerFx {
            layer_id: layer.id.clone(),
            quad: index,
            scene_order: layer.scene_order,
            slot,
            pipelines,
            ping: FxTargetStorage::Owned(ping),
            pong: FxTargetStorage::Owned(pong),
            base_quad,
            verts,
            inputs,
            remap: (0..fbos.len()).map(std::cell::Cell::new).collect(),
            fbos: fbos
                .into_iter()
                .map(|(name, target)| (name, FxTargetStorage::Owned(target)))
                .collect(),
            fbo_owners,
            swaps,
            pass_targets,
            binds: pass_binds,
            owner: pass_owner,
            passes: metas,
            output: None,
            source_dynamic,
            source_clock: layer.live_text.is_some(),
            dependency_dynamic: false,
            retain_targets: false,
            snapshot: false,
            passthrough: layer.passthrough,
            snapshot_slot,
            crop,
            crop_quad,
            fallback_tint: fallback_tint(layer),
            sampled_targets: BTreeSet::new(),
            uniforms: layer_uniforms(layer, canvas, scene_lights),
        });
    }
    let mut particles = Vec::new();
    for (index, layer) in std::mem::take(&mut model.particles).into_iter().enumerate() {
        let mut system = layer.system;
        let Some(mut texture) = system.texture.take() else {
            continue;
        };
        let slot = match texture_interner.intern_texture(&mut renderer, &mut textures, &mut texture)
        {
            Ok(slot) => slot,
            Err(err) => {
                tracing::warn!("skwd-wall-vk: particle texture upload failed: {err:#}");
                if strict {
                    return Err(err)
                        .context("strict native scene startup rejected particle texture upload");
                }
                continue;
            }
        };
        let mut sim = paper_scene::particles::Sim::new(0x9E37_79B9 ^ (index as u32 + 1));
        sim.prewarm(&system, 1.0 / 30.0);
        let ratio = texture.img_width.max(1) as f32 / texture.img_height.max(1) as f32;
        let frames = texture.frames.clone();
        let render_var1 = sprite_grid(&texture);
        let engine = match system.pass.take() {
            Some(mut pass) => {
                let label = format!("particles:{}", pass.name);
                match renderer.create_particle_pipeline(
                    &pass.vertex,
                    &pass.fragment,
                    pass.hlsl.as_ref(),
                    &label,
                    system.blend,
                ) {
                    Ok(pipeline) => {
                        let capacity = paper_scene::particles::Sim::capacity(&system);
                        let mut extra_textures = Vec::new();
                        for extra in pass.textures.iter_mut().skip(1).flatten() {
                            if let Ok(slot) =
                                texture_interner.intern_texture(&mut renderer, &mut textures, extra)
                            {
                                extra_textures.push(slot);
                            }
                        }
                        let vertex_bytes = capacity
                            * paper_scene::particles::SPRITE_VERTICES
                            * paper_scene::particles::SPRITE_FLOATS_PER_VERTEX
                            * 4;
                        let index_words = paper_scene::particles::sprite_indices(capacity);
                        let buffers = renderer
                            .create_dyn_buffer(
                                vertex_bytes,
                                ash::vk::BufferUsageFlags::VERTEX_BUFFER,
                            )
                            .and_then(|vertices| {
                                renderer
                                    .create_dyn_buffer(
                                        index_words.len() * 4,
                                        ash::vk::BufferUsageFlags::INDEX_BUFFER,
                                    )
                                    .map(|indices| (vertices, indices))
                            });
                        match buffers {
                            Ok((vertices, indices)) => {
                                let bytes: Vec<u8> = index_words
                                    .iter()
                                    .flat_map(|word| word.to_le_bytes())
                                    .collect();
                                renderer.write_dyn_buffer(&indices, &bytes);
                                Some(ParticleEngine {
                                    pipeline,
                                    meta: paper_scene::effects::PassMeta::of(&pass),
                                    vertices,
                                    indices,
                                    capacity,
                                    extra_textures,
                                    render_var1,
                                    texture_size: [
                                        texture.width as f32,
                                        texture.height as f32,
                                        texture.img_width as f32,
                                        texture.img_height as f32,
                                    ],
                                    scratch: Vec::new(),
                                })
                            }
                            Err(err) => {
                                tracing::info!(
                                    "skwd-wall-vk: particle buffers failed ({err:#}), billboards"
                                );
                                renderer.destroy_effect_pipeline(pipeline);
                                None
                            }
                        }
                    }
                    Err(err) => {
                        tracing::info!(
                            "skwd-wall-vk: particle shader skipped ({err:#}), billboards"
                        );
                        None
                    }
                }
            }
            None => None,
        };
        particles.push(ParticleGroup {
            system,
            sim,
            texture: slot,
            ratio,
            frames,
            scene_order: layer.scene_order,
            engine,
        });
    }
    let referenced: BTreeSet<String> = fx
        .iter()
        .flat_map(|layer| &layer.binds)
        .flatten()
        .filter_map(|(_, binding)| match binding {
            EffectBind::LayerComposite { layer, .. } => Some(layer.clone()),
            _ => None,
        })
        .collect();
    let active_layers: BTreeSet<usize> = fx.iter().map(|layer| layer.quad).collect();
    let mut passive_scene_targets = Vec::new();
    let mut passive_video_targets = Vec::new();
    let mut base_scene_targets = Vec::new();
    for index in passive_provider_indices(&model.layers, &active_layers, &referenced) {
        let layer = &model.layers[index];
        if let Some(puppet) = puppets.iter().find(|puppet| puppet.layer == index) {
            base_scene_targets.push(BaseLayerTarget {
                layer_id: layer.id.clone(),
                view: puppet.target.view,
                sampler: puppet.target.sampler,
                extent: puppet.target.extent,
                dynamic: puppet.animated(),
            });
            continue;
        }
        let (width, height) = layer_effect_dimensions(layer, dimensions);
        let target = if layer.texture.clamp {
            renderer.create_scene_target(width, height)?
        } else {
            renderer.create_scene_target_repeat(width, height)?
        };
        let target_quads = layer
            .visible
            .then(|| passive_target_quad(layer, layer_slots[index], target.extent))
            .into_iter()
            .collect::<Vec<_>>();
        renderer.render_scene(&target, [0.0; 4], &target_quads, &textures)?;
        if layer.texture.video.is_some()
            && let Some(quad) = target_quads.first()
        {
            passive_video_targets.push((passive_scene_targets.len(), quad.clone()));
        }
        base_scene_targets.push(BaseLayerTarget {
            layer_id: layer.id.clone(),
            view: target.view,
            sampler: target.sampler,
            extent: target.extent,
            dynamic: layer.texture.video.is_some(),
        });
        passive_scene_targets.push(target);
    }
    let unique_textures = texture_interner.slots.len();
    let reused_textures = texture_interner.reused;
    let reused_payload_bytes = texture_interner.reused_bytes;
    let uploaded_payload_bytes =
        texture_interner.slots.keys().map(|key| key.pixels.bytes()).sum::<usize>();
    let videos = std::mem::take(&mut texture_interner.videos);
    drop(texture_interner);
    let unused_payload_bytes = release_model_texture_payloads(model);
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
    tracing::info!(
        unique_textures,
        reused_textures,
        deduplicated_mib = reused_payload_bytes as f64 / 1_048_576.0,
        released_mib = (uploaded_payload_bytes + reused_payload_bytes + unused_payload_bytes)
            as f64
            / 1_048_576.0,
        "skwd-wall-vk: released decoded scene texture payloads after GPU upload"
    );
    let ndc_quad = if fx.is_empty() { None } else { Some(renderer.create_quad_buffer_ndc()?) };
    let ndc_quad_d3d =
        if fx.is_empty() { None } else { Some(renderer.create_quad_buffer_ndc_d3d()?) };
    let texture_bytes: u64 = textures.iter().map(|texture| texture.allocation_bytes).sum();
    tracing::info!(
        chains_admitted = fx.len(),
        chains_dynamic = dynamic_chains,
        effect_target_bytes = admitted_bytes,
        effect_target_budget = byte_limit,
        particle_systems = particles.len(),
        particle_capacity = particles
            .iter()
            .map(|group| group.engine.as_ref().map_or(0, |engine| engine.capacity))
            .sum::<usize>(),
        texture_slots = textures.len(),
        texture_bytes,
        "skwd-wall-vk: scene budget"
    );
    let live_text: Vec<LiveText> = model
        .layers
        .iter_mut()
        .enumerate()
        .filter_map(|(index, layer)| {
            let prepared = layer.live_text.take()?;
            let now = paper_scene::dynamic_text::local_now()?;
            let shown = prepared.value(now);
            Some(LiveText { slot: *layer_slots.get(index)?, prepared, shown })
        })
        .collect();
    if !live_text.is_empty() {
        tracing::info!("skwd-wall-vk: {} live text layer(s) follow the clock", live_text.len());
    }

    let audio_fx: Vec<usize> = fx
        .iter()
        .enumerate()
        .filter(|(_, layer)| {
            layer.passes.iter().any(paper_scene::effects::PassMeta::audio_dependent)
        })
        .map(|(index, _)| index)
        .collect();
    let audio = start_audio(&audio_fx);
    if audio.is_some() {
        for index in &audio_fx {
            if let Some(layer) = fx.get_mut(*index) {
                seed_bands(&mut layer.uniforms);
            }
        }
    }

    let mut group = Group {
        renderer,
        target,
        scene_snapshot: None,
        scene_grab: None,
        frozen: false,
        from: None,
        quads,
        quad_scene_order,
        textures,
        videos,
        passive_video_targets,
        base_scene_targets,
        passive_scene_targets,
        fx,
        baked_fx_outputs: Vec::new(),
        puppets,
        fx_scratch: Vec::new(),
        ndc_quad,
        ndc_quad_d3d,
        particles,
        animations,
        audio,
        audio_fx,
        bands: paper_audio::Bands::default(),
        live_text,
        live_text_due: 0.0,
        effects_animated: false,
        scene_uniforms: std::collections::BTreeMap::from([
            ("g_LightAmbientColor".to_string(), model.ambient.to_vec()),
            ("g_LightSkylightColor".to_string(), model.skylight.to_vec()),
        ]),
        fixed_daytime: None,
        canvas: (dimensions.logical[0], dimensions.logical[1]),
        clear: model.clear,
    };
    if group.audio.is_some() {
        seed_bands(&mut group.scene_uniforms);
    }
    if let Err(error) = group.configure_scene_targets(strict) {
        group.destroy();
        return Err(error);
    }
    for entry in &group.fx {
        group.quads[entry.quad].uv = [0.0, 0.0, 1.0, 1.0];
    }
    group.compose(0.0, 1.0 / 30.0)?;
    let effect_layers = group.fx.len();
    let effect_target_bytes_before = group.effect_target_allocation_bytes();
    let was_animated = group.animated() || !group.live_text.is_empty();
    let memory = if was_animated {
        group.bake_static_effects()?
    } else {
        group.release_composition_inputs();
        group.frozen = true;
        EffectBakeReport {
            baked_layers: effect_layers,
            released_transient_bytes: effect_target_bytes_before,
            ..EffectBakeReport::default()
        }
    };
    group.release_unused_scene_snapshot();
    let alias_enabled = std::env::var("SKWD_VK_FX_ALIAS").as_deref() != Ok("0");
    let alias = if was_animated && alias_enabled {
        group.alias_dynamic_effect_scratch()
    } else {
        EffectAliasReport::default()
    };
    let effect_target_bytes_after = group.effect_target_allocation_bytes();
    debug_assert_eq!(
        effect_target_bytes_before,
        memory.retained_bytes + memory.transient_bytes + memory.released_transient_bytes
    );
    debug_assert_eq!(
        effect_target_bytes_before,
        effect_target_bytes_after + memory.released_transient_bytes + alias.released_bytes
    );
    if effect_target_bytes_before > 0 {
        tracing::info!(
            effect_layers,
            baked_layers = memory.baked_layers,
            alias_enabled,
            alias_planned_layers = alias.planned_layers,
            alias_fallback_layers = alias.fallback_layers,
            logical_scratch_targets = alias.logical_scratch_targets,
            physical_scratch_targets = alias.physical_scratch_targets,
            before_bytes = effect_target_bytes_before,
            after_bytes = effect_target_bytes_after,
            scratch_bytes = alias.scratch_bytes,
            released_bytes = memory.released_transient_bytes + alias.released_bytes,
            before_mib = effect_target_bytes_before as f64 / 1_048_576.0,
            after_mib = effect_target_bytes_after as f64 / 1_048_576.0,
            scratch_mib = alias.scratch_bytes as f64 / 1_048_576.0,
            released_mib =
                (memory.released_transient_bytes + alias.released_bytes) as f64 / 1_048_576.0,
            "skwd-wall-vk: effect target allocation accounting"
        );
    }
    if !was_animated {
        tracing::info!(
            "skwd-wall-vk: static scene frozen; released layer/effect composition resources"
        );
    }
    Ok(group)
}

fn build_presenter(
    sd: &shared::SharedDevice,
    width: u32,
    height: u32,
    n_exports: usize,
    modifiers: Option<(&[u64], Option<u64>)>,
) -> Result<Presenter> {
    let renderer = shared_renderer(sd, width, height).context("shared scene presenter")?;
    let exports = if let Some((tiled, linear)) = modifiers {
        let mut exports = Vec::with_capacity(n_exports);
        exports.push(renderer.create_xr24_export(width, height, tiled, linear)?);
        let selected = exports[0]
            .modifier
            .filter(|modifier| *modifier != DRM_MOD_LINEAR && *modifier != DRM_MOD_INVALID);
        for _ in 1..n_exports {
            exports.push(renderer.create_xr24_export(
                width,
                height,
                selected.as_slice(),
                selected.is_none().then_some(exports[0].modifier).flatten(),
            )?);
        }
        exports
    } else {
        (0..n_exports)
            .map(|_| renderer.create_export_image_opts(width, height, false))
            .collect::<Result<Vec<_>>>()?
    };
    let direct_scene_copy = !crate::surface::needs_shader()
        && !exports[0].direct_render
        && renderer.scene_export_blit_supported()
        && !crate::surface::needs_shader()
        && std::env::var("SKWD_VK_SCENE_DIRECT_COPY").as_deref() != Ok("0");
    let rts = if direct_scene_copy {
        Vec::new()
    } else if exports[0].direct_render {
        exports.iter().map(|exp| renderer.create_export_rt(exp)).collect::<Result<_>>()?
    } else {
        vec![renderer.create_render_target(width, height)?]
    };
    Ok(Presenter {
        rts,
        exports,
        stream_semaphores: Vec::new(),
        renderer,
        width,
        height,
        direct_scene_copy,
    })
}

fn build_presenters(
    sd: &shared::SharedDevice,
    dimensions: &[(u32, u32)],
    n_exports: usize,
    modifiers: Option<(&[u64], Option<u64>)>,
) -> Result<Vec<Presenter>> {
    dimensions
        .iter()
        .map(|&(width, height)| build_presenter(sd, width, height, n_exports, modifiers))
        .collect()
}

fn build_wayland_presenters(
    target: &mut wayland::Target,
    sd: &shared::SharedDevice,
    dimensions: &[(u32, u32)],
    n_exports: usize,
    share_exports: bool,
) -> Result<(Vec<Presenter>, bool)> {
    let force_shm = std::env::var("SKWD_VK_SCENE_SHM").as_deref() == Ok("1");
    let force_linear = std::env::var("SKWD_VK_XR24_LINEAR").as_deref() == Ok("1");
    let (tiled, linear) = xr24_export_modifiers(&target.app.dmabuf_formats, force_linear);
    if !force_shm {
        for modifiers in [(!tiled.is_empty()).then_some(tiled.as_slice()), linear.map(|_| &[][..])]
            .into_iter()
            .flatten()
        {
            let presenters = match build_presenters(
                sd,
                if share_exports { &dimensions[..1] } else { dimensions },
                n_exports,
                Some((modifiers, linear)),
            ) {
                Ok(presenters) => presenters,
                Err(error) => {
                    tracing::info!(
                        "skwd-wall-vk: scene DMA-BUF allocation unavailable ({error:#})"
                    );
                    continue;
                }
            };
            let mut accepted = true;
            for presenter in &presenters {
                for export in &presenter.exports {
                    if !target.probe_dmabuf_import(
                        export.fd,
                        presenter.width,
                        presenter.height,
                        export.offset,
                        export.stride,
                        export.modifier,
                    )? {
                        accepted = false;
                        break;
                    }
                }
                if !accepted {
                    break;
                }
            }
            if accepted {
                return Ok((presenters, true));
            }
            tracing::info!("skwd-wall-vk: compositor rejected scene DMA-BUF import");
        }
    }
    Ok((build_presenters(sd, dimensions, n_exports, None)?, false))
}

fn shared_scene_slot<'a>(
    free: impl Iterator<Item = &'a [usize]> + Clone,
    count: usize,
) -> Option<usize> {
    (0..count).find(|bi| free.clone().all(|output| output.contains(bi)))
}

fn scene_presenter_layout(
    output_dimensions: &[(u32, u32)],
    scene_dimensions: (u32, u32),
    shared_composition: bool,
) -> (Vec<(u32, u32)>, Vec<usize>) {
    let dimensions = if shared_composition {
        vec![scene_dimensions; output_dimensions.len()]
    } else {
        output_dimensions.to_vec()
    };
    let indices = (0..output_dimensions.len()).collect();
    (dimensions, indices)
}

fn build_stream_presenter(sd: &shared::SharedDevice, width: u32, height: u32) -> Result<Presenter> {
    let renderer = shared_renderer(sd, width, height).context("shared scene stream presenter")?;
    let exports =
        (0..3).map(|_| renderer.create_stream_export(width, height)).collect::<Result<Vec<_>>>()?;
    let stream_semaphores =
        (0..3).map(|_| renderer.create_external_semaphore()).collect::<Result<Vec<_>>>()?;
    let rts = exports
        .iter()
        .map(|export| renderer.create_export_rt(export))
        .collect::<Result<Vec<_>>>()?;
    Ok(Presenter {
        rts,
        exports,
        stream_semaphores,
        renderer,
        width,
        height,
        direct_scene_copy: false,
    })
}

fn load_from_frame(group: &mut Group, path: &str) -> Result<()> {
    let mut dec = decode::SwDecoder::open_threads(path, 2).context("fade source decode")?;
    let (frame, _) = dec.next().context("fade source frame")?;
    let up =
        group.renderer.create_upload_path(dec.width, dec.height).context("fade source upload")?;
    group.renderer.upload_nv12(
        &up,
        frame.data(0),
        frame.stride(0),
        frame.data(1),
        frame.stride(1),
    )?;
    group.from =
        Some(FadeSource { source: FadeFrom::Nv12(up), width: dec.width, height: dec.height });
    Ok(())
}

fn transition_style(shader: Option<&str>) -> TransitionStyle {
    let Some(mut name) = shader else {
        return TransitionStyle::Fade;
    };
    if name == "random" {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.subsec_nanos())
            .unwrap_or(0);
        let sand = paper_shaders::SAND_STYLES.len();
        let pick = nanos as usize % (sand + paper_shaders::EFFECTS.len());
        name = if pick < sand {
            paper_shaders::SAND_STYLES[pick]
        } else {
            paper_shaders::EFFECTS[pick - sand].0
        };
    }
    if let Some(style) = paper_shaders::sand_style_index(name) {
        return TransitionStyle::Sand(style);
    }
    if let Some(effect) = paper_shaders::effect_index(name) {
        return TransitionStyle::Effect(effect);
    }
    if !name.is_empty() && name != "fade" {
        tracing::info!("skwd-wall-vk: scene transition shader {name} unsupported, fading");
    }
    TransitionStyle::Fade
}

fn locate_pkg(dir: &str) -> Result<std::path::PathBuf> {
    ["scene.pkg", "gifscene.pkg"]
        .iter()
        .map(|name| std::path::Path::new(dir).join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| anyhow!("no scene.pkg in {dir}"))
}

struct SceneAudio {
    files: Vec<tempfile::NamedTempFile>,
    voices: Vec<paper_audio::Voice>,
    sources: Vec<String>,
}

impl SceneAudio {
    fn extract(
        pkg: &paper_scene::pkg::Package,
        properties: &paper_scene::model::Properties,
    ) -> Result<Option<Self>> {
        let sounds = paper_scene::sound::scene_sounds(pkg, properties)?;
        if sounds.is_empty() {
            return Ok(None);
        }
        let mut files = Vec::new();
        let mut voices = Vec::new();
        let mut sources = Vec::new();
        for sound in sounds {
            let mut clips = Vec::with_capacity(sound.clips.len());
            for clip in &sound.clips {
                let Some(bytes) = pkg.find(clip) else {
                    tracing::warn!(clip, "skwd-wall-vk: scene clip vanished from package");
                    continue;
                };
                let file = stage_clip(clip, bytes)?;
                clips.push(file.path().to_string_lossy().into_owned());
                files.push(file);
                sources.push(clip.clone());
            }
            if clips.is_empty() {
                continue;
            }
            let (min_gap, max_gap) = sound.gap_range();
            voices.push(paper_audio::Voice {
                name: sound.name,
                clips,
                gain: sound.volume,
                mode: match sound.mode {
                    paper_scene::sound::PlaybackMode::Loop => paper_audio::VoiceMode::Loop,
                    paper_scene::sound::PlaybackMode::Once => paper_audio::VoiceMode::Once,
                    paper_scene::sound::PlaybackMode::Random => paper_audio::VoiceMode::Random,
                },
                min_gap,
                max_gap,
            });
        }
        if voices.is_empty() {
            return Ok(None);
        }
        tracing::info!(
            voices = voices.len(),
            clips = sources.len(),
            "skwd-wall-vk: native scene sounds ready"
        );
        Ok(Some(Self { files, voices, sources }))
    }

    fn voices(&self) -> Vec<paper_audio::Voice> {
        self.voices.clone()
    }

    fn summary(&self) -> String {
        self.sources.join(", ")
    }
}

fn stage_clip(clip: &str, bytes: &[u8]) -> Result<tempfile::NamedTempFile> {
    let suffix = std::path::Path::new(clip)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|ext| {
            !ext.is_empty() && ext.len() <= 8 && ext.bytes().all(|b| b.is_ascii_alphanumeric())
        })
        .map_or_else(String::new, |ext| format!(".{ext}"));
    let mut file = tempfile::Builder::new()
        .prefix("skwd-wall-scene-audio-")
        .suffix(&suffix)
        .tempfile()
        .context("create native scene audio file")?;
    file.write_all(bytes).context("extract native scene audio")?;
    file.flush().context("flush native scene audio")?;
    Ok(file)
}

fn extract_scene_audio(
    pkg: &paper_scene::pkg::Package,
    properties: &paper_scene::model::Properties,
) -> Option<SceneAudio> {
    match SceneAudio::extract(pkg, properties) {
        Ok(audio) => audio,
        Err(error) => {
            tracing::warn!("skwd-wall-vk: native scene audio unavailable: {error:#}");
            None
        }
    }
}

pub(super) fn parse_scene_properties(raw: &str) -> paper_scene::model::Properties {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| match value {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
        .as_ref()
        .map(paper_scene::effects::parse_property_overrides)
        .unwrap_or_default()
}

fn scene_properties_argv(properties: &paper_scene::model::Properties) -> Option<String> {
    if properties.is_empty() {
        return None;
    }
    let map: serde_json::Map<String, serde_json::Value> = properties
        .iter()
        .map(|(name, values)| {
            let joined = values.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ");
            (name.clone(), serde_json::Value::String(joined))
        })
        .collect();
    serde_json::to_string(&map).ok()
}

pub(super) fn run_scene(
    target: &mut wayland::Target,
    dir: &str,
    properties: &paper_scene::model::Properties,
    mute: bool,
    volume: u32,
    start_fade: Option<StartFade>,
) -> Result<()> {
    let pkg_path = locate_pkg(dir)?;
    let pkg = paper_scene::pkg::Package::open(&pkg_path)?;
    let strict = strict_scene_startup();
    validate_native_compatibility(&pkg, strict)?;
    let mut model =
        paper_scene::model::load_from_dir_with(&pkg, std::path::Path::new(dir), properties)?;
    let mut scene_audio = extract_scene_audio(&pkg, properties);
    drop(pkg);
    let particles_disabled = std::env::var("SKWD_PAPER_WE_DISABLE_PARTICLES").as_deref() == Ok("1");
    if particles_disabled {
        let disabled = model.particles.len();
        model.particles.clear();
        tracing::info!(disabled, "skwd-wall-vk: scene particles disabled by policy");
    }
    validate_scene_skips(strict, &model.skipped)?;
    if model.layers.is_empty() && model.particles.is_empty() {
        return Err(anyhow!(
            "scene has no renderable image layers ({})",
            model.skipped.iter().take(4).cloned().collect::<Vec<_>>().join("; ")
        ));
    }
    tracing::info!(
        "skwd-wall-vk: scene {} canvas {}x{} layers {} skipped {}",
        pkg_path.display(),
        model.canvas.0,
        model.canvas.1,
        model.layers.len(),
        model.skipped.len()
    );
    for skip in model.skipped.iter().take(8) {
        tracing::info!("skwd-wall-vk: scene skip {skip}");
    }

    let sd = shared::create(target.display_ptr()).context("shared device")?;
    let n_surf = target.surface_count();
    let dims: Vec<(u32, u32)> = (0..n_surf).map(|si| target.size_at(si)).collect();
    let n_exports = 2usize;
    let mode = fill_mode();
    let mut group = build_group(&sd, &mut model, strict, &dims, mode)?;
    drop(model);
    let shared_composition = target.supports_viewporter()
        && matches!(mode, FillMode::Fill | FillMode::Stretch | FillMode::Span);
    let (presenter_dims, mut ridx) = scene_presenter_layout(
        &dims,
        (group.target.extent.width, group.target.extent.height),
        shared_composition,
    );
    tracing::info!(
        outputs = n_surf,
        output_dimensions = ?dims,
        presenter_dimensions = ?presenter_dims,
        shared_composition,
        canvas_w = group.target.extent.width,
        canvas_h = group.target.extent.height,
        "skwd-wall-vk: one shared scene composition"
    );
    let share_exports = shared_composition
        && n_surf > 1
        && std::env::var("SKWD_VK_SCENE_POOL").as_deref() != Ok("0");
    let (mut presenters, dmabuf_present) =
        build_wayland_presenters(target, &sd, &presenter_dims, n_exports, share_exports)?;
    let shared_pool = share_exports && dmabuf_present;
    if shared_pool {
        ridx.fill(0);
    }
    tracing::info!(
        direct_scene_copy = presenters.first().is_some_and(|presenter| presenter.direct_scene_copy),
        shared_pool,
        presentation_buffers =
            presenters.iter().map(|presenter| presenter.exports.len()).sum::<usize>(),
        "skwd-wall-vk: scene presentation path"
    );
    if let Ok(frames) = std::env::var("SKWD_VK_SCENE_BENCH")
        && let Ok(frames) = frames.parse::<u32>()
    {
        let started = Instant::now();
        let mut gpu_times = Vec::new();
        for index in 0..frames.max(1) {
            group.compose(index as f32 / 30.0, 1.0 / 30.0)?;
            if let Some(nanoseconds) = group.renderer.take_scene_gpu_time_ns() {
                gpu_times.push(nanoseconds);
            }
        }
        let total = started.elapsed();
        tracing::info!(
            "skwd-wall-vk: scene bench {} frames in {:.1}ms ({:.2}ms/frame)",
            frames.max(1),
            total.as_secs_f64() * 1000.0,
            total.as_secs_f64() * 1000.0 / f64::from(frames.max(1))
        );
        if !gpu_times.is_empty() {
            let total_gpu_ns = gpu_times.iter().copied().map(u128::from).sum::<u128>();
            tracing::info!(
                samples = gpu_times.len(),
                total_gpu_ns,
                average_gpu_ns = total_gpu_ns / gpu_times.len() as u128,
                min_gpu_ns = gpu_times.iter().min().copied().unwrap_or(0),
                max_gpu_ns = gpu_times.iter().max().copied().unwrap_or(0),
                "skwd-wall-vk: scene GPU timestamps"
            );
        }
        if std::env::var_os("SKWD_VK_SCENE_DUMP").is_none() {
            return Ok(());
        }
    }
    if let Ok(path) = std::env::var("SKWD_VK_SCENE_DUMP") {
        let scene_target =
            std::mem::replace(&mut group.target, group.renderer.create_scene_target(1, 1)?);
        let (w, h, rgba) = group.renderer.read_scene_target(&scene_target)?;
        group.renderer.destroy_scene_target(std::mem::replace(&mut group.target, scene_target));
        let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
        for px in rgba.chunks_exact(4) {
            ppm.extend_from_slice(&[px[0], px[1], px[2]]);
        }
        std::fs::write(&path, ppm)?;
        tracing::info!("skwd-wall-vk: scene dumped {w}x{h} to {path}");
        return Ok(());
    }
    let rbs: Vec<vk::ReadbackBuf> = presenters
        .iter()
        .zip(&presenter_dims)
        .filter(|_| !dmabuf_present)
        .map(|(presenter, &(w, h))| {
            presenter.renderer.create_readback_buf(u64::from(w) * u64::from(h) * 4)
        })
        .collect::<Result<_>>()?;
    let mut buffers = if dmabuf_present {
        let exports: Vec<_> =
            presenters.iter().map(|presenter| presenter.exports.as_slice()).collect();
        create_buffers(target, &presenter_dims, &exports, &ridx)?
    } else {
        Vec::with_capacity(n_surf)
    };
    let mut rings = Vec::with_capacity(n_surf);
    for (si, &presenter_index) in ridx.iter().enumerate() {
        let (w, h) = presenter_dims[presenter_index];
        if !dmabuf_present {
            let (ring, ptrs, stride) = target.create_shm_ring(si, w, h, n_exports)?;
            buffers.push(ring);
            rings.push((ptrs, stride));
        }
    }
    tracing::info!(
        "skwd-wall-vk: scene path = {}",
        if dmabuf_present {
            "Vulkan DMA-BUF (no CPU readback)"
        } else {
            "Vulkan SHM readback fallback"
        }
    );
    init_free_buffers(target, n_exports);
    for si in 0..n_surf {
        if !target.app.surfaces[si].closed {
            if shared_composition && matches!(fill_mode(), FillMode::Fill | FillMode::Span) {
                target.set_viewport_cover(
                    si,
                    group.target.extent.width,
                    group.target.extent.height,
                )?;
            } else {
                target.set_viewport_dst(si)?;
            }
        }
    }

    let mut fade_ms = start_fade
        .as_ref()
        .filter(|sf| !sf.from.is_empty())
        .map_or(0u64, |sf| sf.duration_ms.clamp(80, 4000));
    if fade_ms > 0
        && let Some(sf) = &start_fade
        && let Err(err) = load_from_frame(&mut group, &sf.from)
    {
        tracing::info!("skwd-wall-vk: scene fade source unavailable ({err:#}), cutting");
        group.from = None;
    }
    let fading_enabled = fade_ms > 0 && group.from.is_some();
    let mut trans_style = transition_style(start_fade.as_ref().and_then(|sf| sf.shader.as_deref()));

    let mut animated = group.animated();
    let fps = std::env::var("SKWD_PAPER_WE_FPS")
        .ok()
        .and_then(|text| text.parse::<u32>().ok())
        .unwrap_or(30)
        .clamp(1, 240);
    let frame_gap = Duration::from_secs_f64(1.0 / f64::from(fps));
    tracing::info!(
        "skwd-wall-vk: scene {} ({} effect chain(s)){}",
        if animated { "animated" } else { "static" },
        group.fx.len(),
        if animated { format!(", {fps}fps cap") } else { String::new() }
    );

    let mut ctl = ctl::Ctl::start_opts(dir, mute, volume, false, true);
    if let Some(audio) = &scene_audio {
        ctl.set_scene_voices(audio.voices());
    }
    target.ctl_fd = ctl.wake_fd();
    let mut active_dir = dir.to_string();
    let mut active_properties = properties.clone();
    let mut presented = false;
    let mut committed_outputs = vec![false; n_surf];
    let mut fade_start: Option<Instant> = fading_enabled.then(Instant::now);
    let mut fade_first_frame = fading_enabled;
    let mut fade_first_outputs = vec![false; n_surf];
    let mut fade_final_outputs = vec![false; n_surf];
    let mut fade_commits = vec![0u64; n_surf];
    let mut epoch = Instant::now();
    let mut next_frame = epoch;
    let mut last_sim = epoch;
    let mut suspended_at: Option<Instant> = None;
    let mut idle_paused = false;

    loop {
        target.pump()?;
        if target.app.closed {
            break;
        }
        if target.take_resized()? {
            drop(ctl);
            group.destroy();
            let argv_properties = scene_properties_argv(&active_properties);
            return wayland::Target::reexec(&wayland::ReexecSource::Scene {
                dir: &active_dir,
                properties: argv_properties.as_deref(),
            });
        }
        if let Some(req) = ctl.poll() {
            tracing::info!("skwd-wall-vk: scene swap to {}", req.to);
            let next_properties = req
                .properties
                .as_ref()
                .map(paper_scene::effects::parse_property_overrides)
                .unwrap_or_default();
            let loaded = locate_pkg(&req.to)
                .and_then(|path| paper_scene::pkg::Package::open(&path))
                .and_then(|pkg| {
                    let model = paper_scene::model::load_from_dir_with(
                        &pkg,
                        std::path::Path::new(&req.to),
                        &next_properties,
                    )?;
                    Ok((model, extract_scene_audio(&pkg, &next_properties)))
                });
            let (mut next, next_audio) = match loaded {
                Ok(next) => next,
                Err(error) => {
                    signal_swap_failure(&req.to, &format!("{error:#}"));
                    continue;
                }
            };
            if particles_disabled {
                next.particles.clear();
            }
            if next.layers.is_empty() && next.particles.is_empty() {
                signal_swap_failure(&req.to, "Scene has no layers or particles");
                continue;
            }
            if let Err(error) = validate_scene_skips(strict, &next.skipped) {
                signal_swap_failure(&req.to, &format!("{error:#}"));
                continue;
            }
            let next_group = match build_group(&sd, &mut next, strict, &dims, mode) {
                Ok(built) => built,
                Err(error) => {
                    signal_swap_failure(&req.to, &format!("{error:#}"));
                    continue;
                }
            };
            drop(next);
            active_dir.clone_from(&req.to);
            active_properties = next_properties;
            let mut old = std::mem::replace(&mut group, next_group);
            ctl.set_scene_voices(next_audio.as_ref().map(SceneAudio::voices).unwrap_or_default());
            scene_audio = next_audio;
            tracing::info!(
                sources = scene_audio.as_ref().map_or_else(String::new, SceneAudio::summary),
                voices = ctl.scene_voice_count(),
                "skwd-wall-vk: scene stream audio swapped"
            );
            tracing::info!(
                sources = scene_audio.as_ref().map_or_else(String::new, SceneAudio::summary),
                voices = ctl.scene_voice_count(),
                "skwd-wall-vk: native scene audio swapped"
            );
            old.drop_from();
            let swap_duration = scene_swap_duration(req.duration_ms);
            if swap_duration.is_some() {
                let old_width = old.target.extent.width;
                let old_height = old.target.extent.height;
                let dummy = old.renderer.create_scene_target(1, 1)?;
                let taken = std::mem::replace(&mut old.target, dummy);
                group.from = Some(FadeSource {
                    source: FadeFrom::Scene(taken),
                    width: old_width,
                    height: old_height,
                });
            }
            old.destroy();
            trans_style = transition_style(req.shader.as_deref());
            let now = Instant::now();
            fade_ms = swap_duration.unwrap_or(0);
            fade_start = swap_duration.map(|_| now);
            fade_first_frame = swap_duration.is_some();
            fade_first_outputs.fill(false);
            fade_final_outputs.fill(false);
            fade_commits.fill(0);
            presented = false;
            committed_outputs.fill(false);
            animated = group.animated();
            epoch = now;
            next_frame = epoch;
            last_sim = epoch;
            if suspended_at.is_some() {
                suspended_at = Some(now);
            }
            tracing::info!(
                "skwd-wall-vk: swapped to {} scene ({} effect chain(s), {fps}fps cap)",
                if animated { "animated" } else { "static" },
                group.fx.len()
            );
            continue;
        }
        if let Some(capture) = ctl.take_capture() {
            let frame = if capture.source == active_dir && presented {
                group.renderer.read_scene_target(&group.target)
            } else {
                Err(anyhow::anyhow!("scene changed before thumbnail capture"))
            };
            crate::freeze::capture_scene(capture, frame);
        }
        if ctl.freeze_pending() {
            match group.renderer.read_scene_target(&group.target) {
                Ok((width, height, rgba)) => {
                    crate::freeze::write_rgba_requested(&mut ctl, width, height, &rgba)?;
                }
                Err(error) => crate::freeze::fail_requested(&mut ctl, &error),
            }
        }
        // Commit one frame before parking so startup/swap readiness cannot deadlock while idle.
        let idle_should_pause = target.app.idle && presented;
        if !idle_should_pause && idle_paused {
            idle_paused = false;
            if let Some(audio) = &mut ctl.audio {
                audio.set_pause(ctl.paused);
            }
        }
        if ctl.paused || idle_should_pause {
            if idle_should_pause {
                idle_paused = true;
                if let Some(audio) = &mut ctl.audio {
                    // Idle remains authoritative if an unpause command arrives while idle.
                    audio.set_pause(true);
                }
            }
            suspended_at.get_or_insert_with(Instant::now);
            target.dispatch_wait_events(Instant::now() + Duration::from_secs(30))?;
            continue;
        }
        if let Some(started) = suspended_at.take() {
            let now = Instant::now();
            let suspended_for = now.saturating_duration_since(started);
            epoch += suspended_for;
            if let Some(started) = &mut fade_start {
                *started += suspended_for;
            }
            next_frame = now;
            last_sim = now;
        }
        let frame_driven = animated || fade_start.is_some();
        if frame_driven {
            let now = Instant::now();
            if now < next_frame {
                target.dispatch_until(next_frame)?;
                continue;
            }
            next_frame = now + frame_gap;
            if animated {
                let time = epoch.elapsed().as_secs_f32();
                let dt = now.duration_since(last_sim).as_secs_f32().clamp(1.0 / 240.0, 0.1);
                last_sim = now;
                group.compose(time, dt)?;
            }
        }
        let fade_step = scene_fade_step(
            fade_start.map(|started| started.elapsed().as_secs_f32() * 1000.0 / fade_ms as f32),
            fade_first_frame,
        );
        if presented && fade_start.is_none() && !animated {
            let elapsed = epoch.elapsed().as_secs_f32();
            let wait = group.live_text_wait(elapsed);
            if wait.is_none_or(|seconds| seconds > 0.0) {
                let seconds = wait.map_or(30.0, |seconds| seconds.clamp(0.0, 30.0));
                target.dispatch_wait_events(Instant::now() + Duration::from_secs_f32(seconds))?;
                continue;
            }
            let now = Instant::now();
            let dt = now.duration_since(last_sim).as_secs_f32().clamp(1.0 / 240.0, 0.1);
            last_sim = now;
            group.compose(elapsed, dt)?;
        }

        let mut committed = false;
        let now_ns = monotonic_ns();
        let mut shared_frame = None;
        for (si, &gi) in ridx.iter().enumerate() {
            if target.app.surfaces[si].closed || !target.commit_due_at(si, now_ns) {
                continue;
            }
            let bi = if shared_pool {
                let count = presenters[0].exports.len();
                if count.saturating_sub(target.app.surfaces[si].free_buffers.len()) >= n_exports {
                    continue;
                }
                let bi = if let Some(bi) = shared_frame {
                    bi
                } else {
                    match shared_scene_slot(
                        target.app.surfaces.iter().map(|surface| surface.free_buffers.as_slice()),
                        count,
                    ) {
                        Some(bi) => bi,
                        None if count < n_surf * n_exports => {
                            presenters[0].grow_scene_pool(target, &mut buffers)?
                        }
                        None => continue,
                    }
                };
                target.app.surfaces[si].free_buffers.retain(|&free| free != bi);
                bi
            } else {
                let Some(bi) = target.take_free_buffer_at(si) else {
                    continue;
                };
                bi
            };
            if !shared_pool || shared_frame.is_none() {
                if fade_step.render_transition {
                    presenters[gi].fade(&group, bi, fade_step.mix, trans_style)?;
                } else {
                    presenters[gi].present(&group.target, bi)?;
                }
                if dmabuf_present {
                    presenters[gi].wait_render()?;
                }
                shared_frame = shared_pool.then_some(bi);
            }
            if !dmabuf_present {
                let (w, h) = presenter_dims[gi];
                presenters[gi].read_export_to(bi, &rbs[gi])?;
                let src_stride = (w * 4) as usize;
                let (ptrs, dst_stride) = &rings[si];
                let dst_stride = *dst_stride as usize;
                unsafe {
                    let src = rbs[gi].ptr;
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
            target.attach_at(si, &buffers[si][bi]);
            target.request_presentation_feedback_at(si);
            target.commit_at(si);
            committed_outputs[si] = true;
            if fade_step.render_transition {
                fade_commits[si] += 1;
                if fade_first_frame {
                    fade_first_outputs[si] = true;
                }
                if fade_step.finish_after_commit {
                    fade_final_outputs[si] = true;
                }
            }
            committed = true;
        }
        target.flush()?;
        let all_live_seen = |seen: &[bool]| {
            target.app.surfaces.iter().enumerate().all(|(si, surface)| surface.closed || seen[si])
        };
        if committed && fade_start.is_some() {
            if fade_first_frame && all_live_seen(&fade_first_outputs) {
                fade_first_frame = false;
                fade_start = Some(Instant::now());
            }
            if fade_step.finish_after_commit && all_live_seen(&fade_final_outputs) {
                let frames = target
                    .app
                    .surfaces
                    .iter()
                    .enumerate()
                    .filter(|(_, surface)| !surface.closed)
                    .map(|(si, surface)| (surface.name.as_str(), fade_commits[si]))
                    .collect::<Vec<_>>();
                tracing::info!(
                    duration_ms = fade_start.map(|started| started.elapsed().as_millis()),
                    frames = ?frames,
                    "skwd-wall-vk: scene transition complete"
                );
                fade_start = None;
                group.drop_from();
            }
        }
        if !presented
            && target
                .app
                .surfaces
                .iter()
                .enumerate()
                .all(|(si, surface)| surface.closed || committed_outputs[si])
        {
            presented = true;
            tracing::info!(
                "skwd-wall-vk: first scene frame committed on {n_surf} output(s){}",
                if fade_start.is_some() { " (transition started)" } else { "" }
            );
            signal_ready();
        }
    }

    group.destroy();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dump_scene(
    dir: &str,
    properties: &paper_scene::model::Properties,
    width: u32,
    height: u32,
    frames: u32,
    warmup: u32,
    dt: f32,
    time0: f32,
    daytime: f32,
    out_dir: &std::path::Path,
) -> Result<()> {
    let (width, height) = (width.max(16), height.max(16));
    let pkg_path = locate_pkg(dir)?;
    let pkg_len = std::fs::metadata(&pkg_path).map(|meta| meta.len()).unwrap_or(0);
    let pkg = paper_scene::pkg::Package::open(&pkg_path)?;
    let strict = strict_scene_startup();
    let mut model =
        paper_scene::model::load_from_dir_with(&pkg, std::path::Path::new(dir), properties)?;
    drop(pkg);
    validate_scene_skips(strict, &model.skipped)?;
    if model.layers.is_empty() && model.particles.is_empty() {
        return Err(anyhow!(
            "scene has no renderable image layers ({})",
            model.skipped.iter().take(4).cloned().collect::<Vec<_>>().join("; ")
        ));
    }
    let skipped = model.skipped.clone();
    let sd = shared::create(std::ptr::null_mut()).context("scene dump shared device")?;
    let mut group = build_group(&sd, &mut model, strict, &[(width, height)], fill_mode())?;
    drop(model);
    group.fixed_daytime = Some(daytime);
    std::fs::create_dir_all(out_dir)?;
    let started = Instant::now();
    let mut written = Vec::new();
    for index in 0..warmup {
        let time = time0 - (warmup - index) as f32 * dt;
        group.compose(time.max(0.0), dt)?;
    }
    for index in 0..frames.max(1) {
        let time = time0 + index as f32 * dt;
        group.compose(time, dt)?;
        let (w, h, rgba) = group.read_canvas()?;
        let name = format!("frame_{index:04}.png");
        let image = image::RgbaImage::from_raw(w, h, rgba)
            .ok_or_else(|| anyhow!("frame {index} buffer size mismatch"))?;
        image.save(out_dir.join(&name)).with_context(|| format!("write {name}"))?;
        written.push(name);
    }
    let manifest = serde_json::json!({
        "scene": dir,
        "pkg": pkg_path.to_string_lossy(),
        "pkg_bytes": pkg_len,
        "canvas": [group.target.extent.width, group.target.extent.height],
        "logical_canvas": [group.canvas.0, group.canvas.1],
        "frames": written,
        "time0": time0,
        "dt": dt,
        "daytime": daytime,
        "pointer": [0.5, 0.5],
        "fill_mode": format!("{:?}", fill_mode()),
        "effect_chains": group.fx.len(),
        "particle_systems": group.particles.len(),
        "animated": group.animated(),
        "skipped": skipped,
        "render_ms": started.elapsed().as_secs_f64() * 1000.0,
    });
    std::fs::write(out_dir.join("manifest.json"), serde_json::to_vec_pretty(&manifest)?)?;
    group.destroy();
    Ok(())
}

pub(super) fn stream_scene(
    dir: &str,
    properties: &paper_scene::model::Properties,
    width: u32,
    height: u32,
    fps: u32,
    socket: RawFd,
    mute: bool,
    volume: u32,
    paused: bool,
) -> Result<()> {
    let (width, height) = (width.max(16), height.max(16));
    let fps = fps.clamp(1, 144);
    let pkg_path = locate_pkg(dir)?;
    let pkg = paper_scene::pkg::Package::open(&pkg_path)?;
    let strict = strict_scene_startup();
    validate_native_compatibility(&pkg, strict)?;
    let mut model =
        paper_scene::model::load_from_dir_with(&pkg, std::path::Path::new(dir), properties)?;
    let mut _scene_audio = extract_scene_audio(&pkg, properties);
    drop(pkg);
    let particles_disabled = std::env::var("SKWD_PAPER_WE_DISABLE_PARTICLES").as_deref() == Ok("1");
    if particles_disabled {
        model.particles.clear();
    }
    validate_scene_skips(strict, &model.skipped)?;
    if model.layers.is_empty() && model.particles.is_empty() {
        return Err(anyhow!(
            "scene has no renderable image layers ({})",
            model.skipped.iter().take(4).cloned().collect::<Vec<_>>().join("; ")
        ));
    }
    let sd = shared::create(std::ptr::null_mut()).context("scene stream shared device")?;
    let mut group = build_group(&sd, &mut model, strict, &[(width, height)], fill_mode())?;
    drop(model);
    let mut presenter = build_stream_presenter(&sd, width, height)?;
    for (slot, export) in presenter.exports.iter().enumerate() {
        let init = crate::preview::packet(
            4,
            slot as u8,
            width,
            height,
            export.stride,
            export.offset,
            export.allocation_size,
        );
        crate::preview::send_packet(socket, &init, Some(export.fd))
            .context("send scene stream slot")?;
        crate::preview::send_packet(
            socket,
            &crate::preview::packet(5, slot as u8, 0, 0, 0, 0, 0),
            Some(presenter.stream_semaphores[slot].fd),
        )
        .context("send scene stream semaphore")?;
    }
    let mut ctl = ctl::Ctl::start_opts(dir, mute, volume, false, true);
    if let Some(audio) = &_scene_audio {
        ctl.set_scene_voices(audio.voices());
    }
    ctl.set_paused(paused);
    let mut animated = group.animated();
    let frame_gap = Duration::from_secs_f64(1.0 / f64::from(fps));
    let mut epoch = Instant::now();
    let mut last_frame = epoch;
    let mut next_frame = epoch;
    let mut suspended_at = None;
    let mut presented = false;
    let mut fade_start: Option<Instant> = None;
    let mut fade_ms = 0u64;
    let mut fade_first_frame = false;
    let mut trans_style = TransitionStyle::Fade;
    let mut free = [true; 3];
    loop {
        while let Some(slot) = crate::preview::receive_ack(socket, false)? {
            if let Some(value) = free.get_mut(slot) {
                *value = true;
            }
        }
        if let Some(req) = ctl.poll() {
            let next_properties = req
                .properties
                .as_ref()
                .map(paper_scene::effects::parse_property_overrides)
                .unwrap_or_default();
            let loaded = locate_pkg(&req.to)
                .and_then(|path| paper_scene::pkg::Package::open(&path))
                .and_then(|pkg| {
                    let model = paper_scene::model::load_from_dir_with(
                        &pkg,
                        std::path::Path::new(&req.to),
                        &next_properties,
                    )?;
                    Ok((model, extract_scene_audio(&pkg, &next_properties)))
                });
            let (mut next, next_audio) = match loaded {
                Ok(next) => next,
                Err(error) => {
                    signal_swap_failure(&req.to, &format!("{error:#}"));
                    continue;
                }
            };
            if particles_disabled {
                next.particles.clear();
            }
            if next.layers.is_empty() && next.particles.is_empty() {
                signal_swap_failure(&req.to, "Scene has no layers or particles");
                continue;
            }
            if let Err(error) = validate_scene_skips(strict, &next.skipped) {
                signal_swap_failure(&req.to, &format!("{error:#}"));
                continue;
            }
            let next_group =
                match build_group(&sd, &mut next, strict, &[(width, height)], fill_mode()) {
                    Ok(group) => group,
                    Err(error) => {
                        signal_swap_failure(&req.to, &format!("{error:#}"));
                        continue;
                    }
                };
            let mut old = std::mem::replace(&mut group, next_group);
            ctl.set_scene_voices(next_audio.as_ref().map(SceneAudio::voices).unwrap_or_default());
            _scene_audio = next_audio;
            old.drop_from();
            let duration = scene_swap_duration(req.duration_ms);
            if duration.is_some() {
                let old_width = old.target.extent.width;
                let old_height = old.target.extent.height;
                let dummy = old.renderer.create_scene_target(1, 1)?;
                let target = std::mem::replace(&mut old.target, dummy);
                group.from = Some(FadeSource {
                    source: FadeFrom::Scene(target),
                    width: old_width,
                    height: old_height,
                });
            }
            old.destroy();
            let now = Instant::now();
            fade_ms = duration.unwrap_or(0);
            fade_start = duration.map(|_| now);
            fade_first_frame = duration.is_some();
            trans_style = transition_style(req.shader.as_deref());
            animated = group.animated();
            epoch = now;
            last_frame = now;
            next_frame = now;
            presented = false;
        }
        if ctl.paused && presented {
            suspended_at.get_or_insert_with(Instant::now);
            if let Some(fd) = ctl.wake_fd() {
                let mut event = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
                let result = unsafe { libc::poll(&raw mut event, 1, 30_000) };
                if result < 0
                    && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                {
                    return Err(std::io::Error::last_os_error().into());
                }
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }
            if unsafe { libc::getppid() } <= 1 {
                return Ok(());
            }
            continue;
        }
        if let Some(started) = suspended_at.take() {
            let suspended = Instant::now().saturating_duration_since(started);
            epoch += suspended;
            last_frame += suspended;
            next_frame = Instant::now();
            if let Some(fade) = &mut fade_start {
                *fade += suspended;
            }
        }
        if presented && !animated && fade_start.is_none() {
            if let Some(fd) = ctl.wake_fd() {
                let mut event = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
                let result = unsafe { libc::poll(&raw mut event, 1, 30_000) };
                if result < 0
                    && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                {
                    return Err(std::io::Error::last_os_error().into());
                }
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }
            if unsafe { libc::getppid() } <= 1 {
                return Ok(());
            }
            continue;
        }
        while !free.iter().any(|value| *value) {
            if let Some(slot) = crate::preview::receive_ack(socket, true)?
                && let Some(value) = free.get_mut(slot)
            {
                *value = true;
            }
        }
        let now = Instant::now();
        if now < next_frame {
            std::thread::sleep(next_frame - now);
        }
        let now = Instant::now();
        next_frame = now + frame_gap;
        if animated {
            let dt = now.duration_since(last_frame).as_secs_f32().clamp(1.0 / 240.0, 0.1);
            last_frame = now;
            group.compose(epoch.elapsed().as_secs_f32(), dt)?;
        }
        let slot = free.iter().position(|value| *value).unwrap();
        let fade_step = scene_fade_step(
            fade_start.map(|started| started.elapsed().as_secs_f32() * 1000.0 / fade_ms as f32),
            fade_first_frame,
        );
        if fade_step.render_transition {
            presenter.fade(&group, slot, fade_step.mix, trans_style)?;
        } else {
            presenter.present(&group.target, slot)?;
        }
        presenter.wait_render()?;
        presenter.renderer.signal_external_semaphore(&presenter.stream_semaphores[slot])?;
        crate::preview::send_packet(
            socket,
            &crate::preview::packet(2, slot as u8, 0, 0, 0, 0, 0),
            None,
        )
        .context("send scene stream frame")?;
        free[slot] = false;
        if !presented {
            paper_runtime::plasma::frame_ready()?;
        }
        presented = true;
        if fade_first_frame {
            fade_first_frame = false;
        }
        if fade_step.finish_after_commit {
            fade_start = None;
            group.drop_from();
        }
    }
}

#[cfg(test)]
mod tests;
pub(super) mod thumbnail;
