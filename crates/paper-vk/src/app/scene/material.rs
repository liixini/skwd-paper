use super::{
    EffectBind, FX_ALT_MODEL, FX_ALT_NORMAL, FX_ALT_VIEW_PROJECTION, FX_MODEL_INVERSE,
    FX_MODEL_MATRIX, FX_MODEL_PROJECTION, FX_NORMAL_MATRIX, FX_PROJECTION_INVERSE, Group,
    LayerPose, MATRIX_COLUMN_COMPONENTS, Mat4, SceneModel, TextureInterner, authored_perspective,
    flip_clip_y, layer_normal_matrix, layer_projection_inverse, layer_uniforms, mat4_mul,
    model_inverse, model_matrix, pass_blend, pose_uniforms, scene_view_projection,
    spatial_uniforms, vk,
};
use anyhow::{Context, Result, anyhow};
use paper_scene::effects::{FrameClock, PassMeta};
use std::collections::BTreeMap;

mod channels;
mod prelit;

const COLOR: &str = paper_scene::effects::SOURCE_COLOR_UNIFORM;
const FLAT: &str = "flat";
const FLAT_COLOR: &str = "g_Color";
const FLAT_ALPHA: &str = "g_Alpha";
const AMBIENT: &str = "g_LightAmbientColor";
const SKYLIGHT: &str = "g_LightSkylightColor";
const ROTATION: &str = paper_scene::effects::material::UV_ROTATION;
const TRANSLATION: &str = "g_Texture0Translation";
const HALF: f32 = 0.5;
const AXES: usize = 3;
const CHANNELS: usize = 4;
const MIP_INFO: &str = "MipMapInfo";
const MODEL_TRANSLATION_X: usize = 12;
const MODEL_TRANSLATION_Y: usize = 13;
const TEXTURE_PREFIX: &str = "g_Texture";

pub(super) fn history_mips(values: &mut BTreeMap<String, Vec<f32>>, slot: usize) {
    values.insert(format!("{TEXTURE_PREFIX}{slot}{MIP_INFO}"), vec![vk::SceneHistory::MIP_INFO]);
}

enum Input {
    Texture(usize),
    History,
    Reflection,
    ShadowAtlas,
    LightCookie,
}

struct Gpu {
    device: ash::Device,
    pipelines: Vec<vk::EffectPipeline>,
    mirror: Option<vk::EffectPipeline>,
    vertices: Option<vk::QuadBuffer>,
    mesh: Option<(vk::DynBuffer, vk::DynBuffer, u32)>,
    target: Option<vk::SceneTarget>,
}

impl Drop for Gpu {
    fn drop(&mut self) {
        let _ = crate::shared::wait_idle(&self.device);
        for pipeline in std::mem::take(&mut self.pipelines) {
            pipeline.destroy(&self.device);
        }
        if let Some(pipeline) = self.mirror.take() {
            pipeline.destroy(&self.device);
        }
        if let Some((vertices, indices, _)) = self.mesh.take() {
            drop(vertices);
            drop(indices);
        }
        if let Some(vertices) = self.vertices.take() {
            vertices.destroy(&self.device);
        }
        if let Some(target) = self.target.take() {
            target.destroy(&self.device);
        }
    }
}

struct Puppet {
    id: String,
    model: Option<paper_scene::model::Puppet>,
    sampled: Vec<paper_scene::puppet::playback::State>,
    bone_revision: u64,
    clock: Option<Clock>,
    order: Option<paper_scene::puppet::draw_order::Sorter>,
    reordered: Option<Vec<u8>>,
}

struct Clock {
    playback: paper_scene::puppet::playback::Playback,
    time: f32,
    dt: f32,
    spring: Option<Box<paper_scene::puppet::spring::Simulation>>,
    world: Vec<Mat4>,
}

impl Puppet {
    fn sample(
        &mut self,
        host: &paper_scene::script::SceneScripts,
        palette: &mut paper_scene::effects::material::Palette,
    ) -> Result<bool> {
        let Some(puppet) = &self.model else {
            return Ok(false);
        };
        if let Some(first) = puppet.layers.first()
            && host.puppet_sample(&self.id, first.id).is_none()
        {
            return Err(anyhow!("puppet material clock missing"));
        }
        if host.puppet_owner_retired(&self.id) {
            self.model = None;
            return Ok(false);
        }
        let states = host.puppet_states(&self.id);
        let revision = host.puppet_bone_revision(&self.id);
        self.apply(states, revision, Some(host), palette)
    }

    fn sample_authored(
        &mut self,
        frame: FrameClock,
        palette: &mut paper_scene::effects::material::Palette,
    ) -> Result<bool> {
        let clock = self.clock.as_mut().context("puppet authored clock missing")?;
        clock.playback.advance((frame.time - clock.time).max(0.0));
        clock.time = frame.time;
        clock.dt = frame.dt;
        let states = clock.playback.states(&self.id);
        self.apply(states, 0, None, palette)
    }

    fn apply(
        &mut self,
        states: Vec<paper_scene::puppet::playback::State>,
        revision: u64,
        host: Option<&paper_scene::script::SceneScripts>,
        palette: &mut paper_scene::effects::material::Palette,
    ) -> Result<bool> {
        let Some(puppet) = &self.model else {
            return Ok(false);
        };
        if self.sampled != states
            || self.bone_revision != revision
            || self.clock.as_ref().is_some_and(|clock| clock.spring.is_some())
        {
            let samples: Vec<_> = states
                .iter()
                .filter_map(|state| match state.sample {
                    paper_scene::puppet::playback::Sample::Frame(seconds) => {
                        Some((state.layer, seconds))
                    }
                    paper_scene::puppet::playback::Sample::Rest
                    | paper_scene::puppet::playback::Sample::Retired => None,
                })
                .collect();
            let mut bones = if let Some((_, world)) =
                host.and_then(|host| host.puppet_bone_palette(&self.id))
            {
                world
                    .into_iter()
                    .map(|matrix| Some(paper_scene::puppet::BonePose::Spatial(matrix)))
                    .collect()
            } else {
                paper_scene::puppet::palette_layers(&puppet.mesh, &samples)?
            };
            if let Some(clock) = &mut self.clock
                && let Some(spring) = &mut clock.spring
            {
                anyhow::ensure!(
                    puppet.planar_owner,
                    paper_scene::puppet::spring::OWNER_UNSUPPORTED
                );
                let owner = &puppet.world;
                anyhow::ensure!(
                    paper_scene::puppet::spring::owner_supported(owner),
                    paper_scene::puppet::spring::OWNER_UNSUPPORTED
                );
                paper_scene::puppet::sampled_world(&puppet.mesh, &samples, &mut clock.world, None)?;
                anyhow::ensure!(
                    spring.step(
                        &puppet.mesh.springs,
                        &puppet.mesh.bones,
                        &mut clock.world,
                        &paper_scene::puppet::spring::Frame {
                            dt: clock.dt,
                            owner,
                            scale: paper_scene::puppet::spring::owner_scale(owner),
                        },
                    ),
                    paper_scene::puppet::spring::POSE_INVALID
                );
                anyhow::ensure!(
                    spring.palette(std::sync::Arc::make_mut(&mut bones)),
                    paper_scene::puppet::spring::PALETTE_INVALID
                );
            }
            if bones.len() != palette.bones.len() {
                return Err(anyhow!("puppet material palette changed"));
            }
            palette.bones = bones;
            palette.morph = paper_scene::puppet::morph::uniforms(
                &puppet.mesh,
                &samples,
                host.and_then(|host| host.puppet_morph_weights(&self.id)).as_deref(),
            )?;
            if let Some(morph) = &mut palette.morph {
                morph.pose(&puppet.mesh, &palette.bones)?;
            }
            palette.channels = paper_scene::puppet::channels::weights(&puppet.mesh, &samples)?;
            if let Some(alpha) = &mut palette.alpha {
                alpha.weights = paper_scene::puppet::alpha::weights(&puppet.mesh, &samples);
            }
            if let Some(order) = self.order.as_mut()
                && order.update(&puppet.mesh, &samples)
            {
                self.reordered = Some(order.bytes(&puppet.mesh.indices));
            }
            self.sampled = states;
            self.bone_revision = revision;
        }
        Ok(true)
    }
}

pub(super) struct Material {
    pub quad: usize,
    pub uniforms: BTreeMap<String, Vec<f32>>,
    palette: Option<paper_scene::effects::material::Palette>,
    puppet: Option<Box<Puppet>>,
    channels: Option<channels::Channels>,
    prelit: Option<prelit::Prelit>,
    rect: [f32; 2],
    gpu: Gpu,
    passes: Vec<BasePass>,
    pub slot: usize,
    mirror_slot: Option<usize>,
    source: usize,
    source_uv: [f32; 2],
    effect: bool,
    local_quad: vk::SceneQuad,
    canvas: (f32, f32),
    pub size: (u32, u32),
    image_scale: [f32; 2],
}

struct BasePass {
    meta: PassMeta,
    script_values: BTreeMap<String, Vec<f32>>,
    inputs: Vec<Input>,
    sizes: Vec<Option<[f32; 4]>>,
}

impl Material {
    pub(super) fn mirror_slot(&self) -> Option<usize> {
        self.mirror_slot
    }

    #[cfg(test)]
    pub(super) fn has_spring(&self) -> bool {
        self.puppet
            .as_ref()
            .and_then(|puppet| puppet.clock.as_ref())
            .is_some_and(|clock| clock.spring.is_some())
    }

    pub fn skin_source(&self) -> Option<(usize, [f32; 2])> {
        self.palette.as_ref().map(|_| (self.source, self.source_uv))
    }

    pub fn prepare_effect(&mut self, size: (u32, u32)) {
        self.effect = true;
        self.uniforms.insert(ROTATION.into(), vec![1.0, 0.0, 0.0, 1.0]);
        for pass in &mut self.passes {
            pass.sizes[0] = Some([size.0 as f32, size.1 as f32, size.0 as f32, size.1 as f32]);
        }
    }

    pub fn finish_effect(
        &self,
        renderer: &vk::Renderer,
        textures: &mut [vk::SceneTexture],
        output: usize,
        source: (ash::vk::ImageView, ash::vk::Sampler),
    ) {
        for pipeline in &self.gpu.pipelines {
            renderer.write_effect_input(pipeline, 0, source);
        }
        let [original, result] =
            textures.get_disjoint_mut([self.slot, output]).expect("distinct material effect slots");
        result.effect = original.effect;
        result.effect_tail.clone_from(&original.effect_tail);
    }
    pub fn set_value(&mut self, name: &str, values: &[f32]) {
        if let Some(pass) = self.passes.first_mut() {
            let resolved = pass
                .meta
                .apply_script_values(&serde_json::json!({(name): values}), &BTreeMap::new());
            pass.script_values.extend(resolved);
        }
        if let Some(prelit) = &mut self.prelit {
            let resolved = prelit
                .pass
                .meta
                .apply_script_values(&serde_json::json!({(name): values}), &BTreeMap::new());
            prelit.pass.script_values.extend(resolved);
        }
    }

    pub fn uses_history(&self) -> bool {
        self.prelit.as_ref().is_some_and(prelit::Prelit::uses_history)
            || self
                .passes
                .iter()
                .flat_map(|pass| &pass.inputs)
                .any(|input| matches!(input, Input::History))
    }

    pub fn pointer_dependent(&self) -> bool {
        self.passes.iter().any(|pass| pass.meta.pointer_dependent())
    }

    pub fn frame_dependent(&self) -> bool {
        self.channels.as_ref().is_some_and(channels::Channels::frame_dependent)
            || self
                .passes
                .iter()
                .any(|pass| pass.meta.time_dependent() || pass.meta.audio_dependent())
            || self
                .puppet
                .as_ref()
                .filter(|puppet| puppet.clock.is_some())
                .and_then(|puppet| puppet.model.as_ref())
                .is_some_and(|puppet| {
                    !puppet.mesh.springs.is_empty()
                        || puppet.layers.iter().any(paper_scene::puppet::AnimationLayer::animated)
                })
    }

    pub fn effect_frame_dependent(&self) -> bool {
        (self.palette.is_none() || self.channels.is_some() || self.prelit.is_some())
            && self.frame_dependent()
    }

    pub fn audio_dependent(&self) -> bool {
        self.channels.as_ref().is_some_and(channels::Channels::audio_dependent)
            || self.passes.iter().any(|pass| pass.meta.audio_dependent())
    }

    pub(super) fn bypasses_effects(&self, fx: &super::LayerFx) -> bool {
        self.quad == fx.quad
            && fx.base_quad.texture == self.slot
            && !fx.passthrough
            && !fx.copy_background
            && self.gpu.mesh.is_none()
            && self.palette.is_none()
            && self.gpu.target.is_some()
            && fx.sampled_targets.is_empty()
            && fx.owner.iter().all(|owner| !fx.effects[*owner].visible)
    }

    pub fn target_bytes(&self) -> u64 {
        self.gpu.target.as_ref().map_or(0, |target| target.allocation_bytes)
            + self.channels.as_ref().map_or(0, channels::Channels::bytes)
            + self.prelit.as_ref().map_or(0, prelit::Prelit::bytes)
    }

    pub fn effect_target(
        &mut self,
        renderer: &vk::Renderer,
        textures: &mut [vk::SceneTexture],
        target: Option<vk::SceneTarget>,
        canvas: (f32, f32),
    ) {
        if self.palette.is_some() {
            self.effect = false;
            self.uniforms
                .insert(ROTATION.into(), vec![self.source_uv[0], 0.0, 0.0, self.source_uv[1]]);
            let texture = &textures[self.source];
            let size = [texture.extent.width as f32, texture.extent.height as f32];
            for pass in &mut self.passes {
                pass.sizes[0] = Some([
                    size[0],
                    size[1],
                    size[0] * self.source_uv[0],
                    size[1] * self.source_uv[1],
                ]);
            }
        }
        if let Some(old) = self.gpu.target.take() {
            renderer.destroy_scene_target(old);
        }
        let (view, sampler) = if let Some(target) = &target {
            (target.view, target.sampler)
        } else {
            let source = &textures[self.source];
            (source.view, source.sampler)
        };
        textures[self.slot].view = view;
        textures[self.slot].sampler = sampler;
        renderer.point_slot_at(&textures[self.slot], view);
        let [_, _, width, height] = self.local_quad.rect;
        self.canvas = target.as_ref().map_or(canvas, |_| (width, height));
        self.gpu.target = target;
    }

    fn insert_world(&self, values: &mut BTreeMap<String, Vec<f32>>, screen: Mat4, offscreen: bool) {
        let mut model = self.uniforms[FX_MODEL_MATRIX].clone();
        if offscreen {
            for (column, scale) in
                model.chunks_exact_mut(MATRIX_COLUMN_COMPONENTS).zip(self.image_scale)
            {
                column.iter_mut().for_each(|value| *value *= scale);
            }
        }
        values.insert(FX_ALT_MODEL.into(), model);
        values.insert(FX_ALT_NORMAL.into(), self.uniforms[FX_NORMAL_MATRIX].clone());
        values.insert(FX_ALT_VIEW_PROJECTION.into(), screen.to_vec());
    }

    pub fn sprite(&mut self, frame: &paper_scene::model::SpriteFrame) {
        self.uniforms.insert(ROTATION.into(), frame.axes.to_vec());
        self.uniforms.insert(TRANSLATION.into(), frame.uv[..2].to_vec());
    }

    pub fn update(
        &mut self,
        state: &paper_scene::model::script::Frame,
        canvas: (f32, f32),
        world: bool,
    ) {
        self.uniforms.insert(COLOR.into(), state.source_tint.to_vec());
        self.rect = [state.rect[0], state.rect[1]];
        if self.passes.first().is_some_and(|pass| pass.meta.name == FLAT) {
            self.uniforms.insert(FLAT_COLOR.into(), state.source_tint[..AXES].to_vec());
            self.uniforms.insert(FLAT_ALPHA.into(), vec![state.source_tint[AXES]]);
        }
        if let Some(puppet) = &mut self.puppet
            && let Some(model) = &mut puppet.model
            && !model.mesh.springs.is_empty()
        {
            model.planar_owner = state.planar_owner;
            model.world = super::spring_owner(state, canvas);
        }
        let pose = LayerPose::from(state);
        let spatial = (self.palette.is_some() && !self.effect && self.gpu.target.is_none())
            .then(|| spatial_uniforms(pose, canvas))
            .flatten();
        self.uniforms.extend(spatial.unwrap_or_else(|| pose_uniforms(pose, canvas, world)));
    }
}

struct BaseSource<'a> {
    name: &'a str,
    resolution: [f32; 4],
    slot: usize,
    skinned: bool,
    prelit: bool,
    state: paper_scene::effects::material::ImageState,
}

impl BaseSource<'_> {
    fn pipeline(
        &self,
        renderer: &vk::Renderer,
        pass: &paper_scene::effects::EffectPass,
    ) -> vk::EffectPipelineSpec {
        let blend = pass_blend(pass.blend);
        let depth = vk::EffectDepth {
            test: self.state.test == paper_scene::effects::material::DepthMode::Enabled,
            write: self.state.write == paper_scene::effects::material::DepthMode::Enabled,
            cull: self.state.cull == paper_scene::effects::material::CullMode::Normal,
            order: vk::DepthOrder::Greater,
        };
        if !self.skinned {
            return vk::EffectPipelineSpec {
                depth,
                ..vk::EffectPipelineSpec::quad(
                    renderer.scene_format(),
                    vk::SceneBlend::Copy,
                    Some(blend),
                )
            };
        }
        vk::EffectPipelineSpec {
            depth_attachment: true,
            format: renderer.scene_format(),
            samples: if self.prelit {
                ash::vk::SampleCountFlags::TYPE_1
            } else {
                renderer.scene_sample_count()
            },
            blend: if self.prelit { vk::SceneBlend::Copy } else { blend },
            scene: None,
            geometry: if pass.vertex.source.contains(paper_scene::puppet::morph::POSITION) {
                vk::EffectGeometry::Morph
            } else {
                vk::EffectGeometry::Puppet
            },
            coordinates: vk::EffectCoordinates::Viewport,
            depth: if self.prelit { vk::EffectDepth::default() } else { depth },
        }
    }
}

struct PassSources<'a> {
    renderer: &'a vk::Renderer,
    cookie: Option<(ash::vk::ImageView, ash::vk::Sampler, ash::vk::Extent2D)>,
    shadows: Option<&'a super::shadow::Shadows>,
    reflection: Option<&'a super::mesh::Reflection>,
    history: Option<&'a vk::SceneHistory>,
}

impl PassSources<'_> {
    fn views(
        &self,
        textures: &[vk::SceneTexture],
        inputs: &[Input],
        pipe: &vk::EffectPipeline,
    ) -> Result<Vec<(ash::vk::ImageView, ash::vk::Sampler)>> {
        inputs
            .iter()
            .enumerate()
            .map(|(slot, input)| match input {
                Input::Texture(index) => {
                    let texture = &textures[*index];
                    Ok((texture.view, texture.sampler))
                }
                Input::LightCookie => {
                    let (view, sampler, _) = self.cookie.context("base light cookie missing")?;
                    Ok((view, sampler))
                }
                Input::ShadowAtlas => {
                    let sample = self.shadows.context("base shadow atlas missing")?.sample();
                    Ok(self.renderer.effect_depth_sample(pipe, slot, sample))
                }
                Input::Reflection => {
                    let target = self.reflection.context("base reflection missing")?.target();
                    Ok((target.view, target.sampler))
                }
                Input::History => {
                    self.history.context("base material history missing").map(|history| {
                        let (view, sampler, _) = history.sample();
                        (view, sampler)
                    })
                }
            })
            .collect()
    }
}

fn input_mips(inputs: &[Input], values: &mut BTreeMap<String, Vec<f32>>) {
    for (slot, input) in inputs.iter().enumerate() {
        if matches!(input, Input::History) {
            history_mips(values, slot);
        }
    }
}

fn palette_values(
    palette: &paper_scene::effects::material::Palette,
    values: &mut BTreeMap<String, Vec<f32>>,
) -> Result<()> {
    let mut matrices = Vec::with_capacity(
        palette.bones.len() * MATRIX_COLUMN_COMPONENTS * MATRIX_COLUMN_COMPONENTS,
    );
    for bone in palette.bones.iter() {
        let Some(paper_scene::puppet::BonePose::Spatial(matrix)) = bone else {
            return Err(anyhow!("puppet material palette changed"));
        };
        matrices.extend_from_slice(matrix);
    }
    values.insert(palette.uniform.clone(), matrices);
    if let Some(alpha) = &palette.alpha {
        values.insert(alpha.name.clone(), alpha.weights.clone());
    }
    if let Some(morph) = &palette.morph {
        values.insert(paper_scene::puppet::morph::OFFSETS.into(), morph.offsets.to_vec());
        values.insert(paper_scene::puppet::morph::WEIGHTS.into(), morph.weights.to_vec());
        if let Some(modifiers) = &morph.modifiers {
            values.insert(
                paper_scene::puppet::morph::TRANSFORMS.into(),
                modifiers.transforms.iter().flatten().copied().collect(),
            );
            values.insert(
                paper_scene::puppet::morph::RULES.into(),
                modifiers.rules.iter().flatten().copied().collect(),
            );
        }
    }
    Ok(())
}

pub(super) struct Build<'a, 'device> {
    pub mirrored: bool,
    pub unreflected: std::collections::BTreeSet<String>,
    pub renderer: &'a mut vk::Renderer,
    pub interner: &'a mut TextureInterner<'device>,
    pub textures: &'a mut Vec<vk::SceneTexture>,
    pub slots: &'a mut [usize],
    pub quads: &'a mut [vk::SceneQuad],
    pub screen: ash::vk::Extent2D,
    pub world: bool,
}

impl Build<'_, '_> {
    fn base_pass(
        &mut self,
        pass: &mut paper_scene::effects::EffectPass,
        spec: &BaseSource<'_>,
        gpu: &mut Gpu,
    ) -> Result<BasePass> {
        let pipeline_spec = spec.pipeline(self.renderer, pass);
        let meta = PassMeta::of(pass);
        if meta.matrix_offset(FX_MODEL_PROJECTION).is_none() {
            return Err(anyhow!("{}: base shader lacks image projection", spec.name));
        }
        gpu.pipelines.push(
            self.renderer
                .create_effect_pipeline_for(
                    &pass.vertex,
                    &pass.fragment,
                    pass.hlsl.as_ref(),
                    &pass.name,
                    pipeline_spec,
                )
                .with_context(|| format!("{}: base material", spec.name))?,
        );
        let pipeline = gpu.pipelines.last().expect("base pipeline");
        if spec.skinned && (!pipeline.hlsl || !vk::d3d_clip_enabled()) {
            return Err(anyhow!("puppet material requires qualified HLSL clip convention"));
        }
        let mut inputs = Vec::new();
        let mut sizes = Vec::new();
        for slot in 0..pipeline.sampler_count.max(1) as usize {
            if pass.binds.iter().any(|(index, binding)| {
                *index == slot && matches!(binding, EffectBind::SceneHistory)
            }) {
                inputs.push(Input::History);
                sizes.push(Some(super::size4(self.screen)));
            } else if pass
                .binds
                .iter()
                .any(|(index, binding)| *index == slot && matches!(binding, EffectBind::Reflection))
            {
                inputs.push(Input::Reflection);
                sizes.push(Some(super::size4(self.screen)));
            } else if pass.binds.iter().any(|(index, binding)| {
                *index == slot && matches!(binding, EffectBind::LightCookie)
            }) {
                inputs.push(Input::LightCookie);
                sizes.push(None);
            } else if pass.binds.iter().any(|(index, binding)| {
                *index == slot && matches!(binding, EffectBind::ShadowAtlas)
            }) {
                inputs.push(Input::ShadowAtlas);
                sizes.push(None);
            } else if slot == 0 {
                inputs.push(Input::Texture(spec.slot));
                sizes.push(Some(spec.resolution));
            } else if !pass
                .vertex
                .samplers
                .iter()
                .chain(&pass.fragment.samplers)
                .any(|sampler| sampler.index as usize == slot)
            {
                inputs.push(Input::Texture(spec.slot));
                sizes.push(None);
            } else {
                let texture = pass
                    .textures
                    .get_mut(slot)
                    .and_then(Option::as_mut)
                    .with_context(|| format!("{}: missing base sampler {slot}", spec.name))?;
                sizes.push(Some([
                    texture.width as f32,
                    texture.height as f32,
                    texture.img_width as f32,
                    texture.img_height as f32,
                ]));
                inputs.push(Input::Texture(self.interner.intern_texture(
                    self.renderer,
                    self.textures,
                    texture,
                    vk::MipSampling::Anisotropic,
                )?));
            }
        }
        Ok(BasePass { meta, script_values: BTreeMap::new(), inputs, sizes })
    }

    pub fn load(&mut self, model: &mut SceneModel) -> Result<Vec<Material>> {
        let mut materials = Vec::new();
        for (index, layer) in model.layers.iter_mut().enumerate() {
            if let Some(material) = self.load_one(
                index,
                layer,
                model.canvas,
                (model.ambient, model.skylight),
                model.scripts.as_ref(),
            )? {
                materials.push(material);
            }
        }
        Ok(materials)
    }

    #[expect(clippy::too_many_lines, reason = "one base material GPU resource transaction")]
    pub fn load_one(
        &mut self,
        index: usize,
        layer: &mut paper_scene::model::Layer,
        canvas: (f32, f32),
        lights: ([f32; AXES], [f32; AXES]),
        scripts: Option<&paper_scene::script::SceneScripts>,
    ) -> Result<Option<Material>> {
        let mirrored = self.mirrored
            && !self.unreflected.contains(&layer.id)
            && !paper_scene::model::reflection_gap(layer);
        let Some(material) = layer.material.as_mut() else {
            return Ok(None);
        };
        let flat = material.pass.name == FLAT;
        let mut skin = material.skin.take();
        let skinned = skin.is_some();
        let mut gpu = Gpu {
            device: self.renderer.device.clone(),
            pipelines: Vec::new(),
            mirror: None,
            vertices: None,
            mesh: None,
            target: None,
        };
        let source = self.slots[index];
        let channels = skin
            .as_mut()
            .and_then(|skin| skin.channels.take())
            .map(|authored| channels::Channels::new(self, authored, source, &layer.texture))
            .transpose()?;
        let source = channels.as_ref().map_or(source, |channels| channels.output);
        if let Some(skin) = &skin {
            let vertices = self
                .renderer
                .create_dyn_buffer(skin.vertices.len(), ash::vk::BufferUsageFlags::VERTEX_BUFFER)?;
            self.renderer.write_dyn_buffer(&vertices, &skin.vertices);
            let bytes: Vec<_> = skin.indices.iter().flat_map(|index| index.to_ne_bytes()).collect();
            let indices = self
                .renderer
                .create_dyn_buffer(bytes.len(), ash::vk::BufferUsageFlags::INDEX_BUFFER)?;
            self.renderer.write_dyn_buffer(&indices, &bytes);
            gpu.mesh = Some((vertices, indices, skin.indices.len() as u32));
        }
        let authored = material.prelit.take();
        let baked = authored.is_some();
        let resolution = if channels.is_some() {
            [
                layer.texture.img_width as f32,
                layer.texture.img_height as f32,
                layer.texture.img_width as f32,
                layer.texture.img_height as f32,
            ]
        } else {
            [
                layer.texture.width as f32,
                layer.texture.height as f32,
                layer.texture.img_width as f32,
                layer.texture.img_height as f32,
            ]
        };
        let spec = BaseSource {
            name: &layer.name,
            resolution,
            slot: source,
            skinned,
            prelit: baked,
            state: material.state,
        };
        let prelit = authored
            .map(|authored| {
                let (vertices, indices, count) =
                    gpu.mesh.as_ref().context("prelit mesh missing")?;
                let geometry = vk::MeshGeometry {
                    vertices: vertices.buffer,
                    indices: indices.buffer,
                    index_type: ash::vk::IndexType::UINT16,
                    count: *count,
                };
                prelit::Prelit::new(self, authored, &spec, geometry, &layer.texture)
            })
            .transpose()?;
        let output = prelit.as_ref().map(|prelit| prelit.output);
        let source = output.unwrap_or(source);
        let resolution = if output.is_some() {
            [
                layer.texture.img_width as f32,
                layer.texture.img_height as f32,
                layer.texture.img_width as f32,
                layer.texture.img_height as f32,
            ]
        } else {
            resolution
        };
        let spec = BaseSource { resolution, slot: source, prelit: false, ..spec };
        let passes = vec![self.base_pass(&mut material.pass, &spec, &mut gpu)?];
        let mirror_slot = if mirrored && !skinned && layer.effects.is_empty() {
            gpu.mirror = Some(self.renderer.create_effect_pipeline_for(
                &material.pass.vertex,
                &material.pass.fragment,
                material.pass.hlsl.as_ref(),
                &material.pass.name,
                spec.pipeline(self.renderer, &material.pass),
            )?);
            let mut view = self.renderer.create_view_slot()?;
            view.view = self.textures[source].view;
            view.sampler = self.textures[source].sampler;
            self.renderer.point_slot_at(&view, view.view);
            self.textures.push(view);
            Some(self.textures.len() - 1)
        } else {
            None
        };
        let blend = pass_blend(material.blend);
        let image = (
            layer.size.0 / if layer.scale.0 == 0.0 { 1.0 } else { layer.scale.0 },
            layer.size.1 / if layer.scale.1 == 0.0 { 1.0 } else { layer.scale.1 },
        );
        let image = layer.geometry.map_or(image, |geometry| (geometry.size[0], geometry.size[1]));
        let size = (image.0.max(1.0) as u32, image.1.max(1.0) as u32);
        let still = layer.kind == paper_scene::model::LayerKind::Image
            && !layer.spritesheet
            && layer.texture.frames.is_empty()
            && layer.texture.system_texture.is_none()
            && layer.texture.video.is_none();
        let image_scale = if still && !layer.solid {
            [(layer.texture.img_width, image.0), (layer.texture.img_height, image.1)]
                .map(|(texels, extent)| texels as f32 / extent)
        } else {
            [1.0; 2]
        };
        let inset = if layer.pad_uv {
            [
                paper_scene::model::IMAGE_UV_INSET / layer.texture.width as f32,
                paper_scene::model::IMAGE_UV_INSET / layer.texture.height as f32,
            ]
        } else {
            [0.0; 2]
        };
        let palette = if let Some(skin) = skin {
            Some(skin.palette)
        } else {
            let (right, bottom) = if still { layer.texture.uv_scale() } else { (1.0, 1.0) };
            let [u, v] = inset;
            gpu.vertices = Some(self.renderer.create_quad_buffer(
                size,
                [u, v, right - u, bottom - v],
                image,
            )?);
            None
        };
        let mut view = self.renderer.create_view_slot()?;
        view.view = self.textures[source].view;
        view.sampler = self.textures[source].sampler;
        self.renderer.point_slot_at(&view, view.view);
        let slot = self.textures.len();
        self.textures.push(view);
        let [width, height, image_width, image_height] = resolution;
        let source_uv = [image_width / width, image_height / height];
        let mut uniforms = layer_uniforms(layer, canvas, lights, self.world);
        if skinned {
            if layer.effects.is_empty()
                && let Some(spatial) = spatial_uniforms(LayerPose::from(&*layer), canvas)
            {
                uniforms.extend(spatial);
            }
            uniforms.insert(ROTATION.into(), vec![source_uv[0], 0.0, 0.0, source_uv[1]]);
            uniforms.insert(TRANSLATION.into(), vec![0.0; 2]);
        }
        uniforms.insert(
            COLOR.into(),
            vec![layer.color[0], layer.color[1], layer.color[2], layer.alpha],
        );
        if flat {
            uniforms.insert(FLAT_COLOR.into(), layer.color.to_vec());
            uniforms.insert(FLAT_ALPHA.into(), vec![layer.alpha]);
        }
        if let Some(frame) = layer.atlas_frames().and_then(|frames| frames.first()) {
            uniforms.insert(ROTATION.into(), frame.axes.to_vec());
            uniforms.insert(TRANSLATION.into(), frame.uv[..2].to_vec());
        }
        let mut local_quad = self.quads[index].clone();
        local_quad.texture = slot;
        local_quad.rect = [image.0 * HALF, image.1 * HALF, image.0, image.1];
        local_quad.angle = 0.0;
        local_quad.projection = None;
        local_quad.uv = [0.0, 0.0, 1.0, 1.0];
        local_quad.uv_axes = None;
        local_quad.tint = [1.0; CHANNELS];
        self.slots[index] = slot;
        self.quads[index].texture = self.slots[index];
        self.quads[index].uv = [0.0, 0.0, 1.0, 1.0];
        self.quads[index].uv_axes = None;
        self.quads[index].blend = blend;
        let puppet = palette
            .as_ref()
            .and_then(|_| layer.puppet.take())
            .map(|mut puppet| {
                let clock = if scripts.is_none() {
                    Some(Clock {
                        playback: paper_scene::puppet::playback::Playback::authored(
                            &layer.id,
                            &puppet.mesh,
                            &puppet.layers,
                        )?,
                        time: 0.0,
                        dt: 0.0,
                        spring: paper_scene::puppet::spring::Simulation::of(&puppet.mesh),
                        world: Vec::new(),
                    })
                } else {
                    None
                };
                let ids = puppet.mesh.animations.iter().map(|clip| clip.id).collect::<Vec<_>>();
                paper_scene::puppet::retain_animations(&mut puppet.mesh, &ids)?;
                let order = paper_scene::puppet::draw_order::Sorter::of(&puppet.mesh);
                Ok::<_, anyhow::Error>(Box::new(Puppet {
                    id: layer.id.clone(),
                    model: Some(puppet),
                    sampled: Vec::new(),
                    bone_revision: 0,
                    clock,
                    order,
                    reordered: None,
                }))
            })
            .transpose()?;
        Ok(Some(Material {
            quad: index,
            uniforms,
            puppet,
            channels,
            prelit,
            rect: [self.quads[index].rect[0], self.quads[index].rect[1]],
            palette,
            gpu,
            passes,
            slot,
            mirror_slot,
            source,
            source_uv,
            effect: false,
            local_quad,
            canvas,
            size,
            image_scale,
        }))
    }
}

impl Group {
    pub(super) fn render_texture_channels(
        &mut self,
        clock: FrameClock,
        batched: bool,
    ) -> Result<()> {
        for material in &mut self.materials {
            if let (Some(channels), Some(palette)) = (&mut material.channels, &material.palette) {
                channels.render(
                    &mut self.renderer,
                    &self.textures,
                    clock,
                    &palette.channels,
                    batched,
                )?;
            }
            if let Some(prelit) = &mut material.prelit {
                prelit.render(&mut self.renderer, &self.textures, batched)?;
            }
        }
        Ok(())
    }

    pub(super) fn material_value(
        &mut self,
        quad: usize,
        name: &str,
        values: &[f32],
        scene: &serde_json::Value,
        properties: &paper_scene::model::Properties,
        resources: &paper_scene::model::ImageResources,
    ) -> Result<bool> {
        if let Some(material) = self.materials.iter_mut().find(|material| material.quad == quad) {
            material.set_value(name, values);
            return Ok(true);
        }
        let Some(mut layer) = resources.base_material_layer(scene, properties, quad)? else {
            return Ok(false);
        };
        self.renderer.wait_render()?;
        let old_quad = self.quads[quad].clone();
        let first_texture = self.textures.len();
        let mut slots: Vec<_> = self.quads.iter().map(|quad| quad.texture).collect();
        let mut interner = TextureInterner::new(None);
        let light = |name| {
            self.scene_uniforms
                .get(name)
                .and_then(|values| <[f32; AXES]>::try_from(values.as_slice()).ok())
                .unwrap_or([0.0; AXES])
        };
        let lights = (light(AMBIENT), light(SKYLIGHT));
        let source_size = super::image_effect_dimensions(&layer);
        let result = Build {
            mirrored: self.reflection.as_ref().is_some_and(|reflection| reflection.sampled),
            unreflected: self.unreflected.clone(),
            renderer: &mut self.renderer,
            interner: &mut interner,
            textures: &mut self.textures,
            slots: &mut slots,
            quads: &mut self.quads,
            screen: self.target.extent,
            world: self.camera.is_some(),
        }
        .load_one(quad, &mut layer, self.canvas, lights, None);
        let mut material = match result {
            Ok(Some(material)) => material,
            Ok(None) => {
                self.quads[quad] = old_quad;
                return Ok(false);
            }
            Err(error) => {
                self.quads[quad] = old_quad;
                for texture in self.textures.drain(first_texture..) {
                    self.renderer.destroy_scene_texture(texture);
                }
                return Err(error);
            }
        };
        let fx_index = self.fx.iter().position(|fx| fx.quad == quad);
        let linked = (|| -> Result<()> {
            let allocated =
                self.effect_target_allocation_bytes().saturating_add(material.target_bytes());
            anyhow::ensure!(
                allocated <= self.effect_budget,
                "base material targets exceed effect budget"
            );
            if let Some(index) = fx_index {
                let size = source_size.unwrap_or(material.size);
                let target = self.renderer.create_scene_target(size.0, size.1)?;
                if allocated.saturating_add(target.allocation_bytes) > self.effect_budget {
                    self.renderer.destroy_scene_target(target);
                    return Err(anyhow!("base material effect target exceeds effect budget"));
                }
                material.effect_target(
                    &self.renderer,
                    &mut self.textures,
                    Some(target),
                    self.canvas,
                );
                let base = &mut self.fx[index].base_quad;
                base.texture = material.slot;
                base.tint = [1.0; CHANNELS];
                base.uv = [0.0, 0.0, 1.0, 1.0];
                base.uv_axes = None;
            }
            Ok(())
        })();
        if let Err(error) = linked {
            self.quads[quad] = old_quad;
            for texture in self.textures.drain(first_texture..) {
                self.renderer.destroy_scene_texture(texture);
            }
            return Err(error);
        }
        material.set_value(name, values);
        self.materials.push(material);
        Ok(true)
    }

    pub(super) fn material_frame(&mut self, clock: FrameClock) -> Result<()> {
        let sources = PassSources {
            renderer: &self.renderer,
            cookie: self.cookie_sample(),
            shadows: self.shadows.as_ref(),
            reflection: self.reflection.as_ref(),
            history: self.scene_history.as_ref(),
        };
        for material in &mut self.materials {
            self.textures[material.slot].effect = None;
            self.textures[material.slot].effect_tail.clear();
            if let Some(slot) = material.mirror_slot {
                self.textures[slot].effect = None;
            }
            if let Some(puppet) = &mut material.puppet {
                let palette = material.palette.as_mut().context("puppet palette missing")?;
                let visible = if let Some(host) =
                    self.scripts.as_ref().and_then(|scripts| scripts.host.as_ref())
                {
                    puppet.sample(host, palette)?
                } else {
                    puppet.sample_authored(clock, palette)?
                };
                if let Some((_, indices, _)) = &material.gpu.mesh
                    && let Some(bytes) = puppet.reordered.take()
                {
                    self.renderer.write_dyn_buffer(indices, &bytes);
                }
                if !visible {
                    continue;
                }
            }
            if let (Some(prelit), Some(palette)) = (&material.prelit, &material.palette) {
                let mut values = self.scene_uniforms.clone();
                values.extend(material.uniforms.clone());
                values.insert(COLOR.into(), vec![1.0; super::RGBA_CHANNELS]);
                let [u, v] = prelit.source_uv;
                values.insert(ROTATION.into(), vec![u, 0.0, 0.0, v]);
                values.insert(TRANSLATION.into(), vec![0.0; 2]);
                input_mips(&prelit.pass.inputs, &mut values);
                palette_values(palette, &mut values)?;
                let mut screen: Mat4 = material.uniforms[super::FX_VIEW_PROJECTION]
                    .as_slice()
                    .try_into()
                    .expect("base view projection");
                super::layer_view(material.quad, self.bloom.quad, &self.screen_quads, self.clip)
                    .apply(&mut screen, false);
                material.insert_world(&mut values, screen, false);
                values.insert(
                    super::mesh::EYE_POSITION.into(),
                    super::orthographic_eye(self.canvas).to_vec(),
                );
                values.insert(FX_MODEL_PROJECTION.into(), prelit.projection.to_vec());
                values.extend(prelit.pass.script_values.clone());
                let bytes = prelit.pass.meta.uniform_bytes_with(
                    clock,
                    None,
                    material.size.0,
                    material.size.1,
                    (self.target.extent.width, self.target.extent.height),
                    &prelit.pass.sizes,
                    &values,
                    prelit.pipeline().hlsl && vk::d3d_clip_enabled(),
                );
                let channeled = material.channels.is_some();
                if let Some(prelit) = material.prelit.as_mut()
                    && let Some(bytes) = prelit.sample(bytes, channeled)
                {
                    let views =
                        sources.views(&self.textures, &prelit.pass.inputs, prelit.pipeline())?;
                    self.renderer.write_effect_inputs(prelit.pipeline(), &views, &bytes);
                }
            }
            let offscreen = material.gpu.target.is_some()
                && !self.fx.iter().any(|fx| material.bypasses_effects(fx));
            let canvas = if offscreen { material.canvas } else { self.canvas };
            let (view, sampler) = if offscreen {
                let target = material.gpu.target.as_ref().expect("base target");
                (target.view, target.sampler)
            } else {
                let source = &self.textures[material.source];
                (source.view, source.sampler)
            };
            let texture = &mut self.textures[material.slot];
            if texture.view != view || texture.sampler != sampler {
                texture.view = view;
                texture.sampler = sampler;
                self.renderer.point_slot_at(texture, view);
            }
            for (pass, pipeline) in material.passes.iter().zip(&material.gpu.pipelines) {
                let pipe = if material.palette.is_some() || offscreen {
                    pipeline
                } else {
                    pipeline.scene.as_ref().context("base image pipeline")?
                };
                let mut model: Mat4 = if offscreen {
                    model_matrix((canvas.0 * HALF, canvas.1 * HALF, 0.0), [1.0; AXES], 0.0)
                } else {
                    material.uniforms[FX_MODEL_MATRIX]
                        .as_slice()
                        .try_into()
                        .expect("base model matrix")
                };
                if material.palette.is_some() && self.target.viewport.is_none() {
                    let rect = self.quads[material.quad].rect;
                    model[MODEL_TRANSLATION_X] += rect[0] - material.rect[0];
                    model[MODEL_TRANSLATION_Y] -= rect[1] - material.rect[1];
                }
                let camera = self.camera.filter(|_| !offscreen);
                let d3d_clip = pipe.hlsl && vk::d3d_clip_enabled();
                let mut view = if material.palette.is_some() {
                    let whole = [self.canvas.0 as u32, self.canvas.1 as u32];
                    super::mesh::orthographic_view(
                        self.target
                            .viewport
                            .or_else(|| {
                                vk::SceneViewport::cover([self.canvas.0, self.canvas.1], whole)
                            })
                            .context("puppet viewport missing")?,
                    )
                } else {
                    camera.map_or_else(
                        || scene_view_projection(canvas),
                        |camera| authored_perspective(camera, self.canvas, true).0,
                    )
                };
                if !d3d_clip {
                    flip_clip_y(&mut view);
                }
                let clip = if offscreen {
                    vk::ClipView::IDENTITY
                } else {
                    super::layer_view(material.quad, self.bloom.quad, &self.screen_quads, self.clip)
                };
                let placed = mat4_mul(&view, &model);
                clip.apply(&mut view, !d3d_clip);
                let projection = mat4_mul(&view, &model);
                let mut values = self.scene_uniforms.clone();
                values.extend(material.uniforms.clone());
                if material.effect && (offscreen || material.palette.is_some()) {
                    values.insert(COLOR.into(), vec![1.0; super::RGBA_CHANNELS]);
                }
                input_mips(&pass.inputs, &mut values);
                values.insert(FX_MODEL_MATRIX.into(), model.to_vec());
                if pass.meta.matrix_offset(FX_ALT_MODEL).is_some() {
                    let mut screen: Mat4 = if let Some(camera) = self.camera {
                        let mut view = authored_perspective(camera, self.canvas, true).0;
                        if !d3d_clip {
                            flip_clip_y(&mut view);
                        }
                        view
                    } else {
                        material.uniforms[super::FX_VIEW_PROJECTION]
                            .as_slice()
                            .try_into()
                            .expect("base view projection")
                    };
                    super::layer_view(
                        material.quad,
                        self.bloom.quad,
                        &self.screen_quads,
                        self.clip,
                    )
                    .apply(&mut screen, false);
                    material.insert_world(&mut values, screen, offscreen);
                }
                if let Some(palette) = &material.palette {
                    palette_values(palette, &mut values)?;
                }
                if camera.is_none()
                    && material.palette.is_none()
                    && !offscreen
                    && let Some(view_projection) = values.get_mut(super::FX_VIEW_PROJECTION)
                    && let Ok(view_projection) =
                        <&mut Mat4>::try_from(view_projection.as_mut_slice())
                {
                    clip.apply(view_projection, false);
                }
                if camera.is_some() || material.palette.is_some() || offscreen {
                    camera_uniforms(
                        &pass.meta,
                        &mut values,
                        &view,
                        &projection,
                        material.palette.as_ref().map(|_| super::orthographic_eye(self.canvas)),
                    )?;
                }
                if offscreen {
                    let inverse =
                        model_inverse((canvas.0 * HALF, canvas.1 * HALF, 0.0), [1.0; AXES], 0.0);
                    values.insert(FX_MODEL_INVERSE.into(), inverse.to_vec());
                    values.insert(
                        FX_PROJECTION_INVERSE.into(),
                        layer_projection_inverse(&inverse, canvas).to_vec(),
                    );
                    values.insert(FX_NORMAL_MATRIX.into(), layer_normal_matrix(0.0).to_vec());
                }
                values.insert(FX_MODEL_PROJECTION.into(), projection.to_vec());
                values.extend(pass.script_values.clone());
                let screen = (self.target.extent.width, self.target.extent.height);
                let (width, height) = material.size;
                let pass_bytes = |values: &BTreeMap<String, Vec<f32>>, hlsl: bool| {
                    pass.meta.uniform_bytes_with(
                        clock,
                        None,
                        width,
                        height,
                        screen,
                        &pass.sizes,
                        values,
                        hlsl && vk::d3d_clip_enabled(),
                    )
                };
                let bytes = pass_bytes(&values, pipe.hlsl);
                let views = sources.views(&self.textures, &pass.inputs, pipe)?;
                self.renderer.write_effect_inputs(pipe, &views, &bytes);
                if let (Some(slot), Some(mirror), false) =
                    (material.mirror_slot, material.gpu.mirror.as_ref(), offscreen)
                {
                    let pipe = mirror.scene.as_ref().context("base image pipeline")?;
                    let mut mirrored = view;
                    super::mesh::mirror_view(&mut mirrored);
                    let projection = mat4_mul(&mirrored, &model);
                    camera_uniforms(&pass.meta, &mut values, &mirrored, &projection, None)?;
                    values.insert(FX_MODEL_PROJECTION.into(), projection.to_vec());
                    super::reflection::mirror_overrides(&mut values, &[super::mesh::EYE_POSITION]);
                    let bytes = pass_bytes(&values, pipe.hlsl);
                    let views = sources.views(&self.textures, &pass.inputs, pipe)?;
                    self.renderer.write_effect_inputs(pipe, &views, &bytes);
                    self.textures[slot].effect = Some(vk::SceneEffect::new(
                        pipe,
                        material.gpu.vertices.as_ref().expect("base vertices"),
                        Some(vk::EffectProjection::fixed(
                            projection,
                            pass.meta.matrix_offset(FX_MODEL_PROJECTION).expect("base projection"),
                        )),
                    )?);
                }

                let mut effect = if let Some((vertices, indices, count)) = &material.gpu.mesh {
                    vk::SceneEffect::mesh(
                        pipe,
                        vk::MeshGeometry {
                            vertices: vertices.buffer,
                            indices: indices.buffer,
                            index_type: ash::vk::IndexType::UINT16,
                            count: *count,
                        },
                        Some(vk::EffectProjection::fixed(
                            projection,
                            pass.meta.matrix_offset(FX_MODEL_PROJECTION).expect("base projection"),
                        )),
                    )?
                } else {
                    vk::SceneEffect::new(
                        pipe,
                        material.gpu.vertices.as_ref().expect("base vertices"),
                        Some(if camera.is_some() {
                            vk::EffectProjection::fixed(
                                projection,
                                pass.meta
                                    .matrix_offset(FX_MODEL_PROJECTION)
                                    .expect("base projection"),
                            )
                        } else {
                            vk::EffectProjection::for_layer(
                                placed,
                                model,
                                canvas,
                                pass.meta
                                    .matrix_offset(FX_MODEL_PROJECTION)
                                    .expect("base projection"),
                            )
                            .viewed(clip)
                        }),
                    )?
                };
                if offscreen {
                    effect.remove_padding();
                }
                if self.textures[material.slot].effect.is_none() {
                    self.textures[material.slot].effect = Some(effect);
                } else {
                    self.textures[material.slot].effect_tail.push(effect);
                }
            }
        }
        for material in &self.materials {
            if let Some(target) = &material.gpu.target
                && self.textures[material.slot].view == target.view
            {
                self.renderer.render_scene_with_canvas(
                    target,
                    [material.canvas.0, material.canvas.1],
                    [0.0; CHANNELS],
                    std::slice::from_ref(&material.local_quad),
                    &self.textures,
                    &[],
                )?;
                self.textures[material.slot].effect = None;
                self.textures[material.slot].effect_tail.clear();
            }
        }
        Ok(())
    }
}

fn camera_uniforms(
    meta: &PassMeta,
    values: &mut BTreeMap<String, Vec<f32>>,
    view: &Mat4,
    projection: &Mat4,
    eye: Option<[f32; AXES]>,
) -> Result<()> {
    values.insert(super::FX_VIEW_PROJECTION.into(), view.to_vec());
    if let Some(eye) = eye {
        values.insert(super::mesh::EYE_POSITION.into(), eye.to_vec());
    }
    if meta.matrix_offset(FX_PROJECTION_INVERSE).is_some() {
        let inverse = paper_scene::matrix::mat4_inverse(projection)
            .context("image camera projection is singular")?;
        values.insert(FX_PROJECTION_INVERSE.into(), inverse.to_vec());
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod instance_tests;
