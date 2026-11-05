use super::pipelines::ShaderModules;
use crate::vk::Renderer;
use anyhow::{Context, Result, anyhow, ensure};
use ash::vk;
use paper_scene::effects::MAX_TEXTURE_SLOTS as MAX_EFFECT_SAMPLERS;
use paper_scene::hlsl::{HlslPair, SAMPLER_BINDING_SHIFT, TEXTURE_BINDING_SHIFT};
use paper_scene::model::mesh::VertexFormat;
use paper_scene::shader::{Stage, Translated};

pub struct EffectDraw<'a> {
    pub target: &'a crate::vk::SceneTarget,
    pub preserve: bool,
    pub final_vertices: bool,
}

pub(super) const MODEL_QUAD_OFFSET: u64 = QUAD_VERTEX_BYTES * 2;
pub(super) const UNPADDED_MODEL_QUAD_OFFSET: u64 = MODEL_QUAD_OFFSET + QUAD_VERTEX_BYTES;
pub(super) const QUAD_VERTEX_BYTES: u64 = std::mem::size_of::<[f32; 20]>() as u64;
const MESH_NORMAL_OFFSET: u32 = std::mem::size_of::<[f32; 3]>() as u32;
const MESH_UV_OFFSET: u32 = std::mem::size_of::<[f32; 6]>() as u32;
const MESH_TANGENT_UV_OFFSET: u32 = std::mem::size_of::<[f32; 10]>() as u32;
const MESH_POSITION: &str = "a_Position";
const MESH_ATTRIBUTES: [(&str, &str, vk::Format, u32); 3] = [
    (MESH_POSITION, "vec3", vk::Format::R32G32B32_SFLOAT, 0),
    ("a_Normal", "vec3", vk::Format::R32G32B32_SFLOAT, MESH_NORMAL_OFFSET),
    ("a_TexCoord", "vec2", vk::Format::R32G32_SFLOAT, MESH_UV_OFFSET),
];
const MESH_TANGENT_UV_ATTRIBUTES: [(&str, &str, vk::Format, u32); 4] = [
    MESH_ATTRIBUTES[0],
    MESH_ATTRIBUTES[1],
    ("a_Tangent4", "vec4", vk::Format::R32G32B32A32_SFLOAT, MESH_UV_OFFSET),
    ("a_TexCoord", "vec2", vk::Format::R32G32_SFLOAT, MESH_TANGENT_UV_OFFSET),
];
const MESH_TANGENT_ATTRIBUTES: [(&str, &str, vk::Format, u32); 5] = [
    MESH_TANGENT_UV_ATTRIBUTES[0],
    MESH_TANGENT_UV_ATTRIBUTES[1],
    MESH_TANGENT_UV_ATTRIBUTES[2],
    ("a_TexCoordVec4", "vec4", vk::Format::R32G32B32A32_SFLOAT, MESH_TANGENT_UV_OFFSET),
    MESH_TANGENT_UV_ATTRIBUTES[3],
];

const PUPPET_VERTEX_BYTES: u32 = 80;
const SCENE_MESH_ATTRIBUTES: [(&str, &str, vk::Format, u32); 3] = [
    (MESH_POSITION, "vec2", vk::Format::R32G32_SFLOAT, 0),
    ("a_TexCoord", "vec2", vk::Format::R32G32_SFLOAT, 8),
    ("a_VertexAlpha", "float", vk::Format::R32_SFLOAT, 16),
];
const PUPPET_ATTRIBUTES: [(&str, &str, vk::Format, u32); 6] = [
    MESH_ATTRIBUTES[0],
    MESH_ATTRIBUTES[1],
    MESH_TANGENT_UV_ATTRIBUTES[2],
    (
        paper_scene::effects::material::PUPPET_INPUTS[3].0,
        paper_scene::effects::material::PUPPET_INPUTS[3].1,
        vk::Format::R32G32B32A32_UINT,
        40,
    ),
    (
        paper_scene::effects::material::PUPPET_INPUTS[4].0,
        paper_scene::effects::material::PUPPET_INPUTS[4].1,
        vk::Format::R32G32B32A32_SFLOAT,
        56,
    ),
    ("a_TexCoord", "vec2", vk::Format::R32G32_SFLOAT, 72),
];

const MORPH_POSITION: &str = paper_scene::puppet::morph::POSITION;
const CHANNEL_ATTRIBUTES: [(&str, &str, vk::Format, u32); 3] = [
    (MESH_POSITION, "vec3", vk::Format::R32G32B32_SFLOAT, 0),
    (
        "a_BlendIndices",
        "uvec4",
        vk::Format::R32G32B32A32_UINT,
        paper_scene::puppet::channels::POSITION_BYTES as u32,
    ),
    (
        "a_TexCoordVec4",
        "vec4",
        vk::Format::R32G32B32A32_SFLOAT,
        (paper_scene::puppet::channels::POSITION_BYTES + paper_scene::puppet::channels::INDEX_BYTES)
            as u32,
    ),
];
const MORPH_ATTRIBUTES: [(&str, &str, vk::Format, u32); 6] = {
    let mut attributes = PUPPET_ATTRIBUTES;
    attributes[0] = (MORPH_POSITION, "vec4", vk::Format::R32G32B32A32_SFLOAT, 0);
    let mut index = 1;
    while index < attributes.len() {
        attributes[index].3 += size_of::<f32>() as u32;
        index += 1;
    }
    attributes
};

const MAX_EFFECT_UBO_BYTES: usize = 16 * 1024;
const SHADER_ENTRY: &std::ffi::CStr = c"main";
const PARTICLE_POSITION_BYTES: u32 = std::mem::size_of::<[f32; 3]>() as u32;
const PARTICLE_VEC4_BYTES: u32 = std::mem::size_of::<[f32; 4]>() as u32;
const ROPE_POINT_ATTRIBUTES: u32 = 6;
const SPRITE_COLOR_LOCATION: u32 = 3;

fn effect_caps(vertex: &Translated, fragment: &Translated) -> Result<(u32, usize)> {
    let max_index =
        fragment.samplers.iter().chain(vertex.samplers.iter()).map(|sampler| sampler.index).max();
    let sampler_count = match max_index {
        None => 0,
        Some(index) if index >= MAX_EFFECT_SAMPLERS => {
            return Err(anyhow!(
                "effect sampler binding {index} exceeds cap {MAX_EFFECT_SAMPLERS}"
            ));
        }
        Some(index) => index + 1,
    };
    let ubo_size = fragment.block_size.max(vertex.block_size).max(16);
    if ubo_size > MAX_EFFECT_UBO_BYTES {
        return Err(anyhow!(
            "effect uniform block is {ubo_size} bytes, cap is {MAX_EFFECT_UBO_BYTES}"
        ));
    }
    Ok((sampler_count, ubo_size))
}

fn mesh_attributes(
    source: &str,
    geometry: EffectGeometry,
) -> Result<Vec<vk::VertexInputAttributeDescription>> {
    let known = match geometry {
        EffectGeometry::Mesh(VertexFormat::Standard) => MESH_ATTRIBUTES.as_slice(),
        EffectGeometry::Mesh(VertexFormat::Tangent) => MESH_TANGENT_UV_ATTRIBUTES.as_slice(),
        EffectGeometry::Mesh(VertexFormat::TangentLightmap) => MESH_TANGENT_ATTRIBUTES.as_slice(),
        EffectGeometry::Puppet => PUPPET_ATTRIBUTES.as_slice(),
        EffectGeometry::SceneMesh => SCENE_MESH_ATTRIBUTES.as_slice(),
        EffectGeometry::Morph => MORPH_ATTRIBUTES.as_slice(),
        EffectGeometry::Channels { .. } => CHANNEL_ATTRIBUTES.as_slice(),
        EffectGeometry::Quad => return Err(anyhow!("quad mesh attributes unsupported")),
    };
    let mut attributes = Vec::new();
    let mut position = false;
    for input in paper_scene::shader::vertex_inputs(source).map_err(anyhow::Error::msg)? {
        let paper_scene::shader::VertexInput { location, kind: ty, name } = input;
        let (_, _, vertex_format, offset) = known
            .iter()
            .find(|(attribute, kind, _, _)| *attribute == name && *kind == ty)
            .ok_or_else(|| anyhow!("mesh vertex attribute {name} unsupported"))?;
        position |= name == MESH_POSITION || name == MORPH_POSITION;
        attributes.push(
            vk::VertexInputAttributeDescription::default()
                .location(location)
                .binding(0)
                .format(*vertex_format)
                .offset(*offset),
        );
    }
    ensure!(position, "mesh shader lacks position");
    Ok(attributes)
}

pub fn d3d_clip_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("SKWD_PAPER_D3D_CLIP").as_deref() != Ok("0"))
}

pub(crate) fn viewport(extent: vk::Extent2D, d3d_clip: bool) -> vk::Viewport {
    let d3d_clip = d3d_clip && d3d_clip_enabled();
    let height = extent.height as f32;
    let (y, height) = if d3d_clip { (height, -height) } else { (0.0, height) };
    vk::Viewport { x: 0.0, y, width: extent.width as f32, height, min_depth: 0.0, max_depth: 1.0 }
}

pub struct EffectPipeline {
    device: Option<std::sync::Arc<ash::Device>>,
    pub(crate) pipeline: vk::Pipeline,
    pub(super) single_sample: vk::Pipeline,
    pub(super) depth_pass: bool,
    pub(crate) layout: vk::PipelineLayout,
    set_layout: vk::DescriptorSetLayout,
    pub(crate) set: vk::DescriptorSet,
    pool: vk::DescriptorPool,
    ubo: vk::Buffer,
    ubo_memory: vk::DeviceMemory,
    pub(super) ubo_mapped: *mut u8,
    pub(super) ubo_size: usize,
    pub sampler_count: u32,
    comparison_samplers: u32,
    pub hlsl: bool,
    pub coordinates: EffectCoordinates,
    pub scene: Option<Box<EffectPipeline>>,
}

fn hlsl_skipped(label: &str) -> bool {
    static SKIP: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    SKIP.get_or_init(|| {
        std::env::var("SKWD_PAPER_HLSL_SKIP")
            .map(|list| list.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect())
            .unwrap_or_default()
    })
    .iter()
    .any(|needle| label.contains(needle.as_str()))
}

struct CompiledStages {
    vertex: Vec<u32>,
    geometry: Option<Vec<u32>>,
    fragment: Vec<u32>,
    hlsl: bool,
}

fn compile_stages(
    vertex: &Translated,
    geometry: Option<&Translated>,
    fragment: &Translated,
    hlsl: Option<&HlslPair>,
    label: &str,
) -> Result<CompiledStages> {
    if let Some(pair) = hlsl
        && geometry.is_none()
        && paper_scene::hlsl::available()
        && !hlsl_skipped(label)
    {
        let compiled =
            paper_scene::hlsl::compile(&pair.vertex, Stage::Vertex, label).and_then(|vert| {
                paper_scene::hlsl::compile(&pair.fragment, Stage::Fragment, label)
                    .map(|frag| (vert, frag))
            });
        match compiled {
            Ok((vertex, fragment)) => {
                return Ok(CompiledStages { vertex, geometry: None, fragment, hlsl: true });
            }
            Err(err) => {
                tracing::info!("skwd-wall-vk: hlsl path fell back to glsl for {label}: {err:#}")
            }
        }
    }
    let vert_words = paper_scene::shader::compile(&vertex.source, Stage::Vertex, label)?;
    let geom_words = geometry
        .map(|stage| paper_scene::shader::compile(&stage.source, Stage::Geometry, label))
        .transpose()?;
    let frag_words = paper_scene::shader::compile(&fragment.source, Stage::Fragment, label)?;
    Ok(CompiledStages {
        vertex: vert_words,
        geometry: geom_words,
        fragment: frag_words,
        hlsl: false,
    })
}

pub struct QuadBuffer {
    device: Option<std::sync::Arc<ash::Device>>,
    pub(super) buffer: vk::Buffer,
    memory: vk::DeviceMemory,
}

pub struct DynBuffer {
    device: Option<std::sync::Arc<ash::Device>>,
    pub buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    ptr: *mut u8,
    pub size: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EffectPipelineSpec {
    pub depth_attachment: bool,
    pub format: vk::Format,
    pub samples: vk::SampleCountFlags,
    pub blend: crate::vk::SceneBlend,
    pub scene: Option<ScenePresent>,
    pub geometry: EffectGeometry,
    pub depth: EffectDepth,
    pub coordinates: EffectCoordinates,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ScenePresent {
    pub blend: crate::vk::SceneBlend,
    pub depth_test: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum EffectCoordinates {
    #[default]
    Canvas,
    Viewport,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EffectGeometry {
    Quad,
    Mesh(VertexFormat),
    Puppet,
    SceneMesh,
    Morph,
    Channels { alpha: bool },
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct EffectDepth {
    pub test: bool,
    pub write: bool,
    pub cull: bool,
    pub order: super::DepthOrder,
}

impl EffectPipelineSpec {
    pub fn quad(
        format: vk::Format,
        blend: crate::vk::SceneBlend,
        scene: Option<crate::vk::SceneBlend>,
    ) -> Self {
        Self {
            depth_attachment: true,
            format,
            samples: vk::SampleCountFlags::TYPE_1,
            blend,
            scene: scene.map(|blend| ScenePresent { blend, depth_test: false }),
            geometry: EffectGeometry::Quad,
            depth: EffectDepth::default(),
            coordinates: EffectCoordinates::Canvas,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PipelineKind {
    Effect(EffectPipelineSpec),
    Particles(ParticlePipelineSpec),
}

#[derive(Clone, Copy)]
enum PassTarget {
    Color(vk::RenderPass),
    Depth(vk::RenderPass),
}

#[derive(Clone, Copy)]
struct PipelineSources<'a> {
    vertex: &'a Translated,
    geometry: Option<&'a Translated>,
    fragment: &'a Translated,
    hlsl: Option<&'a HlslPair>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ParticleMesh {
    SpriteQuads,
    SpritePoints,
    RopePoints,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ParticlePipelineSpec {
    pub blend: paper_scene::particles::ParticleBlend,
    pub samples: vk::SampleCountFlags,
    pub mesh: ParticleMesh,
}

fn particle_blend_state(
    blend: paper_scene::particles::ParticleBlend,
) -> vk::PipelineColorBlendAttachmentState {
    use paper_scene::particles::ParticleBlend;
    let (src, dst, src_a, dst_a) = match blend {
        ParticleBlend::Additive => (
            vk::BlendFactor::SRC_ALPHA,
            vk::BlendFactor::ONE,
            vk::BlendFactor::ZERO,
            vk::BlendFactor::ONE,
        ),
        ParticleBlend::Translucent => (
            vk::BlendFactor::SRC_ALPHA,
            vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            vk::BlendFactor::ONE,
            vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
        ),
        ParticleBlend::Normal => (
            vk::BlendFactor::ONE,
            vk::BlendFactor::ZERO,
            vk::BlendFactor::ONE,
            vk::BlendFactor::ZERO,
        ),
    };
    vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::RGBA)
        .blend_enable(true)
        .src_color_blend_factor(src)
        .dst_color_blend_factor(dst)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(src_a)
        .dst_alpha_blend_factor(dst_a)
        .alpha_blend_op(vk::BlendOp::ADD)
}

impl Renderer {
    pub fn create_quad_buffer(
        &self,
        size: (u32, u32),
        uv: [f32; 4],
        image: (f32, f32),
    ) -> Result<QuadBuffer> {
        let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let [u, v, right, bottom] = uv;
        let (x, y) = (image.0 * 0.5, image.1 * 0.5);
        self.upload_quad([
            0.0,
            h,
            0.0,
            0.0,
            0.0,
            w,
            h,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            w,
            0.0,
            0.0,
            1.0,
            1.0,
            0.0,
            h,
            0.0,
            u,
            v,
            w,
            h,
            0.0,
            right,
            v,
            0.0,
            0.0,
            0.0,
            u,
            bottom,
            w,
            0.0,
            0.0,
            right,
            bottom,
            -x,
            y,
            0.0,
            u,
            v,
            x,
            y,
            0.0,
            right,
            v,
            -x,
            -y,
            0.0,
            u,
            bottom,
            x,
            -y,
            0.0,
            right,
            bottom,
            -x,
            y,
            0.0,
            0.0,
            0.0,
            x,
            y,
            0.0,
            right + u,
            0.0,
            -x,
            -y,
            0.0,
            0.0,
            bottom + v,
            x,
            -y,
            0.0,
            right + u,
            bottom + v,
        ])
    }

    pub fn create_quad_buffer_ndc_d3d(&self) -> Result<QuadBuffer> {
        self.upload_quad([
            -1.0, 1.0, 0.0, 0.0, 0.0, //
            1.0, 1.0, 0.0, 1.0, 0.0, //
            -1.0, -1.0, 0.0, 0.0, 1.0, //
            1.0, -1.0, 0.0, 1.0, 1.0,
        ])
    }

    pub fn create_quad_buffer_ndc(&self) -> Result<QuadBuffer> {
        self.upload_quad([
            -1.0, -1.0, 0.0, 0.0, 0.0, //
            1.0, -1.0, 0.0, 1.0, 0.0, //
            -1.0, 1.0, 0.0, 0.0, 1.0, //
            1.0, 1.0, 0.0, 1.0, 1.0,
        ])
    }

    fn upload_quad<const COMPONENTS: usize>(&self, quad: [f32; COMPONENTS]) -> Result<QuadBuffer> {
        let mut output = QuadBuffer {
            device: Some(self.resource_device()),
            buffer: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
        };
        #[cfg(test)]
        LIVE_QUADS.set(LIVE_QUADS.get() + 1);
        unsafe {
            let size = std::mem::size_of_val(&quad) as u64;
            output.buffer = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let reqs = self.device.get_buffer_memory_requirements(output.buffer);
            output.memory = self.device.allocate_memory(&self.host_memory_info(reqs)?, None)?;
            self.device.bind_buffer_memory(output.buffer, output.memory, 0)?;
            let ptr =
                self.device.map_memory(output.memory, 0, size, vk::MemoryMapFlags::empty())?;
            std::ptr::copy_nonoverlapping(quad.as_ptr(), ptr.cast::<f32>(), quad.len());
            self.device.unmap_memory(output.memory);
        }
        Ok(output)
    }

    pub fn destroy_quad_buffer(&self, quad: QuadBuffer) {
        let _ = self.wait_render();
        quad.destroy(&self.device);
    }

    pub fn create_effect_pipeline(
        &mut self,
        vertex: &Translated,
        fragment: &Translated,
        hlsl: Option<&HlslPair>,
        label: &str,
    ) -> Result<EffectPipeline> {
        self.create_effect_pipeline_for(
            vertex,
            fragment,
            hlsl,
            label,
            EffectPipelineSpec::quad(self.scene_config.format, crate::vk::SceneBlend::Copy, None),
        )
    }

    pub fn create_effect_pipeline_for(
        &mut self,
        vertex: &Translated,
        fragment: &Translated,
        hlsl: Option<&HlslPair>,
        label: &str,
        spec: EffectPipelineSpec,
    ) -> Result<EffectPipeline> {
        let render_pass = if !spec.depth_attachment {
            self.depth_passes_for(
                super::SceneConfig { format: spec.format, depth: None },
                spec.samples,
            )?
            .clear
        } else if self.scene_config.depth.is_none() && (spec.depth.test || spec.depth.write) {
            self.depth_passes_for(
                super::SceneConfig { format: spec.format, depth: Some(spec.depth.order) },
                spec.samples,
            )?
            .clear
        } else {
            self.scene_pass_for(spec.format, spec.samples)?
        };
        let sources = PipelineSources { vertex, geometry: None, fragment, hlsl };
        let mut pipe = self.create_pipeline_kind(
            sources,
            label,
            PipelineKind::Effect(spec),
            PassTarget::Color(render_pass),
        )?;
        if let Some(present) = spec.scene {
            let scene_spec = EffectPipelineSpec {
                blend: present.blend,
                scene: None,
                samples: self.scene_sample_count(),
                depth: EffectDepth {
                    test: spec.depth.test || present.depth_test,
                    order: super::DepthOrder::Greater,
                    ..spec.depth
                },
                ..spec
            };
            let scene_pass = match self.scene_pass_for(scene_spec.format, scene_spec.samples) {
                Ok(pass) => pass,
                Err(error) => {
                    self.destroy_effect_pipeline(pipe);
                    return Err(error);
                }
            };
            match self.create_pipeline_kind(
                sources,
                label,
                PipelineKind::Effect(scene_spec),
                PassTarget::Color(scene_pass),
            ) {
                Ok(final_pipe) => pipe.scene = Some(Box::new(final_pipe)),
                Err(error) => {
                    self.destroy_effect_pipeline(pipe);
                    return Err(error);
                }
            }
        }
        Ok(pipe)
    }

    pub fn create_shadow_pipeline(
        &mut self,
        sources: &paper_scene::effects::material::shadow::Sources,
        label: &str,
        mut spec: EffectPipelineSpec,
        pass: vk::RenderPass,
    ) -> Result<EffectPipeline> {
        spec.samples = vk::SampleCountFlags::TYPE_1;
        spec.scene = None;
        spec.depth =
            EffectDepth { test: true, write: true, cull: false, order: super::DepthOrder::Greater };
        let sources = PipelineSources {
            vertex: &sources.vertex,
            geometry: None,
            fragment: &sources.fragment,
            hlsl: sources.hlsl.as_ref(),
        };
        self.create_pipeline_kind(
            sources,
            label,
            PipelineKind::Effect(spec),
            PassTarget::Depth(pass),
        )
    }

    pub fn create_particle_pipeline(
        &mut self,
        vertex: &Translated,
        geometry: Option<&Translated>,
        fragment: &Translated,
        hlsl: Option<&HlslPair>,
        label: &str,
        spec: ParticlePipelineSpec,
    ) -> Result<EffectPipeline> {
        let render_pass = self.scene_pass_for(self.scene_config.format, spec.samples)?;
        let sources = PipelineSources { vertex, geometry, fragment, hlsl };
        self.create_pipeline_kind(
            sources,
            label,
            PipelineKind::Particles(spec),
            PassTarget::Color(render_pass),
        )
    }

    pub fn create_dyn_buffer(&self, size: usize, usage: vk::BufferUsageFlags) -> Result<DynBuffer> {
        let mut output = DynBuffer {
            device: Some(self.resource_device()),
            buffer: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
            ptr: std::ptr::null_mut(),
            size: size.max(std::mem::size_of::<f32>()),
        };
        #[cfg(test)]
        LIVE_DYNAMIC.set(LIVE_DYNAMIC.get() + 1);
        unsafe {
            output.buffer = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(output.size as u64)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let reqs = self.device.get_buffer_memory_requirements(output.buffer);
            output.memory = self.device.allocate_memory(&self.host_memory_info(reqs)?, None)?;
            self.device.bind_buffer_memory(output.buffer, output.memory, 0)?;
            output.ptr = self
                .device
                .map_memory(output.memory, 0, output.size as u64, vk::MemoryMapFlags::empty())?
                .cast::<u8>();
        }
        Ok(output)
    }

    pub fn write_dyn_buffer(&self, buffer: &DynBuffer, bytes: &[u8]) {
        let len = bytes.len().min(buffer.size);
        if len > 0 && !buffer.ptr.is_null() {
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.ptr, len) };
        }
    }

    pub fn destroy_dyn_buffer(&self, buffer: DynBuffer) {
        let _ = self.wait_render();
        buffer.destroy(&self.device);
    }

    fn create_pipeline_kind(
        &mut self,
        sources: PipelineSources<'_>,
        label: &str,
        kind: PipelineKind,
        target: PassTarget,
    ) -> Result<EffectPipeline> {
        let (render_pass, depth_only) = match target {
            PassTarget::Color(pass) => (pass, false),
            PassTarget::Depth(pass) => (pass, true),
        };
        let PipelineSources { vertex, geometry, fragment, hlsl } = sources;
        self.ensure_scene_pipelines()?;
        let shaders_started = std::time::Instant::now();
        let CompiledStages { vertex: vert_words, geometry: geom_words, fragment: frag_words, hlsl } =
            compile_stages(vertex, geometry, fragment, hlsl, label)?;
        let shaders_ms = shaders_started.elapsed().as_secs_f64() * 1000.0;
        let (sampler_count, ubo_size) = effect_caps(vertex, fragment)?;
        let mut output = EffectPipeline {
            device: Some(self.resource_device()),
            pipeline: vk::Pipeline::null(),
            single_sample: vk::Pipeline::null(),
            depth_pass: !depth_only
                && self.scene_config.depth.is_none()
                && matches!(kind, PipelineKind::Effect(spec) if spec.depth.test || spec.depth.write),
            layout: vk::PipelineLayout::null(),
            set_layout: vk::DescriptorSetLayout::null(),
            set: vk::DescriptorSet::null(),
            pool: vk::DescriptorPool::null(),
            ubo: vk::Buffer::null(),
            ubo_memory: vk::DeviceMemory::null(),
            ubo_mapped: std::ptr::null_mut(),
            ubo_size,
            sampler_count,
            comparison_samplers: vertex
                .samplers
                .iter()
                .chain(&fragment.samplers)
                .filter(|sampler| sampler.comparison)
                .fold(0, |mask, sampler| mask | (1 << sampler.index)),
            hlsl,
            coordinates: match kind {
                PipelineKind::Effect(spec) => spec.coordinates,
                PipelineKind::Particles(..) => EffectCoordinates::Canvas,
            },
            scene: None,
        };
        #[cfg(test)]
        LIVE_PIPELINES.set(LIVE_PIPELINES.get() + 1);
        let single_pass = match kind {
            PipelineKind::Effect(spec)
                if !depth_only && spec.samples != vk::SampleCountFlags::TYPE_1 =>
            {
                let depth = if !spec.depth_attachment {
                    None
                } else if output.depth_pass {
                    Some(spec.depth.order)
                } else {
                    self.scene_config.depth
                };
                Some(
                    self.depth_passes_for(
                        super::SceneConfig { format: spec.format, depth },
                        vk::SampleCountFlags::TYPE_1,
                    )?
                    .clear,
                )
            }
            _ => None,
        };
        let mut modules = ShaderModules::new(&self.device);
        unsafe {
            let mut stages_all = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
            if geometry.is_some() {
                stages_all |= vk::ShaderStageFlags::GEOMETRY;
            }
            let mut bindings = vec![
                vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(stages_all),
            ];
            for slot in 0..sampler_count {
                if hlsl {
                    bindings.push(
                        vk::DescriptorSetLayoutBinding::default()
                            .binding(slot + TEXTURE_BINDING_SHIFT)
                            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                            .descriptor_count(1)
                            .stage_flags(stages_all),
                    );
                    bindings.push(
                        vk::DescriptorSetLayoutBinding::default()
                            .binding(slot + SAMPLER_BINDING_SHIFT)
                            .descriptor_type(vk::DescriptorType::SAMPLER)
                            .descriptor_count(1)
                            .stage_flags(stages_all),
                    );
                } else {
                    bindings.push(
                        vk::DescriptorSetLayoutBinding::default()
                            .binding(slot + 1)
                            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                            .descriptor_count(1)
                            .stage_flags(stages_all),
                    );
                }
            }
            output.set_layout = self.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?;
            let layouts = [output.set_layout];
            let push = if depth_only {
                vec![
                    vk::PushConstantRange::default()
                        .stage_flags(vk::ShaderStageFlags::VERTEX)
                        .size(size_of::<paper_scene::matrix::Mat4>() as u32),
                ]
            } else {
                Vec::new()
            };
            output.layout = self.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&layouts)
                    .push_constant_ranges(&push),
                None,
            )?;

            output.ubo = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(ubo_size as u64)
                    .usage(vk::BufferUsageFlags::UNIFORM_BUFFER)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let reqs = self.device.get_buffer_memory_requirements(output.ubo);
            output.ubo_memory = self.device.allocate_memory(&self.host_memory_info(reqs)?, None)?;
            self.device.bind_buffer_memory(output.ubo, output.ubo_memory, 0)?;
            output.ubo_mapped = self
                .device
                .map_memory(output.ubo_memory, 0, ubo_size as u64, vk::MemoryMapFlags::empty())?
                .cast::<u8>();

            (output.pool, output.set) =
                super::scene::effect_set(&self.device, &mut self.fx_pools, &layouts)?;
            let infos = [vk::DescriptorBufferInfo::default()
                .buffer(output.ubo)
                .offset(0)
                .range(ubo_size as u64)];
            self.device.update_descriptor_sets(
                &[vk::WriteDescriptorSet::default()
                    .dst_set(output.set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&infos)],
                &[],
            );

            let vert = modules.create(&vert_words)?;
            let frag = modules.create(&frag_words)?;
            let mut stages = vec![
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(vert)
                    .name(SHADER_ENTRY),
            ];
            if let Some(words) = geom_words.as_ref() {
                let geom = modules.create(words)?;
                stages.push(
                    vk::PipelineShaderStageCreateInfo::default()
                        .stage(vk::ShaderStageFlags::GEOMETRY)
                        .module(geom)
                        .name(SHADER_ENTRY),
                );
            }
            stages.push(
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(frag)
                    .name(SHADER_ENTRY),
            );
            let stride = match kind {
                PipelineKind::Effect(spec) => match spec.geometry {
                    EffectGeometry::Mesh(format) => {
                        (format.floats() * std::mem::size_of::<f32>()) as u32
                    }
                    EffectGeometry::Puppet => PUPPET_VERTEX_BYTES,
                    EffectGeometry::SceneMesh => {
                        std::mem::size_of::<[f32; super::scene::PUPPET_VERTEX_FLOATS]>() as u32
                    }
                    EffectGeometry::Morph => paper_scene::puppet::MORPH_STRIDE as u32,
                    EffectGeometry::Channels { .. } => paper_scene::puppet::channels::STRIDE as u32,
                    EffectGeometry::Quad => std::mem::size_of::<[f32; 5]>() as u32,
                },
                PipelineKind::Particles(ParticlePipelineSpec {
                    mesh: ParticleMesh::SpritePoints,
                    ..
                }) => {
                    (paper_scene::particles::SPRITE_POINT_FLOATS * std::mem::size_of::<f32>())
                        as u32
                }
                PipelineKind::Particles(ParticlePipelineSpec {
                    mesh: ParticleMesh::RopePoints,
                    ..
                }) => {
                    (paper_scene::particles::ROPE_POINT_FLOATS * std::mem::size_of::<f32>()) as u32
                }
                PipelineKind::Particles(_) => {
                    (paper_scene::particles::SPRITE_FLOATS_PER_VERTEX * std::mem::size_of::<f32>())
                        as u32
                }
            };
            let bind_desc = [vk::VertexInputBindingDescription::default()
                .binding(0)
                .stride(stride)
                .input_rate(vk::VertexInputRate::VERTEX)];
            let attr = |location: u32, format: vk::Format, offset: u32| {
                vk::VertexInputAttributeDescription::default()
                    .location(location)
                    .binding(0)
                    .format(format)
                    .offset(offset)
            };
            let attrs: Vec<vk::VertexInputAttributeDescription> = match kind {
                PipelineKind::Effect(EffectPipelineSpec {
                    geometry:
                        geometry @ (EffectGeometry::Mesh(_)
                        | EffectGeometry::Puppet
                        | EffectGeometry::SceneMesh
                        | EffectGeometry::Morph
                        | EffectGeometry::Channels { .. }),
                    ..
                }) => mesh_attributes(&vertex.source, geometry)?,
                PipelineKind::Effect(_) => vec![
                    attr(0, vk::Format::R32G32B32_SFLOAT, 0),
                    attr(1, vk::Format::R32G32_SFLOAT, 12),
                ],
                PipelineKind::Particles(ParticlePipelineSpec {
                    mesh: ParticleMesh::SpritePoints,
                    ..
                }) => vec![
                    attr(0, vk::Format::R32G32B32_SFLOAT, 0),
                    attr(1, vk::Format::R32G32B32A32_SFLOAT, PARTICLE_POSITION_BYTES),
                    attr(
                        2,
                        vk::Format::R32G32B32A32_SFLOAT,
                        PARTICLE_POSITION_BYTES + PARTICLE_VEC4_BYTES,
                    ),
                    attr(
                        SPRITE_COLOR_LOCATION,
                        vk::Format::R32G32B32A32_SFLOAT,
                        PARTICLE_POSITION_BYTES + 2 * PARTICLE_VEC4_BYTES,
                    ),
                ],
                PipelineKind::Particles(ParticlePipelineSpec {
                    mesh: ParticleMesh::RopePoints,
                    ..
                }) => (0..ROPE_POINT_ATTRIBUTES)
                    .map(|slot| {
                        attr(slot, vk::Format::R32G32B32A32_SFLOAT, slot * PARTICLE_VEC4_BYTES)
                    })
                    .collect(),
                PipelineKind::Particles(_) => vec![
                    attr(0, vk::Format::R32G32B32_SFLOAT, 0),
                    attr(1, vk::Format::R32G32B32A32_SFLOAT, 12),
                    attr(2, vk::Format::R32G32B32A32_SFLOAT, 28),
                    attr(3, vk::Format::R32G32B32A32_SFLOAT, 44),
                    attr(4, vk::Format::R32G32_SFLOAT, 60),
                ],
            };
            let vi = vk::PipelineVertexInputStateCreateInfo::default()
                .vertex_binding_descriptions(&bind_desc)
                .vertex_attribute_descriptions(&attrs);
            let ia = vk::PipelineInputAssemblyStateCreateInfo::default().topology(match kind {
                PipelineKind::Effect(spec)
                    if matches!(
                        spec.geometry,
                        EffectGeometry::Mesh(_)
                            | EffectGeometry::Puppet
                            | EffectGeometry::SceneMesh
                            | EffectGeometry::Morph
                            | EffectGeometry::Channels { .. }
                    ) =>
                {
                    vk::PrimitiveTopology::TRIANGLE_LIST
                }
                PipelineKind::Effect(_) => vk::PrimitiveTopology::TRIANGLE_STRIP,
                PipelineKind::Particles(spec) => match spec.mesh {
                    ParticleMesh::SpriteQuads => vk::PrimitiveTopology::TRIANGLE_LIST,
                    ParticleMesh::SpritePoints | ParticleMesh::RopePoints => {
                        vk::PrimitiveTopology::POINT_LIST
                    }
                },
            });
            let viewport = [vk::Viewport::default()];
            let scissor = [vk::Rect2D::default()];
            let vp = vk::PipelineViewportStateCreateInfo::default()
                .viewports(&viewport)
                .scissors(&scissor);
            let cull = match kind {
                PipelineKind::Effect(spec) if spec.depth.cull => vk::CullModeFlags::BACK,
                _ => vk::CullModeFlags::NONE,
            };
            let front = match kind {
                PipelineKind::Effect(spec) if spec.geometry == EffectGeometry::Quad => {
                    vk::FrontFace::CLOCKWISE
                }
                _ => vk::FrontFace::COUNTER_CLOCKWISE,
            };
            let rs = vk::PipelineRasterizationStateCreateInfo::default()
                .polygon_mode(vk::PolygonMode::FILL)
                .cull_mode(cull)
                .front_face(front)
                .depth_bias_enable(depth_only)
                .depth_bias_slope_factor(super::shadow::SLOPE_BIAS)
                .line_width(1.0);
            let samples = match kind {
                PipelineKind::Effect(spec) => spec.samples,
                PipelineKind::Particles(spec) => spec.samples,
            };
            let ms =
                vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(samples)
                    .alpha_to_coverage_enable(matches!(kind, PipelineKind::Effect(spec) if spec.blend == super::SceneBlend::AlphaCoverage));
            let att = if depth_only {
                Vec::new()
            } else {
                vec![match kind {
                    PipelineKind::Effect(spec) => {
                        let blend = super::scene::scene_blend_state(spec.blend);
                        match spec.geometry {
                            EffectGeometry::Channels { alpha: false } => blend.color_write_mask(
                                vk::ColorComponentFlags::R
                                    | vk::ColorComponentFlags::G
                                    | vk::ColorComponentFlags::B,
                            ),
                            EffectGeometry::Channels { alpha: true }
                                if spec.blend == super::SceneBlend::Alpha =>
                            {
                                blend.src_alpha_blend_factor(vk::BlendFactor::SRC_ALPHA)
                            }
                            _ => blend,
                        }
                    }
                    PipelineKind::Particles(spec) => particle_blend_state(spec.blend),
                }]
            };
            let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&att);
            let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
            let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);
            let depth = match kind {
                PipelineKind::Effect(spec) => vk::PipelineDepthStencilStateCreateInfo::default()
                    .depth_test_enable(spec.depth.test)
                    .depth_write_enable(spec.depth.write)
                    .depth_compare_op(spec.depth.order.compare()),
                PipelineKind::Particles(..) => vk::PipelineDepthStencilStateCreateInfo::default(),
            };
            let pipeline_started = std::time::Instant::now();
            let info = vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(&vi)
                .input_assembly_state(&ia)
                .viewport_state(&vp)
                .rasterization_state(&rs)
                .multisample_state(&ms)
                .depth_stencil_state(&depth)
                .color_blend_state(&blend)
                .dynamic_state(&dynamic)
                .layout(output.layout)
                .render_pass(render_pass)
                .subpass(0);
            let single_ms = ms.rasterization_samples(vk::SampleCountFlags::TYPE_1);
            let mut infos = vec![info];
            if let Some(pass) = single_pass {
                infos.push(info.multisample_state(&single_ms).render_pass(pass));
            }
            let pipelines = self
                .device
                .create_graphics_pipelines(vk::PipelineCache::null(), &infos, None)
                .map_err(|(pipelines, err)| {
                    for pipeline in pipelines {
                        self.device.destroy_pipeline(pipeline, None);
                    }
                    anyhow::Error::from(err).context(format!("effect pipeline {label}"))
                })?;
            output.pipeline = pipelines[0];
            if let Some(&pipeline) = pipelines.get(1) {
                output.single_sample = pipeline;
                #[cfg(test)]
                LIVE_PIPELINES.set(LIVE_PIPELINES.get() + 1);
            }
            tracing::debug!(
                label,
                shaders_ms,
                pipeline_ms = pipeline_started.elapsed().as_secs_f64() * 1000.0,
                "scene pipeline setup"
            );
        }
        Ok(output)
    }

    pub fn destroy_effect_pipeline(&self, pipe: EffectPipeline) {
        let _ = self.wait_render();
        pipe.destroy(&self.device);
    }

    pub fn run_effect_pass(
        &mut self,
        pipe: &EffectPipeline,
        quad: &QuadBuffer,
        draw: EffectDraw<'_>,
        inputs: &[(vk::ImageView, vk::Sampler)],
        uniforms: &[u8],
    ) -> Result<()> {
        self.begin_scene_batch()?;
        self.record_effect_pass(pipe, quad, draw, inputs, uniforms)?;
        self.submit_scene_batch().context("effect pass fence")
    }

    pub fn write_effect_inputs(
        &self,
        pipe: &EffectPipeline,
        inputs: &[(vk::ImageView, vk::Sampler)],
        uniforms: &[u8],
    ) {
        unsafe {
            if !uniforms.is_empty() && !pipe.ubo_mapped.is_null() {
                let len = uniforms.len().min(pipe.ubo_size);
                std::ptr::copy_nonoverlapping(uniforms.as_ptr(), pipe.ubo_mapped, len);
            }
        }
        self.write_effect_samples(
            pipe,
            (0..pipe.sampler_count as usize).filter_map(|slot| {
                inputs.get(slot).or_else(|| inputs.first()).copied().map(|sample| (slot, sample))
            }),
        );
    }

    pub fn effect_depth_sample(
        &self,
        pipe: &EffectPipeline,
        slot: usize,
        sample: super::ShadowSample,
    ) -> (vk::ImageView, vk::Sampler) {
        if slot < pipe.sampler_count as usize && pipe.comparison_samplers & (1 << slot) != 0 {
            sample.comparison()
        } else {
            (sample.view, sample.ordinary)
        }
    }

    pub fn write_effect_input(
        &self,
        pipe: &EffectPipeline,
        slot: usize,
        sample: (vk::ImageView, vk::Sampler),
    ) {
        if slot < pipe.sampler_count as usize {
            self.write_effect_samples(pipe, std::iter::once((slot, sample)));
        }
    }

    fn write_effect_samples(
        &self,
        pipe: &EffectPipeline,
        samples: impl Iterator<Item = (usize, (vk::ImageView, vk::Sampler))>,
    ) {
        unsafe {
            let mut writes = Vec::new();
            let mut infos = Vec::new();
            let mut slots = Vec::new();
            for (slot, (view, sampler)) in samples {
                slots.push(slot);
                infos.push([vk::DescriptorImageInfo::default()
                    .sampler(sampler)
                    .image_view(view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]);
            }
            for (info, &slot) in infos.iter().zip(slots.iter()) {
                if pipe.hlsl {
                    writes.push(
                        vk::WriteDescriptorSet::default()
                            .dst_set(pipe.set)
                            .dst_binding(slot as u32 + TEXTURE_BINDING_SHIFT)
                            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                            .image_info(info),
                    );
                    writes.push(
                        vk::WriteDescriptorSet::default()
                            .dst_set(pipe.set)
                            .dst_binding(slot as u32 + SAMPLER_BINDING_SHIFT)
                            .descriptor_type(vk::DescriptorType::SAMPLER)
                            .image_info(info),
                    );
                } else {
                    writes.push(
                        vk::WriteDescriptorSet::default()
                            .dst_set(pipe.set)
                            .dst_binding(slot as u32 + 1)
                            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                            .image_info(info),
                    );
                }
            }
            if !writes.is_empty() {
                self.device.update_descriptor_sets(&writes, &[]);
            }
        }
    }

    pub fn record_effect_pass(
        &mut self,
        pipe: &EffectPipeline,
        quad: &QuadBuffer,
        draw: EffectDraw<'_>,
        inputs: &[(vk::ImageView, vk::Sampler)],
        uniforms: &[u8],
    ) -> Result<()> {
        let target = draw.target;
        let render_pass = if draw.preserve && target.depth_order != self.scene_config.depth {
            self.depth_passes_for(
                super::SceneConfig { format: target.format, depth: target.depth_order },
                target.samples,
            )?
            .append
        } else if draw.preserve {
            self.scene_pass_append_for(target.format, target.samples)?
        } else {
            target.render_pass
        };
        self.write_effect_inputs(pipe, inputs, uniforms);
        unsafe {
            let clear = super::scene::clear_values(super::scene::CLEAR_COLOR, target.depth_order);
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(render_pass)
                    .framebuffer(target.framebuffer())
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: target.extent,
                    })
                    .clear_values(&clear[..target.clear_value_count()]),
                vk::SubpassContents::INLINE,
            );
            self.device.cmd_set_viewport(self.cmd, 0, &[viewport(target.extent, pipe.hlsl)]);
            self.device.cmd_set_scissor(
                self.cmd,
                0,
                &[vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: target.extent }],
            );
            self.device.cmd_bind_pipeline(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                if target.samples == vk::SampleCountFlags::TYPE_1
                    && pipe.single_sample != vk::Pipeline::null()
                {
                    pipe.single_sample
                } else {
                    pipe.pipeline
                },
            );
            self.device.cmd_bind_descriptor_sets(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.layout,
                0,
                &[pipe.set],
                &[],
            );
            let offset = if draw.final_vertices { QUAD_VERTEX_BYTES } else { 0 };
            self.device.cmd_bind_vertex_buffers(self.cmd, 0, &[quad.buffer], &[offset]);
            self.device.cmd_draw(self.cmd, 4, 1, 0, 0);
            self.device.cmd_end_render_pass(self.cmd);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
thread_local! {
    static LIVE_PIPELINES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LIVE_QUADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LIVE_DYNAMIC: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Renderer {
    pub fn effect_live_allocations() -> [usize; 3] {
        [LIVE_PIPELINES.get(), LIVE_QUADS.get(), LIVE_DYNAMIC.get()]
    }
}

impl EffectPipeline {
    pub fn destroy(mut self, device: &ash::Device) {
        self.device = None;
        self.release(device);
    }

    fn release(&mut self, device: &ash::Device) {
        if let Some(scene) = self.scene.take() {
            scene.destroy(device);
        }
        unsafe {
            if self.set != vk::DescriptorSet::null() {
                let _ = device.free_descriptor_sets(self.pool, &[self.set]);
            }
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline(self.single_sample, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
            if !self.ubo_mapped.is_null() {
                device.unmap_memory(self.ubo_memory);
            }
            device.destroy_buffer(self.ubo, None);
            device.free_memory(self.ubo_memory, None);
        }
        #[cfg(test)]
        LIVE_PIPELINES.set(
            LIVE_PIPELINES.get() - 1 - usize::from(self.single_sample != vk::Pipeline::null()),
        );
    }
}

impl Drop for EffectPipeline {
    fn drop(&mut self) {
        if let Some(device) = self.device.take() {
            let _ = crate::shared::wait_idle(&device);
            self.release(&device);
        }
    }
}

impl QuadBuffer {
    pub fn destroy(mut self, device: &ash::Device) {
        self.device = None;
        self.release(device);
    }

    fn release(&mut self, device: &ash::Device) {
        unsafe {
            device.destroy_buffer(self.buffer, None);
            device.free_memory(self.memory, None);
        }
        #[cfg(test)]
        LIVE_QUADS.set(LIVE_QUADS.get() - 1);
    }
}

impl Drop for QuadBuffer {
    fn drop(&mut self) {
        if let Some(device) = self.device.take() {
            let _ = crate::shared::wait_idle(&device);
            self.release(&device);
        }
    }
}

impl DynBuffer {
    pub fn destroy(mut self, device: &ash::Device) {
        self.device = None;
        self.release(device);
    }

    fn release(&mut self, device: &ash::Device) {
        unsafe {
            if !self.ptr.is_null() {
                device.unmap_memory(self.memory);
            }
            device.destroy_buffer(self.buffer, None);
            device.free_memory(self.memory, None);
        }
        #[cfg(test)]
        LIVE_DYNAMIC.set(LIVE_DYNAMIC.get() - 1);
    }
}

impl Drop for DynBuffer {
    fn drop(&mut self) {
        if let Some(device) = self.device.take() {
            let _ = crate::shared::wait_idle(&device);
            self.release(&device);
        }
    }
}
