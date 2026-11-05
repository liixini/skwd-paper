use super::{
    FX_MODEL_MATRIX, FX_MODEL_PROJECTION, FX_NORMAL_MATRIX, FX_VIEW_PROJECTION, FX_VIEW_RIGHT,
    FX_VIEW_UP, MATRIX_COLUMN_COMPONENTS, Mat4, ORTHOGRAPHIC_EYE_Z, RGBA_CHANNELS, SceneModel,
    TextureInterner, authored_perspective, mat4_mul, orthographic_eye, pass_blend, perspective_eye,
    scene_ortho, scene_perspective, vk,
};
use anyhow::{Context, Result, ensure};
use ash::vk as raw;
use paper_scene::effects::{CompositeBuffer, EffectBind, FrameClock, PassMeta};
use paper_scene::matrix::model_matrix as mesh_model_matrix;
use paper_scene::model::mesh::VertexFormat;
use paper_scene::model::{CameraBasis, ProjectionMode};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

const ALPHA_COMPONENT: usize = RGBA_CHANNELS - 1;
const NORMAL_COMPONENTS: usize = 3;
const UV_COMPONENTS: usize = 2;
const MATRIX_SCALE_X: usize = 0;
const MATRIX_SCALE_Y: usize = 5;
const MATRIX_DEPTH_SCALE: usize = 10;
pub(super) const MATRIX_TRANSLATION_X: usize = 12;
pub(super) const MATRIX_TRANSLATION_Y: usize = 13;
const NORMAL_MATRIX_FLOATS: usize = MATRIX_COLUMN_COMPONENTS * NORMAL_COMPONENTS;
pub(super) const EYE_POSITION: &str = "g_EyePosition";
const AMBIENT_COLOR: &str = "g_LightAmbientColor";
const SKYLIGHT_COLOR: &str = "g_LightSkylightColor";
const COMPOSITE_ALPHA_GAP: &str = "mesh composite alpha coverage unsupported";

struct Rig {
    sampler: paper_scene::puppet::sampler::Sampler,
    samples: Vec<(paper_scene::puppet::AnimationLayer, f32)>,
    bones: usize,
}

impl Rig {
    fn new(layer: &paper_scene::model::MeshLayer) -> Result<Self> {
        let mesh = layer.mesh.rig.as_ref().context("mesh rig missing")?;
        let mut sampler = paper_scene::puppet::sampler::Sampler::new(Rc::clone(mesh))?;
        sampler.set_owner(mesh_matrices(layer.pose, &layer.parents, None).0);
        Ok(Self {
            bones: mesh.bones.len(),
            sampler,
            samples: layer.animations.iter().map(|layer| (*layer, 0.0)).collect(),
        })
    }

    fn sample(&mut self, time: f32) -> Result<paper_scene::puppet::sampler::Sample<'_>> {
        for (layer, seconds) in &mut self.samples {
            *seconds = time * layer.rate;
        }
        self.sampler.sample(&self.samples, time)
    }
}

#[derive(Clone)]
struct Mount {
    rig: Option<Rc<RefCell<Rig>>>,
    bone: usize,
    local: Mat4,
    depth: usize,
    pose: paper_scene::model::MeshPose,
    parents: Vec<paper_scene::model::MeshPose>,
    world: Mat4,
    revision: u64,
}

impl Mount {
    fn new(
        layer: &paper_scene::model::MeshLayer,
        layers: &[paper_scene::model::MeshLayer],
        rigs: &HashMap<String, Rc<RefCell<Rig>>>,
    ) -> Result<Option<Self>> {
        let Some(mount) = &layer.mount else { return Ok(None) };
        let owner = layers
            .iter()
            .find(|owner| owner.id == mount.owner)
            .context("mesh attachment owner missing")?;
        let rig = rigs.get(&owner.id).context("mesh attachment rig missing")?;
        let attachment = owner
            .mesh
            .rig
            .as_ref()
            .and_then(|rig| {
                rig.attachments.iter().find(|attachment| attachment.name == mount.attachment)
            })
            .context("mesh attachment missing")?;
        let mut mounted = Self {
            rig: owner.animated().then(|| Rc::clone(rig)),
            bone: usize::from(attachment.bone),
            local: attachment.matrix,
            depth: mount.depth,
            pose: layer.pose,
            parents: layer.parents.clone(),
            world: paper_scene::matrix::IDENTITY,
            revision: 0,
        };
        mounted.sample(rig, 0.0)?;
        Ok(Some(mounted))
    }

    fn follow(&mut self, time: f32) -> Result<bool> {
        let Some(rig) = self.rig.clone() else { return Ok(false) };
        self.sample(&rig, time)
    }

    fn sample(&mut self, rig: &RefCell<Rig>, time: f32) -> Result<bool> {
        let mut rig = rig.borrow_mut();
        let sampled = rig.sample(time)?;
        if sampled.revision == self.revision {
            return Ok(false);
        }
        let bone = sampled.world.get(self.bone).context("mesh attachment bone missing")?;
        let world = mat4_mul(bone, &self.local);
        self.revision = sampled.revision;
        if !world.iter().all(|value| value.is_finite()) {
            return Ok(false);
        }
        self.world = world;
        Ok(true)
    }

    fn matrices(&self) -> (Mat4, [f32; NORMAL_MATRIX_FLOATS]) {
        mesh_matrices(self.pose, &self.parents, Some((self.depth, &self.world)))
    }
}

struct Skin {
    rig: Rc<RefCell<Rig>>,
    uniform: String,
    offset: usize,
    bytes: Vec<u8>,
    revision: u64,
}

impl Skin {
    fn new(rig: Rc<RefCell<Rig>>, pass: &paper_scene::effects::EffectPass) -> Result<Self> {
        let bones = rig.borrow().bones;
        let uniform = paper_scene::effects::material::bone_uniform(pass, bones)
            .map_err(anyhow::Error::msg)?;
        Ok(Self {
            rig,
            uniform: uniform.name.clone(),
            offset: uniform.offset,
            bytes: Vec::new(),
            revision: 0,
        })
    }
}

pub(super) type CompositeTargets =
    HashMap<(String, CompositeBuffer), (raw::ImageView, raw::Sampler, raw::Extent2D)>;

struct CompositeInput {
    slot: usize,
    key: (String, CompositeBuffer),
    bound: Option<raw::ImageView>,
}

pub(super) struct MeshActor {
    pipeline: vk::EffectPipeline,
    shadow_program: Option<vk::EffectPipeline>,
    vertices: vk::DynBuffer,
    indices: vk::DynBuffer,
    index_type: raw::IndexType,
    pub texture: usize,
    pub id: String,
    transform: MeshTransform,
    meta: PassMeta,
    values: BTreeMap<String, Vec<f32>>,
    size: (u32, u32),
    count: u32,
    stride: u32,
    pub cast_shadow: bool,
    pub perspective: bool,
    pub visible: bool,
    alpha_sampler: Option<usize>,
    shadow_alpha: Option<vk::ShadowAlpha>,
    uv_offset: u32,
    shadow_slot: Option<usize>,
    cookie_sampler: Option<usize>,
    reflection_index: Option<usize>,
    mirrored: Vec<u8>,
    clock: Option<FrameClock>,
    skin: Option<Skin>,
    mount: Option<Mount>,
    composites: Vec<CompositeInput>,
}

impl MeshActor {
    pub fn animated(&self) -> bool {
        self.skin.is_some()
            || self.clock.is_some()
            || self.mount.as_ref().is_some_and(|mount| mount.rig.is_some())
    }

    pub fn animate(
        &mut self,
        renderer: &vk::Renderer,
        texture: &mut vk::SceneTexture,
        time: f32,
    ) -> Result<()> {
        self.animate_skin(renderer, time)?;
        let Some(mount) = &mut self.mount else { return Ok(()) };
        if !mount.follow(time)? {
            return Ok(());
        }
        (self.transform.model, self.transform.normal) = mount.matrices();
        self.upload(renderer, texture)
    }

    fn animate_skin(&mut self, renderer: &vk::Renderer, time: f32) -> Result<()> {
        let Some(skin) = &mut self.skin else { return Ok(()) };
        let mut rig = skin.rig.borrow_mut();
        rig.sampler.set_owner(self.transform.model);
        let sampled = rig.sample(time)?;
        if sampled.revision == skin.revision {
            return Ok(());
        }
        let values = self.values.get_mut(&skin.uniform).context("mesh bone uniform missing")?;
        let count = size_of_val(sampled.matrices);
        let bytes = skin
            .bytes
            .get_mut(skin.offset..skin.offset + count)
            .context("mesh bone uniform span")?;
        ensure!(values.len() * size_of::<f32>() == count, "mesh bone uniform count");
        for ((matrix, values), bytes) in sampled
            .matrices
            .iter()
            .zip(values.chunks_exact_mut(size_of::<Mat4>() / size_of::<f32>()))
            .zip(bytes.chunks_exact_mut(size_of::<Mat4>()))
        {
            values.copy_from_slice(matrix);
            for (value, word) in matrix.iter().zip(bytes.chunks_exact_mut(size_of::<f32>())) {
                word.copy_from_slice(&value.to_le_bytes());
            }
        }
        renderer.write_effect_inputs(&self.pipeline, &[], &skin.bytes);
        if let Some(program) = &self.shadow_program {
            renderer.write_effect_inputs(program, &[], &skin.bytes);
        }
        skin.revision = sampled.revision;
        Ok(())
    }

    #[cfg(test)]
    pub fn shadow_hlsl(&self) -> bool {
        self.pipeline.hlsl && self.shadow_program.as_ref().is_none_or(|program| program.hlsl)
    }

    pub fn shadow_draw(&self, projection: &Mat4, tile: raw::Rect2D) -> vk::ShadowDraw {
        vk::ShadowDraw {
            program: self
                .shadow_program
                .as_ref()
                .map(|program| (program.pipeline, program.layout, program.set)),
            geometry: vk::MeshGeometry {
                vertices: self.vertices.buffer,
                indices: self.indices.buffer,
                index_type: self.index_type,
                count: self.count,
            },
            stride: self.stride,
            alpha: self.shadow_alpha,
            projection: mat4_mul(projection, &self.transform.model),
            tile,
            revision: self.skin.as_ref().map_or(0, |skin| skin.revision),
        }
    }

    pub fn bind_shadow(
        &self,
        renderer: &vk::Renderer,
        sample: vk::ShadowSample,
        reflection: Option<&Reflection>,
    ) {
        let sample = self.shadow_slot.map_or(sample.comparison(), |slot| {
            renderer.effect_depth_sample(&self.pipeline, slot, sample)
        });
        self.bind_input(renderer, self.shadow_slot, sample, reflection);
    }

    pub fn bind_cookie(
        &self,
        renderer: &vk::Renderer,
        sample: (raw::ImageView, raw::Sampler),
        reflection: Option<&Reflection>,
    ) {
        self.bind_input(renderer, self.cookie_sampler, sample, reflection);
    }

    fn bind_input(
        &self,
        renderer: &vk::Renderer,
        slot: Option<usize>,
        sample: (raw::ImageView, raw::Sampler),
        reflection: Option<&Reflection>,
    ) {
        let Some(slot) = slot else { return };

        renderer.write_effect_input(&self.pipeline, slot, sample);
        if let Some(program) = &self.shadow_program {
            renderer.write_effect_input(program, slot, sample);
        }
        if let Some(twin) = reflection
            .and_then(|reflection| self.reflection_index.and_then(|key| reflection.twins.get(&key)))
        {
            let pipeline = &twin.pipeline;
            renderer.write_effect_input(pipeline, slot, sample);
        }
    }

    pub fn stale_composites(&self, targets: &CompositeTargets) -> bool {
        self.composites.iter().any(|input| {
            targets.get(&input.key).is_some_and(|&(view, _, _)| input.bound != Some(view))
        })
    }

    pub fn bind_composites(
        &mut self,
        renderer: &vk::Renderer,
        targets: &CompositeTargets,
        reflection: Option<&Reflection>,
    ) {
        for index in 0..self.composites.len() {
            let input = &self.composites[index];
            let Some(&(view, sampler, _)) = targets.get(&input.key) else { continue };
            if input.bound == Some(view) {
                continue;
            }
            self.bind_input(renderer, Some(input.slot), (view, sampler), reflection);
            self.composites[index].bound = Some(view);
        }
    }

    pub fn twin(&self) -> Option<usize> {
        self.reflection_index
    }

    pub fn reflects_composites(&self) -> bool {
        self.reflection_index.is_some() && !self.composites.is_empty()
    }

    pub fn composite_layers(&self) -> impl Iterator<Item = &str> {
        self.composites.iter().map(|input| input.key.0.as_str())
    }

    pub fn update_lighting(
        &mut self,
        renderer: &vk::Renderer,
        values: &BTreeMap<String, Vec<f32>>,
    ) {
        self.lighting(values);
        self.refresh_uniforms(renderer);
    }

    fn refresh_uniforms(&mut self, renderer: &vk::Renderer) {
        let bytes = self.uniform_bytes();
        renderer.write_effect_inputs(&self.pipeline, &[], &bytes);
        if let Some(program) = &self.shadow_program {
            renderer.write_effect_inputs(program, &[], &bytes);
        }
        if let Some(skin) = &mut self.skin {
            skin.bytes = bytes;
        }
    }

    pub fn set_visible(&mut self, visible: bool, quads: &mut [vk::SceneQuad]) {
        if let Some(quad) = quads.iter_mut().find(|quad| quad.texture == self.texture) {
            quad.tint[ALPHA_COMPONENT] = f32::from(visible);
        }
        self.visible = visible;
    }

    pub fn lighting(&mut self, values: &BTreeMap<String, Vec<f32>>) {
        self.values.extend(values.clone());
    }

    pub fn pose(
        &mut self,
        pose: paper_scene::model::MeshPose,
        parents: &[paper_scene::model::MeshPose],
    ) {
        let (model, normal) = match &mut self.mount {
            Some(mount) => {
                mount.pose = pose;
                mount.parents.clear();
                mount.parents.extend_from_slice(parents);
                mount.matrices()
            }
            None => mesh_matrices(pose, parents, None),
        };
        self.transform.model = model;
        self.transform.normal = normal;
    }

    pub fn camera(
        &mut self,
        renderer: &vk::Renderer,
        texture: &mut vk::SceneTexture,
        view: &View,
    ) -> Result<()> {
        self.transform.view_projection = view.projection;
        self.transform.eye = view.eye;
        self.transform.basis = view.basis;
        self.upload(renderer, texture)
    }

    pub fn tick(&mut self, renderer: &vk::Renderer, clock: FrameClock) {
        if self.clock.is_none() {
            return;
        }
        self.clock = Some(clock);
        self.refresh_uniforms(renderer);
    }

    fn uniform_bytes(&self) -> Vec<u8> {
        self.meta.uniform_bytes_with(
            self.clock.unwrap_or_default(),
            None,
            self.size.0,
            self.size.1,
            self.size,
            &[],
            &self.values,
            false,
        )
    }

    fn geometry(&self) -> vk::MeshGeometry {
        vk::MeshGeometry {
            vertices: self.vertices.buffer,
            indices: self.indices.buffer,
            index_type: self.index_type,
            count: self.count,
        }
    }

    pub fn mirror(
        &mut self,
        renderer: &vk::Renderer,
        textures: &mut [vk::SceneTexture],
        reflection: &mut Reflection,
    ) -> Result<bool> {
        let Some(twin) = self.reflection_index.and_then(|key| reflection.twins.get_mut(&key))
        else {
            return Ok(false);
        };
        let quad = &mut twin.quad;
        let alpha = f32::from(self.visible);
        let shown = quad.tint[ALPHA_COMPONENT] != alpha;
        quad.tint[ALPHA_COMPONENT] = alpha;
        if !self.visible {
            return Ok(shown);
        }
        let mirrored = self.transform.reflected();
        mirrored.write_uniforms(&mut self.values);
        let bytes = self.uniform_bytes();
        self.transform.write_uniforms(&mut self.values);
        if self.mirrored == bytes && !shown {
            return Ok(false);
        }
        let pipeline = &twin.pipeline;
        renderer.write_effect_inputs(pipeline, &[], &bytes);
        self.mirrored = bytes;
        let projection = mesh_projection(&self.meta, mirrored)
            .map(|(offset, matrix)| vk::EffectProjection::fixed(matrix, offset));
        textures[quad.texture].effect =
            Some(vk::SceneEffect::mesh(pipeline, self.geometry(), projection)?);
        Ok(true)
    }

    fn upload(&mut self, renderer: &vk::Renderer, texture: &mut vk::SceneTexture) -> Result<()> {
        self.transform.projection =
            mat4_mul(&self.transform.view_projection, &self.transform.model);
        self.transform.write_uniforms(&mut self.values);
        let uniforms = self.uniform_bytes();
        renderer.write_effect_inputs(&self.pipeline, &[], &uniforms);
        if let Some(program) = &self.shadow_program {
            renderer.write_effect_inputs(program, &[], &uniforms);
        }
        if let Some(skin) = &mut self.skin {
            skin.bytes = uniforms;
        }
        let projection = mesh_projection(&self.meta, self.transform)
            .map(|(offset, matrix)| vk::EffectProjection::fixed(matrix, offset));
        texture.effect = Some(vk::SceneEffect::mesh(&self.pipeline, self.geometry(), projection)?);
        Ok(())
    }
}

pub(super) struct Twin {
    pipeline: vk::EffectPipeline,
    pub quad: vk::SceneQuad,
    pub order: usize,
}

pub(super) struct Reflection {
    device: ash::Device,
    target: Option<vk::SceneTarget>,
    unread: Option<vk::SceneTarget>,
    pub twins: BTreeMap<usize, Twin>,
    next_twin: usize,
    pub ready: bool,
    pub sampled: bool,
    pub cleared: bool,
    pub staging: super::reflection::Staging,
    pub snapshot: Option<vk::SceneTarget>,
}

fn reflection_target(
    renderer: &mut vk::Renderer,
    output: &vk::SceneTarget,
    sampled: bool,
) -> Result<vk::SceneTarget> {
    let mut target = renderer.create_scene_target_opts(vk::SceneTargetSpec {
        depth_attachment: true,
        extent: if sampled { output.extent } else { UNREAD_REFLECTION_EXTENT },
        format: output.format,
        repeat: false,
        samples: output.samples,
    })?;
    if sampled {
        target.viewport = output.viewport;
    }
    Ok(target)
}

impl Reflection {
    pub fn create(
        renderer: &mut vk::Renderer,
        model: &SceneModel,
        output: &vk::SceneTarget,
    ) -> Result<Option<Self>> {
        if !model.reflection.bound {
            return Ok(None);
        }
        Self::new(renderer, output, model.reflection.sampled).map(Some)
    }

    pub fn new(
        renderer: &mut vk::Renderer,
        output: &vk::SceneTarget,
        sampled: bool,
    ) -> Result<Self> {
        Ok(Self {
            device: renderer.device.clone(),
            target: Some(reflection_target(renderer, output, sampled)?),
            unread: None,
            twins: BTreeMap::new(),
            next_twin: 0,
            ready: false,
            sampled,
            cleared: false,
            staging: super::reflection::Staging::default(),
            snapshot: None,
        })
    }

    fn adopt(&mut self, pipeline: vk::EffectPipeline, quad: vk::SceneQuad) -> usize {
        let key = self.next_twin;
        self.next_twin += 1;
        self.twins.insert(key, Twin { pipeline, quad, order: 0 });
        key
    }

    pub fn retire(&mut self, actors: &[MeshActor]) {
        for key in actors.iter().filter_map(|actor| actor.reflection_index) {
            self.twins.remove(&key);
        }
        self.ready = false;
    }

    pub fn prepare_snapshot(
        &mut self,
        renderer: &mut vk::Renderer,
        output: &vk::SceneTarget,
    ) -> Result<raw::ImageView> {
        if self.snapshot.is_none() {
            self.snapshot = Some(reflection_target(renderer, output, true)?);
        }
        Ok(self.snapshot.as_ref().expect("reflection snapshot").view)
    }

    pub fn activate(
        &mut self,
        renderer: &mut vk::Renderer,
        output: &vk::SceneTarget,
    ) -> Result<()> {
        if !self.sampled {
            let target = reflection_target(renderer, output, true)?;
            self.unread = self.target.replace(target);
            self.sampled = true;
            self.ready = false;
            self.cleared = false;
        }
        Ok(())
    }

    pub fn target(&self) -> &vk::SceneTarget {
        self.target.as_ref().expect("reflection target")
    }
}

impl Drop for Reflection {
    fn drop(&mut self) {
        let _ = crate::shared::wait_idle(&self.device);
        for (_, twin) in std::mem::take(&mut self.twins) {
            twin.pipeline.destroy(&self.device);
        }
        for target in
            [self.target.take(), self.unread.take(), self.snapshot.take()].into_iter().flatten()
        {
            target.destroy(&self.device);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Settings {
    pub projection: ProjectionMode,
    pub depth: bool,
    pub camera: Option<paper_scene::model::Camera>,
    pub orientation: Option<Mat4>,
    pub perspective_fov: f32,
    pub ambient: [f32; 3],
    pub skylight: [f32; 3],
}

pub(super) struct View {
    pub projection: Mat4,
    eye: [f32; NORMAL_COMPONENTS],
    basis: Option<CameraBasis>,
}

impl Settings {
    pub fn view(
        self,
        canvas: (f32, f32),
        viewport: Option<vk::SceneViewport>,
        clip: vk::ClipView,
        perspective: bool,
    ) -> Result<View> {
        let mut view = match self.projection {
            ProjectionMode::Orthographic | ProjectionMode::Automatic => {
                let viewport =
                    viewport.unwrap_or_else(|| vk::SceneViewport::whole([canvas.0, canvas.1]));
                let (projection, eye) = if perspective {
                    (
                        scene_perspective(viewport.source, self.perspective_fov, true),
                        perspective_eye(viewport.source, self.perspective_fov),
                    )
                } else {
                    (orthographic_view(viewport), orthographic_eye(canvas))
                };
                View {
                    projection: self
                        .orientation
                        .map_or(projection, |view| mat4_mul(&projection, &view)),
                    eye,
                    basis: None,
                }
            }
            ProjectionMode::Other => {
                let camera = self.camera.context("mesh camera missing")?;
                View {
                    projection: authored_perspective(camera, canvas, true).0,
                    eye: camera.eye,
                    basis: camera.basis(),
                }
            }
        };
        clip.apply(&mut view.projection, false);
        Ok(view)
    }

    pub fn of(model: &SceneModel) -> Self {
        Self {
            projection: model.projection,
            depth: !model.meshes.is_empty(),
            camera: model.camera,
            orientation: None,
            perspective_fov: model.perspective_fov,
            ambient: model.ambient,
            skylight: model.skylight,
        }
    }
}

pub(super) struct Build<'a, 'device> {
    pub renderer: &'a mut vk::Renderer,
    pub interner: &'a mut TextureInterner<'device>,
    pub scene: Settings,
    pub textures: &'a mut Vec<vk::SceneTexture>,
    pub quads: &'a mut Vec<vk::SceneQuad>,
    pub order: &'a mut Vec<usize>,
    pub canvas: (f32, f32),
    pub viewport: Option<vk::SceneViewport>,
    pub reflection: Option<&'a mut Reflection>,
    pub cookie: Option<(raw::ImageView, raw::Sampler, raw::Extent2D)>,
    pub shadows: Option<&'a super::shadow::Shadows>,
    pub history: Option<&'a vk::SceneHistory>,
    pub clip: vk::ClipView,
}

const MIRROR_AXIS: usize = 1;
const UNREAD_REFLECTION_EXTENT: raw::Extent2D = raw::Extent2D { width: 1, height: 1 };

pub(super) fn mirror_world(matrix: &mut Mat4) {
    let axis = MATRIX_COLUMN_COMPONENTS * MIRROR_AXIS;
    for value in &mut matrix[axis..axis + MATRIX_COLUMN_COMPONENTS] {
        *value = -*value;
    }
}

pub(super) fn mirror_view(matrix: &mut Mat4) {
    for column in matrix.chunks_exact_mut(MATRIX_COLUMN_COMPONENTS) {
        column[MIRROR_AXIS] = -column[MIRROR_AXIS];
    }
    mirror_world(matrix);
}

pub(super) fn mirror_vector(mut vector: [f32; NORMAL_COMPONENTS]) -> [f32; NORMAL_COMPONENTS] {
    vector[MIRROR_AXIS] = -vector[MIRROR_AXIS];
    vector
}

#[derive(Clone, Copy)]
struct MeshTransform {
    model: Mat4,
    view_projection: Mat4,
    projection: Mat4,
    normal: [f32; NORMAL_MATRIX_FLOATS],
    eye: [f32; NORMAL_COMPONENTS],
    basis: Option<CameraBasis>,
}

impl MeshTransform {
    fn reflected(mut self) -> Self {
        mirror_view(&mut self.view_projection);
        self.projection = mat4_mul(&self.view_projection, &self.model);
        self.eye = mirror_vector(self.eye);
        if let Some(basis) = &mut self.basis {
            basis.right = mirror_vector(basis.right);
            basis.up = mirror_vector(basis.up);
            basis.forward = mirror_vector(basis.forward);
        }
        self
    }

    fn write_uniforms(self, values: &mut BTreeMap<String, Vec<f32>>) {
        values.insert(FX_MODEL_MATRIX.into(), self.model.to_vec());
        values.insert(FX_MODEL_PROJECTION.into(), self.projection.to_vec());
        values.insert(FX_VIEW_PROJECTION.into(), self.view_projection.to_vec());
        values.insert(FX_NORMAL_MATRIX.into(), self.normal.to_vec());
        values.insert(EYE_POSITION.into(), self.eye.to_vec());
        if let Some(basis) = self.basis {
            values.insert(FX_VIEW_RIGHT.into(), basis.right.to_vec());
            values.insert(FX_VIEW_UP.into(), basis.up.to_vec());
        }
    }
}

fn mesh_matrices(
    pose: paper_scene::model::MeshPose,
    parents: &[paper_scene::model::MeshPose],
    mount: Option<(usize, &Mat4)>,
) -> (Mat4, [f32; NORMAL_MATRIX_FLOATS]) {
    let mut model = mesh_model_matrix(pose.origin, pose.scale, pose.angles);
    let mut normal = mesh_normal_model(pose.scale, pose.angles);
    for (depth, parent) in parents.iter().enumerate() {
        if let Some((mounted, world)) = mount
            && mounted == depth
        {
            model = mat4_mul(world, &model);
            normal = mat4_mul(&mount_normal(world), &normal);
        }
        model = mat4_mul(&mesh_model_matrix(parent.origin, parent.scale, parent.angles), &model);
        normal = mat4_mul(&mesh_normal_model(parent.scale, parent.angles), &normal);
    }
    (model, mesh_normal_matrix(&normal))
}

fn mesh_normal_model(scale: [f32; NORMAL_COMPONENTS], angles: [f32; NORMAL_COMPONENTS]) -> Mat4 {
    mesh_model_matrix([0.0; NORMAL_COMPONENTS], scale.map(f32::signum), angles)
}

fn mount_normal(world: &Mat4) -> Mat4 {
    let mut basis = paper_scene::matrix::IDENTITY;
    for (target, source) in basis
        .chunks_exact_mut(MATRIX_COLUMN_COMPONENTS)
        .zip(world.chunks_exact(MATRIX_COLUMN_COMPONENTS))
        .take(NORMAL_COMPONENTS)
    {
        let length =
            source[..NORMAL_COMPONENTS].iter().map(|value| value * value).sum::<f32>().sqrt();
        if length <= f32::EPSILON {
            continue;
        }
        for (target, source) in target.iter_mut().zip(source).take(NORMAL_COMPONENTS) {
            *target = source / length;
        }
    }
    basis
}

fn mesh_normal_matrix(rotation: &Mat4) -> [f32; NORMAL_MATRIX_FLOATS] {
    let mut normal = [0.0; NORMAL_MATRIX_FLOATS];
    for (target, source) in normal
        .chunks_exact_mut(MATRIX_COLUMN_COMPONENTS)
        .zip(rotation.chunks_exact(MATRIX_COLUMN_COMPONENTS))
    {
        target[..NORMAL_COMPONENTS].copy_from_slice(&source[..NORMAL_COMPONENTS]);
    }
    normal
}

fn mesh_projection(meta: &PassMeta, transform: MeshTransform) -> Option<(usize, Mat4)> {
    meta.matrix_offset(FX_MODEL_PROJECTION)
        .map(|offset| (offset, transform.projection))
        .or_else(|| meta.matrix_offset(FX_MODEL_MATRIX).map(|offset| (offset, transform.model)))
}

impl Build<'_, '_> {
    pub fn load(&mut self, layers: Vec<paper_scene::model::MeshLayer>) -> Result<Vec<MeshActor>> {
        let mut actors: Vec<MeshActor> = Vec::new();
        let texture_start = self.textures.len();
        let quad_start = self.quads.len();
        let owners: Vec<&str> = layers
            .iter()
            .filter_map(|layer| layer.mount.as_ref())
            .map(|mount| mount.owner.as_str())
            .collect();
        let rigs = layers
            .iter()
            .filter(|layer| layer.animated() || owners.contains(&layer.id.as_str()))
            .map(|layer| Ok((layer.id.clone(), Rc::new(RefCell::new(Rig::new(layer)?)))))
            .collect::<Result<HashMap<_, _>>>()?;
        let mounts = layers
            .iter()
            .map(|layer| Mount::new(layer, &layers, &rigs))
            .collect::<Result<Vec<_>>>()?;
        for (layer, mount) in layers.into_iter().zip(mounts) {
            let rig = if layer.animated() { rigs.get(&layer.id).cloned() } else { None };
            let (model, normal) = mount
                .as_ref()
                .map_or_else(|| mesh_matrices(layer.pose, &layer.parents, None), Mount::matrices);
            let View { projection: view_projection, eye, basis } =
                self.scene.view(self.canvas, self.viewport, self.clip, layer.perspective)?;
            let transform = MeshTransform {
                model,
                view_projection,
                projection: mat4_mul(&view_projection, &model),
                normal,
                eye,
                basis,
            };
            for (part, material) in layer.mesh.parts.into_iter().zip(layer.materials) {
                let mut actor = match self.create(
                    &part,
                    material,
                    transform,
                    layer.reflected,
                    &layer.id,
                    rig.clone(),
                ) {
                    Ok(actor) => actor,
                    Err(error) => {
                        if let Some(reflection) = self.reflection.as_deref_mut() {
                            reflection.retire(&actors);
                        }
                        for actor in actors {
                            drop(actor);
                        }
                        for texture in self.textures.drain(texture_start..) {
                            self.renderer.destroy_scene_texture(texture);
                        }
                        self.quads.truncate(quad_start);
                        self.order.truncate(quad_start);
                        return Err(error);
                    }
                };
                actor.0.cast_shadow = layer.cast_shadow;
                actor.0.mount.clone_from(&mount);
                actor.0.perspective = layer.perspective;
                self.order.push(layer.scene_order);
                if let Some(twin) = actor.0.reflection_index.and_then(|key| {
                    self.reflection
                        .as_deref_mut()
                        .and_then(|reflection| reflection.twins.get_mut(&key))
                }) {
                    twin.order = layer.scene_order;
                }
                self.quads.push(actor.2);
                actor.0.set_visible(layer.visible, &mut self.quads[quad_start..]);
                self.textures.push(actor.1);
                actors.push(actor.0);
            }
        }
        Ok(actors)
    }

    fn create(
        &mut self,
        part: &paper_scene::model::mesh::Part,
        material: paper_scene::effects::material::MeshMaterial,
        transform: MeshTransform,
        reflected: bool,
        id: &str,
        rig: Option<Rc<RefCell<Rig>>>,
    ) -> Result<(MeshActor, vk::SceneTexture, vk::SceneQuad)> {
        use paper_scene::effects::material::{CullMode, DepthMode};

        let mut pass = material.pass;
        let meta = PassMeta::of(&pass);
        let format = self.renderer.scene_format();
        let samples = self.renderer.scene_sample_count();
        let spec = || vk::EffectPipelineSpec {
            depth_attachment: true,
            format,
            samples,
            blend: pass_blend(material.blend),
            scene: None,
            geometry: if part.skin.is_some() {
                vk::EffectGeometry::Puppet
            } else {
                vk::EffectGeometry::Mesh(part.format)
            },
            coordinates: vk::EffectCoordinates::Viewport,
            depth: vk::EffectDepth {
                test: material.test == DepthMode::Enabled,
                write: material.write == DepthMode::Enabled,
                cull: material.cull == CullMode::Normal,
                order: vk::DepthOrder::Greater,
            },
        };
        let shadow_program = self
            .shadows
            .map(|shadows| {
                let sources = paper_scene::effects::material::shadow::sources(&pass)?;
                self.renderer.create_shadow_pipeline(
                    &sources,
                    &pass.name,
                    spec(),
                    shadows.render_pass(),
                )
            })
            .transpose()?;
        let pipeline = self.pipeline(&pass, spec())?;
        let vertices = self.vertices(part)?;
        let indices = self.indices(part)?;
        let mut values = pass.values.clone();
        values.insert(AMBIENT_COLOR.into(), self.scene.ambient.to_vec());
        values.insert(SKYLIGHT_COLOR.into(), self.scene.skylight.to_vec());
        let size = self.raster_size();
        let skin = rig.map(|rig| Skin::new(rig, &pass)).transpose()?;
        let mut actor = MeshActor {
            pipeline,
            shadow_program,
            vertices,
            indices,
            index_type: vk::SceneIndices::from(&part.indices).index_type(),
            texture: self.textures.len(),
            id: id.into(),
            transform,
            meta: PassMeta::of(&pass),
            values,
            size,
            count: part.indices.len() as u32,
            stride: part
                .skin
                .as_ref()
                .map_or((part.format.floats() * size_of::<f32>()) as u32, |bytes| {
                    (bytes.len() / part.vertices.len()) as u32
                }),
            cast_shadow: true,
            perspective: false,
            visible: true,
            alpha_sampler: material.alpha_sampler,
            shadow_alpha: None,
            shadow_slot: light_slot(&pass, &paper_scene::effects::EffectBind::ShadowAtlas),
            cookie_sampler: light_slot(&pass, &paper_scene::effects::EffectBind::LightCookie),
            reflection_index: None,
            mirrored: Vec::new(),
            clock: pass.clocked().then(FrameClock::default),
            skin,
            mount: None,
            composites: Vec::new(),
            uv_offset: ((part.skin.as_ref().map_or(part.format.floats(), |bytes| {
                bytes.len() / part.vertices.len() / size_of::<f32>()
            }) - UV_COMPONENTS
                - if part.format == VertexFormat::TangentLightmap { UV_COMPONENTS } else { 0 })
                * size_of::<f32>()) as u32,
        };
        let reflected_pipe =
            if reflected && self.reflection.as_ref().is_some_and(|reflection| reflection.sampled) {
                Some(self.pipeline(&pass, spec())?)
            } else {
                None
            };
        let (slot, quad, twin) = self.slot(
            &mut actor,
            &mut pass,
            &meta,
            &transform,
            part.indices.len() as u32,
            reflected_pipe.as_ref(),
        )?;
        actor.texture = quad.texture;
        if let (Some(pipeline), Some(twin)) = (reflected_pipe, twin) {
            let reflection = self.reflection.as_deref_mut().expect("reflection producer");
            actor.reflection_index = Some(reflection.adopt(pipeline, twin));
        }
        Ok((actor, slot, quad))
    }

    fn pipeline(
        &mut self,
        pass: &paper_scene::effects::EffectPass,
        spec: vk::EffectPipelineSpec,
    ) -> Result<vk::EffectPipeline> {
        self.renderer.create_effect_pipeline_for(
            &pass.vertex,
            &pass.fragment,
            pass.hlsl.as_ref(),
            &pass.name,
            spec,
        )
    }

    fn raster_size(&self) -> (u32, u32) {
        self.viewport.map_or((self.canvas.0 as u32, self.canvas.1 as u32), |view| {
            (view.raster[0], view.raster[1])
        })
    }

    fn vertices(&self, part: &paper_scene::model::mesh::Part) -> Result<vk::DynBuffer> {
        if let Some(bytes) = &part.skin {
            let buffer = self
                .renderer
                .create_dyn_buffer(bytes.len(), raw::BufferUsageFlags::VERTEX_BUFFER)?;
            self.renderer.write_dyn_buffer(&buffer, bytes);
            return Ok(buffer);
        }
        let components = part.format.floats();
        let mut bytes = Vec::with_capacity(part.vertices.len() * components * size_of::<f32>());
        for vertex in &part.vertices {
            for value in
                vertex.position.into_iter().chain(vertex.normal.unwrap_or([0.0; NORMAL_COMPONENTS]))
            {
                bytes.extend_from_slice(&value.to_ne_bytes());
            }
            if part.format != VertexFormat::Standard {
                for value in vertex.tangent.expect("tangent mesh vertex") {
                    bytes.extend_from_slice(&value.to_ne_bytes());
                }
            }
            for value in vertex.uv {
                bytes.extend_from_slice(&value.to_ne_bytes());
            }
            if part.format == VertexFormat::TangentLightmap {
                for value in vertex.secondary_uv.expect("secondary UV mesh vertex") {
                    bytes.extend_from_slice(&value.to_ne_bytes());
                }
            }
        }
        let buffer =
            self.renderer.create_dyn_buffer(bytes.len(), raw::BufferUsageFlags::VERTEX_BUFFER)?;
        self.renderer.write_dyn_buffer(&buffer, &bytes);
        Ok(buffer)
    }

    fn indices(&self, part: &paper_scene::model::mesh::Part) -> Result<vk::DynBuffer> {
        let bytes = part.indices.bytes();
        let buffer =
            self.renderer.create_dyn_buffer(bytes.len(), raw::BufferUsageFlags::INDEX_BUFFER)?;
        self.renderer.write_dyn_buffer(&buffer, &bytes);
        Ok(buffer)
    }

    fn inputs(
        &mut self,
        pass: &mut paper_scene::effects::EffectPass,
        sampler_count: u32,
        alpha_sampler: Option<usize>,
    ) -> Result<MeshInputs> {
        let mut alpha_set = None;
        let mut inputs = Vec::with_capacity(sampler_count as usize);
        let mut sizes = Vec::with_capacity(inputs.capacity());
        let mut composites = Vec::new();
        for index in 0..sampler_count as usize {
            let shadow = pass.binds.iter().any(|(slot, bind)| {
                *slot == index && *bind == paper_scene::effects::EffectBind::ShadowAtlas
            });
            let unused = !pass
                .vertex
                .samplers
                .iter()
                .chain(&pass.fragment.samplers)
                .any(|sampler| sampler.index as usize == index);
            let empty = pass.binds.iter().any(|(slot, bind)| {
                *slot == index && *bind == paper_scene::effects::EffectBind::SceneSoFar
            });
            if unused || empty {
                let slot = self.interner.intern_rgba(
                    self.renderer,
                    self.textures,
                    super::TextureKey::clear(),
                )?;
                let texture = &self.textures[slot];
                inputs.push((texture.view, texture.sampler));
                sizes.push(Some([1.0; super::RGBA_CHANNELS]));
                continue;
            }
            if pass.binds.iter().any(|(slot, bind)| {
                *slot == index && *bind == paper_scene::effects::EffectBind::LightCookie
            }) {
                let (view, sampler, extent) = self.cookie.context("mesh light cookie missing")?;
                inputs.push((view, sampler));
                sizes.push(Some(super::size4(extent)));
                continue;
            }
            if shadow {
                let atlas = self.shadows.context("mesh shadow atlas missing")?;
                let sample = atlas.sample();
                let comparison = pass
                    .vertex
                    .samplers
                    .iter()
                    .chain(&pass.fragment.samplers)
                    .any(|sampler| sampler.index as usize == index && sampler.comparison);
                inputs.push(if comparison {
                    sample.comparison()
                } else {
                    (sample.view, sample.ordinary)
                });
                sizes.push(Some(super::size4(atlas.extent())));
                continue;
            }
            if pass.binds.iter().any(|(slot, bind)| {
                *slot == index && *bind == paper_scene::effects::EffectBind::SceneHistory
            }) {
                let (view, sampler, extent) =
                    self.history.context("mesh scene history missing")?.sample();
                inputs.push((view, sampler));
                sizes.push(Some(super::size4(extent)));
                super::material::history_mips(&mut pass.values, index);
                continue;
            }
            if let Some((layer, buffer)) = pass.binds.iter().find_map(|(slot, bind)| match bind {
                EffectBind::LayerComposite { layer, buffer } if *slot == index => {
                    Some((layer.clone(), *buffer))
                }
                _ => None,
            }) {
                ensure!(Some(index) != alpha_sampler, COMPOSITE_ALPHA_GAP);
                let slot = self.interner.intern_rgba(
                    self.renderer,
                    self.textures,
                    super::TextureKey::clear(),
                )?;
                let texture = &self.textures[slot];
                inputs.push((texture.view, texture.sampler));
                sizes.push(None);
                composites.push(CompositeInput { slot: index, key: (layer, buffer), bound: None });
                continue;
            }
            if pass.binds.iter().any(|(slot, bind)| {
                *slot == index && *bind == paper_scene::effects::EffectBind::Reflection
            }) {
                let target = self.reflection.as_ref().context("mesh reflection missing")?.target();
                inputs.push((target.view, target.sampler));
                sizes.push(Some(super::size4(target.extent)));
                continue;
            }
            let texture = pass
                .textures
                .get_mut(index)
                .and_then(Option::as_mut)
                .with_context(|| format!("mesh sampler {index} missing"))?;
            sizes.push(Some([
                texture.width as f32,
                texture.height as f32,
                texture.img_width as f32,
                texture.img_height as f32,
            ]));
            let slot = self.interner.intern_texture(
                self.renderer,
                self.textures,
                texture,
                vk::MipSampling::Anisotropic,
            )?;
            let texture = &self.textures[slot];
            if Some(index) == alpha_sampler {
                alpha_set = Some(texture.sample_set());
            }
            inputs.push((texture.view, texture.sampler));
        }
        Ok(MeshInputs { alpha_set, inputs, sizes, composites })
    }

    fn slot(
        &mut self,
        actor: &mut MeshActor,
        pass: &mut paper_scene::effects::EffectPass,
        meta: &PassMeta,
        transform: &MeshTransform,
        count: u32,
        reflected_pipe: Option<&vk::EffectPipeline>,
    ) -> Result<(vk::SceneTexture, vk::SceneQuad, Option<vk::SceneQuad>)> {
        let mut values = pass.values.clone();
        transform.write_uniforms(&mut values);
        values.insert(AMBIENT_COLOR.into(), self.scene.ambient.to_vec());
        values.insert(SKYLIGHT_COLOR.into(), self.scene.skylight.to_vec());
        let MeshInputs { alpha_set, inputs, sizes, composites } =
            self.inputs(pass, actor.pipeline.sampler_count, actor.alpha_sampler)?;
        actor.composites = composites;
        if actor.alpha_sampler.is_some() {
            actor.shadow_alpha = Some(vk::ShadowAlpha {
                offset: actor.uv_offset,
                set: alpha_set.context("shadow albedo missing")?,
            });
        }
        let size = self.raster_size();
        let uniforms = |values: &BTreeMap<String, Vec<f32>>| {
            meta.uniform_bytes_with(
                FrameClock::default(),
                None,
                size.0,
                size.1,
                size,
                &sizes,
                values,
                false,
            )
        };
        let bytes = uniforms(&values);
        self.renderer.write_effect_inputs(&actor.pipeline, &inputs, &bytes);
        if let Some(program) = &actor.shadow_program {
            self.renderer.write_effect_inputs(program, &inputs, &bytes);
        }
        if let Some(skin) = &mut actor.skin {
            skin.bytes = bytes;
        }
        let geometry = vk::MeshGeometry {
            vertices: actor.vertices.buffer,
            indices: actor.indices.buffer,
            index_type: actor.index_type,
            count,
        };
        let mut quad = vk::SceneQuad {
            rect: [self.canvas.0 * 0.5, self.canvas.1 * 0.5, self.canvas.0, self.canvas.1],
            uv: [0.0, 0.0, 1.0, 1.0],
            uv_axes: None,
            uv_padding: false,
            tint: [1.0; RGBA_CHANNELS],
            angle: 0.0,
            projection: None,
            order_bias: 0,
            texture: self.textures.len(),
            blend: pass_blend(pass.blend),
        };
        let mut twin = None;
        if let Some(pipeline) = reflected_pipe {
            let reflected = transform.reflected();
            reflected.write_uniforms(&mut values);
            self.renderer.write_effect_inputs(pipeline, &inputs, &uniforms(&values));
            let slot =
                mesh_slot(self.renderer, pipeline, geometry, mesh_projection(meta, reflected))?;
            twin = Some(quad.clone());
            self.textures.push(slot);
            quad.texture = self.textures.len();
        }
        let slot =
            mesh_slot(self.renderer, &actor.pipeline, geometry, mesh_projection(meta, *transform))?;
        Ok((slot, quad, twin))
    }
}

fn light_slot(
    pass: &paper_scene::effects::EffectPass,
    kind: &paper_scene::effects::EffectBind,
) -> Option<usize> {
    pass.binds.iter().find_map(|(slot, bind)| (bind == kind).then_some(*slot))
}

struct MeshInputs {
    alpha_set: Option<raw::DescriptorSet>,
    inputs: Vec<(raw::ImageView, raw::Sampler)>,
    sizes: Vec<Option<[f32; super::RGBA_CHANNELS]>>,
    composites: Vec<CompositeInput>,
}

fn mesh_slot(
    renderer: &mut vk::Renderer,
    pipeline: &vk::EffectPipeline,
    geometry: vk::MeshGeometry,
    projection: Option<(usize, Mat4)>,
) -> Result<vk::SceneTexture> {
    let mut slot = renderer.create_view_slot()?;
    match vk::SceneEffect::mesh(
        pipeline,
        geometry,
        projection.map(|(offset, matrix)| vk::EffectProjection::fixed(matrix, offset)),
    ) {
        Ok(effect) => {
            slot.effect = Some(effect);
            Ok(slot)
        }
        Err(error) => {
            renderer.destroy_scene_texture(slot);
            Err(error)
        }
    }
}

fn framebuffer_layers(model: &SceneModel) -> bool {
    use paper_scene::effects::EffectBind;

    model.particles.is_empty()
        && model.layers.iter().all(|layer| {
            !layer.copy_background
                && !layer.passthrough
                && layer.puppet.is_none()
                && layer.kind == paper_scene::model::LayerKind::Image
                && layer.material.as_ref().is_none_or(|material| {
                    layer.effects.is_empty() && material.pass.binds.is_empty()
                })
                && (layer.effects.is_empty() || layer.base_scene_input)
                && layer.effects.iter().enumerate().all(|(index, effect)| {
                    if index == 0 {
                        return effect.scene_capture();
                    }
                    effect.fbos.is_empty()
                        && effect.swaps.is_empty()
                        && !effect.passes.is_empty()
                        && effect.passes.iter().all(|pass| {
                            pass.target.is_none()
                                && pass
                                    .binds
                                    .iter()
                                    .all(|(_, bind)| matches!(bind, EffectBind::Previous))
                        })
                })
        })
}

pub(super) fn viewport(
    model: &SceneModel,
    outputs: &[(u32, u32)],
    mode: paper_geom::FillMode,
) -> Result<Option<vk::SceneViewport>> {
    if model.layers.iter().any(|layer| {
        matches!(super::base_scene_blend(layer), Some(vk::SceneBlend::Alpha | vk::SceneBlend::Add))
    }) {
        ensure!(
            model.projection == ProjectionMode::Orthographic,
            "base material blending requires explicit orthographic projection"
        );
        ensure!(framebuffer_layers(model), "framebuffer input mapping unsupported");
    }
    let framebuffer =
        model.meshes.is_empty() && model.layers.iter().any(|layer| layer.base_scene_input);
    let skinned = model
        .layers
        .iter()
        .any(|layer| layer.material.as_ref().is_some_and(|material| material.skin.is_some()));
    if model.meshes.is_empty() && !framebuffer && !skinned {
        return Ok(None);
    }
    if framebuffer {
        ensure!(
            model.projection == ProjectionMode::Orthographic,
            "framebuffer input requires explicit orthographic projection"
        );
        ensure!(framebuffer_layers(model), "framebuffer input mapping unsupported");
    }
    ensure!(
        model.projection != ProjectionMode::Automatic,
        "orthographic mesh automatic projection unsupported"
    );
    if model.projection != ProjectionMode::Orthographic {
        return Ok(None);
    }
    let hosted = model.meshes.is_empty() && !framebuffer;
    if hosted && (mode != paper_geom::FillMode::Fill || !model.skin_viewport) {
        return Ok(None);
    }
    ensure!(mode == paper_geom::FillMode::Fill, "orthographic mesh fill mode unsupported");
    let &(width, height) = outputs.first().context("orthographic mesh output missing")?;
    let view = vk::SceneViewport::cover([model.canvas.0, model.canvas.1], [width, height])
        .context("orthographic mesh output invalid")?;
    let fits = view.same_outputs(outputs)
        && width <= super::scene_limit()
        && height <= super::scene_limit();
    if hosted && !fits {
        return Ok(None);
    }
    ensure!(view.same_outputs(outputs), "orthographic mesh differing output sizes unsupported");
    ensure!(
        width <= super::scene_limit() && height <= super::scene_limit(),
        "orthographic mesh output exceeds scene limit"
    );
    let layers_ready =
        framebuffer || model.layers.iter().all(paper_scene::model::Layer::orthographic_mesh_ready);
    let particles_ready = model.particles.iter().all(|layer| {
        layer.system.grab_slot.is_none()
            && !layer.system.perspective
            && layer.system.renderer == paper_scene::particles::Renderer::Sprite
            && layer.system.children.is_empty()
    });
    if (!layers_ready || !particles_ready) && !framebuffer && (hosted || view.uncropped()) {
        return Ok(None);
    }
    ensure!(layers_ready, "orthographic mesh mixed effects or background mapping unsupported");
    ensure!(
        particles_ready,
        "orthographic mesh particle background, perspective or non-sprite mapping unsupported"
    );
    Ok(Some(view))
}

pub(super) fn orthographic_view(view: vk::SceneViewport) -> Mat4 {
    let [_, _, width, height] = view.source;
    let mut projection = scene_ortho((1.0, 1.0), true);
    projection[MATRIX_SCALE_X] = 2.0 / width;
    projection[MATRIX_SCALE_Y] = 2.0 / height;
    projection[MATRIX_DEPTH_SCALE] = 1.0 / (2.0 * ORTHOGRAPHIC_EYE_Z);
    projection[MATRIX_TRANSLATION_X] = -view.canvas[0] / width;
    projection[MATRIX_TRANSLATION_Y] = -view.canvas[1] / height;
    projection
}

#[cfg(test)]
#[path = "mesh_tests.rs"]
mod tests;
