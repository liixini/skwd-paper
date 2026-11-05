use crate::shared::renderer as shared_renderer;
mod bloom_runtime;
mod clip;
mod cookie;
mod fx_builder;
mod fx_chain;
mod material;
mod media;
mod mesh;
mod mouse;
mod properties;
mod reflection;
mod resize;
mod sampling;
mod script;
mod shadow;
mod sprite_stream;
mod text;
mod video;
mod volume;

use self::fx_builder::{FxBuilder, FxInput};
use super::dmabuf_helpers::{create_buffers, init_free_buffers, monotonic_ns};
use super::model::StartFade;
use super::readiness::{signal_ready, signal_swap_failure};
use crate::dmabuf::{DRM_MOD_INVALID, DRM_MOD_LINEAR, xr24_export_modifiers};
use crate::fill::{fill_mode, mode_uv};
use crate::{ctl, decode, shared, vk, wayland};
use anyhow::{Context, Result, anyhow, ensure};
use paper_geom::FillMode;
use paper_scene::effect_lifetime::{self as lifetime, Target as FxTargetId, TargetPlan};
use paper_scene::effects::{CompositeBuffer, EffectBind};
use paper_scene::matrix::{Mat4, mat4_mul};
use paper_scene::model::{Camera, SceneModel};
use paper_scene::scene_targets::{
    LayerTargetNode, PassiveLayerTarget, SceneTargetPlan, plan_scene_targets,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::os::fd::RawFd;
use std::time::{Duration, Instant};

const SCENE_LIMIT_ENV: &str = "SKWD_VK_SCENE_MAX";
const SCENE_DEFAULT_LIMIT: u32 = 4096;
const SCENE_MIN_LIMIT: u32 = 256;
const SCENE_FRAME_DT_MIN: f32 = 1.0 / 240.0;
const SCENE_FRAME_DT_MAX: f32 = paper_scene::puppet::spring::MAX_STEP_SECONDS;
const PARTICLE_PIPELINES_KEY: &str = "particle_shader_pipelines";
const ORTHOGRAPHIC_EYE_Z: f32 = 2000.0;
const HISTORY_BUDGET_GAP: &str = "scene-history-budget";

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

const MIN_SCENE_FADE_MS: u64 = 80;

fn scene_swap_duration(duration_ms: u64) -> Option<u64> {
    (duration_ms > 0)
        .then(|| duration_ms.clamp(MIN_SCENE_FADE_MS, paper_control::transitions::MAX_DURATION_MS))
}

const FX_MODEL_MATRIX: &str = paper_scene::effects::material::PUPPET_MATRICES[0];
const FX_MODEL_PROJECTION: &str = paper_scene::effects::material::PUPPET_MATRICES[1];
const FX_VIEW_PROJECTION: &str = paper_scene::effects::material::PUPPET_MATRICES[2];
const FX_VIEW_UP: &str = "g_ViewUp";
const FX_VIEW_RIGHT: &str = "g_ViewRight";
const FX_NORMAL_MATRIX: &str = paper_scene::effects::material::PUPPET_NORMAL_MATRICES[0];
const FX_LAYER_MODEL: &str = "g_LayerModelMatrix";
const FX_MODEL_INVERSE: &str = "g_ModelMatrixInverse";
const FX_ALT_MODEL: &str = paper_scene::effects::material::PUPPET_ALT_MATRICES[0];
const FX_ALT_NORMAL: &str = paper_scene::effects::material::PUPPET_NORMAL_MATRICES[1];
const FX_ALT_VIEW_PROJECTION: &str = paper_scene::effects::material::PUPPET_ALT_MATRICES[1];
const FX_PROJECTION_INVERSE: &str = "g_ModelViewProjectionMatrixInverse";
const FX_EFFECT_MODEL: &str = "g_EffectModelMatrix";
const FX_TEXTURE_PROJECTION: &str = "g_EffectTextureProjectionMatrix";
const FX_TEXTURE_INVERSE: &str = "g_EffectTextureProjectionMatrixInverse";
const MATRIX_COLUMN_COMPONENTS: usize = 4;
const PARTICLE_SEED: u32 = 0x9E37_79B9;
const DYNAMIC_CAMERA_ENGINE_UNAVAILABLE: &str =
    "dynamic particle engine unavailable for authored camera";

fn last_effect_pass(
    owners: &[usize],
    effects: &[paper_scene::effects::EffectState],
) -> Option<usize> {
    owners.iter().rposition(|owner| effects[*owner].visible)
}

fn active_effect_counts(
    owners: &[usize],
    swaps: &[usize],
    effects: &[paper_scene::effects::EffectState],
) -> (usize, usize) {
    let materials = owners.iter().filter(|owner| effects[**owner].visible).count();
    let commands = swaps.iter().filter(|owner| effects[**owner].visible).count();
    (materials, materials + commands)
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
    text_backdrop: Option<vk::SceneQuad>,
    verts: Option<vk::QuadBuffer>,
    presentation: Option<usize>,
    presentation_inputs: Vec<FxTargetId>,
    inputs: Vec<Vec<usize>>,
    fbos: Vec<(String, FxTargetStorage)>,
    fbo_clears: Vec<Option<[f32; 4]>>,
    fbo_specs: Vec<paper_scene::effects::Fbo>,
    clear_functions: Vec<BTreeMap<String, Vec<String>>>,
    pending_clears: Vec<(usize, [f32; 4])>,
    active_counts: (usize, usize),
    reset_targets: bool,
    fbo_owners: Vec<Option<usize>>,
    remap: Vec<std::cell::Cell<usize>>,
    swaps: Vec<(Option<usize>, usize, usize)>,
    pass_targets: Vec<Option<String>>,
    binds: Vec<Vec<(usize, EffectBind)>>,
    owner: Vec<usize>,
    effects: Vec<paper_scene::effects::EffectState>,
    visibility_dynamic: bool,
    swap_owners: Vec<usize>,
    passes: Vec<paper_scene::effects::PassMeta>,
    material_values: Vec<BTreeMap<String, Vec<f32>>>,
    output: Option<FxTargetId>,
    source_dynamic: bool,
    audio_dependent: bool,
    source_clock: bool,
    source_keyframes: bool,
    source_pointer: bool,
    dependency_dynamic: bool,
    retain_targets: bool,
    snapshot: bool,
    passthrough: bool,
    copy_background: bool,
    base_scene_input: bool,
    base_blend: Option<vk::SceneBlend>,
    bare_blend: Option<vk::SceneBlend>,
    mesh_blend: Option<MeshBlend>,
    snapshot_slot: Option<usize>,
    crop: Option<FxTargetStorage>,
    crop_quad: Option<vk::SceneQuad>,
    fallback_tint: [f32; 4],
    sampled_targets: BTreeSet<CompositeBuffer>,
    uniforms: std::collections::BTreeMap<String, Vec<f32>>,
}

#[derive(Clone, Copy)]
struct MeshBlend {
    pass: usize,
    image: [f32; 2],
}

struct LiveText {
    slot: usize,
    prepared: paper_scene::text::Prepared,
    shown: String,
}

struct LayerAnimation {
    quad: usize,
    object: Option<usize>,
    frames: Vec<paper_scene::model::SpriteFrame>,
    total: f32,
    source: FrameSource,
    slot: usize,
    mesh_slot: Option<usize>,
    image: i32,
    control: SpriteControl,
    rate: f32,
    refresh: Option<SpriteRefresh>,
}

enum FrameSource {
    Pages(Vec<usize>),
    Stream(Box<sprite_stream::SpriteStream>),
}

#[derive(Default)]
struct SpriteRefresh {
    previous: Option<f32>,
    frame: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SpriteControl {
    Follow,
    Hold(usize),
    Play { frame: usize, since: f32 },
}

fn frame_start(frames: &[paper_scene::model::SpriteFrame], index: usize) -> f32 {
    let index = if frames.is_empty() { 0 } else { index % frames.len() };
    frames.iter().take(index).map(|frame| frame.time).sum::<f32>()
        + frames.get(index).map_or(0.0, |frame| frame.time * 0.5)
}

impl LayerAnimation {
    fn new(
        layer: &paper_scene::model::Layer,
        quad: usize,
        object: Option<usize>,
        slot: usize,
        mesh_slot: Option<usize>,
        source: Option<FrameSource>,
    ) -> Option<Self> {
        let source = source.unwrap_or_else(|| FrameSource::Pages(vec![slot]));
        let streamed = matches!(source, FrameSource::Stream(_));
        if layer.texture.stream.is_some() != streamed {
            return None;
        }
        let mut frames = layer.atlas_frames()?.to_vec();
        let refresh = frames.iter().all(|frame| frame.time == 0.0).then(|| {
            for frame in &mut frames {
                frame.time = 1.0;
            }
            SpriteRefresh::default()
        });
        let total = frames.iter().map(|frame| frame.time).sum();
        Some(Self {
            quad,
            object,
            frames,
            total,
            source,
            slot,
            mesh_slot,
            image: if streamed { 0 } else { -1 },
            control: SpriteControl::Follow,
            rate: 1.0,
            refresh,
        })
    }

    fn clock_time(&self, time: f32) -> f32 {
        self.refresh.as_ref().map_or(time, |clock| clock.frame)
    }

    fn advance_clock(&mut self, time: f32) {
        if let Some(clock) = &mut self.refresh {
            if clock.previous.is_some_and(|previous| time > previous) {
                clock.frame += 1.0;
            }
            clock.previous = Some(time);
        }
    }

    fn advance(&mut self, time: f32) -> &paper_scene::model::SpriteFrame {
        self.advance_clock(time);
        animation_frame(&self.frames, self.total, self.time(time))
    }

    fn time(&self, time: f32) -> f32 {
        let time = self.clock_time(time);
        match self.control {
            SpriteControl::Follow => time,
            SpriteControl::Hold(frame) => frame_start(&self.frames, frame),
            SpriteControl::Play { frame, since } => {
                frame_start(&self.frames, frame) + (time - since) * self.rate
            }
        }
    }

    fn current_frame(&self, time: f32) -> usize {
        let wanted = self.time(time);
        let mut cursor = if self.total > 0.0 { wanted.rem_euclid(self.total) } else { 0.0 };
        for (index, frame) in self.frames.iter().enumerate() {
            if cursor < frame.time {
                return index;
            }
            cursor -= frame.time;
        }
        self.frames.len().saturating_sub(1)
    }

    fn apply(&mut self, op: paper_scene::script::SpriteOp, time: f32) {
        use paper_scene::script::SpriteOp;
        self.advance_clock(time);
        let current = self.current_frame(time);
        let time = self.clock_time(time);
        self.control = match op {
            SpriteOp::Play => match self.control {
                SpriteControl::Hold(frame) => SpriteControl::Play { frame, since: time },
                other => other,
            },
            SpriteOp::Stop => SpriteControl::Hold(0),
            SpriteOp::Pause => SpriteControl::Hold(current),
            SpriteOp::Frame(frame) => match self.control {
                SpriteControl::Hold(_) => SpriteControl::Hold(frame),
                _ => SpriteControl::Play { frame, since: time },
            },
            SpriteOp::Rate(rate) => {
                self.rate = rate;
                match self.control {
                    SpriteControl::Hold(frame) => SpriteControl::Hold(frame),
                    _ => SpriteControl::Play { frame: current, since: time },
                }
            }
            SpriteOp::Join => {
                self.rate = 1.0;
                SpriteControl::Follow
            }
        };
    }
}

fn animation_frame(
    frames: &[paper_scene::model::SpriteFrame],
    total: f32,
    time: f32,
) -> &paper_scene::model::SpriteFrame {
    let mut cursor = if total > 0.0 { time.rem_euclid(total) } else { 0.0 };
    for frame in frames {
        if cursor < frame.time {
            return frame;
        }
        cursor -= frame.time;
    }
    frames.last().expect("animation has frames")
}

struct PuppetGroup {
    id: String,
    puppet: paper_scene::model::Puppet,
    spring: Option<Box<paper_scene::puppet::spring::Simulation>>,
    spring_world: Vec<Mat4>,
    samples: Vec<(paper_scene::puppet::AnimationLayer, f32)>,
    pose: paper_scene::puppet::Frame,
    alphas: Vec<f32>,
    mesh: vk::SceneMesh,
    order: Option<paper_scene::puppet::draw_order::Sorter>,
    texture: usize,
    layer: usize,
    revealable: bool,
    shown: bool,
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
        self.spring.is_some()
            || self.puppet.layers.iter().any(|layer| {
                layer.animated()
                    && self.puppet.mesh.animations.iter().any(|animation| animation.id == layer.id)
            })
    }

    fn sample_authored(&mut self, time: f32, dt: f32, scripted: bool) -> Result<()> {
        self.samples.clear();
        self.samples.extend(paper_scene::puppet::layer_samples(&self.puppet.layers, time));
        self.pose = paper_scene::puppet::skin_samples(&self.puppet.mesh, &self.samples);
        if scripted {
            return Ok(());
        }
        let Some(spring) = &mut self.spring else { return Ok(()) };
        anyhow::ensure!(self.puppet.planar_owner, paper_scene::puppet::spring::OWNER_UNSUPPORTED);
        let owner = &self.puppet.world;
        anyhow::ensure!(
            paper_scene::puppet::spring::owner_supported(owner),
            paper_scene::puppet::spring::OWNER_UNSUPPORTED
        );
        paper_scene::puppet::sampled_world(
            &self.puppet.mesh,
            &self.samples,
            &mut self.spring_world,
            None,
        )?;
        anyhow::ensure!(
            spring.step(
                &self.puppet.mesh.springs,
                &self.puppet.mesh.bones,
                &mut self.spring_world,
                &paper_scene::puppet::spring::Frame {
                    dt,
                    owner,
                    scale: paper_scene::puppet::spring::owner_scale(owner),
                },
            ),
            paper_scene::puppet::spring::POSE_INVALID
        );
        anyhow::ensure!(
            spring.skin(&self.puppet.mesh, &mut self.pose),
            paper_scene::puppet::spring::PALETTE_INVALID
        );
        paper_scene::puppet::refresh_attachments(
            &self.puppet.mesh,
            &self.spring_world,
            &mut self.pose,
        );
        Ok(())
    }
}

fn puppet_shown(quads: &[vk::SceneQuad], puppet: &PuppetGroup) -> bool {
    quads[puppet.layer].tint[ALPHA_CHANNEL] > 0.0
}

fn layer_puppet(puppets: &[PuppetGroup], layer: usize) -> Option<&PuppetGroup> {
    puppets.iter().find(|puppet| puppet.layer == layer)
}

fn flat_slot(puppets: &[PuppetGroup], layer: usize, slots: &[usize]) -> usize {
    layer_puppet(puppets, layer).map_or(slots[layer], |puppet| puppet.texture)
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
        FboFormat::Rgba16 => ash::vk::Format::R16G16B16A16_UNORM,
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
    let scene_sampled = (target == plan.output && sampled.contains(&CompositeBuffer::A))
        || (target == FxTargetId::Pong && sampled.contains(&CompositeBuffer::B));
    plan.lifetimes[target.index()].scratch_eligible() && !scene_sampled
}

fn clear_function_targets<'a>(
    names: impl Iterator<Item = &'a str>,
    owners: &[Option<usize>],
    remap: &[std::cell::Cell<usize>],
    specs: &[paper_scene::effects::Fbo],
    owner: usize,
    declared: &[String],
) -> Option<Vec<(usize, [f32; 4])>> {
    let names: Vec<_> = names.collect();
    declared
        .iter()
        .map(|name| {
            let logical = lifetime::resolve_fbo_scoped(names.iter().copied(), owners, owner, name)?;
            let physical = remap.get(logical)?.get();
            (physical < names.len()).then_some((physical, specs.get(logical)?.clear))
        })
        .collect()
}

fn merge_pending_clears(
    pending: &mut Vec<(usize, [f32; 4])>,
    incoming: impl IntoIterator<Item = (usize, [f32; 4])>,
) {
    for (index, clear) in incoming {
        if let Some((_, previous)) = pending.iter_mut().find(|(target, _)| *target == index) {
            *previous = clear;
        } else {
            pending.push((index, clear));
        }
    }
}

impl LayerFx {
    fn allocation_bytes(&self) -> u64 {
        self.target_ids()
            .filter_map(|target| self.target_storage(target).owned())
            .chain(self.crop.as_ref().and_then(FxTargetStorage::owned))
            .map(|target| target.allocation_bytes)
            .sum()
    }

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
        let output = self.output?;
        let mut bytes = classify_effect_target_bytes(&allocations, output)?;
        for target in &self.presentation_inputs {
            if *target != output {
                let allocation = self.target_storage(*target).owned()?.allocation_bytes;
                bytes.retained += allocation;
                bytes.transient -= allocation;
            }
        }
        Some(bytes)
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

    fn model_matrix(&self) -> Mat4 {
        self.uniforms[FX_MODEL_MATRIX].as_slice().try_into().expect("model matrix")
    }

    fn plan(&self) -> Result<TargetPlan, lifetime::TargetPlanError> {
        let chain = self.mesh_blend.map_or(self.owner.len(), |blend| blend.pass);
        lifetime::plan_targets_with(
            &self.fbo_names(),
            &self.fbo_owners,
            &self.swaps,
            &self.pass_targets[..chain],
            &self.binds[..chain],
            &self.owner[..chain],
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

    fn queue_clear_function(&mut self, source: usize, name: &str) -> bool {
        let Some(owner) = self.effects.iter().position(|state| state.source == Some(source)) else {
            return false;
        };
        if !self.effects[owner].available {
            return false;
        }
        let Some(names) = self.clear_functions[owner].get(name) else {
            return false;
        };
        let Some(clears) = clear_function_targets(
            self.fbos.iter().map(|(name, _)| name.as_str()),
            &self.fbo_owners,
            &self.remap,
            &self.fbo_specs,
            owner,
            names,
        ) else {
            return false;
        };
        merge_pending_clears(&mut self.pending_clears, clears);
        true
    }

    fn apply_swaps(&self, after: Option<usize>) {
        for (&(when, a, b), &owner) in self.swaps.iter().zip(&self.swap_owners) {
            if self.effects[owner].visible
                && when == after
                && a != b
                && a < self.remap.len()
                && b < self.remap.len()
            {
                let (physical_a, physical_b) = (self.remap[a].get(), self.remap[b].get());
                self.remap[a].set(physical_b);
                self.remap[b].set(physical_a);
            }
        }
    }

    fn intrinsic_dynamic(&self, include_audio: bool) -> bool {
        self.source_dynamic
            || self
                .binds
                .iter()
                .flatten()
                .any(|(_, binding)| matches!(binding, EffectBind::SceneHistory))
            || (include_audio && self.audio_dependent)
            || self.passes.iter().any(paper_scene::effects::PassMeta::time_dependent)
            || self
                .plan()
                .map(|plan| plan.lifetimes.iter().any(|lifetime| lifetime.loop_carried))
                .unwrap_or(true)
    }

    fn frame_state_dependent(&self) -> bool {
        self.source_keyframes
            || self.source_clock
            || self.source_pointer
            || self.intrinsic_dynamic(true)
            || self.dependency_dynamic
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct TextureKey {
    pixels: std::sync::Arc<paper_scene::tex::Pixels>,
    clamp: bool,
    nearest: bool,
    sampling: vk::MipSampling,
    text: bool,
    text_batch: Option<paper_scene::text::batch::Batch>,
}

impl TextureKey {
    fn take(texture: &mut paper_scene::model::Texture, sampling: vk::MipSampling) -> Self {
        Self {
            pixels: std::mem::take(&mut texture.pixels),
            clamp: texture.clamp,
            nearest: texture.nearest,
            sampling,
            text: false,
            text_batch: texture.text_batch.take(),
        }
    }

    fn bytes(&self) -> usize {
        self.pixels.bytes() + self.text_batch.as_ref().map_or(0, |batch| batch.pixels.bytes())
    }

    fn clear() -> Self {
        Self {
            pixels: std::sync::Arc::new(paper_scene::tex::Pixels::rgba(1, 1, vec![0, 0, 0, 0])),
            clamp: true,
            nearest: false,
            sampling: vk::MipSampling::Disabled,
            text: false,
            text_batch: None,
        }
    }
}

fn layer_sampling(is_text: bool) -> vk::MipSampling {
    if is_text { vk::MipSampling::Disabled } else { vk::MipSampling::Anisotropic }
}

struct TextureInterner<'a> {
    device: Option<&'a shared::SharedDevice>,
    media_slots: HashMap<paper_scene::model::MediaTexture, usize>,
    videos: Vec<video::VideoTexture>,
    video_slots: HashMap<(video::VideoKey, bool, bool), usize>,
    slots: HashMap<TextureKey, usize>,
    reused: usize,
    reused_bytes: usize,
}

impl<'a> TextureInterner<'a> {
    fn new(device: Option<&'a shared::SharedDevice>) -> Self {
        Self {
            device,
            media_slots: HashMap::new(),
            videos: Vec::new(),
            video_slots: HashMap::new(),
            slots: HashMap::new(),
            reused: 0,
            reused_bytes: 0,
        }
    }

    fn intern_key(
        &mut self,
        renderer: &mut vk::Renderer,
        textures: &mut Vec<vk::SceneTexture>,
        key: TextureKey,
    ) -> std::result::Result<usize, (anyhow::Error, TextureKey)> {
        if let Some((stored, &slot)) = self.slots.get_key_value(&key) {
            self.reused += 1;
            if !std::sync::Arc::ptr_eq(&stored.pixels, &key.pixels) {
                self.reused_bytes += key.bytes();
            }
            return Ok(slot);
        }
        let uploaded = if key.text {
            renderer.create_text_texture(&key.pixels, key.sampling, key.text_batch.as_ref())
        } else {
            renderer.create_scene_texture_pixels(&key.pixels, key.clamp, key.nearest, key.sampling)
        };
        let uploaded = match uploaded {
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
        sampling: vk::MipSampling,
    ) -> Result<usize> {
        if let Some(kind) = texture.system_texture {
            if let Some(slot) = self.media_slots.get(&kind) {
                return Ok(*slot);
            }
            let slot = textures.len();
            textures.push(renderer.create_scene_texture_pixels(
                &texture.pixels,
                true,
                false,
                vk::MipSampling::Disabled,
            )?);
            self.media_slots.insert(kind, slot);
            return Ok(slot);
        }
        if let Some(payload) = &texture.video {
            let key = (video::VideoKey::of(payload), texture.clamp, texture.nearest);
            if let Some(&slot) = self.video_slots.get(&key) {
                return Ok(slot);
            }
            let video = video::VideoTexture::open(
                self.device.context("video device missing")?,
                payload,
                texture.clamp,
                texture.nearest,
            )?;
            let texture_slot = video.view_slot(renderer)?;
            let slot = textures.len();
            textures.push(texture_slot);
            self.videos.push(video);
            self.video_slots.insert(key, slot);
            return Ok(slot);
        }
        let key = TextureKey::take(texture, sampling);
        match self.intern_key(renderer, textures, key) {
            Ok(slot) => Ok(slot),
            Err((err, key)) => {
                texture.pixels = key.pixels;
                texture.text_batch = key.text_batch;
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
    render: paper_scene::particles::ParticleRender,
    grab_slot: Option<usize>,
    reflection_slot: Option<usize>,
    pipeline: vk::EffectPipeline,
    mirror: Option<vk::EffectPipeline>,
    meta: paper_scene::effects::PassMeta,
    buffers: Option<(vk::DynBuffer, vk::DynBuffer)>,
    capacity: usize,
    failed_growth: bool,
    limit: usize,
    mesh: vk::ParticleMesh,
    extra_textures: Vec<usize>,
    shadow_slot: Option<usize>,
    cookie_slot: Option<usize>,
    render_var1: [f32; 4],
    texture_size: [f32; 4],
    scratch: Vec<f32>,
}

#[cfg(test)]
thread_local! {
    static PARTICLE_INDEX_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn particle_stride(mesh: vk::ParticleMesh) -> usize {
    match mesh {
        vk::ParticleMesh::SpritePoints => paper_scene::particles::SPRITE_POINT_FLOATS,
        vk::ParticleMesh::RopePoints => paper_scene::particles::ROPE_POINT_FLOATS,
        vk::ParticleMesh::SpriteQuads => {
            paper_scene::particles::SPRITE_VERTICES
                * paper_scene::particles::SPRITE_FLOATS_PER_VERTEX
        }
    }
}

fn particle_buffers(
    renderer: &vk::Renderer,
    mesh: vk::ParticleMesh,
    capacity: usize,
) -> Result<(vk::DynBuffer, vk::DynBuffer)> {
    let vertices = renderer.create_dyn_buffer(
        capacity * particle_stride(mesh) * std::mem::size_of::<f32>(),
        ash::vk::BufferUsageFlags::VERTEX_BUFFER,
    )?;
    #[cfg(test)]
    if PARTICLE_INDEX_FAILURE.replace(false) {
        return Err(anyhow!("injected particle index allocation failure"));
    }
    let words = if mesh == vk::ParticleMesh::SpriteQuads {
        paper_scene::particles::sprite_indices(capacity)
    } else {
        (0..capacity as u32).collect()
    };
    let indices = renderer.create_dyn_buffer(
        words.len() * std::mem::size_of::<u32>(),
        ash::vk::BufferUsageFlags::INDEX_BUFFER,
    )?;
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    renderer.write_dyn_buffer(&indices, &bytes);
    Ok((vertices, indices))
}

impl ParticleEngine {
    fn reserve(&mut self, renderer: &vk::Renderer, count: usize) -> Result<()> {
        let count = count.min(self.limit);
        if count <= self.capacity {
            self.failed_growth = false;
            return Ok(());
        }
        let capacity = count.next_power_of_two().min(self.limit);
        self.buffers = Some(particle_buffers(renderer, self.mesh, capacity)?);
        self.capacity = capacity;
        self.failed_growth = false;
        Ok(())
    }

    fn destroy(self, renderer: &vk::Renderer) {
        renderer.destroy_effect_pipeline(self.pipeline);
        if let Some(mirror) = self.mirror {
            renderer.destroy_effect_pipeline(mirror);
        }
        if let Some((vertices, indices)) = self.buffers {
            renderer.destroy_dyn_buffer(vertices);
            renderer.destroy_dyn_buffer(indices);
        }
    }
}

fn particle_engines(
    system: &mut paper_scene::particles::ParticleSystem,
    texture: &paper_scene::model::Texture,
    renderer: &mut vk::Renderer,
    textures: &mut Vec<vk::SceneTexture>,
    texture_interner: &mut TextureInterner<'_>,
    mirrored: bool,
) -> Vec<ParticleEngine> {
    let passes =
        std::iter::once(system.pass.take()).chain(std::mem::take(&mut system.extra_passes));
    let renders =
        std::iter::once(system.primary_render()).chain(system.extra_renderers.iter().copied());
    renders
        .zip(passes)
        .filter_map(|(render, pass)| {
            let mut pass = pass?;
            let mut engine = particle_engine(
                system,
                render,
                &mut pass,
                texture,
                renderer,
                textures,
                texture_interner,
            )?;
            if mirrored && engine.reflection_slot.is_none() {
                engine.mirror = particle_pipeline(renderer, &pass, system, engine.mesh)
                    .inspect_err(|err| tracing::warn!("skwd-wall-vk: reflected particles: {err:#}"))
                    .ok();
            }
            Some(engine)
        })
        .collect()
}

fn particle_renderers_ready(
    system: &paper_scene::particles::ParticleSystem,
    engines: &[ParticleEngine],
) -> bool {
    system.emitters.is_empty()
        || std::iter::once(system.primary_render())
            .chain(system.extra_renderers.iter().copied())
            .all(|render| engines.iter().any(|engine| engine.render == render))
}

fn particle_pipeline(
    renderer: &mut vk::Renderer,
    pass: &paper_scene::effects::EffectPass,
    system: &paper_scene::particles::ParticleSystem,
    mesh: vk::ParticleMesh,
) -> Result<vk::EffectPipeline> {
    renderer.create_particle_pipeline(
        &pass.vertex,
        pass.geometry.as_ref(),
        &pass.fragment,
        pass.hlsl.as_ref(),
        &format!("particles:{}", pass.name),
        vk::ParticlePipelineSpec {
            blend: system.blend,
            samples: renderer.scene_sample_count(),
            mesh,
        },
    )
}

fn particle_engine(
    system: &paper_scene::particles::ParticleSystem,
    render: paper_scene::particles::ParticleRender,
    pass: &mut paper_scene::effects::EffectPass,
    texture: &paper_scene::model::Texture,
    renderer: &mut vk::Renderer,
    textures: &mut Vec<vk::SceneTexture>,
    texture_interner: &mut TextureInterner<'_>,
) -> Option<ParticleEngine> {
    let render_var1 = sprite_grid(texture);
    let mesh = match (render.renderer, pass.geometry.is_some()) {
        (
            paper_scene::particles::Renderer::Rope | paper_scene::particles::Renderer::Ribbon,
            true,
        ) => vk::ParticleMesh::RopePoints,
        (
            paper_scene::particles::Renderer::Rope | paper_scene::particles::Renderer::Ribbon,
            false,
        ) => return None,
        (_, true) => vk::ParticleMesh::SpritePoints,
        (_, false) => vk::ParticleMesh::SpriteQuads,
    };
    match particle_pipeline(renderer, pass, system, mesh) {
        Ok(pipeline) => {
            let capacity = paper_scene::particles::Followers::capacity(system);
            let capacity = if mesh == vk::ParticleMesh::RopePoints
                && render.renderer == paper_scene::particles::Renderer::Ribbon
            {
                capacity.saturating_mul(system.ribbon_trail().segments)
            } else {
                capacity
            };
            let mut extra_textures = Vec::new();
            for extra in pass.textures.iter_mut().skip(1).flatten() {
                if let Ok(slot) = texture_interner.intern_texture(
                    renderer,
                    textures,
                    extra,
                    vk::MipSampling::Anisotropic,
                ) {
                    extra_textures.push(slot);
                }
            }
            Some(ParticleEngine {
                render,
                grab_slot: pass.binds.iter().find_map(|(slot, bind)| {
                    matches!(bind, EffectBind::SceneSoFar).then_some(*slot)
                }),
                reflection_slot: pass.binds.iter().find_map(|(slot, bind)| {
                    matches!(bind, EffectBind::Reflection).then_some(*slot)
                }),
                pipeline,
                mirror: None,
                meta: paper_scene::effects::PassMeta::of(pass),
                buffers: None,
                capacity: 0,
                failed_growth: false,
                limit: capacity,
                mesh,
                extra_textures,
                shadow_slot: pass.binds.iter().find_map(|(slot, bind)| {
                    matches!(bind, EffectBind::ShadowAtlas).then_some(*slot)
                }),
                cookie_slot: pass.binds.iter().find_map(|(slot, bind)| {
                    matches!(bind, EffectBind::LightCookie).then_some(*slot)
                }),
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
            tracing::info!("skwd-wall-vk: particle shader skipped ({err:#}), billboards");
            None
        }
    }
}

struct ParticleGroup {
    parallax: [f32; 2],
    camera_offset: [f32; 2],
    id: String,
    visible: bool,
    system: paper_scene::particles::ParticleSystem,
    sim: paper_scene::particles::Sim,
    texture: Option<usize>,
    ratio: f32,
    frames: Vec<paper_scene::model::SpriteFrame>,
    scene_order: usize,
    engines: Vec<ParticleEngine>,
    screen_basis: Option<ScreenBasis>,
    parent: Option<ParticleParent>,
    followers: paper_scene::particles::Followers,
    reset: bool,
    prewarm_steps: usize,
    prewarm_dt: f32,
    dynamic_slots: Option<Vec<usize>>,
    media_kinds: Vec<paper_scene::model::MediaTexture>,
    dynamic_videos: Vec<video::VideoTexture>,
}

impl ParticleGroup {
    fn hide(&mut self, parent: Option<&Self>) {
        self.reset = parent.map_or(!self.sim.particles.is_empty(), |parent| parent.reset);
        if self.reset {
            self.sim.hide();
            self.followers = paper_scene::particles::Followers::default();
        }
    }
}

#[derive(Clone, Copy)]
struct ScreenBasis {
    forward: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
}

fn screen_basis(camera: Camera, model: &Mat4) -> Option<ScreenBasis> {
    let forward = camera.basis()?.forward.map(|value| -value);
    let column = |index: usize| {
        let start = index * MATRIX_COLUMN_COMPONENTS;
        [model[start], model[start + 1], model[start + 2]]
    };
    let columns = [column(0), column(1), column(2)];
    let dot = |left: [f32; 3], right: [f32; 3]| {
        left.iter().zip(right).map(|(first, second)| first * second).sum::<f32>()
    };
    let seed = columns[1];
    let projection = dot(forward, seed);
    let up = std::array::from_fn(|axis| seed[axis] - forward[axis] * projection);
    let local = |axis| columns.map(|column| dot(column, axis));
    let local_forward = local(forward);
    let local_up = local(up);
    let local_right = [
        local_up[1] * local_forward[2] - local_up[2] * local_forward[1],
        local_up[2] * local_forward[0] - local_up[0] * local_forward[2],
        local_up[0] * local_forward[1] - local_up[1] * local_forward[0],
    ];
    let normalize = |axis: [f32; 3]| {
        let squared = dot(axis, axis);
        squared.is_normal().then(|| axis.map(|value| value / squared.sqrt()))
    };
    Some(ScreenBasis {
        forward: normalize(local_forward)?,
        right: normalize(local_right)?,
        up: normalize(local_up)?,
    })
}

struct ParticleParent {
    index: usize,
    origin: (f32, f32, f32),
    angle: f32,
    scale: [f32; 3],
}

struct ParticleStep<'a> {
    dt: f32,
    frame: u32,
    clock: paper_scene::effects::FrameClock,
    mouse: Option<&'a mouse::SceneMouse>,
    audio: (&'a [f32], &'a [f32]),
    prewarm_step: Option<usize>,
}

fn bind_child(
    system: &mut paper_scene::particles::ParticleSystem,
    parent: &paper_scene::particles::ParticleSystem,
    binding: &ParticleParent,
) {
    system.spatial = None;
    system.world_matrix = None;
    if system.world && system.child_event.is_none() {
        system.origin = binding.origin;
        system.angle = binding.angle;
        system.scale3 = binding.scale;
        system.scale = system.scale3[0];
        return;
    }
    let (sin, cos) = parent.angle.sin_cos();
    let x = binding.origin.0 * parent.scale3[0];
    let y = binding.origin.1 * parent.scale3[1];
    system.origin = (
        parent.origin.0 + x * cos - y * sin,
        parent.origin.1 + x * sin + y * cos,
        parent.origin.2 + binding.origin.2 * parent.scale3[2],
    );
    system.angle = parent.angle + binding.angle;
    let matrix = parent.spatial.or(parent.world_matrix).map(|matrix| {
        mat4_mul(&matrix, &model_matrix(binding.origin, binding.scale, binding.angle))
    });
    if system.world {
        system.world_matrix = matrix;
    } else {
        system.spatial = matrix;
    }
    system.scale3 = std::array::from_fn(|axis| parent.scale3[axis] * binding.scale[axis]);
    system.scale = system.scale3[0];
}

fn advance_particles(
    groups: &mut [ParticleGroup],
    step: &ParticleStep<'_>,
    query: &mut dyn FnMut(&paper_scene::particles::image::ImageSource),
) {
    let &ParticleStep { dt, frame, clock, mouse, audio, prewarm_step } = step;
    for index in 0..groups.len() {
        let (before, after) = groups.split_at_mut(index);
        let group = &mut after[0];
        if let Some(binding) = &group.parent {
            let parent = &before[binding.index];
            bind_child(&mut group.system, &parent.system, binding);
            group.system.alpha = parent.system.alpha;
            group.system.count_scale = parent.system.count_scale;
            group.system.rate_scale = parent.system.rate_scale;
            group.system.size_scale = parent.system.size_scale;
            group.system.inherit_instance_factors(&parent.system);
            group.visible = parent.visible;
            group.sim.set_parent_hidden(!parent.visible);
        }
        group.sim.set_layer_visible(group.visible);
        group.reset = false;
        if !group.visible && prewarm_step.is_none() {
            group.hide(group.parent.as_ref().map(|binding| &before[binding.index]));
            if group.parent.is_none() {
                continue;
            }
        }
        if prewarm_step.is_some_and(|step| step >= group.prewarm_steps) {
            continue;
        }
        if prewarm_step.is_some() && group.parent.is_some() && group.system.child_event.is_some() {
            continue;
        }
        let dt = if prewarm_step.is_some() { group.prewarm_dt } else { dt };
        group.sim.set_scene_frame(frame);
        group.sim.set_scene_clock(clock);
        group.camera_offset = if let Some(binding) = &group.parent {
            before[binding.index].camera_offset
        } else {
            mouse.map_or([0.0; 2], |mouse| mouse.particle_offset(group.parallax))
        };
        if let Some(mouse) = mouse {
            group.sim.set_pointer(mouse.particle_pointer(group.camera_offset));
        }
        if let Some(binding) = &group.parent
            && group.system.child_event.is_some()
        {
            let parent = &before[binding.index];
            group.followers.step(
                &mut group.sim,
                &group.system,
                (&parent.system, &parent.sim),
                dt,
                audio,
                query,
            );
        } else {
            group.sim.step_with_audio(&group.system, dt, audio.0, audio.1, query);
        }
    }
}

fn prewarm_particles(
    groups: &mut [ParticleGroup],
    frame: u32,
    clock: paper_scene::effects::FrameClock,
) {
    for group in groups.iter_mut().filter(|group| group.prewarm_steps > 0) {
        group.sim.begin_prewarm();
    }
    for step in 0..groups.iter().map(|group| group.prewarm_steps).max().unwrap_or(0) {
        advance_particles(
            groups,
            &ParticleStep {
                dt: 0.0,
                frame,
                clock,
                mouse: None,
                audio: (&[], &[]),
                prewarm_step: Some(step),
            },
            &mut |_| {},
        );
    }
    for group in groups.iter_mut().filter(|group| group.prewarm_steps > 0) {
        group.sim.finish_prewarm();
    }
}

struct OrderedParticleQuad {
    scene_order: usize,
    renderer: (usize, usize),
    quad: vk::SceneQuad,
}

struct OrderedParticleDraw {
    scene_order: usize,
    group: usize,
    renderer: usize,
    engine: usize,
    index_count: u32,
}

fn clip_y(d3d_clip: bool) -> f32 {
    if d3d_clip { 1.0 } else { -1.0 }
}

fn scene_ortho(canvas: (f32, f32), d3d_clip: bool) -> Mat4 {
    let (w, h) = (canvas.0.max(1.0), canvas.1.max(1.0));
    let ys = clip_y(d3d_clip);
    [2.0 / w, 0.0, 0.0, 0.0, 0.0, ys * 2.0 / h, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -ys, 0.5, 1.0]
}

fn perspective_eye_distance(height: f32, fov: f32) -> f32 {
    height.max(1.0) * 0.5 / (fov.to_radians() * 0.5).tan()
}

fn orthographic_eye(canvas: (f32, f32)) -> [f32; 3] {
    [canvas.0 * 0.5, canvas.1 * 0.5, ORTHOGRAPHIC_EYE_Z]
}

fn perspective_eye(source: [f32; 4], fov: f32) -> [f32; 3] {
    let [x, y, w, h] = source;
    [x + w * 0.5, y + h * 0.5, perspective_eye_distance(h, fov)]
}

fn particle_eye(
    projection: paper_scene::model::ProjectionMode,
    camera: Option<Camera>,
    canvas: (f32, f32),
    fov: f32,
) -> [f32; 3] {
    match (camera, projection) {
        (Some(camera), _) => camera.eye,
        (None, paper_scene::model::ProjectionMode::Orthographic) => orthographic_eye(canvas),
        (None, _) => perspective_eye([0.0, 0.0, canvas.0, canvas.1], fov),
    }
}

struct PerspectiveSpec {
    vertical_scale: f32,
    aspect: f32,
    depth: f32,
    depth_offset: f32,
    d3d_clip: bool,
}

fn perspective_matrix(spec: &PerspectiveSpec) -> Mat4 {
    let ys = clip_y(spec.d3d_clip);
    [
        spec.vertical_scale / spec.aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        ys * spec.vertical_scale,
        0.0,
        0.0,
        0.0,
        0.0,
        spec.depth,
        -1.0,
        0.0,
        0.0,
        spec.depth_offset,
        0.0,
    ]
}

fn scene_perspective(source: [f32; 4], fov: f32, d3d_clip: bool) -> Mat4 {
    let [x, y, w, h] = source;
    let (w, h) = (w.max(1.0), h.max(1.0));
    let (near, far) = (1.0f32, 100_000.0f32);
    let [eye_x, eye_y, eye_distance] = perspective_eye([x, y, w, h], fov);
    let f = eye_distance / (h * 0.5);
    let a = far / (near - far);
    let b = near * far / (near - far);
    let projection = perspective_matrix(&PerspectiveSpec {
        vertical_scale: f,
        aspect: w / h,
        depth: a,
        depth_offset: b,
        d3d_clip,
    });
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
        -eye_x,
        -eye_y,
        -eye_distance,
        1.0,
    ];
    mat4_mul(&projection, &view)
}

const CAMERA_SINE: [f32; 6] =
    [-2.388_985_9e-8, 2.752_556_2e-6, -0.000_198_408_74, 0.008_333_331, -0.166_666_67, 1.0];
const CAMERA_COSINE: [f32; 6] =
    [-2.605_161_5e-7, 2.476_049_5e-5, -0.001_388_837_8, 0.041_666_638, -0.5, 1.0];

fn camera_scale(fov: f32) -> f32 {
    let half = fov.to_radians() * 0.5;
    let square = half * half;
    let [sin, cos] = [CAMERA_SINE, CAMERA_COSINE].map(|coefficients| {
        coefficients.into_iter().reduce(|value, coefficient| value * square + coefficient).unwrap()
    });
    cos / (sin * half)
}

fn camera_view(eye: [f32; 3], basis: paper_scene::model::CameraBasis) -> Mat4 {
    let paper_scene::model::CameraBasis { right, up, forward } = basis;
    let dot = |first: [f32; 3], second: [f32; 3]| {
        first.iter().zip(second).map(|(left, right)| left * right).sum::<f32>()
    };
    [
        right[0],
        up[0],
        -forward[0],
        0.0,
        right[1],
        up[1],
        -forward[1],
        0.0,
        right[2],
        up[2],
        -forward[2],
        0.0,
        -dot(right, eye),
        -dot(up, eye),
        dot(forward, eye),
        1.0,
    ]
}

fn authored_perspective(
    camera: Camera,
    canvas: (f32, f32),
    d3d_clip: bool,
) -> (Mat4, [f32; 3], [f32; 3]) {
    let paper_scene::model::CameraBasis { right, up, forward } =
        camera.basis().expect("validated camera");
    let view = camera_view(camera.eye, paper_scene::model::CameraBasis { right, up, forward });
    let field = camera_scale(camera.fov);
    let depth = camera.near / (camera.far - camera.near);
    let projection = perspective_matrix(&PerspectiveSpec {
        vertical_scale: field,
        aspect: canvas.0 / canvas.1,
        depth,
        depth_offset: camera.far * depth,
        d3d_clip,
    });
    (mat4_mul(&projection, &view), right, up)
}

fn present_uniforms(
    uniforms: &mut std::collections::BTreeMap<String, Vec<f32>>,
    meta: &paper_scene::effects::PassMeta,
    projection: Mat4,
    (clock, screen): (paper_scene::effects::FrameClock, (u32, u32)),
    sizes: &[Option<[f32; 4]>],
    d3d_clip: bool,
) -> Vec<u8> {
    uniforms.insert(FX_MODEL_PROJECTION.into(), projection.to_vec());
    meta.uniform_bytes_with(clock, None, screen.0, screen.1, screen, sizes, uniforms, d3d_clip)
}

struct PresentView {
    canvas: (f32, f32),
    camera: Option<Camera>,
    view: vk::ClipView,
    d3d_clip: bool,
}

impl PresentView {
    fn projection(&self, model: &Mat4) -> Mat4 {
        self.camera.map_or_else(
            || {
                let mut projection = mat4_mul(&scene_view_projection(self.canvas), model);
                if !self.d3d_clip {
                    flip_clip_y(&mut projection);
                }
                projection
            },
            |camera| camera_projection(camera, self.canvas, model, self.d3d_clip, self.view),
        )
    }

    fn binding(&self, projection: Mat4, model: Mat4, offset: usize) -> vk::EffectProjection {
        if self.camera.is_some() {
            return vk::EffectProjection::fixed(projection, offset);
        }
        vk::EffectProjection::for_layer(projection, model, self.canvas, offset).viewed(self.view)
    }
}

fn camera_projection(
    camera: Camera,
    canvas: (f32, f32),
    model: &Mat4,
    d3d_clip: bool,
    view: vk::ClipView,
) -> Mat4 {
    let mut projection = mat4_mul(&authored_perspective(camera, canvas, true).0, model);
    if !d3d_clip {
        flip_clip_y(&mut projection);
    }
    view.apply(&mut projection, !d3d_clip);
    projection
}

#[derive(Clone, Copy)]
struct LayerPose {
    geometry: Option<paper_scene::model::Geometry>,
    rect: [f32; 4],
    scale: [f32; 2],
    angle: f32,
    depth: f32,
}

impl From<&paper_scene::model::Layer> for LayerPose {
    fn from(layer: &paper_scene::model::Layer) -> Self {
        Self {
            geometry: layer.geometry,
            rect: [layer.center.0, layer.center.1, layer.size.0, layer.size.1],
            scale: [layer.scale.0, layer.scale.1],
            angle: layer.angle,
            depth: layer.depth,
        }
    }
}

impl From<&paper_scene::model::script::Frame> for LayerPose {
    fn from(frame: &paper_scene::model::script::Frame) -> Self {
        Self {
            geometry: frame.geometry,
            rect: frame.rect,
            scale: frame.scale,
            angle: frame.angle,
            depth: frame.depth,
        }
    }
}

fn layer_affine(pose: LayerPose, canvas: (f32, f32)) -> paper_scene::affine::Affine {
    pose.geometry.map_or_else(
        || {
            paper_scene::affine::Affine::new(
                [pose.rect[0], canvas.1 - pose.rect[1]],
                pose.scale,
                -pose.angle,
            )
        },
        |geometry| {
            let t = geometry.transform;
            paper_scene::affine::Affine {
                a: t.a,
                b: -t.b,
                c: -t.c,
                d: t.d,
                x: t.x,
                y: canvas.1 - t.y,
            }
        },
    )
}

fn affine_matrix(t: paper_scene::affine::Affine, depth: f32, depth_scale: f32) -> Mat4 {
    [t.a, t.b, 0.0, 0.0, t.c, t.d, 0.0, 0.0, 0.0, 0.0, depth_scale, 0.0, t.x, t.y, depth, 1.0]
}

fn affine_inverse(t: paper_scene::affine::Affine, depth: f32, depth_scale: f32) -> Mat4 {
    if !depth_scale.is_finite() || depth_scale.abs() < f32::EPSILON {
        return Mat4::default();
    }
    t.inverse().map_or(Mat4::default(), |inverse| {
        affine_matrix(inverse, -depth / depth_scale, depth_scale.recip())
    })
}

fn spring_owner(state: &paper_scene::model::script::Frame, canvas: (f32, f32)) -> Mat4 {
    let pose = LayerPose::from(state);
    let mut owner = pose.geometry.and_then(|geometry| geometry.spatial).unwrap_or_else(|| {
        affine_matrix(
            layer_affine(pose, canvas),
            pose.depth,
            pose.geometry.map_or(1.0, |geometry| geometry.depth_scale),
        )
    });
    let [_, _, _, _, _, _, _, _, _, _, _, _, x, y, z, _] = &mut owner;
    *x = state.origin[0];
    *y = state.origin[1];
    *z = state.depth;
    owner
}

fn layer_model_matrix(pose: LayerPose, canvas: (f32, f32)) -> (Mat4, Mat4) {
    let affine = layer_affine(pose, canvas);
    let depth_scale = pose.geometry.map_or(1.0, |geometry| geometry.depth_scale);
    (
        affine_matrix(affine, pose.depth, depth_scale),
        affine_inverse(affine, pose.depth, depth_scale),
    )
}

fn layer_projection_inverse(inverse: &Mat4, canvas: (f32, f32)) -> Mat4 {
    let (w, h) = (canvas.0 * 0.5, canvas.1 * 0.5);
    let projection = [w, 0.0, 0.0, 0.0, 0.0, h, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, w, h, 0.0, 1.0];
    mat4_mul(inverse, &projection)
}

fn layer_uniforms(
    layer: &paper_scene::model::Layer,
    canvas: (f32, f32),
    lights: ([f32; 3], [f32; 3]),
    world: bool,
) -> std::collections::BTreeMap<String, Vec<f32>> {
    let mut values = pose_uniforms(LayerPose::from(layer), canvas, world);
    values.extend([
        (FX_VIEW_PROJECTION.to_string(), scene_view_projection(canvas).to_vec()),
        ("g_LightAmbientColor".to_string(), lights.0.to_vec()),
        ("g_LightSkylightColor".to_string(), lights.1.to_vec()),
    ]);
    values
}

fn pose_uniforms(pose: LayerPose, canvas: (f32, f32), world: bool) -> BTreeMap<String, Vec<f32>> {
    if world && let Some(values) = spatial_uniforms(pose, canvas) {
        return values;
    }
    let (matrix, inverse) = layer_model_matrix(pose, canvas);
    matrix_uniforms(pose, canvas, matrix, inverse)
}

fn spatial_uniforms(pose: LayerPose, canvas: (f32, f32)) -> Option<BTreeMap<String, Vec<f32>>> {
    let matrix = pose.geometry?.spatial?;
    if matrix.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let inverse = paper_scene::matrix::mat4_inverse(&matrix).unwrap_or_default();
    Some(matrix_uniforms(pose, canvas, matrix, inverse))
}

fn matrix_uniforms(
    pose: LayerPose,
    canvas: (f32, f32),
    matrix: Mat4,
    inverse: Mat4,
) -> BTreeMap<String, Vec<f32>> {
    let (texture_projection, texture_projection_inverse) = effect_texture_projection(pose, canvas);
    let [xx, xy, xz, _, yx, yy, yz, _, zx, zy, zz, _, _, _, _, _] = inverse;
    std::collections::BTreeMap::from([
        (FX_LAYER_MODEL.to_string(), matrix.to_vec()),
        (FX_MODEL_MATRIX.to_string(), matrix.to_vec()),
        (FX_EFFECT_MODEL.to_string(), matrix.to_vec()),
        (FX_MODEL_INVERSE.to_string(), inverse.to_vec()),
        (FX_PROJECTION_INVERSE.to_string(), layer_projection_inverse(&inverse, canvas).to_vec()),
        (FX_TEXTURE_PROJECTION.to_string(), texture_projection.to_vec()),
        (FX_TEXTURE_INVERSE.to_string(), texture_projection_inverse.to_vec()),
        (
            FX_NORMAL_MATRIX.to_string(),
            [xx, yx, zx, 0.0, xy, yy, zy, 0.0, xz, yz, zz, 0.0].to_vec(),
        ),
    ])
}

fn flip_clip_y(view: &mut Mat4) {
    for column in view.chunks_exact_mut(MATRIX_COLUMN_COMPONENTS) {
        column[1] = -column[1];
    }
}

fn layer_normal_matrix(angle: f32) -> [f32; 12] {
    let (sin, cos) = (-angle).sin_cos();
    [cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
}

fn scene_view_projection(canvas: (f32, f32)) -> Mat4 {
    let (w, h) = (canvas.0.max(1.0), canvas.1.max(1.0));
    [2.0 / w, 0.0, 0.0, 0.0, 0.0, 2.0 / h, 0.0, 0.0, 0.0, 0.0, 2.0e-4, 0.0, -1.0, -1.0, 0.5, 1.0]
}

fn effect_texture_projection(pose: LayerPose, canvas: (f32, f32)) -> (Mat4, Mat4) {
    let [_, _, width, height] = pose.rect;
    let size = pose.geometry.map_or([width, height], |geometry| geometry.size);
    let mut affine = if pose.geometry.is_some() {
        layer_affine(pose, canvas)
    } else {
        paper_scene::affine::Affine::new(
            [pose.rect[0], canvas.1 - pose.rect[1]],
            [1.0; 2],
            -pose.angle,
        )
    };
    affine.a *= size[0] * 0.5;
    affine.b *= size[0] * 0.5;
    affine.c *= size[1] * 0.5;
    affine.d *= size[1] * 0.5;
    let to_clip = paper_scene::affine::Affine::new(
        [-1.0; 2],
        [2.0 / canvas.0.max(1.0), 2.0 / canvas.1.max(1.0)],
        0.0,
    );
    let projection = to_clip.then(affine);
    let mut matrix = affine_matrix(projection, 0.5, 1.0);
    matrix[MATRIX_COLUMN_COMPONENTS * 2 + 2] = 0.0;
    (matrix, affine_inverse(projection, 0.0, 1.0))
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

fn particle_model(
    system: &paper_scene::particles::ParticleSystem,
    origin: (f32, f32, f32),
) -> Mat4 {
    let Some(mut matrix) = system.spatial else {
        return model_matrix(origin, system.draw_scale3(), system.angle);
    };
    let [_, _, _, _, _, _, _, _, _, _, _, _, x, y, z, _] = &mut matrix;
    *x += origin.0 - system.origin.0;
    *y += origin.1 - system.origin.1;
    *z += origin.2 - system.origin.2;
    matrix
}

fn particle_inverse(
    system: &paper_scene::particles::ParticleSystem,
    origin: (f32, f32, f32),
) -> Mat4 {
    if system.spatial.is_none() {
        return model_inverse(origin, system.draw_scale3(), system.angle);
    }
    paper_scene::matrix::mat4_inverse(&particle_model(system, origin)).unwrap_or_default()
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

#[derive(Clone, Copy)]
struct ParticleBatches {
    from: usize,
    before: Option<usize>,
    mirrored: bool,
}

fn particle_batches<'a>(
    groups: &'a [ParticleGroup],
    image_orders: &[usize],
    particle_quads: &[OrderedParticleQuad],
    draws: &[OrderedParticleDraw],
    ParticleBatches { from, before, mirrored }: ParticleBatches,
    grab: Option<&'a vk::SceneTarget>,
) -> Vec<vk::ParticleDraw<'a>> {
    let mut out: Vec<vk::ParticleDraw<'a>> = draws
        .iter()
        .filter(|draw| {
            draw.scene_order >= from && before.is_none_or(|limit| draw.scene_order < limit)
        })
        .filter_map(|draw| {
            let group = groups.get(draw.group)?;
            let engine = group.engines.get(draw.engine)?;
            let (vertices, indices) = engine.buffers.as_ref()?;
            let after = image_orders
                .iter()
                .filter(|order| {
                    **order >= from
                        && before.is_none_or(|limit| **order < limit)
                        && **order <= draw.scene_order
                })
                .count()
                + particle_quads
                    .iter()
                    .filter(|quad| {
                        quad.scene_order >= from
                            && before.is_none_or(|limit| quad.scene_order < limit)
                            && (quad.scene_order, quad.renderer)
                                <= (draw.scene_order, (draw.group, draw.renderer))
                    })
                    .count();
            let pipeline = if mirrored { engine.mirror.as_ref()? } else { &engine.pipeline };
            Some(vk::ParticleDraw {
                after,
                pipeline,
                vertices: vertices.buffer,
                indices: indices.buffer,
                index_count: draw.index_count,
                grab: engine.grab_slot.and(grab),
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

const MIN_ROPE_SPAN: f32 = 0.5;

struct RopeDraw {
    model: paper_scene::affine::Affine,
    canvas_height: f32,
    texture: usize,
    blend: vk::SceneBlend,
}

impl RopeDraw {
    fn quad(
        &self,
        start: [f32; 3],
        end: [f32; 3],
        width: f32,
        uv: [f32; 4],
        tint: [f32; 4],
        extra: f32,
    ) -> Option<vk::SceneQuad> {
        let [ax, ay, _] = self.model.point(start);
        let [bx, by, _] = self.model.point(end);
        let (dx, dy) = (bx - ax, by - ay);
        let span = dx.hypot(dy);
        (span >= MIN_ROPE_SPAN).then_some(vk::SceneQuad {
            uv_axes: None,
            uv_padding: false,
            projection: None,
            order_bias: 0,
            rect: [(ax + bx) * 0.5, self.canvas_height - (ay + by) * 0.5, width, span],
            uv,
            tint,
            angle: (-dx).atan2(dy) + extra,
            texture: self.texture,
            blend: self.blend,
        })
    }
}

fn rope_uv(uv: [f32; 4], index: usize, count: usize) -> [f32; 4] {
    let [u, v, width, height] = uv;
    let segments = count.saturating_sub(1).max(1) as f32;
    [u, v + height * (1.0 - index as f32 / segments), width, -height / segments]
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
        paper_scene::particles::Animation::Once => {
            let life = (particle.age / particle.lifetime).clamp(0.0, 1.0);
            let pos =
                (life * count as f32 * group.system.sequence_multiplier).min(count as f32 - 1.0);
            let index = (pos as usize).min(count - 1);
            let blend = pos - index as f32;
            let (uv, ratio, extra) = frame_of(group, index);
            let next = (blend > 0.01 && index + 1 < count).then(|| {
                let (uv2, _, extra2) = frame_of(group, index + 1);
                (uv2, extra2, blend)
            });
            FrameSample { uv, ratio, extra, next }
        }
    }
}

enum FadeFrom {
    Nv12(vk::UploadPath),
    Scene(vk::PresentedTarget),
}

struct FadeSource {
    source: FadeFrom,
    width: u32,
    height: u32,
}

#[derive(Default)]
struct BloomRuntime {
    quad: Option<usize>,
    base_slot: Option<usize>,
    active_slots: Vec<usize>,
    spare_slots: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransitionStyle {
    Sand(i32),
    Effect(usize),
    Fade,
}

struct Group {
    bloom: BloomRuntime,
    volumes: Option<volume::Volumes>,
    lights: Vec<paper_scene::model::lighting::Light>,
    sun_view: paper_scene::model::lighting::SunView,
    fog: paper_scene::model::fog::Volume,
    shadows: Option<shadow::Shadows>,
    cookie_slot: Option<usize>,
    cookie_layout: paper_scene::model::lighting::CookieLayout,
    cookie_current: Option<(String, String)>,
    scripts: Option<script::Scripts>,
    media: Option<media::Media>,
    media_slots: HashMap<paper_scene::model::MediaTexture, usize>,
    static_media: HashSet<paper_scene::model::MediaTexture>,
    media_art: Option<paper_scene::tex::Pixels>,
    target: vk::PresentedTarget,
    scene_snapshot: Option<vk::SceneTarget>,
    scene_grab: Option<vk::SceneTarget>,
    scene_history: Option<vk::SceneHistory>,
    reflection: Option<mesh::Reflection>,
    mirror_fx: Vec<LayerFx>,
    layer_poses: HashMap<usize, LayerPose>,
    reflection_excluded: BTreeSet<usize>,
    unreflected: BTreeSet<String>,
    history_budget: u64,
    effect_budget: u64,
    #[cfg(test)]
    replay_snapshots: bool,
    #[cfg(test)]
    snapshot_draws: Vec<(usize, usize)>,
    frozen: bool,
    resize_inputs_static: bool,
    composed_at: f32,
    from: Option<FadeSource>,
    quads: Vec<vk::SceneQuad>,
    quad_scene_order: Vec<usize>,
    textures: Vec<vk::SceneTexture>,
    videos: Vec<video::VideoTexture>,
    video_device: video::VideoDevice,
    video_objects: HashMap<usize, usize>,
    image_videos: HashMap<usize, video::VideoTexture>,
    passive_video_targets: Vec<(usize, vk::SceneQuad)>,
    base_scene_targets: Vec<BaseLayerTarget>,
    mesh_composites: BTreeSet<(String, CompositeBuffer)>,
    passive_scene_targets: Vec<vk::SceneTarget>,
    fx: Vec<LayerFx>,
    materials: Vec<material::Material>,
    meshes: Vec<mesh::MeshActor>,
    mesh_settings: mesh::Settings,
    scene_view: paper_scene::model::CameraView,
    clip: vk::ClipView,
    screen_quads: Vec<usize>,
    dynamic_depth: bool,
    baked_fx_outputs: Vec<vk::SceneTarget>,
    baked_fx_draws: Vec<(vk::EffectPipeline, vk::QuadBuffer)>,
    baked_fx_quads: Vec<usize>,
    unavailable_effects: Vec<(String, paper_scene::effects::EffectState)>,
    puppets: Vec<PuppetGroup>,
    clip_planes: Option<vk::ClipPlanes>,
    fx_scratch: Vec<vk::SceneTarget>,
    ndc_quad: Option<vk::QuadBuffer>,
    ndc_quad_d3d: Option<vk::QuadBuffer>,
    particles: Vec<ParticleGroup>,
    spare_particle_slots: Vec<usize>,
    particle_frame: u32,
    image_samples: Vec<sampling::ImageSample>,
    animations: Vec<LayerAnimation>,
    script_sounds: Vec<(String, paper_audio::VoiceOp)>,
    audio: Option<paper_audio::Analyser>,
    system_audio: Option<bool>,
    bands: paper_audio::Bands,
    live_text: Vec<LiveText>,
    live_text_due: f32,
    effects_animated: bool,
    mouse: mouse::SceneMouse,
    scene_uniforms: std::collections::BTreeMap<String, Vec<f32>>,
    fixed_daytime: Option<f32>,
    canvas: (f32, f32),
    camera: Option<Camera>,
    camera_rest: Option<Camera>,
    eye_offset: [f32; 3],
    cameras_move: bool,
    camera_source: paper_scene::model::CameraSource,
    eye: [f32; 3],
    clear: [f32; 3],
    clear_enabled: bool,
    composed_once: bool,
    renderer: vk::Renderer,
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

struct SceneRing {
    ptrs: Vec<*mut u8>,
    stride: u32,
    bytes: usize,
}

struct ScenePresentationBuffers {
    rbs: Vec<vk::ReadbackBuf>,
    buffers: Vec<Vec<wayland_client::protocol::wl_buffer::WlBuffer>>,
    rings: Vec<SceneRing>,
}

impl Drop for SceneRing {
    fn drop(&mut self) {
        if let Some(&base) = self.ptrs.first() {
            unsafe { libc::munmap(base.cast(), self.bytes) };
        }
    }
}

struct RetiredScenePresentation {
    generation: u64,
    presenters: Vec<Presenter>,
    memory: ScenePresentationBuffers,
}

impl RetiredScenePresentation {
    fn destroy(self, target: &mut wayland::Target) {
        for buffer in self.memory.buffers.iter().flatten() {
            target.retire_scene_buffer(buffer);
        }
        drop((self.presenters, self.memory));
    }
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
            if let TransitionStyle::Effect(effect) = style {
                self.renderer.prepare_transition(effect)?;
            } else {
                self.renderer.clear_transition()?;
            }
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
            self.renderer.clear_transition()?;
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
    fn orient_camera(&mut self, eye: [f32; 3], center: [f32; 3], up: [f32; 3], fov: Option<f32>) {
        let shift =
            |point: [f32; 3]| std::array::from_fn(|axis| point[axis] + self.eye_offset[axis]);
        let (shaken_eye, shaken_center) = (shift(eye), shift(center));
        if let Some(basis) = paper_scene::model::CameraBasis::of(eye, center, up) {
            self.mesh_settings.orientation = Some(camera_view(shaken_eye, basis));
            self.sun_view.forward = basis.forward;
        }
        if let (Some(authored), Some(rest)) = (&mut self.camera, &mut self.camera_rest) {
            let candidate = Camera { eye, center, up, fov: fov.unwrap_or(rest.fov), ..*rest };
            if candidate.basis().is_some() {
                *rest = candidate;
                *authored = Camera { eye: shaken_eye, center: shaken_center, ..candidate };
                self.eye = shaken_eye;
                self.sun_view.eye = shaken_eye;
            }
        }
        self.mesh_settings.camera = self.camera;
        if let Some(camera) = self.camera {
            for group in &mut self.particles {
                group.screen_basis =
                    screen_basis(camera, &particle_model(&group.system, group.system.origin));
            }
        }
    }

    fn update_eye_offset(&mut self, time: f32) -> Result<()> {
        let Some(offset) = self.mouse.eye_offset(time) else { return Ok(()) };
        let Some(rest) = self.camera_rest else { return Ok(()) };
        if offset == self.eye_offset {
            return Ok(());
        }
        self.eye_offset = offset;
        self.orient_camera(rest.eye, rest.center, rest.up, None);
        if let Some(camera) = self.camera {
            let (view, ..) = authored_perspective(camera, self.canvas, false);
            self.mouse.reproject(&view);
        }
        self.refresh_mesh_views()
    }

    fn set_scene_view(&mut self, view: paper_scene::model::CameraView) {
        self.scene_view = view;
        self.clip = vk::ClipView::new(view, self.canvas);
    }

    fn view_quad(&self, index: usize) -> vk::SceneQuad {
        let view = layer_view(index, self.bloom.quad, &self.screen_quads, self.clip);
        if view == vk::ClipView::IDENTITY {
            return self.quads[index].clone();
        }
        viewed_quad(&self.quads[index], self.canvas, view)
    }

    fn view_quads(&self) -> std::borrow::Cow<'_, [vk::SceneQuad]> {
        if self.scene_view.identity() {
            return std::borrow::Cow::Borrowed(&self.quads);
        }
        (0..self.quads.len()).map(|index| self.view_quad(index)).collect()
    }

    fn view_presentations(&mut self) {
        let clip = self.clip;
        for (index, quad) in self.quads.iter().enumerate() {
            let view = layer_view(index, self.bloom.quad, &self.screen_quads, clip);
            let Some(texture) = self.textures.get_mut(quad.texture) else { continue };
            for effect in texture.effect.iter_mut().chain(&mut texture.effect_tail) {
                effect.set_view(view);
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "dynamic particle staging and rollback form one resource transaction"
    )]
    fn add_dynamic_particle(&mut self, layer: paper_scene::model::ParticleLayer) -> Result<()> {
        let first_texture = self.textures.len();
        let mut groups: Vec<ParticleGroup> = Vec::new();
        let mut media_slots = self.media_slots.clone();
        let mut staged_media = None;
        let mut pending = std::collections::VecDeque::from([(layer, None)]);
        let result = (|| -> Result<()> {
            #[cfg(test)]
            script::tests::fault(script::tests::Fault::Particle)?;
            while let Some((layer, parent)) = pending.pop_front() {
                let index = groups.len();
                let mut system = layer.system;
                let mut texture =
                    system.texture.take().context("dynamic particle texture missing")?;
                let mut media_kinds = texture.system_texture.into_iter().collect::<Vec<_>>();
                if let Some(pass) = &system.pass {
                    media_kinds.extend(
                        pass.textures
                            .iter()
                            .skip(1)
                            .flatten()
                            .filter_map(|extra| extra.system_texture),
                    );
                }
                media_kinds.sort_unstable_by_key(|kind| match kind {
                    paper_scene::model::MediaTexture::Current => 0,
                    paper_scene::model::MediaTexture::Previous => 1,
                });
                media_kinds.dedup();
                let mut interner = TextureInterner::new(Some(self.video_device.shared()));
                interner.media_slots.clone_from(&media_slots);
                let slot = interner.intern_texture(
                    &mut self.renderer,
                    &mut self.textures,
                    &mut texture,
                    vk::MipSampling::Anisotropic,
                )?;
                let ratio = texture.img_width.max(1) as f32 / texture.img_height.max(1) as f32;
                let frames = texture.frames.clone();
                let screen_basis = self.camera.and_then(|camera| {
                    screen_basis(camera, &particle_model(&system, system.origin))
                });
                let engines = particle_engines(
                    &mut system,
                    &texture,
                    &mut self.renderer,
                    &mut self.textures,
                    &mut interner,
                    self.reflection.as_ref().is_some_and(|reflection| reflection.sampled)
                        && !self.unreflected.contains(&layer.id),
                );
                if self.camera.is_some() && !particle_renderers_ready(&system, &engines) {
                    for engine in engines {
                        engine.destroy(&self.renderer);
                    }
                    return Err(anyhow!(DYNAMIC_CAMERA_ENGINE_UNAVAILABLE));
                }
                for child in std::mem::take(&mut system.children).into_iter().rev() {
                    let binding = ParticleParent {
                        index,
                        origin: child.origin,
                        angle: child.angle,
                        scale: child.scale3,
                    };
                    pending.push_front((
                        paper_scene::model::ParticleLayer {
                            parallax: layer.parallax,
                            id: layer.id.clone(),
                            visible: layer.visible,
                            system: child,
                            depth: layer.depth,
                            scene_order: layer.scene_order,
                        },
                        Some(binding),
                    ));
                }
                let prewarm_dt = system.prewarm_step();
                let prewarm_steps = system.prewarm_steps(prewarm_dt);
                let mut dynamic_slots = vec![slot];
                for engine in &engines {
                    dynamic_slots.extend(&engine.extra_textures);
                }
                dynamic_slots.sort_unstable();
                dynamic_slots.dedup();
                dynamic_slots
                    .retain(|slot| !interner.media_slots.values().any(|media| media == slot));
                media_slots = interner.media_slots;
                groups.push(ParticleGroup {
                    parallax: layer.parallax,
                    camera_offset: [0.0; 2],
                    id: layer.id,
                    visible: layer.visible,
                    system,
                    sim: paper_scene::particles::Sim::new(
                        PARTICLE_SEED ^ (self.particles.len() + index + 1) as u32,
                    ),
                    texture: Some(slot),
                    ratio,
                    frames,
                    scene_order: layer.scene_order,
                    engines,
                    screen_basis,
                    parent,
                    followers: paper_scene::particles::Followers::default(),
                    reset: false,
                    prewarm_steps,
                    prewarm_dt,
                    dynamic_slots: Some(dynamic_slots),
                    media_kinds,
                    dynamic_videos: std::mem::take(&mut interner.videos),
                });
            }
            prewarm_particles(
                &mut groups,
                self.particle_frame,
                self.frame_clock(self.composed_at, 0.0),
            );
            if media_slots.len() > self.media_slots.len() && self.media.is_none() {
                staged_media = Some(media::Media::start()?);
            }
            Ok(())
        })();
        if let Err(error) = result {
            for group in &mut groups {
                for engine in group.engines.drain(..) {
                    engine.destroy(&self.renderer);
                }
            }
            for texture in self.textures.drain(first_texture..) {
                self.renderer.destroy_scene_texture(texture);
            }
            return Err(error);
        }
        let mut moved = HashMap::new();
        let fresh_media = media_slots
            .values()
            .filter(|slot| **slot >= first_texture)
            .copied()
            .collect::<Vec<_>>();
        for slot in
            groups.iter().flat_map(|group| group.dynamic_slots.iter().flatten()).chain(&fresh_media)
        {
            if *slot < first_texture || moved.contains_key(slot) {
                continue;
            }
            let Some(spare) = self.spare_particle_slots.pop() else {
                continue;
            };
            let uploaded = std::mem::replace(&mut self.textures[*slot], vk::SceneTexture::vacant());
            self.textures[spare] = uploaded;
            moved.insert(*slot, spare);
        }
        while self.textures.len() > first_texture
            && self
                .textures
                .last()
                .is_some_and(|texture| texture.view == ash::vk::ImageView::null())
        {
            self.textures.pop();
        }
        for group in &mut groups {
            if let Some(slot) = group.texture.and_then(|texture| moved.get(&texture)) {
                group.texture = Some(*slot);
            }
            for engine in &mut group.engines {
                for slot in &mut engine.extra_textures {
                    if let Some(mapped) = moved.get(slot) {
                        *slot = *mapped;
                    }
                }
            }
            for slot in group.dynamic_slots.iter_mut().flatten() {
                if let Some(mapped) = moved.get(slot) {
                    *slot = *mapped;
                }
            }
        }
        for slot in media_slots.values_mut() {
            if let Some(mapped) = moved.get(slot) {
                *slot = *mapped;
            }
        }
        if let Some(media) = staged_media {
            self.media = Some(media);
        }
        self.media_slots = media_slots;
        let offset = self.particles.len();
        for mut group in groups {
            if let Some(parent) = &mut group.parent {
                parent.index += offset;
            }
            self.mouse.enabled |= group.system.follows_mouse();
            self.particles.push(group);
        }
        Ok(())
    }

    fn retire_dynamic_particle(&mut self, id: &str, scripts_media: bool) {
        let mut index = 0;
        while index < self.particles.len() {
            if self.particles[index].id != id || self.particles[index].dynamic_slots.is_none() {
                index += 1;
                continue;
            }
            let group = self.particles.remove(index);
            for engine in group.engines {
                engine.destroy(&self.renderer);
            }
            for slot in group.dynamic_slots.into_iter().flatten() {
                let texture =
                    std::mem::replace(&mut self.textures[slot], vk::SceneTexture::vacant());
                self.renderer.destroy_scene_texture(texture);
                self.spare_particle_slots.push(slot);
            }
            for kind in group.media_kinds {
                if self.static_media.contains(&kind)
                    || self.particles.iter().any(|particle| particle.media_kinds.contains(&kind))
                {
                    continue;
                }
                if let Some(slot) = self.media_slots.remove(&kind) {
                    let texture =
                        std::mem::replace(&mut self.textures[slot], vk::SceneTexture::vacant());
                    self.renderer.destroy_scene_texture(texture);
                    self.spare_particle_slots.push(slot);
                }
            }
            for particle in &mut self.particles[index..] {
                if let Some(parent) = &mut particle.parent
                    && parent.index > index
                {
                    parent.index -= 1;
                }
            }
        }
        while !self.textures.is_empty()
            && self.spare_particle_slots.contains(&(self.textures.len() - 1))
        {
            let slot = self.textures.len() - 1;
            self.textures.pop();
            self.spare_particle_slots.retain(|spare| *spare != slot);
        }
        if self.media_slots.is_empty()
            && !scripts_media
            && !self.media.as_ref().is_some_and(|media| media.controls)
        {
            self.media.take();
            self.media_art.take();
        }
    }

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
                dynamic: fx.intrinsic_dynamic(include_clocks || self.audio.is_some())
                    || (include_clocks
                        && (fx.source_clock || fx.source_pointer || fx.source_keyframes)),
                passthrough: fx.copy_background || fx.text_backdrop.is_some(),
                prefix_dynamic: self
                    .particles
                    .iter()
                    .any(|particle| particle.scene_order < fx.scene_order)
                    || self.puppets.iter().any(|puppet| {
                        self.quad_scene_order[puppet.layer] < fx.scene_order
                            && puppet.animated()
                            && (puppet.revealable || puppet_shown(&self.quads, puppet))
                    }),
            })
            .collect();
        let passive: Vec<PassiveLayerTarget<'_>> = self
            .base_scene_targets
            .iter()
            .map(|layer| PassiveLayerTarget { id: &layer.layer_id, dynamic: layer.dynamic })
            .collect();
        let mut plan = plan_scene_targets(&nodes, &passive)?;
        for (layer, buffer) in &self.mesh_composites {
            if let Some(index) = self.fx.iter().position(|fx| &fx.layer_id == layer) {
                plan.sampled[index].insert(*buffer);
                plan.retain[index] = true;
            }
        }
        Ok(plan)
    }

    fn apply_scene_target_plan(&mut self, plan: &SceneTargetPlan) -> Result<()> {
        anyhow::ensure!(
            plan.reflection_consumers.is_empty() || self.reflection.is_some(),
            "reflection producer missing"
        );
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
                    .create_scene_target_opts(vk::SceneTargetSpec {
                        depth_attachment: true,
                        extent: self.target.extent,
                        format: self.target.format,
                        repeat: false,
                        samples: self.target.samples,
                    })
                    .context("scene-so-far shadow target")?,
            );
        }
        if let Some(snapshot) = &mut self.scene_snapshot {
            if self.dynamic_depth {
                self.renderer.enable_scene_depth(snapshot, vk::DepthOrder::Greater)?;
            }
            snapshot.viewport = self.target.viewport;
            self.renderer.scene_pass_append_for(snapshot.format, snapshot.samples)?;
            for slot in self.fx.iter().filter_map(|fx| fx.snapshot_slot) {
                self.renderer.point_slot_at(&self.textures[slot], snapshot.view);
            }
        }
        let wants_grab = self
            .particles
            .iter()
            .flat_map(|group| &group.engines)
            .any(|engine| engine.grab_slot.is_some())
            || self.fx.iter().any(|fx| fx.mesh_blend.is_some());
        if self.scene_grab.is_none() && wants_grab {
            self.scene_grab = Some(
                self.renderer
                    .create_scene_target(self.target.extent.width, self.target.extent.height)
                    .context("scene grab target")?,
            );
        }
        let history_needed = self.materials.iter().any(material::Material::uses_history)
            || self
                .fx
                .iter()
                .flat_map(|fx| &fx.binds)
                .flatten()
                .any(|(_, binding)| matches!(binding, EffectBind::SceneHistory));
        if history_needed && self.scene_history.is_none() {
            let history = self.renderer.create_scene_history(self.target.extent)?;
            if history.allocation_bytes() > self.history_budget {
                let bytes = history.allocation_bytes();
                return Err(anyhow!(
                    "[native-scene-gap:{HISTORY_BUDGET_GAP}] history needs {bytes} bytes; remaining budget {}",
                    self.history_budget
                ));
            }
            self.scene_history = Some(history);
        }
        if self.scene_history.is_some() {
            for fx in &mut self.fx {
                for (slot, binding) in fx.binds.iter().flatten() {
                    if matches!(binding, EffectBind::SceneHistory) {
                        material::history_mips(&mut fx.uniforms, *slot);
                    }
                }
            }
        }
        Ok(())
    }

    fn configure_scene_targets(&mut self, strict: bool) -> Result<()> {
        loop {
            match self.scene_target_plan(true) {
                Ok(plan) => {
                    let animation = self.scene_target_plan(false)?;
                    self.effects_animated = animation.dynamic.iter().any(|dynamic| *dynamic);
                    self.apply_scene_target_plan(&plan)?;
                    let shadow_bytes =
                        self.scene_snapshot.as_ref().map_or(0, |target| target.allocation_bytes)
                            + self.scene_grab.as_ref().map_or(0, |target| target.allocation_bytes);
                    let passive_target_bytes = self
                        .passive_scene_targets
                        .iter()
                        .map(|target| target.allocation_bytes)
                        .sum::<u64>();
                    tracing::info!(
                        effect_layers = self.fx.len(),
                        dependencies = plan.dependencies.len(),
                        snapshot_consumers =
                            plan.snapshots.iter().filter(|needed| **needed).count(),
                        sampled_layer_targets =
                            plan.sampled.iter().map(BTreeSet::len).sum::<usize>(),
                        shadow_targets = plan.shadow_targets,
                        shadow_bytes,
                        shadow_mib = shadow_bytes as f64 / 1_048_576.0,
                        passive_targets = self.passive_scene_targets.len(),
                        passive_target_bytes,
                        passive_target_mib = passive_target_bytes as f64 / 1_048_576.0,
                        history_bytes = self
                            .scene_history
                            .as_ref()
                            .map_or(0, vk::SceneHistory::allocation_bytes),
                        history_levels =
                            self.scene_history.as_ref().map_or(0, |history| history.levels),
                        "skwd-wall-vk: planned scene-wide render targets"
                    );
                    return Ok(());
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
                            self.drop_twin(fx.quad);
                            self.quads[fx.quad].tint = fx.fallback_tint;
                            if let Some(blend) = chainless_blend(fx.base_blend, fx.bare_blend) {
                                self.quads[fx.quad].blend = blend;
                            }
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

    pub(super) fn sprite_command(
        &mut self,
        object: usize,
        op: paper_scene::script::SpriteOp,
        time: f32,
    ) {
        for animation in &mut self.animations {
            if animation.object == Some(object) {
                animation.apply(op, time);
            }
        }
    }

    pub(super) fn take_script_sounds(&mut self) -> Vec<(String, paper_audio::VoiceOp)> {
        std::mem::take(&mut self.script_sounds)
    }

    pub(super) fn apply_script_sounds(&mut self, ctl: &mut ctl::Ctl) {
        for (id, op) in self.take_script_sounds() {
            ctl.scene_sound(&id, op);
        }
    }

    fn advance_layer_animations(&mut self, time: f32) -> Result<()> {
        for animation in &mut self.animations {
            let frame = *animation.advance(time);
            if frame.image != animation.image {
                match &mut animation.source {
                    FrameSource::Stream(stream) => stream.show(&self.renderer, frame.image)?,
                    FrameSource::Pages(pages) => {
                        let page = &self.textures[pages[frame.image as usize]];
                        let (view, sampler, extent) = (page.view, page.sampler, page.extent);
                        for slot in std::iter::once(animation.slot).chain(animation.mesh_slot) {
                            let slot = &mut self.textures[slot];
                            slot.extent = extent;
                            slot.view = view;
                            slot.sampler = sampler;
                            self.renderer.point_slot_at(slot, view);
                        }
                    }
                }
                animation.image = frame.image;
            }
            if let Some(material) =
                self.materials.iter_mut().find(|material| material.quad == animation.quad)
            {
                material.sprite(&frame);
                continue;
            }
            let quad = match self.fx.iter_mut().find(|fx| fx.quad == animation.quad) {
                Some(fx) if !fx.passthrough => &mut fx.base_quad,
                Some(_) => continue,
                None => &mut self.quads[animation.quad],
            };
            quad.uv = frame.uv;
            quad.uv_axes = Some(frame.axes);
        }
        Ok(())
    }

    fn animated(&self) -> bool {
        self.scene_history.is_some() || self.animated_inputs()
    }

    fn reconstructable(&self) -> bool {
        self.frozen
            || (self.scene_history.is_some()
                && self.resize_inputs_static
                && !self.animated_inputs()
                && self.scripts.is_none()
                && self.media.is_none()
                && self.live_text.is_empty()
                && !self.mouse.enabled)
    }

    fn clear_color(&self) -> [f32; RGBA_CHANNELS] {
        [self.clear[0], self.clear[1], self.clear[2], 1.0]
    }

    fn fx_reads_reflection(&self, index: usize) -> bool {
        self.fx[index].binds.iter().flatten().any(|(_, bind)| *bind == EffectBind::Reflection)
    }

    fn reflection_readers(&self) -> Option<(Vec<usize>, usize)> {
        self.reflection.as_ref().filter(|reflection| reflection.sampled)?;
        if !(0..self.fx.len()).any(|index| self.fx_reads_reflection(index)) {
            return None;
        }
        let (readers, others): (Vec<usize>, Vec<usize>) =
            (0..self.fx.len()).partition(|&index| self.fx_reads_reflection(index));
        let split = others.len();
        Some(([others, readers].concat(), split))
    }

    fn media_animated(&self) -> bool {
        self.scripts.as_ref().is_some_and(script::Scripts::animated)
            || self.camera_source.paths_live()
            || self.videos.iter().any(video::VideoTexture::playing)
            || self.image_videos.values().any(video::VideoTexture::playing)
            || self.animations.iter().any(|animation| animation.frames.len() > 1)
    }

    fn animated_inputs(&self) -> bool {
        self.media_animated()
            || !self.particles.is_empty()
            || self
                .puppets
                .iter()
                .any(|puppet| puppet.animated() && puppet_shown(&self.quads, puppet))
            || self.meshes.iter().any(mesh::MeshActor::animated)
            || self.fx.iter().flat_map(|fx| &fx.passes).any(|pass| pass.time_dependent())
            || self.materials.iter().any(material::Material::frame_dependent)
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
            let replacement = self.renderer.create_text_texture(
                &texture.pixels,
                vk::MipSampling::Disabled,
                texture.text_batch.as_ref(),
            )?;
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
        self.apply_audio_bands();
    }

    fn sync_audio_source(&mut self, ctl: &ctl::Ctl) {
        let Some(enabled) = ctl.system_audio() else {
            return;
        };
        if self.system_audio == Some(enabled) {
            return;
        }
        self.system_audio = Some(enabled);
        let Some(analyser) = self.audio.as_mut() else {
            return;
        };
        if let Err(error) = analyser.set_system_capture(enabled) {
            tracing::warn!(%error, "audio spectrum source unchanged");
        } else {
            self.bands = paper_audio::Bands::default();
            self.apply_audio_bands();
        }
    }

    fn apply_audio_bands(&mut self) {
        write_bands(&mut self.scene_uniforms, &self.bands);
        for fx in self.fx.iter_mut().filter(|fx| fx.audio_dependent) {
            write_bands(&mut fx.uniforms, &self.bands);
        }
    }

    fn step_particles(&mut self, clock: paper_scene::effects::FrameClock) -> Result<()> {
        let dt = clock.dt;
        self.prepare_image_samples(dt)?;
        let mut samples = sampling::ImageQueries::new(
            &mut self.image_samples,
            &mut self.renderer,
            &self.textures,
            &self.puppets,
            false,
        );
        advance_particles(
            &mut self.particles,
            &ParticleStep {
                dt,
                frame: self.particle_frame,
                clock,
                mouse: Some(&self.mouse),
                audio: (
                    self.bands.slice(16, false).unwrap_or(&[]),
                    self.bands.slice(16, true).unwrap_or(&[]),
                ),
                prewarm_step: None,
            },
            &mut |source| samples.query(source),
        );
        samples.finish()
    }

    fn particle_quads(
        &mut self,
        time: f32,
        dt: f32,
    ) -> Result<(Vec<OrderedParticleQuad>, Vec<OrderedParticleDraw>)> {
        let mut out = Vec::new();
        let mut draws = Vec::new();
        let clock = self.frame_clock(time, dt);
        let screen = (self.target.extent.width, self.target.extent.height);
        let canvas = self.canvas;
        let shadow_sample = self.shadows.as_ref().map(shadow::Shadows::sample);
        let cookie_sample = self.cookie_sample().map(|(view, sampler, _)| (view, sampler));
        for (group_index, group) in self.particles.iter_mut().enumerate() {
            let origin = (
                group.system.origin.0 + group.camera_offset[0],
                group.system.origin.1 + group.camera_offset[1],
                group.system.origin.2,
            );
            let Some(sprite) = group.texture.filter(|_| group.visible) else {
                continue;
            };
            let primary = group.system.primary_render();
            let first_draw = draws.len();
            'gpu: for (engine_index, engine) in group.engines.iter_mut().enumerate() {
                let render = engine.render;
                let system = &group.system;
                let count = match engine.mesh {
                    vk::ParticleMesh::SpritePoints => paper_scene::particles::pack_sprite_points(
                        system,
                        &group.sim,
                        group.frames.len(),
                        &mut engine.scratch,
                    ),
                    vk::ParticleMesh::RopePoints => {
                        if render.renderer == paper_scene::particles::Renderer::Ribbon {
                            paper_scene::particles::pack_rope_trail_points(
                                &group.sim,
                                render,
                                &mut engine.scratch,
                            )
                        } else {
                            paper_scene::particles::pack_rope_points(
                                &group.sim,
                                &mut engine.scratch,
                            )
                        }
                    }
                    vk::ParticleMesh::SpriteQuads => paper_scene::particles::pack_sprites(
                        system,
                        &group.sim,
                        group.frames.len(),
                        &mut engine.scratch,
                    ),
                };
                let count = count.min(engine.limit);
                if count == 0 {
                    continue 'gpu;
                }
                if let Err(err) = engine.reserve(&self.renderer, count) {
                    if !engine.failed_growth {
                        tracing::warn!(
                            "skwd-wall-vk: particle buffer growth failed ({err:#}), billboards"
                        );
                        engine.failed_growth = true;
                    }
                    continue 'gpu;
                }
                let Some((vertices, _)) = engine.buffers.as_ref() else { continue 'gpu };
                let floats = count * particle_stride(engine.mesh);
                let bytes: Vec<u8> =
                    engine.scratch[..floats].iter().flat_map(|value| value.to_le_bytes()).collect();
                self.renderer.write_dyn_buffer(vertices, &bytes);
                let d3d_clip = engine.pipeline.hlsl && vk::d3d_clip_enabled();
                let authored =
                    self.camera.map(|camera| authored_perspective(camera, canvas, d3d_clip));
                let mut view_projection = if let Some((projection, ..)) = authored {
                    projection
                } else if system.perspective {
                    scene_perspective(
                        [0.0, 0.0, canvas.0, canvas.1],
                        self.mesh_settings.perspective_fov,
                        d3d_clip,
                    )
                } else {
                    scene_ortho(canvas, d3d_clip)
                };
                self.clip.apply(&mut view_projection, !d3d_clip);
                let model = particle_model(system, origin);
                let mut overrides = std::collections::BTreeMap::new();
                overrides.insert(
                    "g_ModelViewProjectionMatrix".to_string(),
                    mat4_mul(&view_projection, &model).to_vec(),
                );
                overrides.insert("g_ModelMatrix".to_string(), model.to_vec());
                overrides.insert(
                    FX_MODEL_INVERSE.to_string(),
                    particle_inverse(system, origin).to_vec(),
                );
                let basis = group.screen_basis.unwrap_or(ScreenBasis {
                    forward: [0.0, 0.0, 1.0],
                    right: [1.0, 0.0, 0.0],
                    up: [0.0, 1.0, 0.0],
                });
                overrides.insert("g_OrientationUp".to_string(), basis.up.to_vec());
                overrides.insert("g_OrientationRight".to_string(), basis.right.to_vec());
                overrides.insert("g_OrientationForward".to_string(), basis.forward.to_vec());
                overrides.insert(
                    FX_VIEW_UP.to_string(),
                    authored.map_or([0.0, 1.0, 0.0], |(_, _, up)| up).to_vec(),
                );
                overrides.insert(
                    FX_VIEW_RIGHT.to_string(),
                    authored.map_or([1.0, 0.0, 0.0], |(_, right, _)| right).to_vec(),
                );
                overrides.insert("g_EyePosition".to_string(), self.eye.to_vec());
                let ribbon = render.renderer == paper_scene::particles::Renderer::Ribbon;
                let trail_phase = group.sim.history.first().map_or(0.0, |trail| trail.phase());
                let segment_max = if ribbon {
                    system.ribbon_trail().segments
                } else {
                    system.max_count.saturating_sub(1)
                };
                overrides.insert(
                    "g_RenderVar0".to_string(),
                    if render.renderer == paper_scene::particles::Renderer::Trail {
                        vec![
                            render.trail.length,
                            render.trail.max_length,
                            render.trail.min_length,
                            0.0,
                        ]
                    } else if ribbon && render.trail.uv_scroll {
                        let segments = segment_max.saturating_sub(1) as f32;
                        vec![segments, 0.0, trail_phase, segments * render.trail.uv_factor()]
                    } else if ribbon {
                        vec![0.0, 0.0, trail_phase, segment_max as f32 - 0.5]
                    } else {
                        vec![
                            render.trail.length,
                            render.trail.max_length,
                            trail_phase,
                            segment_max as f32,
                        ]
                    },
                );
                overrides.insert("g_RenderVar1".to_string(), engine.render_var1.to_vec());
                for (name, value) in &self.scene_uniforms {
                    overrides.entry(name.clone()).or_insert_with(|| value.clone());
                }
                let base = &self.textures[sprite];
                let mut views = vec![(base.view, base.sampler)];
                let mut sizes = vec![Some(engine.texture_size)];
                for &slot in &engine.extra_textures {
                    let texture = &self.textures[slot];
                    views.push((texture.view, texture.sampler));
                    sizes.push(None);
                }
                if let (Some(slot), Some(grab)) = (engine.grab_slot, self.scene_grab.as_ref()) {
                    if views.len() <= slot {
                        views.resize(slot + 1, views[0]);
                        sizes.resize(slot + 1, None);
                    }
                    views[slot] = (grab.view, grab.sampler);
                    sizes[slot] = Some(size4(grab.extent));
                }
                let shadow_sample = engine.shadow_slot.zip(shadow_sample).map(|(slot, sample)| {
                    self.renderer.effect_depth_sample(&engine.pipeline, slot, sample)
                });
                let reflection_sample = self.reflection.as_ref().map(|reflection| {
                    let target = reflection.target();
                    (target.view, target.sampler)
                });
                for (slot, sample) in [
                    (engine.shadow_slot, shadow_sample),
                    (engine.cookie_slot, cookie_sample),
                    (engine.reflection_slot, reflection_sample),
                ] {
                    if let Some(slot) = slot {
                        if views.len() <= slot {
                            views.resize(slot + 1, views[0]);
                            sizes.resize(slot + 1, None);
                        }
                        if let Some(sample) = sample {
                            views[slot] = sample;
                        }
                    }
                }
                let uniforms = engine.meta.uniform_bytes_with(
                    clock, None, screen.0, screen.1, screen, &sizes, &overrides, d3d_clip,
                );
                self.renderer.write_effect_inputs(&engine.pipeline, &views, &uniforms);
                if let Some(mirror) = &engine.mirror {
                    overrides.insert(
                        FX_MODEL_PROJECTION.to_string(),
                        reflection::mirrored_projection(&view_projection, &model),
                    );
                    reflection::mirror_overrides(&mut overrides, &reflection::PARTICLE_VECTORS);
                    let uniforms = engine.meta.uniform_bytes_with(
                        clock, None, screen.0, screen.1, screen, &sizes, &overrides, d3d_clip,
                    );
                    self.renderer.write_effect_inputs(mirror, &views, &uniforms);
                }
                for (renderer_index, authored) in std::iter::once(primary)
                    .chain(system.extra_renderers.iter().copied())
                    .enumerate()
                {
                    if authored != render {
                        continue;
                    }
                    draws.push(OrderedParticleDraw {
                        scene_order: group.scene_order,
                        group: group_index,
                        renderer: renderer_index,
                        engine: engine_index,
                        index_count: if engine.mesh == vk::ParticleMesh::SpriteQuads {
                            (count * paper_scene::particles::SPRITE_INDICES) as u32
                        } else {
                            count as u32
                        },
                    });
                }
            }
            if draws.len() - first_draw == group.system.extra_renderers.len() + 1 {
                continue;
            }
            let system = &group.system;
            let blend = if system.additive { vk::SceneBlend::Add } else { vk::SceneBlend::Alpha };
            let scale = system.draw_scale3();
            let model = paper_scene::affine::Affine::new(
                [origin.0, origin.1],
                [scale[0], scale[1]],
                system.angle,
            );
            let rope = RopeDraw { model, canvas_height: self.canvas.1, texture: sprite, blend };
            for (renderer_index, render) in
                std::iter::once(primary).chain(system.extra_renderers.iter().copied()).enumerate()
            {
                if draws[first_draw..].iter().any(|draw| draw.renderer == renderer_index) {
                    continue;
                }
                if render.renderer == paper_scene::particles::Renderer::Rope {
                    let count = group.sim.particles.len();
                    let segments =
                        paper_scene::particles::spline_segments(group.sim.particles.iter());
                    for (index, segment) in segments.enumerate() {
                        let [start, particle, ..] = segment;
                        if !particle.visible()
                            || !segment
                                .iter()
                                .all(|point| paper_scene::particles::finite(point.pos))
                        {
                            continue;
                        }
                        let sample = particle_frame(group, particle);
                        let width = particle.size * system.draw_scale();
                        let tint = [
                            particle.color[0],
                            particle.color[1],
                            particle.color[2],
                            particle.alpha,
                        ];
                        if let Some(quad) = rope.quad(
                            start.pos,
                            particle.pos,
                            width,
                            rope_uv(sample.uv, index, count),
                            tint,
                            sample.extra,
                        ) {
                            out.push(OrderedParticleQuad {
                                scene_order: group.scene_order,
                                renderer: (group_index, renderer_index),
                                quad,
                            });
                        }
                    }
                    continue;
                }
                if render.renderer == paper_scene::particles::Renderer::Ribbon {
                    for (particle, trail) in
                        group.sim.particles.iter().zip(group.sim.history.iter())
                    {
                        if !particle.visible() {
                            continue;
                        }
                        let sample = particle_frame(group, particle);
                        let ([u, v, uv_width, uv_height], extra) = (sample.uv, sample.extra);
                        let segments = trail.points().count().saturating_sub(1);
                        for (index, segment) in
                            paper_scene::particles::spline_segments(trail.points()).enumerate()
                        {
                            let [start, end, ..] = segment;
                            if !segment
                                .iter()
                                .all(|point| paper_scene::particles::finite(point.pos))
                            {
                                continue;
                            }
                            let taper = if render.trail.uv_scroll && render.trail.fade_alpha {
                                1.0 - index as f32 / segments.max(1) as f32
                            } else {
                                1.0
                            };
                            let tint = [
                                particle.color[0],
                                particle.color[1],
                                particle.color[2],
                                particle.alpha * taper,
                            ];
                            if let Some(quad) = rope.quad(
                                start.pos,
                                end.pos,
                                particle.size * system.draw_scale(),
                                [
                                    u,
                                    v + uv_height * index as f32 / segments.max(1) as f32,
                                    uv_width,
                                    uv_height / segments.max(1) as f32,
                                ],
                                tint,
                                extra,
                            ) {
                                out.push(OrderedParticleQuad {
                                    scene_order: group.scene_order,
                                    renderer: (group_index, renderer_index),
                                    quad,
                                });
                            }
                        }
                    }
                    continue;
                }
                let trail = render.renderer == paper_scene::particles::Renderer::Trail;
                for particle in &group.sim.particles {
                    if !particle.drawable()
                        || trail && !paper_scene::particles::finite(particle.vel)
                    {
                        continue;
                    }
                    let sample = particle_frame(group, particle);
                    let size =
                        particle.size * paper_scene::particles::SPRITE_EXTENT * system.draw_scale();
                    let [x, y, _] = model.point(particle.pos);
                    let (width, height, angle) = if trail {
                        let (vx, vy) = (particle.vel[0], particle.vel[1]);
                        let speed = vx.hypot(vy);
                        if speed < 1.0e-4 {
                            (size, size, particle.angle)
                        } else {
                            let stretch = (speed * render.trail.length)
                                .min(render.trail.max_length)
                                .max(render.trail.min_length);
                            (size, size * stretch * sample.ratio, (-vx).atan2(-vy))
                        }
                    } else {
                        (size, size, particle.angle)
                    };
                    let tint =
                        [particle.color[0], particle.color[1], particle.color[2], particle.alpha];
                    let rect = [x, self.canvas.1 - y, width, height];
                    let fade = sample.next.map_or(0.0, |(_, _, blend)| blend);
                    out.push(OrderedParticleQuad {
                        scene_order: group.scene_order,
                        renderer: (group_index, renderer_index),
                        quad: vk::SceneQuad {
                            uv_axes: None,
                            uv_padding: false,
                            projection: None,
                            order_bias: 0,
                            rect,
                            uv: sample.uv,
                            tint: [tint[0], tint[1], tint[2], tint[3] * (1.0 - fade)],
                            angle: angle + sample.extra,
                            texture: sprite,
                            blend,
                        },
                    });
                    if let Some((uv, extra, mix)) = sample.next {
                        out.push(OrderedParticleQuad {
                            scene_order: group.scene_order,
                            renderer: (group_index, renderer_index),
                            quad: vk::SceneQuad {
                                uv_axes: None,
                                uv_padding: false,
                                projection: None,
                                order_bias: 0,
                                rect,
                                uv,
                                tint: [tint[0], tint[1], tint[2], tint[3] * mix],
                                angle: angle + extra,
                                texture: sprite,
                                blend,
                            },
                        });
                    }
                }
            }
        }
        draws.sort_by_key(|draw| (draw.scene_order, draw.group, draw.renderer));
        if !self.scene_view.identity() {
            let view = self.clip;
            for entry in &mut out {
                entry.quad = viewed_quad(&entry.quad, canvas, view);
            }
        }
        Ok((out, draws))
    }

    fn puppet_frame(&self) -> Result<()> {
        for puppet in self.puppets.iter().filter(|puppet| puppet_shown(&self.quads, puppet)) {
            self.renderer.update_scene_mesh(
                &puppet.mesh,
                &puppet.pose.positions,
                &puppet.alphas,
                puppet.puppet.size,
            )?;
        }
        Ok(())
    }

    fn submit_scene(&mut self) -> Result<()> {
        if let Some(history) = &self.scene_history {
            self.renderer.record_scene_history(&self.target, history)?;
        }
        self.renderer.submit_scene_batch()?;
        self.complete_image_samples();
        Ok(())
    }

    fn update_scene_history(&mut self) -> Result<()> {
        if self.scene_history.is_some() {
            self.renderer.begin_scene_batch()?;
            self.submit_scene()?;
        }
        Ok(())
    }

    fn bind_mesh_composites(
        &mut self,
        targets: &mesh::CompositeTargets,
        batched: bool,
    ) -> Result<()> {
        let actors = || {
            self.meshes.iter().chain(self.scripts.iter().flat_map(script::Scripts::shadow_meshes))
        };
        let dynamic = |layer: &str| {
            self.fx.iter().any(|fx| {
                fx.layer_id == layer && (fx.dependency_dynamic || fx.frame_state_dependent())
            }) || self
                .base_scene_targets
                .iter()
                .any(|target| target.layer_id == layer && target.dynamic)
        };
        let stale = actors().any(|actor| actor.stale_composites(targets));
        let refresh = actors().any(|actor| {
            actor.reflects_composites()
                && (actor.stale_composites(targets) || actor.composite_layers().any(dynamic))
        });
        if stale {
            if batched {
                self.renderer.flush_scene_batch()?;
            }
            for actor in self
                .meshes
                .iter_mut()
                .chain(self.scripts.iter_mut().flat_map(script::Scripts::shadow_meshes_mut))
            {
                actor.bind_composites(&self.renderer, targets, self.reflection.as_ref());
            }
        }
        if refresh && let Some(reflection) = &mut self.reflection {
            reflection.ready = false;
        }
        Ok(())
    }

    fn compose(&mut self, time: f32, dt: f32) -> Result<()> {
        if self.frozen {
            return Ok(());
        }
        self.particle_frame = self.particle_frame.wrapping_add(1);
        self.composed_at = time;
        let logical_canvas = [self.canvas.0.max(1.0), self.canvas.1.max(1.0)];
        let clock = self.frame_clock(time, dt);
        let screen = (self.target.extent.width, self.target.extent.height);
        for video in self.videos.iter_mut().chain(self.image_videos.values_mut()) {
            video.advance(f64::from(time))?;
        }
        for group in &mut self.particles {
            for video in &mut group.dynamic_videos {
                video.advance(f64::from(time))?;
            }
        }
        self.advance_live_text(time)?;
        self.advance_audio(dt);
        self.advance_media()?;
        let scripted = self.scripts.as_ref().and_then(|scripts| scripts.host.as_ref()).is_some();
        let quads = &self.quads;
        for puppet in &mut self.puppets {
            puppet.shown = puppet_shown(quads, puppet);
            if puppet.shown {
                puppet.sample_authored(time, dt, scripted)?;
            }
        }
        self.step_particles(clock)?;
        self.advance_scripts(time, dt)?;
        let quads = &self.quads;
        for puppet in self.puppets.iter_mut().filter(|puppet| !puppet.shown) {
            if puppet_shown(quads, puppet) {
                puppet.sample_authored(time, dt, scripted)?;
            }
        }
        for (index, quad) in &self.passive_video_targets {
            self.renderer.render_scene(
                &self.passive_scene_targets[*index],
                [0.0; 4],
                std::slice::from_ref(quad),
                &self.textures,
            )?;
        }
        if let Some(host) = self.scripts.as_ref().and_then(|scripts| scripts.host.as_ref()) {
            let quads = &self.quads;
            for puppet in self.puppets.iter_mut().filter(|puppet| puppet_shown(quads, puppet)) {
                puppet.samples.clear();
                puppet.samples.extend(host.puppet_states(&puppet.id).into_iter().filter_map(
                    |state| match state.sample {
                        paper_scene::puppet::playback::Sample::Frame(seconds) => {
                            Some((state.layer, seconds))
                        }
                        paper_scene::puppet::playback::Sample::Rest
                        | paper_scene::puppet::playback::Sample::Retired => None,
                    },
                ));
                let base = paper_scene::puppet::skin_samples(&puppet.puppet.mesh, &puppet.samples);
                if let Some((_, world)) = host.puppet_bone_world(&puppet.id) {
                    let mut reference = Vec::new();
                    paper_scene::puppet::sampled_world(
                        &puppet.puppet.mesh,
                        &puppet.samples,
                        &mut reference,
                        None,
                    )?;
                    puppet.pose = paper_scene::puppet::skin_edited(
                        &puppet.puppet.mesh,
                        &base,
                        &reference,
                        &world,
                    )?;
                } else {
                    puppet.pose = base;
                }
            }
        }
        let host = self.scripts.as_ref().and_then(|scripts| scripts.host.as_ref());
        let quads = &self.quads;
        for puppet in self.puppets.iter_mut().filter(|puppet| puppet_shown(quads, puppet)) {
            paper_scene::puppet::alpha::vertex_alphas(
                &puppet.puppet.mesh,
                &puppet.samples,
                &mut puppet.alphas,
            );
            let edited = host.and_then(|host| host.puppet_morph_weights(&puppet.id));
            paper_scene::puppet::morph::apply(
                &puppet.puppet.mesh,
                &mut puppet.pose,
                &puppet.samples,
                edited.as_deref(),
                &mut puppet.alphas,
            )?;
            if let Some(order) = &mut puppet.order
                && order.update(&puppet.puppet.mesh, &puppet.samples)
            {
                if puppet.mesh.clipped() {
                    let plan = clip::plan(&puppet.puppet.mesh, Some(order));
                    self.renderer
                        .update_scene_indices(&puppet.mesh, &clip::bytes(&plan.indices))?;
                    puppet.mesh.update_clip(plan);
                } else {
                    let bytes = order.bytes(&puppet.puppet.mesh.indices);
                    self.renderer.update_scene_indices(&puppet.mesh, &bytes)?;
                }
            }
        }
        for fx in &self.fx {
            if let Some(blend) = fx.base_blend {
                let quad = &mut self.quads[fx.quad];
                quad.blend = if quad.tint[3] > 0.0 { blend } else { vk::SceneBlend::Alpha };
            }
        }
        self.advance_layer_animations(time)?;
        for actor in self
            .meshes
            .iter_mut()
            .chain(self.scripts.iter_mut().flat_map(script::Scripts::shadow_meshes_mut))
        {
            if actor.visible {
                actor.animate(&self.renderer, &mut self.textures[actor.texture], time)?;
                actor.tick(&self.renderer, clock);
            }
        }
        self.update_eye_offset(time)?;
        self.advance_mouse(time, dt);
        self.view_presentations();
        self.refresh_shadows()?;
        self.refresh_volumes()?;
        self.render_shadows()?;
        let clear = self.clear_color();
        if let Some(reflection) = &mut self.reflection
            && !reflection.cleared
        {
            self.renderer.render_scene_with_canvas(
                reflection.target(),
                logical_canvas,
                clear,
                &[],
                &self.textures,
                &[],
            )?;
            reflection.cleared = true;
        }
        self.material_frame(clock)?;
        self.prepare_image_samples(dt)?;
        let volume_projection =
            self.volumes.as_ref().map(|_| self.volume_projection()).transpose()?;
        let mut volumes_composed = false;
        let cookie_sample = self.cookie_sample();
        let debug_fx = std::env::var("SKWD_VK_FX_DEBUG").is_ok() && !self.fx.is_empty();
        let batched = !debug_fx;
        if batched {
            self.renderer.begin_scene_batch()?;
        }
        self.puppet_frame()?;
        let (particle_quads, particle_draws) = self.particle_quads(time, dt)?;
        self.render_texture_channels(clock, batched)?;
        self.record_image_samples(batched)?;
        let mut scene_targets = mesh::CompositeTargets::new();
        for layer in &self.base_scene_targets {
            let sample = (layer.view, layer.sampler, layer.extent);
            scene_targets.insert((layer.layer_id.clone(), CompositeBuffer::A), sample);
            scene_targets.insert((layer.layer_id.clone(), CompositeBuffer::B), sample);
        }
        let readers = self.reflection_readers();
        let frame = reflection::ReflectionFrame {
            canvas: logical_canvas,
            clock,
            screen,
            batched,
            debug_fx,
        };
        if !self.fx.is_empty() {
            let ordered_fx = readers.as_ref().map_or_else(
                || self.fx.windows(2).all(|pair| pair[0].scene_order <= pair[1].scene_order),
                |(order, _)| {
                    order
                        .windows(2)
                        .all(|pair| self.fx[pair[0]].scene_order <= self.fx[pair[1]].scene_order)
                },
            );
            let incremental_snapshots = !debug_fx && ordered_fx;
            #[cfg(test)]
            let incremental_snapshots = incremental_snapshots && !self.replay_snapshots;
            #[cfg(test)]
            self.snapshot_draws.clear();
            let mut snapshot_before = None;
            for position in 0..self.fx.len() {
                let fx_index = readers.as_ref().map_or(position, |(order, _)| order[position]);
                if readers.as_ref().is_some_and(|(_, split)| *split == position) {
                    self.record_reflection(&particle_draws, frame)?;
                }
                if self.scripts.is_some()
                    && self.quads[self.fx[fx_index].quad].tint[3] <= 0.0
                    && self.fx[fx_index].sampled_targets.is_empty()
                {
                    continue;
                }
                if self
                    .materials
                    .iter()
                    .any(|material| material.bypasses_effects(&self.fx[fx_index]))
                {
                    let fx = &mut self.fx[fx_index];
                    self.quads[fx.quad].texture = fx.base_quad.texture;
                    self.quads[fx.quad].tint = fx.fallback_tint;
                    self.textures[fx.slot].effect = None;
                    fx.presentation = None;
                    fx.presentation_inputs.clear();
                    fx.output = None;
                    continue;
                }
                let hidden_text = {
                    let fx = &self.fx[fx_index];
                    fx.text_backdrop.is_some()
                        && fx.owner.iter().all(|owner| !fx.effects[*owner].visible)
                };
                if let Some(blend) = self.fx[fx_index].bare_blend {
                    let fx = &self.fx[fx_index];
                    self.quads[fx.quad].blend = bare_quad_blend(blend, &fx.owner, &fx.effects);
                }
                if hidden_text {
                    let fx = &mut self.fx[fx_index];
                    self.quads[fx.quad].texture = fx.base_quad.texture;
                    self.quads[fx.quad].tint = fx.fallback_tint;
                    self.textures[fx.slot].effect = None;
                    fx.presentation = None;
                    fx.presentation_inputs.clear();
                    fx.output = None;
                    if fx.sampled_targets.is_empty() {
                        continue;
                    }
                }
                let (needs_snapshot, scene_order) = {
                    let fx = &self.fx[fx_index];
                    (fx.snapshot, fx.scene_order)
                };
                let snapshot = self
                    .scene_snapshot
                    .as_ref()
                    .map(|target| (target.view, target.sampler, target.extent));
                if needs_snapshot {
                    let from = if incremental_snapshots { snapshot_before.unwrap_or(0) } else { 0 };
                    let target = self
                        .scene_snapshot
                        .as_ref()
                        .context("scene target plan requires a shadow snapshot")?;
                    let prefix = ordered_scene_quads(
                        &self.view_quads(),
                        &self.quad_scene_order,
                        &particle_quads,
                        from,
                        Some(scene_order),
                    );
                    let batches = particle_batches(
                        &self.particles,
                        &self.quad_scene_order,
                        &particle_quads,
                        &particle_draws,
                        ParticleBatches { from, before: Some(scene_order), mirrored: false },
                        self.scene_grab.as_ref(),
                    );
                    #[cfg(test)]
                    self.snapshot_draws.push((prefix.len(), batches.len()));
                    let clear = self.clear_color();
                    if debug_fx {
                        self.renderer.render_scene_with_canvas(
                            target,
                            logical_canvas,
                            clear,
                            &prefix,
                            &self.textures,
                            &batches,
                        )?;
                    } else if incremental_snapshots && snapshot_before.is_some() {
                        self.renderer.record_scene_append_with_canvas(
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
                    if self.fx[fx_index].layer_id == paper_scene::model::bloom::ID
                        && let (Some(volumes), Some(projection)) =
                            (&self.volumes, &volume_projection)
                    {
                        if debug_fx {
                            self.renderer.begin_scene_batch()?;
                        }
                        volumes.record(
                            &mut self.renderer,
                            target,
                            projection,
                            volume::Sources {
                                lights: &self.lights,
                                fog: self.fog,
                                eye: self.camera.map_or_else(
                                    || orthographic_eye(self.canvas),
                                    |camera| camera.eye,
                                ),
                                shadows: self.shadows.as_ref(),
                                cookies: cookie_sample.map(|(view, sampler, _)| {
                                    (&self.cookie_layout, (view, sampler))
                                }),
                            },
                        )?;
                        if debug_fx {
                            self.renderer.submit_scene_batch()?;
                        }
                        volumes_composed = true;
                    }
                    snapshot_before = Some(scene_order);
                }
                if batched && !ordered_fx {
                    self.renderer.flush_scene_batch()?;
                }
                self.run_fx_chain(fx_chain::FxChain {
                    index: fx_index,
                    mirror: false,
                    hidden_text,
                    snapshot,
                    clock,
                    screen,
                    canvas: logical_canvas,
                    debug_fx,
                    scene_targets: Some(&mut scene_targets),
                })?;
            }
        }
        self.bind_mesh_composites(&scene_targets, batched)?;
        if readers.is_none() {
            self.record_reflection(&particle_draws, frame)?;
        }
        let clear = self.clear_color();
        let quads = ordered_scene_quads(
            &self.view_quads(),
            &self.quad_scene_order,
            &particle_quads,
            0,
            None,
        );
        let batches = particle_batches(
            &self.particles,
            &self.quad_scene_order,
            &particle_quads,
            &particle_draws,
            ParticleBatches { from: 0, before: None, mirrored: false },
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
            )?;
            self.renderer.begin_scene_batch()?;
            if !volumes_composed {
                self.record_volumes()?;
            }
            self.submit_scene()
        } else {
            let layers = vk::SceneLayers {
                quads: &quads,
                textures: &self.textures,
                particles: &batches,
                plain: &[],
            };
            let clear = (self.clear_enabled || !self.composed_once).then_some(clear);
            self.renderer.record_scene_layers(&self.target, logical_canvas, clear, layers)?;
            self.composed_once = true;
            if !volumes_composed {
                self.record_volumes()?;
            }
            self.submit_scene()
        }
    }

    fn drop_from(&mut self) {
        if let Some(source) = self.from.take() {
            match source.source {
                FadeFrom::Nv12(up) => self.renderer.destroy_upload_path(up),
                FadeFrom::Scene(rt) => rt.destroy(&self.renderer.device),
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
        if let Some(vertices) = fx.verts {
            self.renderer.destroy_quad_buffer(vertices);
        }
    }

    fn retain_layer_fx_output(&mut self, mut fx: LayerFx) -> vk::SceneTarget {
        self.baked_fx_quads.push(fx.quad);
        if let Some(blend) = fx.mesh_blend {
            let pipeline = fx.pipelines.remove(blend.pass);
            self.baked_fx_draws.push((pipeline, fx.verts.take().expect("effect vertices")));
        }
        if let Some(slot) = fx.presentation {
            let pipeline = fx.pipelines[slot].scene.take().expect("presentation pipeline");
            self.baked_fx_draws.push((*pipeline, fx.verts.take().expect("effect vertices")));
            for target in std::mem::take(&mut fx.presentation_inputs) {
                if Some(target) != fx.output {
                    self.baked_fx_outputs.push(
                        fx.target_storage_mut(target).take_owned().expect("presentation target"),
                    );
                }
            }
        }
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
            if fx.output.is_some() {
                fx.target_bytes().context("effect output missing before static bake")?;
            }
        }

        let old_fx = std::mem::take(&mut self.fx);
        let mut live_fx = Vec::with_capacity(old_fx.len());
        let mut report = EffectBakeReport {
            retained_bytes: self
                .materials
                .iter()
                .map(material::Material::target_bytes)
                .sum::<u64>()
                + self
                    .reflection
                    .as_ref()
                    .map_or(0, |reflection| reflection.target().allocation_bytes)
                + self.volumes.as_ref().map_or(0, volume::Volumes::allocation_bytes)
                + self.shadows.as_ref().map_or(0, shadow::Shadows::allocation_bytes)
                + self.clip_planes.as_ref().map_or(0, vk::ClipPlanes::allocation_bytes),
            ..EffectBakeReport::default()
        };
        for fx in old_fx {
            let Some(bytes) = fx.target_bytes() else {
                report.transient_bytes += fx.allocation_bytes();
                live_fx.push(fx);
                continue;
            };
            if fx.passes.iter().any(paper_scene::effects::PassMeta::time_dependent)
                || fx.frame_state_dependent()
                || fx.retain_targets
                || (fx.layer_id == paper_scene::model::bloom::ID && !self.lights.is_empty())
                || (self.camera.is_some() && (self.scripts.is_some() || self.mouse.pending()))
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
        self.release_empty_fx_quads();
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
            if fx.output.is_none()
                || fx.visibility_dynamic
                || fx.effects.iter().any(|effect| !effect.visible)
            {
                report.fallback_layers += 1;
                plans.push(None);
                candidates.push(Vec::new());
                continue;
            }
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
                .filter(|target| {
                    !fx.presentation_inputs.contains(target)
                        && scratch_candidate(&plan, &fx.sampled_targets, *target)
                })
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
        let owned = self.fx.iter().map(LayerFx::allocation_bytes).sum::<u64>();
        owned
            + self.volumes.as_ref().map_or(0, volume::Volumes::allocation_bytes)
            + self.shadows.as_ref().map_or(0, shadow::Shadows::allocation_bytes)
            + self.reflection.as_ref().map_or(0, |reflection| reflection.target().allocation_bytes)
            + self.materials.iter().map(material::Material::target_bytes).sum::<u64>()
            + self.baked_fx_outputs.iter().map(|target| target.allocation_bytes).sum::<u64>()
            + self.fx_scratch.iter().map(|target| target.allocation_bytes).sum::<u64>()
            + self.clip_planes.as_ref().map_or(0, vk::ClipPlanes::allocation_bytes)
    }

    fn release_composition_inputs(&mut self) {
        self.volumes.take();
        self.shadows.take();
        self.lights.clear();
        self.media.take();
        self.media_art.take();
        for material in &self.materials {
            self.textures[material.slot].effect = None;
            self.textures[material.slot].effect_tail.clear();
        }
        for material in std::mem::take(&mut self.materials) {
            drop(material);
        }
        self.reflection.take();
        self.meshes.clear();
        for fx in
            std::mem::take(&mut self.fx).into_iter().chain(std::mem::take(&mut self.mirror_fx))
        {
            self.destroy_layer_fx(fx);
        }
        for (pipeline, vertices) in std::mem::take(&mut self.baked_fx_draws) {
            self.renderer.destroy_effect_pipeline(pipeline);
            self.renderer.destroy_quad_buffer(vertices);
        }
        self.baked_fx_quads.clear();
        for target in std::mem::take(&mut self.baked_fx_outputs) {
            self.renderer.destroy_scene_target(target);
        }
        self.puppets.clear();
        if let Some(planes) = self.clip_planes.take() {
            self.renderer.destroy_clip_planes(planes);
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
        for animation in &mut self.animations {
            if matches!(animation.source, FrameSource::Stream(_)) {
                animation.source = FrameSource::Pages(Vec::new());
            }
        }
        for texture in std::mem::take(&mut self.textures) {
            self.renderer.destroy_scene_texture(texture);
        }
        self.videos.clear();
        self.image_videos.clear();
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
        self.scene_history = None;
    }

    fn destroy(mut self) {
        if let Some(host) = self.scripts.as_mut().and_then(|s| s.host.as_mut()) {
            host.destroy();
        }
        self.drop_from();
        self.image_samples.clear();
        self.release_composition_inputs();
        for group in std::mem::take(&mut self.particles) {
            for engine in group.engines {
                engine.destroy(&self.renderer);
            }
        }
        self.target.destroy(&self.renderer.device);
    }
}

fn quads(model: &SceneModel, layer_slots: &[usize], canvas: (f32, f32)) -> Vec<vk::SceneQuad> {
    model
        .layers
        .iter()
        .zip(layer_slots)
        .map(|(layer, &slot)| {
            let mut quad = layer_quad(layer, slot, model.canvas);
            if let Some(camera) = model.camera
                && matches!(
                    layer.kind,
                    paper_scene::model::LayerKind::Image | paper_scene::model::LayerKind::Text
                )
                && layer.geometry.is_some()
            {
                let (view, ..) = authored_perspective(camera, canvas, false);
                quad.projection = Some(camera_quad(LayerPose::from(layer), model.canvas, &view));
            }
            quad
        })
        .collect()
}

fn camera_quad(pose: LayerPose, canvas: (f32, f32), view: &Mat4) -> [[f32; 4]; 3] {
    let model = pose
        .geometry
        .and_then(|geometry| geometry.spatial)
        .unwrap_or_else(|| layer_model_matrix(pose, canvas).0);
    let mut matrix = mat4_mul(view, &model);
    let [_, _, width, height] = pose.rect;
    let size = pose.geometry.map_or([width, height], |geometry| geometry.size);
    for (column, scale) in
        matrix.chunks_exact_mut(MATRIX_COLUMN_COMPONENTS).zip([size[0], -size[1]])
    {
        for value in column {
            *value *= scale;
        }
    }
    vk::LayerProjection::from(matrix).pack()
}

const NORMAL_COLOR_BLEND: u32 = 0;

fn chainless_blend(
    base: Option<vk::SceneBlend>,
    bare: Option<vk::SceneBlend>,
) -> Option<vk::SceneBlend> {
    bare.or(base.map(|_| vk::SceneBlend::Alpha))
}

fn bare_quad_blend(
    bare: vk::SceneBlend,
    owner: &[usize],
    effects: &[paper_scene::effects::EffectState],
) -> vk::SceneBlend {
    if owner.iter().all(|owner| !effects[*owner].visible) { bare } else { vk::SceneBlend::Alpha }
}

fn base_scene_blend(layer: &paper_scene::model::Layer) -> Option<vk::SceneBlend> {
    if !layer.base_scene_input || layer.color_blend != NORMAL_COLOR_BLEND {
        return None;
    }
    let [effect] = layer.effects.as_slice() else {
        return None;
    };
    let [pass] = effect.passes.as_slice() else {
        return None;
    };
    match pass.blend {
        paper_scene::effects::PassBlend::Copy => Some(vk::SceneBlend::CopyRgb),
        paper_scene::effects::PassBlend::AlphaCoverage => Some(vk::SceneBlend::AlphaCoverage),
        paper_scene::effects::PassBlend::Alpha => Some(vk::SceneBlend::Alpha),
        paper_scene::effects::PassBlend::Add => Some(vk::SceneBlend::Add),
        paper_scene::effects::PassBlend::Screen => None,
    }
}

fn layer_quad(layer: &paper_scene::model::Layer, slot: usize, canvas: (f32, f32)) -> vk::SceneQuad {
    let base_blend = base_scene_blend(layer).filter(|_| layer.visible);
    vk::SceneQuad {
        uv_axes: None,
        uv_padding: false,
        projection: layer.geometry.map(|geometry| geometry.projection(canvas)),
        order_bias: 0,
        rect: [layer.center.0, layer.center.1, layer.size.0, layer.size.1],
        uv: layer.uv(),
        tint: if !layer.effects.is_empty() {
            [1.0, 1.0, 1.0, if layer.visible { 1.0 } else { 0.0 }]
        } else {
            layer_tint(layer)
        },
        angle: layer.angle,
        texture: slot,
        blend: if let Some(blend) = base_blend {
            blend
        } else if layer.additive() {
            vk::SceneBlend::Add
        } else {
            vk::SceneBlend::Alpha
        },
    }
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
    from: usize,
    before: Option<usize>,
) -> Vec<vk::SceneQuad> {
    let mut ordered: Vec<(usize, usize, vk::SceneQuad)> = image_quads
        .iter()
        .zip(image_order)
        .enumerate()
        .filter(|(_, (_, order))| **order >= from && before.is_none_or(|limit| **order < limit))
        .map(|(serial, (quad, order))| (*order, serial, quad.clone()))
        .collect();
    let offset = ordered.len();
    ordered.extend(
        particle_quads
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.scene_order >= from && before.is_none_or(|limit| entry.scene_order < limit)
            })
            .map(|(serial, entry)| (entry.scene_order, offset + serial, entry.quad.clone())),
    );
    ordered.sort_by_key(|(order, serial, quad)| (*order, quad.order_bias, *serial));
    ordered.into_iter().map(|(_, _, quad)| quad).collect()
}

fn pass_blend(blend: paper_scene::effects::PassBlend) -> vk::SceneBlend {
    match blend {
        paper_scene::effects::PassBlend::Copy => vk::SceneBlend::Copy,
        paper_scene::effects::PassBlend::AlphaCoverage => vk::SceneBlend::AlphaCoverage,
        paper_scene::effects::PassBlend::Alpha => vk::SceneBlend::Alpha,
        paper_scene::effects::PassBlend::Add => vk::SceneBlend::Add,
        paper_scene::effects::PassBlend::Screen => vk::SceneBlend::Screen,
    }
}

fn composite_tint(layer: &paper_scene::model::Layer) -> [f32; 4] {
    if layer.material.is_some() || layer.base_scene_input {
        return [1.0; RGBA_CHANNELS];
    }
    [layer.color[0], layer.color[1], layer.color[2], layer.alpha]
}

fn layer_tint(layer: &paper_scene::model::Layer) -> [f32; 4] {
    let mut tint = composite_tint(layer);
    if !layer.visible {
        tint[ALPHA_CHANNEL] = 0.0;
    }
    tint
}

const SCENE_FX_LIMIT: &str = "SKWD_VK_SCENE_FX";
const MATERIAL_DESCRIPTOR_SETS: usize = 4;
const RGBA_CHANNELS: usize = 4;
const ALPHA_CHANNEL: usize = RGBA_CHANNELS - 1;
const HIDDEN_WHITE: [f32; RGBA_CHANNELS] = [1.0, 1.0, 1.0, 0.0];
const RGB_CHANNELS: usize = 3;

fn chain_admission_order(
    layers: &[paper_scene::model::Layer],
    scripted: bool,
    dependencies: &BTreeSet<String>,
) -> Vec<usize> {
    let mut order: Vec<usize> = (0..layers.len())
        .filter(|&index| {
            let layer = &layers[index];
            (layer.visible || scripted || dependencies.contains(&layer.id))
                && !layer.effects.is_empty()
        })
        .collect();
    order.sort_by_key(|&index| !layers[index].visible);
    order
}

fn pixel_means(pixels: &[u8]) -> (u64, u64) {
    let count = (pixels.len() / RGBA_CHANNELS).max(1) as u64;
    let mut color = 0;
    let mut alpha = 0;
    for pixel in pixels.chunks_exact(RGBA_CHANNELS) {
        let [r, g, b, a] = pixel else { unreachable!() };
        color += u64::from(*r) + u64::from(*g) + u64::from(*b);
        alpha += u64::from(*a);
    }
    (color / (RGB_CHANNELS as u64 * count), alpha / count)
}

fn fallback_tint(layer: &paper_scene::model::Layer) -> [f32; 4] {
    if layer.passthrough || (layer.solid && !layer.effects.is_empty()) {
        HIDDEN_WHITE
    } else {
        layer_tint(layer)
    }
}

fn dynamic_chain_limit() -> usize {
    std::env::var(SCENE_FX_LIMIT).ok().and_then(|text| text.parse().ok()).unwrap_or(usize::MAX)
}

fn reflected_chain(layer: &paper_scene::model::Layer, unreflected: &BTreeSet<String>) -> bool {
    paper_scene::model::reflection_rerun(layer)
        && !paper_scene::model::reflection_gap(layer)
        && !unreflected.contains(&layer.id)
}

fn effect_textures(
    layer: &paper_scene::model::Layer,
) -> Vec<Vec<Option<paper_scene::model::Texture>>> {
    layer
        .effects
        .iter()
        .flat_map(|effect| &effect.passes)
        .map(|pass| pass.textures.clone())
        .collect()
}

fn restore_effect_textures(
    layer: &mut paper_scene::model::Layer,
    textures: Vec<Vec<Option<paper_scene::model::Texture>>>,
) {
    for (pass, saved) in
        layer.effects.iter_mut().flat_map(|effect| &mut effect.passes).zip(textures)
    {
        pass.textures = saved;
    }
}

fn layer_view(
    quad: usize,
    bloom: Option<usize>,
    screen: &[usize],
    view: vk::ClipView,
) -> vk::ClipView {
    if bloom == Some(quad) || screen.contains(&quad) { vk::ClipView::IDENTITY } else { view }
}

fn viewed_quad(quad: &vk::SceneQuad, canvas: (f32, f32), view: vk::ClipView) -> vk::SceneQuad {
    let mut projection = quad_projection(quad, canvas);
    view.apply_rows(&mut projection, true);
    vk::SceneQuad { projection: Some(projection), ..quad.clone() }
}

fn quad_projection(quad: &vk::SceneQuad, canvas: (f32, f32)) -> [[f32; 4]; 3] {
    quad.projection.unwrap_or_else(|| {
        let [x, y, width, height] = quad.rect;
        paper_scene::model::Geometry {
            spatial: None,
            depth_scale: 1.0,
            transform: paper_scene::affine::Affine::new([x, y], [1.0; 2], quad.angle),
            size: [width, height],
            offset: [0.0; 2],
        }
        .projection(canvas)
    })
}

fn inverse_projection([x, y, w]: [[f32; 4]; 3]) -> Option<[[f32; 4]; 3]> {
    let [xu, xv, _, xt] = x.map(f64::from);
    let [yu, yv, _, yt] = y.map(f64::from);
    let [wu, wv, _, wt] = w.map(f64::from);
    let a = yv * wt - yt * wv;
    let b = xt * wv - xv * wt;
    let c = xv * yt - xt * yv;
    let determinant = xu * a + yu * b + wu * c;
    let magnitude = xu.hypot(yu).hypot(wu) * xv.hypot(yv).hypot(wv) * xt.hypot(yt).hypot(wt);
    if !determinant.is_finite()
        || !magnitude.is_finite()
        || determinant.abs() <= magnitude * f64::from(f32::EPSILON)
    {
        return None;
    }
    let inverse = [
        [a, b, 0.0, c],
        [yt * wu - yu * wt, xu * wt - xt * wu, 0.0, xt * yu - xu * yt],
        [yu * wv - yv * wu, xv * wu - xu * wv, 0.0, xu * yv - xv * yu],
    ]
    .map(|row| row.map(|value| (value / determinant) as f32));
    inverse.iter().flatten().all(|value| value.is_finite()).then_some(inverse)
}

fn background_projection(quad: &vk::SceneQuad, canvas: (f32, f32)) -> [[f32; 4]; 3] {
    let Some(mut inverse) = inverse_projection(quad_projection(quad, canvas)) else {
        return [
            [0.0; MATRIX_COLUMN_COMPONENTS],
            [0.0; MATRIX_COLUMN_COMPONENTS],
            [0.0, 0.0, 0.0, 1.0],
        ];
    };
    for row in &mut inverse {
        row[0] *= 2.0;
        row[1] *= 2.0;
    }
    for row in &mut inverse[..2] {
        for value in row {
            *value *= 2.0;
        }
    }
    inverse
}

fn passive_target_quad(
    layer: &paper_scene::model::Layer,
    texture: usize,
    extent: ash::vk::Extent2D,
) -> vk::SceneQuad {
    let (uvw, uvh) = layer.texture.uv_scale();
    vk::SceneQuad {
        uv_axes: None,
        uv_padding: false,
        projection: None,
        order_bias: 0,
        rect: [
            extent.width as f32 * 0.5,
            extent.height as f32 * 0.5,
            extent.width as f32,
            extent.height as f32,
        ],
        uv: [0.0, 0.0, uvw, uvh],
        tint: composite_tint(layer),
        angle: 0.0,
        texture,
        blend: vk::SceneBlend::Copy,
    }
}

fn uncopyable_provider<'a>(
    layers: &'a [paper_scene::model::Layer],
    referenced: &BTreeSet<String>,
) -> Option<&'a paper_scene::model::Layer> {
    layers.iter().find(|layer| {
        layer.material.as_ref().is_some_and(|material| !material.copyable())
            && referenced.contains(&layer.id)
    })
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
    let scale = (f64::from(limit) / f64::from(raw_w).max(f64::from(raw_h))).min(output_scale);
    let raster_w = ((f64::from(raw_w) * scale).round() as u32).max(1);
    let raster_h = ((f64::from(raw_h) * scale).round() as u32).max(1);
    SceneDimensions { logical, raster: (raster_w, raster_h) }
}

fn effect_uv_inset(layer: &paper_scene::model::Layer) -> [f32; 2] {
    if layer.passthrough
        || layer.copy_background
        || layer.material.is_some()
        || layer.base_scene_input
        || layer.solid
        || layer.texture.system_texture.is_some()
        || !matches!(
            layer.kind,
            paper_scene::model::LayerKind::Image | paper_scene::model::LayerKind::Text
        )
    {
        return [0.0; 2];
    }
    [
        paper_scene::model::IMAGE_UV_INSET / layer.texture.img_width.max(1) as f32,
        paper_scene::model::IMAGE_UV_INSET / layer.texture.img_height.max(1) as f32,
    ]
}

fn image_effect_dimensions(layer: &paper_scene::model::Layer) -> Option<(u32, u32)> {
    let source_image = layer.kind == paper_scene::model::LayerKind::Image
        && layer.texture_name.is_some()
        && layer.texture.system_texture.is_none()
        && layer.texture.video.is_none()
        && layer.texture.frames.is_empty()
        && layer.composition_size.is_none()
        && !layer.passthrough
        && !layer.copy_background
        && !layer.spritesheet
        && layer.puppet.is_none()
        && layer.material.as_ref().is_none_or(|material| material.skin.is_none());
    source_image.then(|| {
        bounded_effect_dimensions(
            f64::from(layer.texture.img_width),
            f64::from(layer.texture.img_height),
        )
    })
}

fn layer_effect_dimensions(
    layer: &paper_scene::model::Layer,
    dimensions: SceneDimensions,
) -> (u32, u32) {
    if layer.visible
        && let Some((width, height)) = layer.composition_size
    {
        return bounded_effect_dimensions(f64::from(width.abs()), f64::from(height.abs()));
    }
    if let Some(size) = image_effect_dimensions(layer) {
        return size;
    }
    if layer.is_text() {
        return bounded_effect_dimensions(
            f64::from(layer.texture.width),
            f64::from(layer.texture.height),
        );
    }
    effect_dimensions_for(layer.size, dimensions)
}

fn deferred_composition_layers(
    layers: &[paper_scene::model::Layer],
    dimensions: SceneDimensions,
) -> Vec<String> {
    layers
        .iter()
        .filter(|layer| !layer.visible && !layer.effects.is_empty())
        .filter_map(|layer| {
            let (width, height) = layer.composition_size?;
            let authored =
                bounded_effect_dimensions(f64::from(width.abs()), f64::from(height.abs()));
            (layer_effect_dimensions(layer, dimensions) != authored).then(|| layer.id.clone())
        })
        .collect()
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

fn scene_limit() -> u32 {
    std::env::var(SCENE_LIMIT_ENV)
        .ok()
        .and_then(|text| text.parse::<u32>().ok())
        .unwrap_or(SCENE_DEFAULT_LIMIT)
        .max(SCENE_MIN_LIMIT)
}

fn scene_dimensions(model: &SceneModel, outputs: &[(u32, u32)], mode: FillMode) -> SceneDimensions {
    if model.camera.is_some()
        && let Some(dimensions) = camera_dimensions(outputs, scene_limit())
    {
        return dimensions;
    }
    let dimensions = scene_dimensions_for_outputs(model.canvas, scene_limit(), outputs, mode);
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

fn camera_dimensions(outputs: &[(u32, u32)], limit: u32) -> Option<SceneDimensions> {
    let &(width, height) = outputs
        .iter()
        .filter(|(width, height)| *width > 0 && *height > 0)
        .max_by_key(|(width, height)| u64::from(*width) * u64::from(*height))?;
    Some(scene_dimensions_for((width as f32, height as f32), limit))
}

fn release_texture_payload(texture: &mut paper_scene::model::Texture) -> usize {
    let pixels = std::mem::take(&mut texture.pixels);
    let video = texture.video.take();
    let sole = |count: usize, bytes: usize| if count == 1 { bytes } else { 0 };
    sole(std::sync::Arc::strong_count(&pixels), pixels.bytes())
        + texture.text_batch.take().map_or(0, |batch| batch.pixels.bytes())
        + match video {
            Some(paper_scene::model::Video::Bytes(payload)) => {
                sole(std::sync::Arc::strong_count(&payload), payload.len())
            }
            _ => 0,
        }
}

fn release_model_texture_payloads(model: &mut SceneModel) -> usize {
    let mut released = 0;
    for layer in &mut model.layers {
        released += release_texture_payload(&mut layer.texture);
        if let Some(material) = &mut layer.material {
            for texture in material.pass.textures.iter_mut().flatten() {
                released += release_texture_payload(texture);
            }
        }
        for effect in &mut layer.effects {
            for pass in &mut effect.passes {
                for texture in pass.textures.iter_mut().flatten() {
                    released += release_texture_payload(texture);
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
    if strict && !compatibility.has_no_known_gaps() {
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

fn validate_scene_content(model: &SceneModel) -> Result<()> {
    if !model.layers.is_empty()
        || !model.meshes.is_empty()
        || !model.particles.is_empty()
        || model.scripts.as_ref().is_some_and(paper_scene::script::SceneScripts::has_created_models)
        || (model.background_only && model.skipped.is_empty())
    {
        return Ok(());
    }
    Err(anyhow!(
        "scene has no renderable image layers ({})",
        model.skipped.iter().take(4).cloned().collect::<Vec<_>>().join("; ")
    ))
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

const EFFECT_REDUCTION_CODE: &str = "effect-reduction";

fn record_effect_skip(
    strict: bool,
    skipped: &mut Vec<String>,
    layer: &str,
    reason: &str,
) -> Result<()> {
    skipped.push(format!("{layer}: {reason}"));
    tracing::warn!(code = EFFECT_REDUCTION_CODE, layer, reason, "skwd-wall-vk: effect reduced");
    validate_pipeline_skip(strict, layer, reason)
}

fn composite_layer(binding: &EffectBind) -> Option<String> {
    match binding {
        EffectBind::LayerComposite { layer, .. } => Some(layer.clone()),
        _ => None,
    }
}

fn create_puppets(
    renderer: &mut vk::Renderer,
    model: &mut SceneModel,
    layer_slots: &mut [usize],
    textures: &mut Vec<vk::SceneTexture>,
    extent: ash::vk::Extent2D,
) -> Result<(Vec<PuppetGroup>, Option<vk::ClipPlanes>)> {
    let mut clipping = clip::Clipping::new(model, extent);
    let mut outputs = Vec::new();
    let scripted = paper_scene::model::scene_scripted(model.scripts.is_some(), &model.keyframes);
    for (index, layer) in model.layers.iter_mut().enumerate() {
        if !layer.visible && !model.image_sources.contains(&layer.id) && !scripted {
            continue;
        }
        if layer.material.as_ref().is_some_and(|material| material.skin.is_some()) {
            continue;
        }
        let Some(puppet) = layer.puppet.take() else {
            continue;
        };
        let texture = layer_slots[index];
        let source = textures
            .get(texture)
            .ok_or_else(|| anyhow!("puppet atlas slot missing for {}", layer.name))?;
        let mut slot = renderer
            .create_view_slot()
            .with_context(|| format!("puppet slot for {}", layer.name))?;
        slot.extent = source.extent;
        slot.view = source.view;
        slot.sampler = source.sampler;
        slot.premultiplied = source.premultiplied;
        let order = paper_scene::puppet::draw_order::Sorter::of(&puppet.mesh);
        let clip = clipping.clip(renderer, &puppet, textures, order.as_ref());
        let padded = clip.as_ref().map(|clip| clip::padded(&puppet.mesh, clip.indices()));
        let mut mesh = renderer
            .create_scene_mesh(
                puppet.mesh.vertices.iter().map(|vertex| {
                    [
                        vertex.position[0] / puppet.size.0.abs().max(1.0),
                        -vertex.position[1] / puppet.size.1.abs().max(1.0),
                        vertex.uv[0],
                        vertex.uv[1],
                        1.0,
                    ]
                }),
                vk::SceneIndices::U16(padded.as_deref().unwrap_or(&puppet.mesh.indices)),
            )
            .with_context(|| format!("puppet mesh for {}", layer.name))?;
        mesh.cull = puppet.cull;
        if let Some(clip) = clip {
            mesh.set_clip(clip);
        }
        slot.use_scene_mesh(&mesh);
        renderer.point_slot_at(&slot, slot.view);
        layer_slots[index] = textures.len();
        textures.push(slot);
        tracing::info!(
            layer = %layer.name,
            vertices = puppet.mesh.vertices.len(),
            indices = puppet.mesh.indices.len(),
            "skwd-wall-vk: assembled puppet mesh"
        );
        let pose = paper_scene::puppet::skin(&puppet.mesh, &puppet.layers, 0.0);
        let spring = if model.scripts.is_none() {
            paper_scene::puppet::spring::Simulation::of(&puppet.mesh)
        } else {
            None
        };
        outputs.push(PuppetGroup {
            id: layer.id.clone(),
            puppet,
            spring,
            spring_world: Vec::new(),
            samples: Vec::new(),
            alphas: Vec::new(),
            pose,
            mesh,
            order,
            texture,
            layer: index,
            revealable: scripted,
            shown: false,
        });
    }
    Ok((outputs, clipping.into_planes()))
}

fn binds_history<'a>(binds: impl IntoIterator<Item = &'a (usize, EffectBind)>) -> bool {
    binds.into_iter().any(|(_, binding)| matches!(binding, EffectBind::SceneHistory))
}

fn build_group(
    sd: &shared::SharedDevice,
    model: &mut SceneModel,
    strict: bool,
    outputs: &[(u32, u32)],
    mode: FillMode,
) -> Result<Group> {
    let video_device = video::VideoDevice::new(sd)?;
    #[cfg(target_os = "linux")]
    let _admission = crate::health::SceneBudget::new();
    let _compilation = paper_scene::shader::CompilationSession::new()?;
    let resize_inputs_static = resize::static_inputs(model);
    let strict = strict || model.custom_shaders;
    let scripted = paper_scene::model::scene_scripted(model.scripts.is_some(), &model.keyframes);
    let viewport = mesh::viewport(model, outputs, mode)?;
    if let Some(view) = viewport {
        let limits = unsafe { sd.instance.get_physical_device_properties(sd.phys) }.limits;
        anyhow::ensure!(
            view.supported(&limits),
            "orthographic mesh viewport exceeds device limits"
        );
    }
    let dimensions = scene_dimensions(model, outputs, mode);
    for layer in &mut model.layers {
        layer.fit_viewport((dimensions.logical[0], dimensions.logical[1]), dimensions.raster);
    }
    let (canvas_w, canvas_h) =
        viewport.map_or(dimensions.raster, |view| (view.raster[0], view.raster[1]));
    let mut renderer =
        shared_renderer(sd, canvas_w, canvas_h).context("shared scene composition renderer")?;
    renderer.set_scene_multisample(!model.meshes.is_empty());
    let model_depth = !model.meshes.is_empty()
        || model.layers.iter().any(|layer| {
            layer.compose_depth
                || layer.material.as_ref().is_some_and(|material| material.state.has_depth())
        });
    if model.hdr || model_depth {
        renderer.configure_scene(vk::SceneConfig {
            format: if model.hdr {
                ash::vk::Format::R16G16B16A16_SFLOAT
            } else {
                ash::vk::Format::R8G8B8A8_UNORM
            },
            depth: model_depth.then_some(vk::DepthOrder::Greater),
        })?;
    }
    let shadows =
        shadow::Shadows::new(&renderer, &model.lights, scripted && !model.lights.is_empty())?;
    let fx_limit = dynamic_chain_limit();
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
    let material_sets: usize = model
        .layers
        .iter()
        .filter_map(|layer| layer.material.as_ref())
        .map(|material| material.pass.textures.len() + MATERIAL_DESCRIPTOR_SETS)
        .sum();
    let mesh_sets: usize = model.meshes.iter().map(|mesh| mesh.mesh.parts.len()).sum();
    let mesh_textures: usize = model
        .meshes
        .iter()
        .flat_map(|mesh| &mesh.materials)
        .flat_map(|material| &material.pass.textures)
        .filter(|texture| texture.is_some())
        .count();
    let reflection_sets: usize = if model.reflection.bound {
        model
            .meshes
            .iter()
            .filter(|layer| layer.reflected)
            .map(|layer| layer.mesh.parts.len() * 2)
            .sum()
    } else {
        0
    };
    let sets = reflection_sets
        + material_sets
        + mesh_sets
        + mesh_textures
        + model.layers.len()
        + model.layers.iter().filter(|layer| layer.is_text()).count()
        + model
            .layers
            .iter()
            .map(|layer| layer.texture.pages.len() + usize::from(!layer.texture.pages.is_empty()))
            .sum::<usize>()
        + fx_layers * 4
        + fx_textures
        + fx_fbos
        + model.particles.len()
        + 16;
    renderer.ensure_scene_pool(sets.max(1) as u32)?;
    let mut target = renderer
        .create_scene_target_opts(vk::SceneTargetSpec {
            depth_attachment: true,
            extent: ash::vk::Extent2D { width: canvas_w, height: canvas_h },
            format: renderer.scene_format(),
            repeat: false,
            samples: renderer.scene_sample_count(),
        })
        .context("scene target")?;
    target.viewport = viewport;
    #[cfg(test)]
    resize::tests::after_target()?;
    let mut reflection = match mesh::Reflection::create(&mut renderer, model, &target) {
        Ok(reflection) => reflection,
        Err(error) => {
            renderer.destroy_scene_target(target);
            return Err(error);
        }
    };
    let mut textures = Vec::with_capacity(model.layers.len());
    let mut texture_interner = TextureInterner::new(Some(sd));
    let mut layer_slots = Vec::with_capacity(model.layers.len());
    let mut layer_sources = HashMap::new();
    let cookie_current = cookie::selection(&model.lights);
    let mutable_cookie = scripted && model.lights.iter().any(|light| light.cone.is_some());
    let (cookie_slot, cookie_layout) = if let Some(cookies) = model.cookies.take() {
        let sampling = if cookies.layout.packed() {
            vk::MipSampling::Disabled
        } else {
            vk::MipSampling::Linear
        };
        let texture = renderer.create_scene_texture_pixels(
            &cookies.texture.pixels,
            cookies.texture.clamp,
            cookies.texture.nearest,
            sampling,
        )?;
        let slot = textures.len();
        textures.push(texture);
        (Some(slot), cookies.layout)
    } else if mutable_cookie || model.lights.iter().any(|light| light.cookie.is_some()) {
        let slot = textures.len();
        textures.push(renderer.create_scene_texture(1, 1, &[0; RGBA_CHANNELS])?);
        (Some(slot), paper_scene::model::lighting::CookieLayout::default())
    } else {
        (None, paper_scene::model::lighting::CookieLayout::default())
    };
    let mut hidden_target_texture = None;
    for layer in &mut model.layers {
        if !layer.visible
            && !scripted
            && !model.image_sources.contains(&layer.id)
            && !model.dependencies.contains(&layer.id)
        {
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
        if layer.is_text() {
            let slot = if layer.script_text.is_some() || layer.live_text.is_some() {
                let texture = renderer.create_text_texture(
                    &layer.texture.pixels,
                    layer_sampling(true),
                    layer.texture.text_batch.as_ref(),
                )?;
                let slot = textures.len();
                textures.push(texture);
                slot
            } else {
                let mut key = TextureKey::take(&mut layer.texture, layer_sampling(true));
                key.text = true;
                match texture_interner.intern_key(&mut renderer, &mut textures, key) {
                    Ok(slot) => slot,
                    Err((err, key)) => {
                        layer.texture.pixels = key.pixels;
                        layer.texture.text_batch = key.text_batch;
                        return Err(err);
                    }
                }
            };
            layer_slots.push(slot);
            continue;
        }
        let dynamic = model.scripts.as_ref().is_some_and(|host| {
            layer
                .object_index(&host.scene)
                .is_some_and(|slot| host.created_layer_slots().contains(&slot))
        });
        if dynamic {
            let texture = renderer.create_scene_texture_pixels(
                &layer.texture.pixels,
                layer.texture.clamp,
                layer.texture.nearest,
                layer_sampling(false),
            )?;
            layer_slots.push(textures.len());
            textures.push(texture);
            continue;
        }
        let sampling = layer_sampling(layer.is_text());
        let base = texture_interner
            .intern_texture(&mut renderer, &mut textures, &mut layer.texture, sampling)
            .with_context(|| format!("texture for layer {}", layer.name))?;
        if let Some(cells) = layer.texture.stream.clone().filter(|_| layer.atlas_frames().is_some())
        {
            let stream = sprite_stream::SpriteStream::create(&renderer, cells)
                .with_context(|| format!("sprite stream for layer {}", layer.name))?;
            let mut slot = renderer.create_view_slot()?;
            slot.extent = stream.frame().extent;
            slot.view = stream.frame().view;
            slot.sampler = textures[base].sampler;
            renderer.point_slot_at(&slot, slot.view);
            layer_sources.insert(layer_slots.len(), FrameSource::Stream(Box::new(stream)));
            layer_slots.push(textures.len());
            textures.push(slot);
        } else if layer.texture.pages.is_empty() {
            layer_slots.push(base);
        } else {
            let mut pages = vec![base];
            for pixels in std::mem::take(&mut layer.texture.pages) {
                let key = TextureKey {
                    pixels,
                    clamp: layer.texture.clamp,
                    nearest: layer.texture.nearest,
                    sampling,
                    text: false,
                    text_batch: None,
                };
                pages.push(texture_interner.intern_rgba(&mut renderer, &mut textures, key)?);
            }
            let mut slot = renderer.create_view_slot()?;
            slot.extent = textures[base].extent;
            slot.view = textures[base].view;
            slot.sampler = textures[base].sampler;
            renderer.point_slot_at(&slot, slot.view);
            layer_sources.insert(layer_slots.len(), FrameSource::Pages(pages));
            layer_slots.push(textures.len());
            textures.push(slot);
        }
    }
    let puppeted: Vec<bool> = model.layers.iter().map(|layer| layer.puppet.is_some()).collect();
    let (puppets, clip_planes) =
        create_puppets(&mut renderer, model, &mut layer_slots, &mut textures, target.extent)?;
    let mut quads = quads(model, &layer_slots, (dimensions.logical[0], dimensions.logical[1]));
    let animations: Vec<LayerAnimation> = model
        .layers
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| {
            LayerAnimation::new(
                layer,
                index,
                model.scripts.as_ref().and_then(|scripts| layer.object_index(&scripts.scene)),
                flat_slot(&puppets, index, &layer_slots),
                layer_puppet(&puppets, index).map(|_| layer_slots[index]),
                layer_sources.remove(&index),
            )
        })
        .collect();
    for animation in &animations {
        quads[animation.quad].uv = animation.frames[0].uv;
        quads[animation.quad].uv_axes = Some(animation.frames[0].axes);
        quads[animation.quad].uv_padding = model.layers[animation.quad].pad_uv;
    }
    let video_objects = model
        .layers
        .iter()
        .enumerate()
        .filter_map(|(quad, layer)| {
            let object = layer.object_index(&model.scripts.as_ref()?.scene)?;
            let view = textures[layer_slots[quad]].view;
            let video =
                texture_interner.videos.iter().position(|video| video.target().view == view)?;
            Some((object, video))
        })
        .collect();
    let mut materials = material::Build {
        mirrored: reflection.as_ref().is_some_and(|reflection| reflection.sampled),
        unreflected: model.unreflected.clone(),
        renderer: &mut renderer,
        interner: &mut texture_interner,
        textures: &mut textures,
        slots: &mut layer_slots,
        quads: &mut quads,
        screen: target.extent,
        world: model.camera.is_some(),
    }
    .load(model)?;
    if !animations.is_empty() {
        tracing::info!("skwd-wall-vk: animated image layers {}", animations.len());
    }
    let mut quad_scene_order = model.layers.iter().map(|layer| layer.scene_order).collect();
    let clip = vk::ClipView::new(model.camera_view, (dimensions.logical[0], dimensions.logical[1]));
    let layer_poses = model
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| layer.geometry.is_some())
        .map(|(index, layer)| (index, LayerPose::from(layer)))
        .collect();
    let mut reflection_excluded: BTreeSet<usize> = model
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| {
            model.unreflected.contains(&layer.id) || paper_scene::model::reflection_gap(layer)
        })
        .map(|(index, _)| index)
        .collect();
    let screen_quads = model
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| layer.fullscreen)
        .map(|(index, _)| index)
        .collect();
    let mesh_order = model
        .meshes
        .iter()
        .map(|mesh| {
            (
                model
                    .scripts
                    .as_ref()
                    .and_then(|scripts| paper_scene::model::object_index(&scripts.scene, &mesh.id)),
                mesh.scene_order,
            )
        })
        .collect();
    let mesh_settings = mesh::Settings::of(model);
    let cookie_sample = cookie_slot.map(|slot| {
        let texture = &textures[slot];
        (texture.view, texture.sampler, texture.extent)
    });
    let mesh_composites: BTreeSet<(String, CompositeBuffer)> = model
        .meshes
        .iter()
        .flat_map(|mesh| &mesh.materials)
        .flat_map(|material| &material.pass.binds)
        .filter_map(|(_, binding)| match binding {
            EffectBind::LayerComposite { layer, buffer } => Some((layer.clone(), *buffer)),
            _ => None,
        })
        .collect();
    let mesh_history = binds_history(
        model
            .meshes
            .iter()
            .flat_map(|mesh| &mesh.materials)
            .flat_map(|material| &material.pass.binds),
    )
    .then(|| renderer.create_scene_history(target.extent))
    .transpose()?;
    let meshes = if model.meshes.is_empty() {
        Vec::new()
    } else {
        mesh::Build {
            cookie: cookie_sample,
            shadows: shadows.as_ref(),
            history: mesh_history.as_ref(),
            renderer: &mut renderer,
            interner: &mut texture_interner,
            scene: mesh_settings,
            textures: &mut textures,
            quads: &mut quads,
            order: &mut quad_scene_order,
            canvas: (dimensions.logical[0], dimensions.logical[1]),
            viewport,
            reflection: reflection.as_mut(),
            clip,
        }
        .load(std::mem::take(&mut model.meshes))?
    };
    let mut fx = Vec::new();
    let mut mirror_fx = Vec::new();
    let canvas = model.canvas;
    let mut dynamic_chains = 0usize;
    let byte_limit = std::env::var("SKWD_VK_SCENE_FX_BYTES")
        .ok()
        .and_then(|text| text.parse::<u64>().ok())
        .unwrap_or(512)
        .saturating_mul(1024 * 1024);
    let history_needed = mesh_history.is_some()
        || model.layers.iter().any(|layer| {
            layer.material.as_ref().is_some_and(|material| {
                binds_history(&material.pass.binds)
                    || material
                        .prelit
                        .as_ref()
                        .is_some_and(|prelit| binds_history(&prelit.pass.binds))
            }) || binds_history(
                layer.effects.iter().flat_map(|effect| &effect.passes).flat_map(|pass| &pass.binds),
            )
        });
    let history_reservation = if history_needed {
        vk::SceneHistory::expected_bytes(target.extent, renderer.scene_format())
            .context("scene history format unsupported")?
    } else {
        0
    };
    ensure!(
        history_reservation <= byte_limit,
        "[native-scene-gap:{HISTORY_BUDGET_GAP}] history needs at least {history_reservation} bytes; budget {byte_limit}"
    );
    let volumes = volume::Volumes::new(&mut renderer, &target, &model.lights)?;
    let lighting_bytes = volumes.as_ref().map_or(0, volume::Volumes::allocation_bytes)
        + shadows.as_ref().map_or(0, shadow::Shadows::allocation_bytes);
    let reflection_bytes =
        reflection.as_ref().map_or(0, |reflection| reflection.target().allocation_bytes);
    let text_snapshot_bytes =
        if model.layers.iter().any(|layer| layer.is_text() && !layer.effects.is_empty()) {
            target.allocation_bytes
        } else {
            0
        };
    let mut admitted_bytes = history_reservation
        .saturating_add(reflection_bytes)
        .saturating_add(text_snapshot_bytes)
        .saturating_add(lighting_bytes)
        .saturating_add(materials.iter().map(material::Material::target_bytes).sum());
    anyhow::ensure!(admitted_bytes <= byte_limit, "scene input targets exceed effect budget");
    let scene_lights = (model.ambient, model.skylight);
    let pointer_scene = model.mouse.amount != 0.0
        || model.layers.iter().any(|layer| {
            layer.mouse.clock.is_some()
                || layer
                    .effects
                    .iter()
                    .flat_map(|effect| &effect.passes)
                    .any(|pass| paper_scene::effects::PassMeta::of(pass).pointer_dependent())
        });
    let defaults = paper_scene::model::Properties::new();
    let (fx_scene, fx_properties) = model
        .scripts
        .as_ref()
        .map(|host| (&host.scene, &host.properties))
        .or_else(|| model.animated_scene.as_ref().map(|(scene, properties)| (scene, properties)))
        .unwrap_or((&serde_json::Value::Null, &defaults));
    let mut twins = Vec::new();
    let mut builder = FxBuilder {
        mirror: false,
        renderer: &mut renderer,
        interner: &mut texture_interner,
        textures: &mut textures,
        quads: &mut quads,
        materials: &mut materials,
        skipped: &mut model.skipped,
        dimensions,
        canvas,
        scene: fx_scene,
        properties: fx_properties,
        scene_lights,
        pointer_scene,
        strict,
        scripted: model.scripts.is_some(),
        world: model.camera.is_some(),
        pass_cap,
        fx_limit,
        byte_limit,
        admitted_bytes: &mut admitted_bytes,
        dynamic_chains: &mut dynamic_chains,
    };
    for index in chain_admission_order(&model.layers, scripted, &model.dependencies) {
        let text_prefix_dynamic = model.layers[index].is_text()
            && model.layers.iter().any(|earlier| {
                earlier.scene_order < model.layers[index].scene_order
                    && (earlier.texture.video.is_some()
                        || earlier.texture.system_texture.is_some()
                        || earlier.atlas_frames().is_some()
                        || earlier.mouse.clock.is_some()
                        || model.animated_scene.as_ref().is_some_and(|(scene, _)| {
                            model.keyframes.affects_layer(scene, &earlier.id)
                        }))
            });
        let text_prefix_clock = model.layers[index].is_text()
            && model.layers.iter().any(|earlier| {
                earlier.scene_order < model.layers[index].scene_order && earlier.live_text.is_some()
            });
        let backdrop_moves =
            model.scripts.is_some() || (model.camera.is_some() && model.shake.is_some());
        let source_keyframes = (backdrop_moves && model.layers[index].copy_background)
            || model
                .scripts
                .as_ref()
                .map(|host| &host.scene)
                .or_else(|| model.animated_scene.as_ref().map(|(scene, _)| scene))
                .is_some_and(|scene| model.keyframes.affects_layer(scene, &model.layers[index].id));
        let input = FxInput {
            layer_slot: flat_slot(&puppets, index, &layer_slots),
            text_prefix_dynamic,
            text_prefix_clock,
            source_keyframes,
            mesh: layer_puppet(&puppets, index).map(|puppet| &puppet.mesh),
            puppeted: puppeted[index],
        };
        let saved = (reflection.as_ref().is_some_and(|reflection| reflection.sampled)
            && reflected_chain(&model.layers[index], &model.unreflected))
        .then(|| effect_textures(&model.layers[index]));
        if let Some(chain) = builder.layer(&mut model.layers[index], index, &input)? {
            fx.push(chain);
            twins.extend(saved.map(|textures| (index, input, textures)));
        }
        #[cfg(test)]
        resize::tests::after_effect()?;
    }
    builder.mirror = true;
    for (index, input, textures) in twins {
        restore_effect_textures(&mut model.layers[index], textures);
        let quad = builder.quads[index].clone();
        let built = builder.layer(&mut model.layers[index], index, &input);
        builder.quads[index] = quad;
        match built {
            Ok(fx) => mirror_fx.extend(fx),
            Err(error) => tracing::warn!("scene reflection effect {index} skipped: {error:#}"),
        }
    }
    if let Some(reflection) = reflection.as_mut().filter(|reflection| reflection.sampled)
        && !mirror_fx.is_empty()
    {
        let view = reflection.prepare_snapshot(&mut renderer, &target)?;
        for slot in mirror_fx.iter().filter_map(|fx| fx.snapshot_slot) {
            renderer.point_slot_at(&textures[slot], view);
        }
    }
    for (index, layer) in model.layers.iter().enumerate() {
        if reflected_chain(layer, &model.unreflected)
            && !mirror_fx.iter().any(|fx| fx.quad == index)
        {
            reflection_excluded.insert(index);
        }
    }
    let mut mouse = mouse::SceneMouse::new(model);
    if model.camera.is_some() {
        for (index, quad) in quads.iter().enumerate() {
            mouse.set_script_quad(index, quad);
        }
        for (index, layer) in model.layers.iter().enumerate() {
            if matches!(
                layer.kind,
                paper_scene::model::LayerKind::Image | paper_scene::model::LayerKind::Text
            ) && layer.geometry.is_some()
            {
                mouse.set_pose(index, LayerPose::from(layer), model.canvas);
            }
        }
    }
    let mut particles: Vec<ParticleGroup> = Vec::new();
    let mut pending: std::collections::VecDeque<_> =
        std::mem::take(&mut model.particles).into_iter().map(|layer| (layer, None)).collect();
    while let Some((layer, parent)) = pending.pop_front() {
        let index = particles.len();
        let mut system = layer.system;
        mouse.enabled |= system.follows_mouse();
        let (slot, ratio, frames, engines) = if system.emitters.is_empty() {
            (None, 1.0, Vec::new(), Vec::new())
        } else if let Some(mut texture) = system.texture.take() {
            let slot = match texture_interner.intern_texture(
                &mut renderer,
                &mut textures,
                &mut texture,
                vk::MipSampling::Anisotropic,
            ) {
                Ok(slot) => slot,
                Err(err) => {
                    tracing::warn!("skwd-wall-vk: particle texture upload failed: {err:#}");
                    if strict {
                        return Err(err).context(
                            "strict native scene startup rejected particle texture upload",
                        );
                    }
                    continue;
                }
            };
            let engines = particle_engines(
                &mut system,
                &texture,
                &mut renderer,
                &mut textures,
                &mut texture_interner,
                reflection.as_ref().is_some_and(|reflection| reflection.sampled)
                    && !model.unreflected.contains(&layer.id),
            );
            let ratio = texture.img_width.max(1) as f32 / texture.img_height.max(1) as f32;
            (Some(slot), ratio, texture.frames.clone(), engines)
        } else {
            continue;
        };
        let sim = paper_scene::particles::Sim::new(PARTICLE_SEED ^ (index as u32 + 1));
        if model.camera.is_some() && !particle_renderers_ready(&system, &engines) {
            for engine in engines {
                engine.destroy(&renderer);
            }
            return Err(anyhow!("particle engine unavailable for authored camera"));
        }
        let screen_basis = model
            .camera
            .and_then(|camera| screen_basis(camera, &particle_model(&system, system.origin)));
        for child in std::mem::take(&mut system.children).into_iter().rev() {
            let binding = ParticleParent {
                index,
                origin: child.origin,
                angle: child.angle,
                scale: child.scale3,
            };
            pending.push_front((
                paper_scene::model::ParticleLayer {
                    parallax: layer.parallax,
                    id: layer.id.clone(),
                    visible: layer.visible,
                    system: child,
                    depth: layer.depth,
                    scene_order: layer.scene_order,
                },
                Some(binding),
            ));
        }
        let prewarm_dt = system.prewarm_step();
        let prewarm_steps = system.prewarm_steps(prewarm_dt);
        particles.push(ParticleGroup {
            parallax: layer.parallax,
            camera_offset: [0.0; 2],
            id: layer.id,
            visible: layer.visible,
            system,
            sim,
            texture: slot,
            ratio,
            frames,
            scene_order: layer.scene_order,
            engines,
            screen_basis,
            parent,
            followers: paper_scene::particles::Followers::default(),
            reset: false,
            prewarm_steps,
            prewarm_dt,
            dynamic_slots: None,
            media_kinds: Vec::new(),
            dynamic_videos: Vec::new(),
        });
    }
    let image_samples = sampling::bind_sources(model, &layer_slots, &puppets, &mut particles);
    prewarm_particles(&mut particles, 0, paper_scene::effects::FrameClock::now(0.0, 0.0));
    let mut referenced: BTreeSet<String> = fx
        .iter()
        .flat_map(|layer| &layer.binds)
        .flatten()
        .filter_map(|(_, binding)| composite_layer(binding))
        .collect();
    referenced.extend(mesh_composites.iter().map(|(layer, _)| layer.clone()));
    if let Some(layer) = uncopyable_provider(&model.layers, &referenced) {
        return Err(anyhow!("{}: custom base composite provider unsupported", layer.name));
    }
    let active_layers: BTreeSet<usize> = fx.iter().map(|layer| layer.quad).collect();
    let mut passive_scene_targets = Vec::new();
    let mut passive_video_targets = Vec::new();
    let mut base_scene_targets = Vec::new();
    for index in passive_provider_indices(&model.layers, &active_layers, &referenced) {
        let layer = &model.layers[index];
        let (width, height) = layer_effect_dimensions(layer, dimensions);
        let target = if layer.texture.clamp {
            renderer.create_scene_target(width, height)?
        } else {
            renderer.create_scene_target_repeat(width, height)?
        };
        let target_quads = (layer.visible || model.dependencies.contains(&layer.id))
            .then(|| {
                passive_target_quad(layer, flat_slot(&puppets, index, &layer_slots), target.extent)
            })
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
    if let Some((layer, _)) = mesh_composites.iter().find(|(layer, _)| {
        fx.iter().all(|chain| &chain.layer_id != layer)
            && base_scene_targets.iter().all(|target| &target.layer_id != layer)
    }) {
        return Err(anyhow!("mesh layer composite {layer} has no provider"));
    }
    let unique_textures = texture_interner.slots.len();
    let reused_textures = texture_interner.reused;
    let reused_payload_bytes = texture_interner.reused_bytes;
    let uploaded_payload_bytes =
        texture_interner.slots.keys().map(|key| key.bytes()).sum::<usize>();
    let videos = std::mem::take(&mut texture_interner.videos);
    let media_slots = std::mem::take(&mut texture_interner.media_slots);
    let static_media = media_slots.keys().copied().collect();
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
    let chained = !fx.is_empty() || !mirror_fx.is_empty();
    let ndc_quad = if chained { Some(renderer.create_quad_buffer_ndc()?) } else { None };
    let ndc_quad_d3d = if chained { Some(renderer.create_quad_buffer_ndc_d3d()?) } else { None };
    let texture_bytes: u64 = textures.iter().map(|texture| texture.allocation_bytes).sum();
    tracing::info!(
        chains_admitted = fx.len(),
        chains_dynamic = dynamic_chains,
        effect_target_bytes = admitted_bytes,
        history_reservation,
        effect_target_budget = byte_limit,
        particle_systems = particles.len(),
        particle_capacity = particles
            .iter()
            .flat_map(|group| &group.engines)
            .map(|engine| engine.capacity)
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
        .filter(|(_, layer)| layer.audio_dependent)
        .map(|(index, _)| index)
        .collect();
    let audio = if model.scripts.as_ref().is_some_and(|s| s.needs_audio())
        || particles.iter().any(|particle| particle.system.needs_audio())
        || materials.iter().any(material::Material::audio_dependent)
    {
        start_audio(&[0])
    } else {
        start_audio(&audio_fx)
    };
    if audio.is_some() {
        for index in &audio_fx {
            if let Some(layer) = fx.get_mut(*index) {
                seed_bands(&mut layer.uniforms);
            }
        }
    }

    let media =
        if !media_slots.is_empty() || model.scripts.as_ref().is_some_and(|s| s.needs_media()) {
            media::Media::start()
                .map_err(|e| tracing::warn!("media integration unavailable: {e:#}"))
                .ok()
        } else {
            None
        };
    let deferred_compositions = deferred_composition_layers(&model.layers, dimensions);
    if let Some(scripts) = &mut model.scripts {
        scripts.restrict_hidden_layer_updates(&deferred_compositions);
    }
    let mut scripts = script::Scripts::take(model, &layer_slots, &particles, mesh_order);
    if let Some(scripts) = &mut scripts {
        scripts.install_backdrops(
            &mut renderer,
            &mut quads,
            &mut quad_scene_order,
            &mut textures,
        )?;
    }
    let bloom = fx
        .iter()
        .find(|entry| entry.layer_id == paper_scene::model::bloom::ID)
        .map(|entry| BloomRuntime {
            quad: Some(entry.quad),
            base_slot: Some(entry.base_quad.texture),
            active_slots: entry.snapshot_slot.into_iter().chain([entry.slot]).collect(),
            spare_slots: Vec::new(),
        })
        .unwrap_or_default();
    let mut group = Group {
        lights: model.lights.clone(),
        sun_view: model.sun_view,
        fog: model.fog,
        shadows,
        cookie_slot,
        cookie_layout,
        cookie_current,
        bloom,
        volumes,
        scripts,
        media,
        media_slots,
        static_media,
        media_art: None,
        renderer,
        target: vk::PresentedTarget::new(target),
        scene_snapshot: None,
        scene_grab: None,
        scene_history: mesh_history,
        reflection,
        mirror_fx,
        layer_poses,
        reflection_excluded,
        unreflected: std::mem::take(&mut model.unreflected),
        history_budget: byte_limit.saturating_sub(admitted_bytes - history_reservation),
        effect_budget: byte_limit,
        #[cfg(test)]
        replay_snapshots: tests::REPLAY_SNAPSHOTS.get(),
        #[cfg(test)]
        snapshot_draws: Vec::new(),
        frozen: false,
        resize_inputs_static,
        composed_at: 0.0,
        from: None,
        quads,
        quad_scene_order,
        textures,
        videos,
        video_device,
        video_objects,
        image_videos: HashMap::new(),
        passive_video_targets,
        base_scene_targets,
        mesh_composites,
        passive_scene_targets,
        fx,
        materials,
        meshes,
        mesh_settings,
        scene_view: model.camera_view,
        clip,
        screen_quads,
        dynamic_depth: false,
        baked_fx_outputs: Vec::new(),
        baked_fx_draws: Vec::new(),
        baked_fx_quads: Vec::new(),
        unavailable_effects: model
            .layers
            .iter()
            .flat_map(|layer| {
                layer
                    .effects
                    .iter()
                    .filter(|effect| !effect.state.available)
                    .map(|effect| (layer.id.clone(), effect.state))
            })
            .collect(),
        puppets,
        clip_planes,
        fx_scratch: Vec::new(),
        ndc_quad,
        ndc_quad_d3d,
        particles,
        spare_particle_slots: Vec::new(),
        particle_frame: 0,
        image_samples,
        animations,
        script_sounds: Vec::new(),
        audio,
        bands: paper_audio::Bands::default(),
        system_audio: None,
        live_text,
        live_text_due: 0.0,
        effects_animated: false,
        mouse,
        scene_uniforms: std::collections::BTreeMap::from([
            ("g_LightAmbientColor".to_string(), model.ambient.to_vec()),
            ("g_LightSkylightColor".to_string(), model.skylight.to_vec()),
        ]),
        fixed_daytime: None,
        canvas: (dimensions.logical[0], dimensions.logical[1]),
        camera: model.camera,
        camera_rest: model.camera,
        eye_offset: [0.0; 3],
        cameras_move: model.cameras_move,
        camera_source: std::mem::take(&mut model.camera_source),
        eye: particle_eye(
            model.projection,
            model.camera,
            (dimensions.logical[0], dimensions.logical[1]),
            model.perspective_fov,
        ),
        clear: model.clear,
        clear_enabled: model.clear_enabled,
        composed_once: false,
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
        group.quads[entry.quad].uv_axes = None;
    }
    let composed = group.compose(0.0, 0.0);
    #[cfg(test)]
    let composed = composed.and_then(|()| resize::tests::after_compose());
    if let Err(error) = composed {
        group.destroy();
        return Err(error);
    }
    let effect_layers = group.fx.len();
    let effect_target_bytes_before = group.effect_target_allocation_bytes();
    let retain_composition_inputs = group.animated()
        || group.fx.iter().any(|fx| fx.audio_dependent)
        || !group.live_text.is_empty()
        || group.mouse.enabled
        || group.scripts.is_some()
        || group.media.is_some();
    let memory = if retain_composition_inputs {
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
    let alias = if retain_composition_inputs && alias_enabled {
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
    if !retain_composition_inputs {
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

fn create_scene_presentation_buffers(
    target: &mut wayland::Target,
    presenters: &[Presenter],
    presenter_dims: &[(u32, u32)],
    ridx: &[usize],
    dmabuf_present: bool,
    n_exports: usize,
) -> Result<ScenePresentationBuffers> {
    let rbs = presenters
        .iter()
        .zip(presenter_dims)
        .filter(|_| !dmabuf_present)
        .map(|(presenter, &(w, h))| {
            presenter.renderer.create_readback_buf(u64::from(w) * u64::from(h) * 4)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut buffers = if dmabuf_present {
        let exports: Vec<_> =
            presenters.iter().map(|presenter| presenter.exports.as_slice()).collect();
        create_buffers(target, presenter_dims, &exports, ridx)?
    } else {
        Vec::with_capacity(ridx.len())
    };
    let mut rings = Vec::with_capacity(ridx.len());
    for (si, &presenter_index) in ridx.iter().enumerate() {
        let (w, h) = presenter_dims[presenter_index];
        if !dmabuf_present {
            let allocated = target.create_shm_ring(si, w, h, n_exports);
            let (ring, ptrs, stride) = match allocated {
                Ok(ring) => ring,
                Err(error) => {
                    for buffer in buffers.iter().flatten() {
                        target.retire_scene_buffer(buffer);
                    }
                    return Err(error);
                }
            };
            buffers.push(ring);
            rings.push(SceneRing { ptrs, stride, bytes: stride as usize * h as usize * n_exports });
        }
    }
    Ok(ScenePresentationBuffers { rbs, buffers, rings })
}

fn shared_scene_slot<'a>(
    free: impl Iterator<Item = &'a [usize]> + Clone,
    count: usize,
    displayed: &[Option<usize>],
) -> Option<usize> {
    (0..count).find(|bi| {
        !displayed.contains(&Some(*bi)) && free.clone().all(|output| output.contains(bi))
    })
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

fn scene_presentation_size(
    mode: FillMode,
    target: &wayland::Target,
    canvas: (u32, u32),
) -> (u32, u32) {
    if mode == FillMode::Span {
        target.span_size().map_or(canvas, |(width, height)| {
            scene_dimensions_for((width as f32, height as f32), scene_limit()).raster
        })
    } else {
        canvas
    }
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
    let Some(name) = shader.map(paper_shaders::resolve_shader) else {
        return TransitionStyle::Fade;
    };
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
    Ok(paper_control::we_project::Project::scene_package_at(std::path::Path::new(dir))?)
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
        let sounds = paper_scene::sound::scene_sounds_with_silent(
            pkg,
            properties,
            paper_scene::sound::scripts_drive_sounds(pkg),
        )?;
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
                id: sound.id,
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
                autostart: sound.autostart,
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
    let mut file = paper_runtime::scratch::file(paper_runtime::scratch::AUDIO, &suffix)
        .context("create native scene audio file")?;
    file.write_all(bytes).context("extract native scene audio")?;
    file.flush().context("flush native scene audio")?;
    Ok(file)
}

fn extract_scene_audio(
    pkg: &paper_scene::pkg::Package,
    directory: &str,
    properties: &paper_scene::model::Properties,
) -> Option<SceneAudio> {
    match SceneAudio::extract(pkg, &properties::resolved(directory, properties)) {
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
    let capture = paper_scene::source::Capture::start()?;
    let pkg_path = locate_pkg(dir)?;
    let pkg = paper_scene::pkg::Package::open(&pkg_path)?;
    let strict = strict_scene_startup();
    validate_native_compatibility(&pkg, strict)?;
    let mut model = paper_scene::model::load_from_dir_with_storage(
        &pkg,
        std::path::Path::new(dir),
        properties,
        paper_scene::script::Storage::open(
            std::path::Path::new(dir),
            &target.app.surfaces.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(","),
        ),
        script::screen_resolution(&target.app.surfaces),
    )?;
    let mut source_identity = capture.finish()?;
    let mut scene_audio = extract_scene_audio(&pkg, dir, properties);
    drop(pkg);
    let particles_disabled = std::env::var("SKWD_PAPER_WE_DISABLE_PARTICLES").as_deref() == Ok("1");
    if particles_disabled {
        let disabled = model.particles.len();
        model.particles.clear();
        tracing::info!(disabled, "skwd-wall-vk: scene particles disabled by policy");
    }
    validate_scene_skips(strict, &model.skipped)?;
    validate_scene_content(&model)?;
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
    let mut dims: Vec<(u32, u32)> = (0..n_surf).map(|si| target.size_at(si)).collect();
    let n_exports = 2usize;
    let mode = fill_mode();
    let mut composition_dims = resize::composition_outputs(&dims, mode, target.span_size());
    let mut group = build_group(&sd, &mut model, strict, &composition_dims, mode)?;
    drop(model);
    let shared_composition = target.supports_viewporter()
        && matches!(mode, FillMode::Fill | FillMode::Stretch | FillMode::Span);
    let mut canvas = (group.target.extent.width, group.target.extent.height);
    let mut presentation_size = scene_presentation_size(mode, target, canvas);
    let (mut presenter_dims, mut ridx) =
        scene_presenter_layout(&dims, presentation_size, shared_composition);
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
    let (mut presenters, mut dmabuf_present) =
        build_wayland_presenters(target, &sd, &presenter_dims, n_exports, share_exports)?;
    let mut shared_pool = share_exports && dmabuf_present;
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
        let scene_target = std::mem::replace(
            &mut group.target,
            vk::PresentedTarget::new(group.renderer.create_scene_target(1, 1)?),
        );
        let (w, h, rgba) = group.renderer.read_scene_target(&scene_target)?;
        std::mem::replace(&mut group.target, scene_target).destroy(&group.renderer.device);
        let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
        for px in rgba.chunks_exact(4) {
            ppm.extend_from_slice(&[px[0], px[1], px[2]]);
        }
        std::fs::write(&path, ppm)?;
        tracing::info!("skwd-wall-vk: scene dumped {w}x{h} to {path}");
        return Ok(());
    }
    let ScenePresentationBuffers { mut rbs, mut buffers, mut rings } =
        create_scene_presentation_buffers(
            target,
            &presenters,
            &presenter_dims,
            &ridx,
            dmabuf_present,
            n_exports,
        )?;
    init_free_buffers(target, n_exports);
    tracing::info!(
        "skwd-wall-vk: scene path = {}",
        if dmabuf_present {
            "Vulkan DMA-BUF (no CPU readback)"
        } else {
            "Vulkan SHM readback fallback"
        }
    );
    for si in 0..n_surf {
        if !target.app.surfaces[si].closed {
            if shared_composition && matches!(fill_mode(), FillMode::Fill | FillMode::Span) {
                target.set_viewport_cover(si, presentation_size.0, presentation_size.1)?;
            } else {
                target.set_viewport_dst(si)?;
            }
        }
    }

    let mut fade_ms = start_fade
        .as_ref()
        .filter(|sf| !sf.from.is_empty())
        .and_then(|sf| scene_swap_duration(sf.duration_ms))
        .unwrap_or(0);
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
    let transition_fps = super::shared_support::fade_fps_cap();
    let playback_fps: Vec<u32> =
        target.app.surfaces.iter().map(|surface| surface.fps_limit).collect();
    tracing::info!(
        "skwd-wall-vk: scene {} ({} effect chain(s)){}",
        if animated { "animated" } else { "static" },
        group.fx.len(),
        if animated { format!(", {fps}fps cap") } else { String::new() }
    );

    let mut ctl = ctl::Ctl::start_opts(dir, mute, volume, false, true);
    if let Some(audio) = &scene_audio {
        ctl.set_scene_voices(
            audio.voices(),
            group.audio.as_ref().map(paper_audio::Analyser::playback_tap),
        );
    }
    group.apply_script_sounds(&mut ctl);
    target.ctl_fd = ctl.wake_fd();
    let mut active_dir = dir.to_string();
    let mut active_properties = properties.clone();
    let mut property_source = properties::PropertySource::new(dir, properties);
    let mut property_frame = false;
    target.enable_pointer();
    target.sync_desktop_pointer(group.mouse.reads_pointer);
    let mut observed_screen = script::screen_resolution(&target.app.surfaces);
    let mut presented = false;
    let mut committed_outputs = vec![false; n_surf];
    let mut last_buffers = vec![None; n_surf];
    let mut render_pending = vec![true; n_surf];
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
    let mut retired_presentations: Vec<RetiredScenePresentation> = Vec::new();

    loop {
        target.pump()?;
        let mut retired_index = 0;
        while retired_index < retired_presentations.len() {
            if target.scene_buffers_released(retired_presentations[retired_index].generation) {
                retired_presentations.swap_remove(retired_index).destroy(target);
            } else {
                retired_index += 1;
            }
        }
        if target.app.closed {
            break;
        }
        if target.take_resized()? {
            if target.surface_count() == n_surf {
                let new_dims: Vec<_> = (0..n_surf).map(|si| target.size_at(si)).collect();
                if let Some(view) = group.target.viewport
                    && !group.reconstructable()
                {
                    anyhow::ensure!(
                        view.same_outputs(&new_dims),
                        "orthographic mesh output resize requires reload"
                    );
                }
                let new_composition_dims =
                    resize::composition_outputs(&new_dims, mode, target.span_size());
                let mut replacement =
                    if new_composition_dims != composition_dims && group.reconstructable() {
                        match (resize::SceneSource {
                            shared: &sd,
                            dir: &active_dir,
                            properties: &active_properties,
                            identity: &source_identity,
                            outputs: &new_composition_dims,
                            mode,
                            strict,
                            particles_disabled,
                        })
                        .build()
                        {
                            Ok(candidate) => Some(candidate),
                            Err(error) => {
                                tracing::warn!("scene resize rejected: {error:#}");
                                continue;
                            }
                        }
                    } else {
                        None
                    };
                let new_canvas = replacement.as_ref().map_or(canvas, |candidate| {
                    (candidate.target.extent.width, candidate.target.extent.height)
                });
                let new_size = scene_presentation_size(mode, target, new_canvas);
                if new_dims != dims || new_size != presentation_size || replacement.is_some() {
                    let (new_presenter_dims, mut new_ridx) =
                        scene_presenter_layout(&new_dims, new_size, shared_composition);
                    let prepared = build_wayland_presenters(
                        target,
                        &sd,
                        &new_presenter_dims,
                        n_exports,
                        share_exports,
                    );
                    let (new_presenters, new_dmabuf_present) = match prepared {
                        Ok(presenters) => presenters,
                        Err(error) => {
                            if let Some(candidate) = replacement.take() {
                                candidate.destroy();
                            }
                            tracing::warn!("scene resize presentation rejected: {error:#}");
                            continue;
                        }
                    };
                    let new_shared_pool = share_exports && new_dmabuf_present;
                    if new_shared_pool {
                        new_ridx.fill(0);
                    }
                    target.stage_scene_buffers(true);
                    let prepared = create_scene_presentation_buffers(
                        target,
                        &new_presenters,
                        &new_presenter_dims,
                        &new_ridx,
                        new_dmabuf_present,
                        n_exports,
                    );
                    target.stage_scene_buffers(false);
                    let ScenePresentationBuffers {
                        rbs: new_rbs,
                        buffers: new_buffers,
                        rings: new_rings,
                    } = match prepared {
                        Ok(memory) => memory,
                        Err(error) => {
                            if let Some(candidate) = replacement.take() {
                                candidate.destroy();
                            }
                            tracing::warn!("scene resize buffers rejected: {error:#}");
                            continue;
                        }
                    };
                    tracing::info!(
                        old_composition_bytes = group.target.allocation_bytes,
                        new_composition_bytes = replacement
                            .as_ref()
                            .map_or(0, |candidate| candidate.target.allocation_bytes),
                        old_export_bytes = presenters
                            .iter()
                            .flat_map(|presenter| &presenter.exports)
                            .map(|export| export.allocation_size)
                            .sum::<u64>(),
                        new_export_bytes = new_presenters
                            .iter()
                            .flat_map(|presenter| &presenter.exports)
                            .map(|export| export.allocation_size)
                            .sum::<u64>(),
                        retired_export_bytes = retired_presentations
                            .iter()
                            .flat_map(|retired| &retired.presenters)
                            .flat_map(|presenter| &presenter.exports)
                            .map(|export| export.allocation_size)
                            .sum::<u64>(),
                        "scene resize staged"
                    );
                    let generation = target.rotate_scene_buffers(&buffers);
                    init_free_buffers(target, n_exports);
                    retired_presentations.push(RetiredScenePresentation {
                        generation,
                        presenters: std::mem::replace(&mut presenters, new_presenters),
                        memory: ScenePresentationBuffers {
                            rbs: std::mem::replace(&mut rbs, new_rbs),
                            buffers: std::mem::replace(&mut buffers, new_buffers),
                            rings: std::mem::replace(&mut rings, new_rings),
                        },
                    });
                    if let Some(mut candidate) = replacement.take() {
                        candidate.from = group.from.take();
                        let old = std::mem::replace(&mut group, candidate);
                        old.destroy();
                    }
                    canvas = new_canvas;
                    composition_dims = new_composition_dims;
                    dims = new_dims;
                    presentation_size = new_size;
                    presenter_dims = new_presenter_dims;
                    ridx = new_ridx;
                    dmabuf_present = new_dmabuf_present;
                    shared_pool = new_shared_pool;
                    last_buffers.fill(None);
                    tracing::info!(
                        output_dimensions = ?dims,
                        presenter_dimensions = ?presenter_dims,
                        dmabuf_present,
                        "skwd-wall-vk: scene presentation resized in place across {} outputs",
                        dims.len()
                    );
                }
                let current = script::screen_resolution(&target.app.surfaces);
                if current != observed_screen {
                    if let Some(resolution) = current {
                        group.resize_screen(resolution);
                    }
                    observed_screen = current;
                }
                for si in 0..n_surf {
                    if target.app.surfaces[si].closed {
                        continue;
                    }
                    if shared_composition && matches!(mode, FillMode::Fill | FillMode::Span) {
                        target.set_viewport_cover(si, presentation_size.0, presentation_size.1)?;
                    } else {
                        target.set_viewport_dst(si)?;
                    }
                }
                let now = Instant::now();
                let elapsed = epoch.elapsed().as_secs_f32();
                let dt = now
                    .duration_since(last_sim)
                    .as_secs_f32()
                    .clamp(SCENE_FRAME_DT_MIN, SCENE_FRAME_DT_MAX);
                group.sync_sound_states(&ctl);
                group.compose(elapsed, dt)?;
                animated = group.animated();
                group.apply_script_sounds(&mut ctl);
                last_sim = now;
                next_frame = now + frame_gap;
                render_pending.fill(true);
            } else {
                drop(ctl);
                group.destroy();
                let argv_properties = scene_properties_argv(&active_properties);
                return wayland::Target::reexec(&wayland::ReexecSource::Scene {
                    dir: &active_dir,
                    properties: argv_properties.as_deref(),
                });
            }
        }
        let request = ctl.poll();
        group.sync_audio_source(&ctl);
        if let Some(req) = request {
            tracing::info!("skwd-wall-vk: scene swap to {}", req.to);
            let swap_started = Instant::now();
            let next_properties = req
                .properties
                .as_ref()
                .map(paper_scene::effects::parse_property_overrides)
                .unwrap_or_default();
            if req.duration_ms == 0
                && group.update_properties(
                    &mut property_source,
                    &req.to,
                    &next_properties,
                    &mut ctl,
                )
            {
                active_properties = next_properties;
                for presenter in &mut presenters {
                    presenter.renderer.clear_transition()?;
                }
                group.drop_from();
                fade_start = None;
                property_frame = true;
                presented = false;
                committed_outputs.fill(false);
                render_pending.fill(true);
                animated = group.animated();
                next_frame = Instant::now();
                continue;
            }
            let loaded = paper_scene::source::Capture::start()
                .and_then(|capture| {
                    let path = locate_pkg(&req.to)?;
                    let pkg = paper_scene::pkg::Package::open(&path)?;
                    Ok((capture, pkg))
                })
                .and_then(|(capture, pkg)| {
                    let model = paper_scene::model::load_from_dir_with_storage(
                        &pkg,
                        std::path::Path::new(&req.to),
                        &next_properties,
                        paper_scene::script::Storage::open(
                            std::path::Path::new(&req.to),
                            &target
                                .app
                                .surfaces
                                .iter()
                                .map(|s| s.name.as_str())
                                .collect::<Vec<_>>()
                                .join(","),
                        ),
                        script::screen_resolution(&target.app.surfaces),
                    )?;
                    let identity = capture.finish()?;
                    Ok((model, extract_scene_audio(&pkg, &req.to, &next_properties), identity))
                });
            let (mut next, next_audio, next_identity) = match loaded {
                Ok(next) => next,
                Err(error) => {
                    signal_swap_failure(&req.to, &format!("{error:#}"));
                    continue;
                }
            };
            if particles_disabled {
                next.particles.clear();
            }
            if let Err(error) = validate_scene_content(&next) {
                signal_swap_failure(&req.to, &format!("{error:#}"));
                continue;
            }
            if let Err(error) = validate_scene_skips(strict, &next.skipped) {
                signal_swap_failure(&req.to, &format!("{error:#}"));
                continue;
            }
            let load_ms = swap_started.elapsed().as_millis();
            let current_composition_dims =
                resize::composition_outputs(&dims, mode, target.span_size());
            let next_group =
                match build_group(&sd, &mut next, strict, &current_composition_dims, mode) {
                    Ok(built) => built,
                    Err(error) => {
                        signal_swap_failure(&req.to, &format!("{error:#}"));
                        continue;
                    }
                };
            tracing::info!(
                load_ms,
                build_ms = swap_started.elapsed().as_millis() - load_ms,
                same_scene = req.to == active_dir,
                "skwd-wall-vk: scene swap built"
            );
            drop(next);
            source_identity = next_identity;
            composition_dims = current_composition_dims;
            active_dir.clone_from(&req.to);
            active_properties = next_properties;
            property_source = properties::PropertySource::new(&active_dir, &active_properties);
            property_frame = false;
            let mut old = std::mem::replace(&mut group, next_group);
            target.sync_desktop_pointer(group.mouse.reads_pointer);
            ctl.set_scene_voices(
                next_audio.as_ref().map(SceneAudio::voices).unwrap_or_default(),
                group.audio.as_ref().map(paper_audio::Analyser::playback_tap),
            );
            group.apply_script_sounds(&mut ctl);
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
                let dummy = vk::PresentedTarget::new(old.renderer.create_scene_target(1, 1)?);
                let taken = std::mem::replace(&mut old.target, dummy);
                group.from = Some(FadeSource {
                    source: FadeFrom::Scene(taken),
                    width: old_width,
                    height: old_height,
                });
            } else {
                for presenter in &mut presenters {
                    presenter.renderer.clear_transition()?;
                }
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
            render_pending.fill(true);
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
        if let Some(outputs) = ctl.retained_outputs.take() {
            wayland::Target::retain_reexec_outputs(&outputs);
            target.retain_outputs(&outputs)?;
            for (last, surface) in last_buffers.iter_mut().zip(&target.app.surfaces) {
                if surface.closed {
                    *last = None;
                }
            }
            if target.app.closed {
                break;
            }
        }
        if std::mem::take(&mut ctl.reveal) && target.reveal_prepared()? {
            signal_ready();
        }
        if let Some(capture) = ctl.take_capture() {
            let frame = if capture.source == active_dir && presented {
                group.renderer.read_scene_target(&group.target)
            } else {
                Err(anyhow::anyhow!("scene changed before thumbnail capture"))
            };
            crate::freeze::capture_scene(capture, frame);
        }
        let surface_changed = ctl.take_surface_change();
        if surface_changed {
            presented = false;
            render_pending.fill(true);
        }
        if ctl.freeze_pending() && !render_pending.iter().any(|pending| *pending) {
            let frame = target
                .app
                .surfaces
                .iter()
                .enumerate()
                .find(|(_, surface)| !surface.closed && ctl.snapshots_surface(&surface.name))
                .and_then(|(si, _)| last_buffers[si].map(|bi| (si, bi)))
                .ok_or_else(|| anyhow::anyhow!("scene snapshot output is not ready"))
                .and_then(|(si, bi)| {
                    let presenter = &mut presenters[ridx[si]];
                    let pixels = super::dmabuf_helpers::read_rgba_export(
                        &mut presenter.renderer,
                        &presenter.exports[bi],
                        (presenter.width, presenter.height),
                    )?;
                    Ok((si, pixels))
                });
            match frame {
                Ok((si, pixels)) => {
                    let (size, crop) = if resize::freezes_composition(
                        mode,
                        shared_composition,
                        ctl.snapshot_output(),
                    ) {
                        (pixels.dimensions(), None)
                    } else {
                        (target.size_at(si), target.app.surfaces[si].source)
                    };
                    crate::freeze::write_view_requested(&mut ctl, pixels, size, crop)?;
                }
                Err(error) => crate::freeze::fail_requested(&mut ctl, &error),
            }
        }
        // Commit one frame before parking so startup/swap readiness cannot deadlock while idle.
        let idle_should_pause = target.app.idle && presented && fade_start.is_none();
        if !idle_should_pause && idle_paused {
            idle_paused = false;
            if let Some(audio) = &mut ctl.audio {
                audio.set_pause(ctl.paused);
            }
        }
        if !ctl.freeze_pending()
            && ((presented && ctl.render_paused(fade_start.is_some())) || idle_should_pause)
        {
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
        if !property_frame && let Some(started) = suspended_at.take() {
            let now = Instant::now();
            let suspended_for = now.saturating_duration_since(started);
            epoch += suspended_for;
            if let Some(started) = &mut fade_start {
                *started += suspended_for;
            }
            next_frame = now;
            last_sim = now;
        }
        for (pending, surface) in render_pending.iter_mut().zip(&target.app.surfaces) {
            if surface.closed {
                *pending = false;
            }
        }
        let pointer = target.app.mouse;
        if let Some((revision, position, buttons)) = ctl.pointer() {
            if let Some(surface) = target.app.surfaces.iter().find(|surface| !surface.closed) {
                group.mouse.update(
                    revision,
                    position,
                    buttons,
                    (
                        surface.width * surface.scale.max(1) as u32,
                        surface.height * surface.scale.max(1) as u32,
                    ),
                    mode,
                );
            }
        } else if let Some(surface) = pointer
            .surface
            .and_then(|index| target.app.surfaces.get(index))
            .filter(|surface| !surface.closed)
        {
            group.mouse.update(
                pointer.revision,
                pointer.position,
                pointer.buttons,
                (
                    surface.width * surface.scale.max(1) as u32,
                    surface.height * surface.scale.max(1) as u32,
                ),
                mode,
            );
        }
        let mouse_driven = group.mouse.pending();
        target.set_transition_fps(fade_start.map(|_| transition_fps), &playback_fps);
        let frame_gap = if fade_start.is_some() {
            target
                .app
                .surfaces
                .iter()
                .map(|surface| {
                    super::shared_support::fade_frame_interval(transition_fps, surface.refresh_ns)
                })
                .min()
                .unwrap_or(frame_gap)
        } else {
            frame_gap
        };
        let frame_driven = animated || fade_start.is_some() || mouse_driven;
        if frame_driven && !property_frame && !ctl.paused {
            let now = Instant::now();
            if now < next_frame {
                target.dispatch_until(next_frame)?;
                continue;
            }
            next_frame = now + frame_gap;
            if animated || mouse_driven {
                let time = epoch.elapsed().as_secs_f32();
                let dt = now
                    .duration_since(last_sim)
                    .as_secs_f32()
                    .clamp(SCENE_FRAME_DT_MIN, SCENE_FRAME_DT_MAX);
                last_sim = now;
                group.sync_sound_states(&ctl);
                group.compose(time, dt)?;
                animated = group.animated();
                group.apply_script_sounds(&mut ctl);
                render_pending.fill(true);
            }
        }
        let fade_step = scene_fade_step(
            fade_start.map(|started| started.elapsed().as_secs_f32() * 1000.0 / fade_ms as f32),
            fade_first_frame,
        );
        if presented
            && fade_start.is_none()
            && !animated
            && !mouse_driven
            && !render_pending.iter().any(|pending| *pending)
        {
            let elapsed = epoch.elapsed().as_secs_f32();
            let media_pending = group.media.as_ref().is_some_and(media::Media::has_pending);
            let wait = if media_pending { Some(0.0) } else { group.live_text_wait(elapsed) };
            if wait.is_none_or(|seconds| seconds > 0.0) {
                let seconds = wait.map_or(30.0, |seconds| seconds.clamp(0.0, 30.0));
                target.dispatch_wait_events_with_fd(
                    Instant::now() + Duration::from_secs_f32(seconds),
                    group.media.as_ref().and_then(media::Media::wake_fd),
                )?;
                continue;
            }
            let now = Instant::now();
            let dt = now
                .duration_since(last_sim)
                .as_secs_f32()
                .clamp(SCENE_FRAME_DT_MIN, SCENE_FRAME_DT_MAX);
            last_sim = now;
            group.sync_sound_states(&ctl);
            group.compose(elapsed, dt)?;
            animated = group.animated();
            render_pending.fill(true);
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
                        &last_buffers,
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
                let ring = &rings[si];
                let dst_stride = ring.stride as usize;
                unsafe {
                    let src = rbs[gi].ptr;
                    let dst = ring.ptrs[bi];
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
            last_buffers[si] = Some(bi);
            render_pending[si] = false;
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
            property_frame = false;
            tracing::info!(
                "skwd-wall-vk: first scene frame committed on {n_surf} output(s){}",
                if fade_start.is_some() { " (transition started)" } else { "" }
            );
            if target.hidden {
                ctl.set_paused(true);
            }
            signal_ready();
        }
        if !committed && render_pending.iter().any(|pending| *pending) {
            target.dispatch_wait_events(Instant::now() + frame_gap)?;
        }
    }

    group.destroy();
    Ok(())
}

pub(super) struct DumpSpec<'a> {
    pub dir: &'a str,
    pub properties: &'a paper_scene::model::Properties,
    pub size: (u32, u32),
    pub frames: u32,
    pub warmup: u32,
    pub real_time: bool,
    pub dt: f32,
    pub time0: f32,
    pub daytime: f32,
    pub out_dir: &'a std::path::Path,
}

pub(super) fn dump_scene(spec: &DumpSpec<'_>) -> Result<()> {
    let &DumpSpec { dir, properties, size, frames, warmup, real_time, dt, time0, daytime, out_dir } =
        spec;
    let (width, height) = (size.0.max(16), size.1.max(16));
    let pkg_path = locate_pkg(dir)?;
    let pkg_len = std::fs::metadata(&pkg_path).map(|meta| meta.len()).unwrap_or(0);
    let pkg = paper_scene::pkg::Package::open(&pkg_path)?;
    let strict = strict_scene_startup();
    let mut model = paper_scene::model::load_from_dir_with_storage(
        &pkg,
        std::path::Path::new(dir),
        properties,
        None,
        Some([width, height]),
    )?;
    drop(pkg);
    validate_scene_skips(strict, &model.skipped)?;
    validate_scene_content(&model)?;
    let sd = shared::create(std::ptr::null_mut()).context("scene dump shared device")?;
    let mut group = build_group(&sd, &mut model, strict, &[(width, height)], fill_mode())?;
    let skipped = model.skipped.clone();
    drop(model);
    group.fixed_daytime = Some(daytime);
    std::fs::create_dir_all(out_dir)?;
    let started = Instant::now();
    let mut written = Vec::new();
    let mut paced = Duration::ZERO;
    for index in 0..warmup {
        let time = time0 - (warmup - index) as f32 * dt;
        group.compose(time.max(0.0), dt)?;
        if real_time {
            let wait =
                Duration::from_secs_f32((index + 1) as f32 * dt).saturating_sub(started.elapsed());
            std::thread::sleep(wait);
            paced += wait;
        }
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
        (PARTICLE_PIPELINES_KEY): group.particles.iter().map(|particle| particle.engines.len()).sum::<usize>(),
        "animated": group.animated(),
        "skipped": skipped,
        "script_runtime": group.scripts.as_ref().is_some_and(|s| s.host.is_some()),
        "script_heap_bytes": group.scripts.as_ref().and_then(|s| s.host.as_ref()).map_or(0, |h| h.heap_bytes()),
        "script_diagnostics": group.scripts.as_ref().and_then(|s| s.host.as_ref()).map(|h| &h.diagnostics),
        "render_ms": started.elapsed().saturating_sub(paced).as_secs_f64() * 1000.0,
    });
    std::fs::write(out_dir.join("manifest.json"), serde_json::to_vec_pretty(&manifest)?)?;
    group.destroy();
    Ok(())
}

fn stream_model(
    pkg: &paper_scene::pkg::Package,
    dir: &str,
    properties: &paper_scene::model::Properties,
    screen_resolution: Option<[u32; 2]>,
) -> Result<paper_scene::model::SceneModel> {
    paper_scene::model::load_from_dir_with_storage(
        pkg,
        std::path::Path::new(dir),
        properties,
        None,
        screen_resolution,
    )
}

pub(super) fn stream_scene(
    dir: &str,
    properties: &paper_scene::model::Properties,
    targets: Vec<crate::preview::StreamTarget>,
    mute: bool,
    volume: u32,
) -> Result<()> {
    let mut targets: Vec<crate::preview::StreamTarget> = targets
        .into_iter()
        .map(|target| crate::preview::StreamTarget {
            width: target.width.max(16),
            height: target.height.max(16),
            ..target
        })
        .collect();
    anyhow::ensure!(!targets.is_empty(), "scene stream needs at least one target");
    let sizes: Vec<(u32, u32)> =
        targets.iter().map(|target| (target.width, target.height)).collect();
    let fps = crate::preview::pacing_fps(&targets).min(144);
    let pkg_path = locate_pkg(dir)?;
    let pkg = paper_scene::pkg::Package::open(&pkg_path)?;
    let strict = strict_scene_startup();
    validate_native_compatibility(&pkg, strict)?;
    let screen_resolution = sizes.first().map(|&(width, height)| [width, height]);
    let mut model = stream_model(&pkg, dir, properties, screen_resolution)?;
    let mut _scene_audio = extract_scene_audio(&pkg, dir, properties);
    drop(pkg);
    let particles_disabled = std::env::var("SKWD_PAPER_WE_DISABLE_PARTICLES").as_deref() == Ok("1");
    if particles_disabled {
        model.particles.clear();
    }
    validate_scene_skips(strict, &model.skipped)?;
    validate_scene_content(&model)?;
    let sd = shared::create(std::ptr::null_mut()).context("scene stream shared device")?;
    let mut group = build_group(&sd, &mut model, strict, &sizes, fill_mode())?;
    drop(model);
    let mut presenters = targets
        .iter()
        .map(|target| build_stream_presenter(&sd, target.width, target.height))
        .collect::<Result<Vec<_>>>()?;
    for (presenter, target) in presenters.iter().zip(&targets) {
        for (slot, export) in presenter.exports.iter().enumerate() {
            let init = crate::preview::packet(
                4,
                slot as u8,
                target.width,
                target.height,
                export.stride,
                export.offset,
                export.allocation_size,
            );
            crate::preview::send_packet(target.socket, &init, Some(export.fd))
                .context("send scene stream slot")?;
            crate::preview::send_packet(
                target.socket,
                &crate::preview::packet(5, slot as u8, 0, 0, 0, 0, 0),
                Some(presenter.stream_semaphores[slot].fd),
            )
            .context("send scene stream semaphore")?;
        }
    }
    let sockets: Vec<RawFd> = targets.iter().map(|target| target.socket).collect();
    let mut ctl = ctl::Ctl::start_opts(dir, mute, volume, false, true);
    ctl.route_output_pauses();
    if let Some(audio) = &_scene_audio {
        ctl.set_scene_voices(
            audio.voices(),
            group.audio.as_ref().map(paper_audio::Analyser::playback_tap),
        );
    }
    group.apply_script_sounds(&mut ctl);
    ctl.set_paused(targets.iter().all(|target| target.paused));
    let mut animated = group.animated();
    let frame_gap = Duration::from_secs_f64(1.0 / f64::from(fps));
    let mut epoch = Instant::now();
    let mut last_frame = epoch;
    let mut next_frame = epoch;
    let mut suspended_at = None;
    let mut presented = false;
    let mut fade_start: Option<Instant> = None;
    let mut property_source = properties::PropertySource::new(dir, properties);
    let mut property_frame = false;
    let mut fade_ms = 0u64;
    let mut fade_first_frame = false;
    let mut trans_style = TransitionStyle::Fade;
    let mut free = vec![[true; 3]; targets.len()];
    let mut emitted = vec![false; targets.len()];
    let mut render_pending = vec![true; targets.len()];
    loop {
        for (socket, slots) in sockets.iter().zip(free.iter_mut()) {
            crate::preview::drain_acks(*socket, slots)?;
        }
        let request = ctl.poll();
        group.sync_audio_source(&ctl);
        if let Some(req) = request {
            let swap_started = Instant::now();
            let next_properties = req
                .properties
                .as_ref()
                .map(paper_scene::effects::parse_property_overrides)
                .unwrap_or_default();
            if req.duration_ms == 0
                && group.update_properties(
                    &mut property_source,
                    &req.to,
                    &next_properties,
                    &mut ctl,
                )
            {
                for presenter in &mut presenters {
                    presenter.renderer.clear_transition()?;
                }
                group.drop_from();
                fade_start = None;
                property_frame = true;
                presented = false;
                emitted.fill(false);
                render_pending.fill(true);
                animated = group.animated();
                next_frame = Instant::now();
                continue;
            }
            let loaded = locate_pkg(&req.to)
                .and_then(|path| paper_scene::pkg::Package::open(&path))
                .and_then(|pkg| {
                    let model = stream_model(&pkg, &req.to, &next_properties, screen_resolution)?;
                    Ok((model, extract_scene_audio(&pkg, &req.to, &next_properties)))
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
            if let Err(error) = validate_scene_content(&next) {
                signal_swap_failure(&req.to, &format!("{error:#}"));
                continue;
            }
            if let Err(error) = validate_scene_skips(strict, &next.skipped) {
                signal_swap_failure(&req.to, &format!("{error:#}"));
                continue;
            }
            let load_ms = swap_started.elapsed().as_millis();
            let next_group = match build_group(&sd, &mut next, strict, &sizes, fill_mode()) {
                Ok(group) => group,
                Err(error) => {
                    signal_swap_failure(&req.to, &format!("{error:#}"));
                    continue;
                }
            };
            tracing::info!(
                load_ms,
                build_ms = swap_started.elapsed().as_millis() - load_ms,
                "skwd-wall-vk: scene stream swap built"
            );
            let mut old = std::mem::replace(&mut group, next_group);
            property_source = properties::PropertySource::new(&req.to, &next_properties);
            property_frame = false;
            ctl.set_scene_voices(
                next_audio.as_ref().map(SceneAudio::voices).unwrap_or_default(),
                group.audio.as_ref().map(paper_audio::Analyser::playback_tap),
            );
            group.apply_script_sounds(&mut ctl);
            _scene_audio = next_audio;
            old.drop_from();
            let duration = scene_swap_duration(req.duration_ms);
            if duration.is_some() {
                let old_width = old.target.extent.width;
                let old_height = old.target.extent.height;
                let dummy = vk::PresentedTarget::new(old.renderer.create_scene_target(1, 1)?);
                let target = std::mem::replace(&mut old.target, dummy);
                group.from = Some(FadeSource {
                    source: FadeFrom::Scene(target),
                    width: old_width,
                    height: old_height,
                });
            } else {
                for presenter in &mut presenters {
                    presenter.renderer.clear_transition()?;
                }
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
            emitted.fill(false);
            render_pending.fill(true);
        }
        let pauses = ctl.take_output_pauses();
        if let Some(all_paused) = crate::preview::apply_output_pauses(&mut targets, pauses) {
            ctl.set_paused(all_paused);
        }
        if let Some((revision, position, buttons)) = ctl.pointer() {
            group.mouse.update(revision, position, buttons, sizes[0], fill_mode());
        }
        let mouse_driven = group.mouse.pending();
        if ctl.paused && presented {
            suspended_at.get_or_insert_with(Instant::now);
            crate::preview::wait_any_free(
                &sockets,
                &mut free,
                &vec![false; targets.len()],
                ctl.wake_fd(),
                None,
            )?;
            continue;
        }
        if !property_frame && let Some(started) = suspended_at.take() {
            let suspended = Instant::now().saturating_duration_since(started);
            epoch += suspended;
            last_frame += suspended;
            next_frame = Instant::now();
            if let Some(fade) = &mut fade_start {
                *fade += suspended;
            }
        }
        if presented
            && !animated
            && !targets
                .iter()
                .zip(&render_pending)
                .any(|(target, pending)| !target.paused && *pending)
            && !mouse_driven
            && fade_start.is_none()
            && !group.media.as_ref().is_some_and(media::Media::has_pending)
        {
            if let Some(fd) = ctl.wake_fd() {
                let mut events = [
                    libc::pollfd { fd, events: libc::POLLIN, revents: 0 },
                    libc::pollfd {
                        fd: group.media.as_ref().and_then(media::Media::wake_fd).unwrap_or(-1),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                let result = unsafe { libc::poll(events.as_mut_ptr(), 2, 30_000) };
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
        let active: Vec<bool> = targets
            .iter()
            .zip(&emitted)
            .zip(&render_pending)
            .map(|((target, emitted), pending)| {
                target.active(*emitted)
                    && (*pending || animated || mouse_driven || fade_start.is_some())
            })
            .collect();
        if !crate::preview::wait_any_free(&sockets, &mut free, &active, ctl.wake_fd(), None)? {
            continue;
        }
        let now = Instant::now();
        if now < next_frame {
            std::thread::sleep(next_frame - now);
        }
        let now = Instant::now();
        next_frame = now + frame_gap;
        if (animated || mouse_driven) && !property_frame {
            let dt = now
                .duration_since(last_frame)
                .as_secs_f32()
                .clamp(SCENE_FRAME_DT_MIN, SCENE_FRAME_DT_MAX);
            last_frame = now;
            group.sync_sound_states(&ctl);
            group.compose(epoch.elapsed().as_secs_f32(), dt)?;
            animated = group.animated();
            render_pending.fill(true);
            group.apply_script_sounds(&mut ctl);
        }
        let fade_step = scene_fade_step(
            fade_start.map(|started| started.elapsed().as_secs_f32() * 1000.0 / fade_ms as f32),
            fade_first_frame,
        );
        if fade_start.is_some() {
            render_pending.fill(true);
        }
        for (index, target) in targets.iter().enumerate() {
            crate::preview::drain_acks(target.socket, &mut free[index])?;
            if !active[index] {
                continue;
            }
            let Some(slot) = free[index].iter().position(|value| *value) else {
                continue;
            };
            let presenter = &mut presenters[index];
            if fade_step.render_transition {
                presenter.fade(&group, slot, fade_step.mix, trans_style)?;
            } else {
                presenter.present(&group.target, slot)?;
            }
            presenter.wait_render()?;
            presenter.renderer.complete_external_signal(&presenter.stream_semaphores[slot])?;
            crate::preview::send_packet(
                target.socket,
                &crate::preview::packet(2, slot as u8, 0, 0, 0, 0, 0),
                None,
            )
            .context("send scene stream frame")?;
            free[index][slot] = false;
            render_pending[index] = false;
            if !emitted[index] {
                paper_runtime::plasma::frame_ready_on(target.socket)?;
                emitted[index] = true;
            }
        }
        presented = emitted.iter().all(|emitted| *emitted);
        if presented {
            property_frame = false;
        }
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
#[path = "scene/audio_tests.rs"]
mod audio_tests;

#[cfg(test)]
#[path = "scene/lighting_tests.rs"]
mod lighting_tests;

#[cfg(test)]
#[path = "scene/clipspace_tests.rs"]
mod clipspace_tests;

#[cfg(test)]
#[path = "scene/effect_space_tests.rs"]
mod effect_space_tests;

#[cfg(test)]
#[path = "scene/zoom_tests.rs"]
mod zoom_tests;

#[cfg(test)]
#[path = "scene/camera_graph_tests.rs"]
mod camera_graph_tests;

#[cfg(test)]
#[path = "scene/effect_lifetime_tests.rs"]
mod effect_lifetime_tests;
#[cfg(test)]
#[path = "scene/puppet_fx_tests.rs"]
mod puppet_fx_tests;
#[cfg(test)]
#[path = "scene/sprite_tests.rs"]
mod sprite_tests;
#[cfg(test)]
mod tests;
pub(super) mod thumbnail;

#[cfg(test)]
#[path = "scene/customshader_tests.rs"]
mod customshader_tests;

#[cfg(test)]
#[path = "scene/binding_tests.rs"]
mod binding_tests;
