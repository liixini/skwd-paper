use crate::vk::Renderer;
use anyhow::{Context, Result, anyhow};
use ash::vk;
use paper_scene::tex::{PixelFormat, Pixels};

pub(super) mod clip;
pub(super) mod text;

const SCENE_POOL_SETS: u32 = 64;

const ALPHA_PREMULTIPLY: u32 = 1;
const ALPHA_UNPREMULTIPLY: u32 = 2;
const UV_TRANSFORM: u32 = 4;
const UV_PADDING: u32 = 8;
const TEXT_MASK: u32 = 16;
const NO_UV_TRANSFORM: [f32; 4] = [0.0; 4];
const RGBA_CHANNELS: usize = 4;
pub(super) const CLEAR_COLOR: [f32; RGBA_CHANNELS] = [0.0; RGBA_CHANNELS];
const SCENE_COLOR_RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};
const SCENE_COLOR_LAYERS: vk::ImageSubresourceLayers = vk::ImageSubresourceLayers {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    mip_level: 0,
    base_array_layer: 0,
    layer_count: 1,
};
pub(super) const LAYER_PUSH_BYTES: u32 = std::mem::size_of::<LayerPush>() as u32;
const ALPHA_COMPONENT: usize = 3;
const QUAD_VERTEX_COUNT: u32 = 4;
pub(super) const PUPPET_VERTEX_FLOATS: usize = 5;
pub(super) const PUPPET_ALPHA_FLOAT: usize = 4;
const MATRIX_TRANSLATION_X: usize = 12;
const MATRIX_TRANSLATION_Y: usize = 13;
const SCENE_MSAA_ENV: &str = "SKWD_PAPER_SCENE_MSAA";
const SCENE_MSAA_DISABLED: &str = "0";
const MAX_SCENE_ATTACHMENTS: usize = 3;
const SINGLE_CLEAR_VALUES: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MipSampling {
    Disabled,
    Linear,
    Anisotropic,
}

impl MipSampling {
    pub fn has_mips(self) -> bool {
        self != Self::Disabled
    }

    pub fn uses_anisotropy(self, nearest: bool, enabled: bool) -> bool {
        self == Self::Anisotropic && !nearest && enabled
    }

    fn mip_layout(self, pixels: &Pixels, enabled: bool) -> (u32, bool) {
        let shipped = pixels.levels.len() as u32;
        let full_chain = u32::BITS - pixels.width().max(pixels.height()).leading_zeros();
        let generate = self == Self::Linear
            && enabled
            && !pixels.format.compressed()
            && shipped == 1
            && full_chain > 1;
        let levels = if generate {
            full_chain
        } else if self.has_mips() && enabled {
            shipped
        } else {
            1
        };
        (levels, generate)
    }
}

#[derive(Clone, Copy)]
pub struct EffectProjection {
    matrix: [f32; 16],
    origin: [f32; 2],
    offset: usize,
    fixed: bool,
    view: super::ClipView,
}

impl EffectProjection {
    pub fn for_layer(
        matrix: [f32; 16],
        model: [f32; 16],
        canvas: (f32, f32),
        offset: usize,
    ) -> Self {
        Self {
            matrix,
            origin: [model[MATRIX_TRANSLATION_X], canvas.1 - model[MATRIX_TRANSLATION_Y]],
            offset,
            fixed: false,
            view: super::ClipView::IDENTITY,
        }
    }

    pub fn fixed(matrix: [f32; 16], offset: usize) -> Self {
        Self { matrix, origin: [0.0; 2], offset, fixed: true, view: super::ClipView::IDENTITY }
    }

    #[must_use]
    pub fn viewed(self, view: super::ClipView) -> Self {
        Self { view, ..self }
    }

    pub fn at(&self, center: [f32; 2], canvas: [f32; 2], inverted_y: bool) -> [f32; 16] {
        let mut matrix = self.matrix;
        if !self.fixed {
            matrix[MATRIX_TRANSLATION_X] += (center[0] - self.origin[0]) * 2.0 / canvas[0];
            let dy = (center[1] - self.origin[1]) * 2.0 / canvas[1];
            matrix[MATRIX_TRANSLATION_Y] += if inverted_y { -dy } else { dy };
        }
        if self.view != super::ClipView::IDENTITY {
            self.view.apply(&mut matrix, !inverted_y);
        }
        matrix
    }
}

pub(super) const EFFECT_POOL_SETS: u32 = 512;
const EFFECT_POOL_SAMPLERS: u32 = 4096;

#[derive(Clone, Copy)]
pub struct SceneEffect {
    depth_pass: bool,
    pipeline: vk::Pipeline,
    single_sample: vk::Pipeline,
    layout: vk::PipelineLayout,
    set: vk::DescriptorSet,
    geometry: EffectGeometry,
    hlsl: bool,
    coordinates: super::effect::EffectCoordinates,
    projection: Option<EffectProjection>,
    uniforms: *mut u8,
    grab: Option<SceneGrab>,
}

#[derive(Clone, Copy)]
pub struct SceneGrab {
    image: vk::Image,
    extent: vk::Extent2D,
}

#[derive(Clone, Copy)]
enum EffectGeometry {
    Quad { vertices: vk::Buffer, offset: u64 },
    Mesh(MeshGeometry),
    Puppet(PuppetDraw),
}

#[derive(Clone, Copy, PartialEq)]
pub struct MeshGeometry {
    pub vertices: vk::Buffer,
    pub indices: vk::Buffer,
    pub index_type: vk::IndexType,
    pub count: u32,
}

impl SceneEffect {
    pub fn set_view(&mut self, view: super::ClipView) {
        if let Some(projection) = self.projection.as_mut().filter(|projection| !projection.fixed) {
            projection.view = view;
        }
    }

    pub fn new(
        pipe: &crate::vk::EffectPipeline,
        vertices: &crate::vk::QuadBuffer,
        projection: Option<EffectProjection>,
    ) -> Result<Self> {
        Self::for_geometry(
            pipe,
            EffectGeometry::Quad {
                vertices: vertices.buffer,
                offset: super::effect::MODEL_QUAD_OFFSET,
            },
            projection,
        )
    }

    pub fn remove_padding(&mut self) {
        if let EffectGeometry::Quad { offset, .. } = &mut self.geometry {
            *offset = super::effect::UNPADDED_MODEL_QUAD_OFFSET;
        }
    }

    pub fn mesh(
        pipe: &crate::vk::EffectPipeline,
        geometry: MeshGeometry,
        projection: Option<EffectProjection>,
    ) -> Result<Self> {
        Self::for_geometry(pipe, EffectGeometry::Mesh(geometry), projection)
    }

    pub fn puppet(
        pipe: &crate::vk::EffectPipeline,
        mesh: &SceneMesh,
        projection: Option<EffectProjection>,
        grab: SceneGrab,
    ) -> Result<Self> {
        if mesh.clip.is_some() {
            return Err(anyhow!("clipped puppet effect geometry"));
        }
        let geometry = EffectGeometry::Puppet(PuppetDraw::of(mesh));
        Ok(Self { grab: Some(grab), ..Self::for_geometry(pipe, geometry, projection)? })
    }

    fn for_geometry(
        pipe: &crate::vk::EffectPipeline,
        geometry: EffectGeometry,
        projection: Option<EffectProjection>,
    ) -> Result<Self> {
        if let Some(projection) = projection
            && projection
                .offset
                .checked_add(std::mem::size_of_val(&projection.matrix))
                .is_none_or(|end| end > pipe.ubo_size)
        {
            return Err(anyhow!("effect projection exceeds uniform buffer"));
        }
        Ok(Self {
            depth_pass: pipe.depth_pass,
            pipeline: pipe.pipeline,
            single_sample: pipe.single_sample,
            layout: pipe.layout,
            set: pipe.set,
            geometry,
            hlsl: pipe.hlsl,
            coordinates: pipe.coordinates,
            projection,
            uniforms: pipe.ubo_mapped,
            grab: None,
        })
    }
}

pub(super) fn scene_blend_state(blend: SceneBlend) -> vk::PipelineColorBlendAttachmentState {
    let state = vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::RGBA);
    let (source, destination, alpha_source, alpha_destination) = match blend {
        SceneBlend::Copy | SceneBlend::AlphaCoverage => return state,
        SceneBlend::CopyRgb => {
            return state.color_write_mask(
                vk::ColorComponentFlags::R
                    | vk::ColorComponentFlags::G
                    | vk::ColorComponentFlags::B,
            );
        }
        SceneBlend::Alpha => (
            vk::BlendFactor::SRC_ALPHA,
            vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            vk::BlendFactor::ONE,
            vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
        ),
        SceneBlend::Add => (
            vk::BlendFactor::SRC_ALPHA,
            vk::BlendFactor::ONE,
            vk::BlendFactor::ZERO,
            vk::BlendFactor::ONE,
        ),
        SceneBlend::Screen => (
            vk::BlendFactor::ONE_MINUS_DST_COLOR,
            vk::BlendFactor::ONE,
            vk::BlendFactor::ZERO,
            vk::BlendFactor::ONE,
        ),
    };
    state
        .blend_enable(true)
        .src_color_blend_factor(source)
        .dst_color_blend_factor(destination)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(alpha_source)
        .dst_alpha_blend_factor(alpha_destination)
        .alpha_blend_op(vk::BlendOp::ADD)
}

pub struct SceneTexture {
    pub extent: vk::Extent2D,
    device: Option<ash::Device>,
    image: vk::Image,
    memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub sampler: vk::Sampler,
    set: vk::DescriptorSet,
    pool: vk::DescriptorPool,
    pub allocation_bytes: u64,
    pub premultiplied: bool,
    pub effect: Option<SceneEffect>,
    pub effect_tail: Vec<SceneEffect>,
    puppet: Option<PuppetDraw>,
    clip: Option<clip::Shared>,
    text_mesh: Option<SceneMesh>,
    text_atlas: Option<Box<SceneTexture>>,
    empty_text: bool,
}

impl SceneTexture {
    pub fn draws_mesh(&self) -> bool {
        self.effect.as_ref().is_some_and(|effect| {
            matches!(effect.geometry, EffectGeometry::Mesh(_) | EffectGeometry::Puppet(_))
        })
    }

    pub fn sample_set(&self) -> vk::DescriptorSet {
        self.set
    }

    pub fn use_scene_mesh(&mut self, mesh: &SceneMesh) {
        self.puppet = Some(PuppetDraw::of(mesh));
        self.clip.clone_from(&mesh.clip);
    }

    pub fn vacant() -> Self {
        Self {
            extent: vk::Extent2D::default(),
            device: None,
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            sampler: vk::Sampler::null(),
            set: vk::DescriptorSet::null(),
            pool: vk::DescriptorPool::null(),
            allocation_bytes: 0,
            premultiplied: false,
            effect: None,
            effect_tail: Vec::new(),
            puppet: None,
            clip: None,
            text_mesh: None,
            text_atlas: None,
            empty_text: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SceneConfig {
    pub format: vk::Format,
    pub depth: Option<super::DepthOrder>,
}

impl Default for SceneConfig {
    fn default() -> Self {
        Self { format: vk::Format::R8G8B8A8_UNORM, depth: None }
    }
}

struct SceneDepth {
    attachment: super::depth::Attachment,
    framebuffer: vk::Framebuffer,
    passes: super::pipelines::DepthPasses,
    initialized: std::cell::Cell<bool>,
    color_append: vk::RenderPass,
    order: super::DepthOrder,
}

pub struct SceneTarget {
    promoted: Option<Box<SceneDepth>>,
    device: Option<std::sync::Arc<ash::Device>>,
    depth: Option<super::depth::Attachment>,
    msaa: Option<super::images::Storage>,
    pub samples: vk::SampleCountFlags,
    pub(super) depth_order: Option<super::DepthOrder>,
    pub viewport: Option<super::SceneViewport>,
    pub(super) image: vk::Image,
    pub(super) memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub sampler: vk::Sampler,
    pub(super) framebuffer: vk::Framebuffer,
    pub extent: vk::Extent2D,
    pub allocation_bytes: u64,
    pub repeat: bool,
    pub format: vk::Format,
    pub(super) render_pass: vk::RenderPass,
}

#[derive(Clone, Copy)]
pub struct SceneTargetSpec {
    pub depth_attachment: bool,
    pub extent: vk::Extent2D,
    pub format: vk::Format,
    pub repeat: bool,
    pub samples: vk::SampleCountFlags,
}

#[derive(Clone, Copy)]
struct PuppetDraw {
    cull: bool,
    buffer: vk::Buffer,
    index_offset: vk::DeviceSize,
    count: u32,
    index_type: vk::IndexType,
}

impl PuppetDraw {
    fn of(mesh: &SceneMesh) -> Self {
        Self {
            cull: mesh.cull,
            buffer: mesh.buffer,
            index_offset: mesh.index_offset,
            count: mesh.drawn,
            index_type: mesh.index_type,
        }
    }
}

pub enum SceneIndices<'a> {
    U16(&'a [u16]),
    U32(&'a [u32]),
}

impl SceneIndices<'_> {
    #[must_use]
    pub fn index_type(&self) -> vk::IndexType {
        match self {
            Self::U16(_) => vk::IndexType::UINT16,
            Self::U32(_) => vk::IndexType::UINT32,
        }
    }
}

impl<'a> From<&'a paper_scene::model::mesh::Indices> for SceneIndices<'a> {
    fn from(indices: &'a paper_scene::model::mesh::Indices) -> Self {
        match indices {
            paper_scene::model::mesh::Indices::U16(indices) => Self::U16(indices),
            paper_scene::model::mesh::Indices::U32(indices) => Self::U32(indices),
        }
    }
}

pub struct SceneMesh {
    pub cull: bool,
    clip: Option<clip::Shared>,
    drawn: u32,
    allocation_bytes: u64,
    device: Option<ash::Device>,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    mapped: *mut f32,
    vertices: usize,
    vertex_width: usize,
    index_offset: vk::DeviceSize,
    index_count: u32,
    index_type: vk::IndexType,
}

impl SceneMesh {
    fn write_mapped(&self, at: usize, bytes: &[u8]) {
        assert!(!self.mapped.is_null());
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.mapped.cast::<u8>().add(at),
                bytes.len(),
            );
        }
    }

    fn write_float(&self, slot: usize, value: f32) {
        assert!(slot < self.vertices * self.vertex_width);
        self.write_mapped(slot * size_of::<f32>(), &value.to_ne_bytes());
    }

    fn write_indices(&self, bytes: &[u8]) {
        assert!(self.index_type == vk::IndexType::UINT16);
        assert!(bytes.len() <= self.index_count as usize * size_of::<u16>());
        self.write_mapped(self.index_offset as usize, bytes);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum SceneBlend {
    Alpha,
    Add,
    Copy,
    Screen,
    CopyRgb,
    AlphaCoverage,
}

impl SceneBlend {
    pub(super) const ALL: [Self; 6] =
        [Self::Alpha, Self::Add, Self::Copy, Self::Screen, Self::CopyRgb, Self::AlphaCoverage];
}

pub struct LayerProjection {
    x: [f32; 3],
    y: [f32; 3],
    z: [f32; 3],
    w: [f32; 3],
}

impl From<paper_scene::matrix::Mat4> for LayerProjection {
    fn from(matrix: paper_scene::matrix::Mat4) -> Self {
        const COLUMNS: [usize; 3] = [0, 1, 3];
        const STRIDE: usize = 4;
        let [x, y, z, w] =
            std::array::from_fn(|row| COLUMNS.map(|column| matrix[column * STRIDE + row]));
        Self { x, y, z, w }
    }
}

impl LayerProjection {
    pub fn pack(self) -> [[f32; 4]; 3] {
        [
            [self.x[0], self.x[1], self.z[0], self.x[2]],
            [self.y[0], self.y[1], self.z[1], self.y[2]],
            [self.w[0], self.w[1], self.z[2], self.w[2]],
        ]
    }
}

#[derive(Clone, PartialEq)]
pub struct SceneQuad {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub uv_axes: Option<[f32; 4]>,
    pub uv_padding: bool,
    pub tint: [f32; 4],
    pub angle: f32,
    pub projection: Option<[[f32; 4]; 3]>,
    pub order_bias: i8,
    pub texture: usize,
    pub blend: SceneBlend,
}

pub struct SceneLayers<'a, 'b> {
    pub quads: &'a [SceneQuad],
    pub textures: &'a [SceneTexture],
    pub particles: &'a [ParticleDraw<'b>],
    pub plain: &'a [usize],
}

pub struct ParticleDraw<'a> {
    pub after: usize,
    pub pipeline: &'a crate::vk::EffectPipeline,
    pub vertices: vk::Buffer,
    pub indices: vk::Buffer,
    pub index_count: u32,
    pub grab: Option<&'a SceneTarget>,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct LayerPush {
    rect: [f32; 4],
    uv: [f32; 4],
    tint: [f32; 4],
    canvas: [f32; 2],
    angle: f32,
    pad: f32,
    projection: [[f32; 4]; 3],
    uv_axes: [f32; 4],
}

pub(super) fn effect_set(
    device: &ash::Device,
    pools: &mut Vec<vk::DescriptorPool>,
    layouts: &[vk::DescriptorSetLayout],
) -> Result<(vk::DescriptorPool, vk::DescriptorSet)> {
    if let Some(found) = pooled_set(device, pools, layouts)? {
        return Ok(found);
    }
    let sizes = [
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(EFFECT_POOL_SETS),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(EFFECT_POOL_SAMPLERS),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(EFFECT_POOL_SAMPLERS),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::SAMPLER)
            .descriptor_count(EFFECT_POOL_SAMPLERS),
    ];
    let pool = unsafe {
        device.create_descriptor_pool(
            &vk::DescriptorPoolCreateInfo::default()
                .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                .max_sets(EFFECT_POOL_SETS)
                .pool_sizes(&sizes),
            None,
        )?
    };
    match pooled_set(device, std::slice::from_ref(&pool), layouts) {
        Ok(Some(found)) => {
            pools.push(pool);
            Ok(found)
        }
        failed => {
            unsafe { device.destroy_descriptor_pool(pool, None) };
            failed?.context("fresh effect descriptor pool exhausted")
        }
    }
}

fn pooled_set(
    device: &ash::Device,
    pools: &[vk::DescriptorPool],
    layouts: &[vk::DescriptorSetLayout],
) -> Result<Option<(vk::DescriptorPool, vk::DescriptorSet)>> {
    for &pool in pools {
        let allocation = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(layouts),
            )
        };
        match allocation {
            Ok(sets) => return Ok(Some((pool, sets[0]))),
            Err(vk::Result::ERROR_OUT_OF_POOL_MEMORY | vk::Result::ERROR_FRAGMENTED_POOL) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(None)
}

fn timestamp_delta_ns(start: u64, end: u64, valid_bits: u32, period: f64) -> u64 {
    let mask = if valid_bits >= 64 { u64::MAX } else { (1u64 << valid_bits) - 1 };
    let ticks = end.wrapping_sub(start) & mask;
    (ticks as f64 * period).round() as u64
}

impl SceneTarget {
    fn draw_viewport(
        &self,
        hlsl: bool,
        coordinates: super::effect::EffectCoordinates,
    ) -> vk::Viewport {
        let mut viewport = super::effect::viewport(self.extent, hlsl);
        if coordinates == super::effect::EffectCoordinates::Viewport {
            viewport.y = self.extent.height as f32;
            viewport.height = -(self.extent.height as f32);
            return viewport;
        }
        if let Some(view) = self.viewport {
            let [x, y, width, height] = view.authored_viewport();
            viewport.x = x;
            viewport.width = width;
            if viewport.height < 0.0 {
                viewport.y = y + height;
                viewport.height = -height;
            } else {
                viewport.y = y;
                viewport.height = height;
            }
        }
        viewport
    }

    pub(super) fn framebuffer(&self) -> ash::vk::Framebuffer {
        self.framebuffer
    }

    pub fn grab(&self) -> SceneGrab {
        SceneGrab { image: self.image, extent: self.extent }
    }
}

impl Renderer {
    pub fn create_scene_mesh<const WIDTH: usize>(
        &self,
        vertices: impl Iterator<Item = [f32; WIDTH]>,
        indices: SceneIndices<'_>,
    ) -> Result<SceneMesh> {
        if WIDTH != PUPPET_VERTEX_FLOATS && WIDTH != text::TEXT_VERTEX_FLOATS {
            return Err(anyhow!("unsupported scene vertex width {WIDTH}"));
        }
        let vertices: Vec<_> = vertices.collect();
        let vertex_bytes = std::mem::size_of_val(vertices.as_slice()) as u64;
        let index_type = indices.index_type();
        let (indices, index_bytes, index_count) = match indices {
            SceneIndices::U16(indices) => (
                indices.as_ptr().cast::<u8>(),
                std::mem::size_of_val(indices) as u64,
                indices.len(),
            ),
            SceneIndices::U32(indices) => (
                indices.as_ptr().cast::<u8>(),
                std::mem::size_of_val(indices) as u64,
                indices.len(),
            ),
        };
        let size = vertex_bytes
            .checked_add(index_bytes)
            .ok_or_else(|| anyhow!("puppet buffer size overflow"))?;
        let mut output = SceneMesh {
            cull: false,
            clip: None,
            drawn: u32::try_from(index_count)?,
            allocation_bytes: 0,
            device: Some(self.device.clone()),
            buffer: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
            mapped: std::ptr::null_mut(),
            vertices: vertices.len(),
            vertex_width: WIDTH,
            index_offset: vertex_bytes,
            index_count: u32::try_from(index_count)?,
            index_type,
        };
        #[cfg(test)]
        LIVE_MESHES.set(LIVE_MESHES.get() + 1);
        unsafe {
            output.buffer = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let reqs = self.device.get_buffer_memory_requirements(output.buffer);
            output.allocation_bytes = reqs.size;
            output.memory = self.device.allocate_memory(&self.host_memory_info(reqs)?, None)?;
            self.device.bind_buffer_memory(output.buffer, output.memory, 0)?;
            let ptr =
                self.device.map_memory(output.memory, 0, size, vk::MemoryMapFlags::empty())?;
            output.mapped = ptr.cast();
            std::ptr::copy_nonoverlapping(
                vertices.as_ptr().cast::<u8>(),
                ptr.cast::<u8>(),
                vertex_bytes as usize,
            );
            std::ptr::copy_nonoverlapping(
                indices,
                ptr.cast::<u8>().add(vertex_bytes as usize),
                index_bytes as usize,
            );
        }
        Ok(output)
    }

    pub fn update_scene_mesh(
        &self,
        mesh: &SceneMesh,
        positions: &[[f32; 3]],
        alphas: &[f32],
        size: (f32, f32),
    ) -> Result<()> {
        if mesh.vertex_width != PUPPET_VERTEX_FLOATS {
            return Err(anyhow!("puppet update requires puppet vertices"));
        }
        if positions.len() != mesh.vertices {
            return Err(anyhow!("puppet position count {} != {}", positions.len(), mesh.vertices));
        }
        if !alphas.is_empty() && alphas.len() != mesh.vertices {
            return Err(anyhow!("puppet alpha count {} != {}", alphas.len(), mesh.vertices));
        }
        let width = size.0.abs().max(1.0);
        let height = size.1.abs().max(1.0);
        for (index, position) in positions.iter().enumerate() {
            let vertex = index * PUPPET_VERTEX_FLOATS;
            let alpha = alphas.get(index).copied().unwrap_or(1.0);
            mesh.write_float(vertex, position[0] / width);
            mesh.write_float(vertex + 1, -position[1] / height);
            mesh.write_float(vertex + PUPPET_ALPHA_FLOAT, alpha);
        }
        if let Some(clip) = &mesh.clip {
            clip.borrow_mut().update_bounds(|index| {
                let position = positions.get(usize::from(index))?;
                Some([position[0] / width, -position[1] / height])
            });
        }
        Ok(())
    }

    pub fn update_scene_indices(&self, mesh: &SceneMesh, bytes: &[u8]) -> Result<()> {
        if mesh.index_type != vk::IndexType::UINT16 {
            return Err(anyhow!("puppet index update requires u16 indices"));
        }
        if !bytes.len().is_multiple_of(size_of::<u16>())
            || bytes.len() > mesh.index_count as usize * size_of::<u16>()
        {
            return Err(anyhow!("puppet index bytes {} != mesh", bytes.len()));
        }
        mesh.write_indices(bytes);
        Ok(())
    }

    #[cfg(test)]
    pub fn render_scene_mesh(
        &mut self,
        target: &SceneTarget,
        texture: &SceneTexture,
        mesh: &SceneMesh,
    ) -> Result<()> {
        let mut slot = self.create_view_slot()?;
        slot.sampler = texture.sampler;
        slot.view = texture.view;
        slot.use_scene_mesh(mesh);
        self.point_slot_at(&slot, slot.view);
        let (width, height) = (target.extent.width as f32, target.extent.height as f32);
        let quad = SceneQuad {
            rect: [width * 0.5, height * 0.5, width, height],
            uv: [0.0, 0.0, 1.0, 1.0],
            uv_axes: None,
            uv_padding: false,
            tint: [1.0; 4],
            angle: 0.0,
            projection: None,
            order_bias: 0,
            texture: 0,
            blend: SceneBlend::Alpha,
        };
        let rendered = self.render_scene(target, [0.0; 4], &[quad], std::slice::from_ref(&slot));
        self.destroy_scene_texture(slot);
        rendered
    }

    pub(super) fn device_local_index(&self, reqs: vk::MemoryRequirements) -> Result<u32> {
        let props = unsafe { self.instance.get_physical_device_memory_properties(self.phys) };
        (0..props.memory_type_count)
            .find(|&i| {
                reqs.memory_type_bits & (1 << i) != 0
                    && props.memory_types[i as usize]
                        .property_flags
                        .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
            })
            .or_else(|| {
                (0..props.memory_type_count).find(|&i| reqs.memory_type_bits & (1 << i) != 0)
            })
            .ok_or_else(|| anyhow!("no device-local memory type"))
    }

    pub(super) fn host_visible_index(&self, reqs: vk::MemoryRequirements) -> Result<u32> {
        let props = unsafe { self.instance.get_physical_device_memory_properties(self.phys) };
        (0..props.memory_type_count)
            .find(|&i| {
                reqs.memory_type_bits & (1 << i) != 0
                    && props.memory_types[i as usize].property_flags.contains(
                        vk::MemoryPropertyFlags::HOST_VISIBLE
                            | vk::MemoryPropertyFlags::HOST_COHERENT,
                    )
            })
            .ok_or_else(|| anyhow!("no host-visible memory type"))
    }

    pub(super) fn host_memory_info(
        &self,
        reqs: vk::MemoryRequirements,
    ) -> Result<vk::MemoryAllocateInfo<'static>> {
        Ok(vk::MemoryAllocateInfo::default()
            .allocation_size(reqs.size)
            .memory_type_index(self.host_visible_index(reqs)?))
    }

    pub fn ensure_scene_pool(&mut self, textures: u32) -> Result<()> {
        if !self.scene_pools.is_empty() {
            return Ok(());
        }
        self.grow_scene_pool(textures)
    }

    fn grow_scene_pool(&mut self, textures: u32) -> Result<()> {
        let capacity = textures.max(SCENE_POOL_SETS);
        let sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(capacity * 2)];
        let pool = unsafe {
            self.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                    .max_sets(capacity)
                    .pool_sizes(&sizes),
                None,
            )?
        };
        self.scene_pools.push(pool);
        Ok(())
    }

    fn scene_set(&mut self) -> Result<(vk::DescriptorPool, vk::DescriptorSet)> {
        let layouts = [self.desc_layout];
        if let Some(found) = pooled_set(&self.device, &self.scene_pools, &layouts)? {
            return Ok(found);
        }
        self.grow_scene_pool(SCENE_POOL_SETS)?;
        let pool = *self.scene_pools.last().unwrap();
        let sets = unsafe {
            self.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&layouts),
            )?
        };
        Ok((pool, sets[0]))
    }

    pub fn create_scene_texture(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<SceneTexture> {
        self.create_scene_texture_opts(width, height, rgba, true, false)
    }

    pub fn create_scene_texture_opts(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
        clamp: bool,
        nearest: bool,
    ) -> Result<SceneTexture> {
        let expect = width as usize * height as usize * 4;
        if rgba.len() < expect {
            return Err(anyhow!("texture payload {} < {expect}", rgba.len()));
        }
        let pixels = Pixels::rgba(width, height, rgba[..expect].to_vec());
        self.create_scene_texture_pixels(&pixels, clamp, nearest, MipSampling::Disabled)
    }

    pub fn create_scene_texture_pixels(
        &mut self,
        pixels: &Pixels,
        clamp: bool,
        nearest: bool,
        sampling: MipSampling,
    ) -> Result<SceneTexture> {
        let decoded;
        let pixels = if pixels.format.compressed() && !self.bc_supported {
            decoded = pixels.decompressed().ok_or_else(|| anyhow!("tex bc decode"))?;
            &decoded
        } else {
            pixels
        };
        let (width, height) = (pixels.width(), pixels.height());
        if width == 0 || height == 0 || pixels.levels.is_empty() {
            return Err(anyhow!("empty texture"));
        }
        let format = match pixels.format {
            PixelFormat::Rgba8 => vk::Format::R8G8B8A8_UNORM,
            PixelFormat::Bc1 => vk::Format::BC1_RGBA_UNORM_BLOCK,
            PixelFormat::Bc2 => vk::Format::BC2_UNORM_BLOCK,
            PixelFormat::Bc3 => vk::Format::BC3_UNORM_BLOCK,
            PixelFormat::Bc7 => vk::Format::BC7_UNORM_BLOCK,
            PixelFormat::Bc6h => vk::Format::BC6H_UFLOAT_BLOCK,
            PixelFormat::R8 => vk::Format::R8_UNORM,
            PixelFormat::Rg8 => vk::Format::R8G8_UNORM,
            PixelFormat::Rgba16f => vk::Format::R16G16B16A16_SFLOAT,
            PixelFormat::Rgba16 => vk::Format::R16G16B16A16_UNORM,
            PixelFormat::Rgba16Snorm => vk::Format::R16G16B16A16_SNORM,
            PixelFormat::Rg16f => vk::Format::R16G16_SFLOAT,
            PixelFormat::R16f => vk::Format::R16_SFLOAT,
            PixelFormat::Rgb565 => vk::Format::R5G6B5_UNORM_PACK16,
            PixelFormat::Rgba1010102 => vk::Format::A2B10G10R10_UNORM_PACK32,
        };
        let (levels, generate) = sampling.mip_layout(pixels, self.tex_mips);
        let sampler = if sampling.uses_anisotropy(nearest, self.tex_mips) {
            if clamp { self.sampler_aniso } else { self.sampler_aniso_repeat }
        } else {
            match (clamp, nearest, levels > 1) {
                (true, false, false) => self.sampler,
                (false, false, false) => self.sampler_repeat,
                (true, true, false) => self.sampler_nearest,
                (false, true, false) => self.sampler_nearest_repeat,
                (true, false, true) => self.sampler_mip,
                (false, false, true) => self.sampler_mip_repeat,
                (true, true, true) => self.sampler_mip_nearest,
                (false, true, true) => self.sampler_mip_nearest_repeat,
            }
        };
        let mut offsets = Vec::with_capacity(levels as usize);
        let mut total = 0usize;
        for level in pixels.levels.iter().take(levels as usize) {
            total = (total + 15) & !15;
            offsets.push(total);
            total += level.data.len();
        }
        let mut usage = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST;
        if generate {
            usage |= vk::ImageUsageFlags::TRANSFER_SRC;
        }
        unsafe {
            let image = self
                .device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(format)
                        .extent(vk::Extent3D { width, height, depth: 1 })
                        .mip_levels(levels)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(usage)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .context("tex create_image")?;
            let reqs = self.device.get_image_memory_requirements(image);
            let memory = self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(reqs.size)
                    .memory_type_index(self.device_local_index(reqs)?),
                None,
            )?;
            self.device.bind_image_memory(image, memory, 0).context("tex bind")?;

            let staging = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(total.max(16) as u64)
                    .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let sreqs = self.device.get_buffer_memory_requirements(staging);
            let smem = self.device.allocate_memory(&self.host_memory_info(sreqs)?, None)?;
            self.device.bind_buffer_memory(staging, smem, 0)?;
            let ptr = self
                .device
                .map_memory(smem, 0, sreqs.size, vk::MemoryMapFlags::empty())
                .context("tex map")?
                .cast::<u8>();
            for (level, offset) in pixels.levels.iter().zip(&offsets) {
                std::ptr::copy_nonoverlapping(
                    level.data.as_ptr(),
                    ptr.add(*offset),
                    level.data.len(),
                );
            }
            self.device.unmap_memory(smem);

            self.begin_frame_cmd()?;
            let range = |base: u32, count: u32| vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: base,
                level_count: count,
                base_array_layer: 0,
                layer_count: 1,
            };
            let barrier = |old: vk::ImageLayout,
                           new: vk::ImageLayout,
                           src: vk::AccessFlags,
                           dst: vk::AccessFlags,
                           range: vk::ImageSubresourceRange| {
                vk::ImageMemoryBarrier::default()
                    .image(image)
                    .old_layout(old)
                    .new_layout(new)
                    .src_access_mask(src)
                    .dst_access_mask(dst)
                    .subresource_range(range)
            };
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier(
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::empty(),
                    vk::AccessFlags::TRANSFER_WRITE,
                    range(0, levels),
                )],
            );
            let regions: Vec<vk::BufferImageCopy> = pixels
                .levels
                .iter()
                .zip(&offsets)
                .enumerate()
                .map(|(index, (level, offset))| {
                    vk::BufferImageCopy::default()
                        .buffer_offset(*offset as u64)
                        .image_subresource(vk::ImageSubresourceLayers {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            mip_level: index as u32,
                            base_array_layer: 0,
                            layer_count: 1,
                        })
                        .image_extent(vk::Extent3D {
                            width: level.width,
                            height: level.height,
                            depth: 1,
                        })
                })
                .collect();
            self.device.cmd_copy_buffer_to_image(
                self.cmd,
                staging,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &regions,
            );
            if generate {
                self.record_mips(image, vk::Extent2D { width, height }, levels);
            } else {
                self.device.cmd_pipeline_barrier(
                    self.cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::VERTEX_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier(
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::AccessFlags::SHADER_READ,
                        range(0, levels),
                    )],
                );
            }
            self.device.end_command_buffer(self.cmd)?;
            let cmds = [self.cmd];
            self.submit_fenced(&[vk::SubmitInfo::default().command_buffers(&cmds)])
                .context("tex submit")?;
            self.wait_render().context("tex fence")?;
            self.device.destroy_buffer(staging, None);
            self.device.free_memory(smem, None);

            let view = self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(range(0, levels)),
                None,
            )?;
            let (pool, set) = self.scene_set()?;
            let infos = [vk::DescriptorImageInfo::default()
                .sampler(sampler)
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            self.device.update_descriptor_sets(
                &[
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&infos),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&infos),
                ],
                &[],
            );
            #[cfg(test)]
            LIVE_TEXTURES.set(LIVE_TEXTURES.get() + 1);
            Ok(SceneTexture {
                extent: vk::Extent2D { width, height },
                device: Some(self.device.clone()),
                image,
                memory,
                view,
                sampler,
                set,
                pool,
                allocation_bytes: reqs.size,
                premultiplied: false,
                effect: None,
                effect_tail: Vec::new(),
                puppet: None,
                clip: None,
                text_mesh: None,
                text_atlas: None,
                empty_text: false,
            })
        }
    }

    pub fn create_video_texture_target(
        &mut self,
        width: u32,
        height: u32,
        clamp: bool,
        nearest: bool,
    ) -> Result<SceneTarget> {
        let mut target = self.create_scene_target_fmt(width, height, !clamp, self.format)?;
        target.sampler = match (clamp, nearest) {
            (true, false) => self.sampler,
            (false, false) => self.sampler_repeat,
            (true, true) => self.sampler_nearest,
            (false, true) => self.sampler_nearest_repeat,
        };
        Ok(target)
    }

    pub fn create_view_slot(&mut self) -> Result<SceneTexture> {
        let (pool, set) = self.scene_set()?;
        Ok(SceneTexture {
            extent: vk::Extent2D::default(),
            device: Some(self.device.clone()),
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            sampler: self.sampler,
            set,
            pool,
            allocation_bytes: 0,
            premultiplied: false,
            effect: None,
            effect_tail: Vec::new(),
            puppet: None,
            clip: None,
            text_mesh: None,
            text_atlas: None,
            empty_text: false,
        })
    }

    pub fn point_slot_at(&self, slot: &SceneTexture, view: vk::ImageView) {
        let infos = [vk::DescriptorImageInfo::default()
            .sampler(slot.sampler)
            .image_view(view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        unsafe {
            self.device.update_descriptor_sets(
                &[
                    vk::WriteDescriptorSet::default()
                        .dst_set(slot.set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&infos),
                    vk::WriteDescriptorSet::default()
                        .dst_set(slot.set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&infos),
                ],
                &[],
            );
        }
    }

    pub fn destroy_scene_texture(&self, mut texture: SceneTexture) {
        texture.device = None;
        texture.release(&self.device);
    }

    pub fn scene_format(&self) -> vk::Format {
        self.scene_config.format
    }

    pub fn set_scene_multisample(&mut self, enabled: bool) {
        self.scene_multisample = enabled;
    }

    pub fn scene_sample_count(&self) -> vk::SampleCountFlags {
        if !self.scene_multisample
            || std::env::var(SCENE_MSAA_ENV).as_deref() == Ok(SCENE_MSAA_DISABLED)
        {
            return vk::SampleCountFlags::TYPE_1;
        }
        let limits = unsafe { self.instance.get_physical_device_properties(self.phys) }.limits;
        let samples = vk::SampleCountFlags::TYPE_4;
        let supported = |format, usage| {
            unsafe {
                self.instance.get_physical_device_image_format_properties(
                    self.phys,
                    format,
                    vk::ImageType::TYPE_2D,
                    vk::ImageTiling::OPTIMAL,
                    usage,
                    vk::ImageCreateFlags::empty(),
                )
            }
            .is_ok_and(|properties| properties.sample_counts.contains(samples))
        };
        let color = limits.framebuffer_color_sample_counts.contains(samples)
            && supported(self.scene_config.format, vk::ImageUsageFlags::COLOR_ATTACHMENT);
        let depth = self.scene_config.depth.is_none()
            || (limits.framebuffer_depth_sample_counts.contains(samples)
                && supported(super::depth::FORMAT, vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT));
        if color && depth { samples } else { vk::SampleCountFlags::TYPE_1 }
    }

    pub fn scene_pixel_bytes(&self) -> u64 {
        use paper_scene::effects::FboFormat;
        let color = if self.scene_config.format == vk::Format::R16G16B16A16_SFLOAT {
            FboFormat::Rgba16f.bytes_per_pixel()
        } else {
            FboFormat::Rgba8.bytes_per_pixel()
        };
        color + if self.scene_config.depth.is_some() { super::depth::PIXEL_BYTES } else { 0 }
    }

    pub fn configure_scene(&mut self, config: SceneConfig) -> Result<()> {
        let format = config.format;
        if !matches!(format, vk::Format::R8G8B8A8_UNORM | vk::Format::R16G16B16A16_SFLOAT) {
            return Err(anyhow!("unsupported scene format {}", format.as_raw()));
        }
        if self.scene_config != config && self.raster_pipelines[0].is_some() {
            return Err(anyhow!("scene configuration changed after pipeline creation"));
        }
        if config.depth.is_some() {
            self.check_scene_depth()?;
        }
        self.scene_config = config;
        Ok(())
    }

    fn check_scene_depth(&self) -> Result<()> {
        let supported = unsafe {
            self.instance.get_physical_device_format_properties(self.phys, super::depth::FORMAT)
        };
        if !supported
            .optimal_tiling_features
            .contains(vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT)
        {
            return Err(anyhow!("scene depth format unsupported"));
        }
        Ok(())
    }

    pub fn create_scene_target(&mut self, width: u32, height: u32) -> Result<SceneTarget> {
        self.create_scene_target_fmt(width, height, false, self.scene_config.format)
    }

    pub fn create_scene_target_repeat(&mut self, width: u32, height: u32) -> Result<SceneTarget> {
        self.create_scene_target_fmt(width, height, true, self.scene_config.format)
    }

    #[cfg(test)]
    pub fn scene_image_allocations(&self) -> usize {
        super::images::allocation_count()
    }

    #[cfg(test)]
    pub fn scene_texture_live_allocations() -> usize {
        LIVE_TEXTURES.get()
    }

    #[cfg(test)]
    pub fn scene_target_live_allocations() -> usize {
        LIVE_TARGETS.get()
    }

    #[cfg(test)]
    pub fn scene_mesh_live_allocations() -> usize {
        LIVE_MESHES.get()
    }

    pub fn scene_target_allocation_size(
        &self,
        width: u32,
        height: u32,
        format: vk::Format,
    ) -> Result<u64> {
        let color = scene_target_spec(width, height, format, vk::SampleCountFlags::TYPE_1)
            .allocation_size(self)?;
        let depth = if self.scene_config.depth.is_some() {
            super::depth::spec(width, height, vk::SampleCountFlags::TYPE_1).allocation_size(self)?
        } else {
            0
        };
        color.checked_add(depth).context("scene target allocation overflow")
    }

    pub fn create_scene_target_fmt(
        &mut self,
        width: u32,
        height: u32,
        repeat: bool,
        format: vk::Format,
    ) -> Result<SceneTarget> {
        self.create_scene_target_opts(SceneTargetSpec {
            depth_attachment: true,
            extent: vk::Extent2D { width, height },
            format,
            repeat,
            samples: vk::SampleCountFlags::TYPE_1,
        })
    }

    pub fn create_scene_target_opts(&mut self, spec: SceneTargetSpec) -> Result<SceneTarget> {
        let SceneTargetSpec { extent, format, repeat, samples, depth_attachment } = spec;
        let (width, height) = (extent.width, extent.height);
        if samples == vk::SampleCountFlags::TYPE_4 {
            self.ensure_scene_msaa_pipelines()?;
        }
        let depth_order = depth_attachment.then_some(self.scene_config.depth).flatten();
        let render_pass = if depth_attachment {
            self.scene_pass_for(format, samples)?
        } else {
            self.depth_passes_for(SceneConfig { format, depth: None }, samples)?.clear
        };
        let sampler = if repeat { self.sampler_repeat } else { self.sampler };
        let storage = super::images::Storage::new(
            self,
            &scene_target_spec(width, height, format, vk::SampleCountFlags::TYPE_1),
        )?;
        let mut msaa = None;
        let mut depth = None;
        let setup = (|| -> Result<vk::Framebuffer> {
            if samples != vk::SampleCountFlags::TYPE_1 {
                msaa = Some(super::images::Storage::new(
                    self,
                    &scene_target_spec(width, height, format, samples),
                )?);
            }
            if depth_order.is_some() {
                depth = Some(super::depth::attachment(self, width, height, samples)?);
            }
            let mut attachments = Vec::with_capacity(MAX_SCENE_ATTACHMENTS);
            attachments.push(msaa.as_ref().map_or(storage.view, |image| image.view));
            if msaa.is_some() {
                attachments.push(storage.view);
            }
            if let Some(depth) = &depth {
                attachments.push(depth.view);
            }
            unsafe {
                self.device
                    .create_framebuffer(
                        &vk::FramebufferCreateInfo::default()
                            .render_pass(render_pass)
                            .attachments(&attachments)
                            .width(width)
                            .height(height)
                            .layers(1),
                        None,
                    )
                    .map_err(Into::into)
            }
        })();
        let framebuffer = match setup {
            Ok(framebuffer) => framebuffer,
            Err(error) => {
                if let Some(depth) = depth {
                    depth.destroy(&self.device);
                }
                if let Some(msaa) = msaa {
                    msaa.destroy(&self.device);
                }
                storage.destroy(&self.device);
                return Err(error);
            }
        };
        let super::images::Storage { image, memory, view, bytes } = storage;
        let allocation_bytes = bytes
            + msaa.as_ref().map_or(0, |image| image.bytes)
            + depth.as_ref().map_or(0, |image| image.bytes);
        #[cfg(test)]
        LIVE_TARGETS.set(LIVE_TARGETS.get() + 1);
        Ok(SceneTarget {
            promoted: None,
            device: Some(self.resource_device()),
            depth,
            msaa,
            samples,
            depth_order,
            viewport: None,
            image,
            memory,
            view,
            sampler,
            framebuffer,
            extent,
            allocation_bytes,
            repeat,
            format,
            render_pass,
        })
    }

    pub fn enable_scene_depth(
        &mut self,
        target: &mut SceneTarget,
        order: super::DepthOrder,
    ) -> Result<()> {
        if target.depth.is_some() || target.promoted.is_some() {
            return Ok(());
        }
        self.wait_render()?;
        self.check_scene_depth()?;
        let passes = self.depth_passes_for(
            SceneConfig { format: target.format, depth: Some(order) },
            target.samples,
        )?;
        let color_append = self.scene_pass_append_for(target.format, target.samples)?;
        let attachment = super::depth::attachment(
            self,
            target.extent.width,
            target.extent.height,
            target.samples,
        )?;
        let mut attachments = Vec::with_capacity(MAX_SCENE_ATTACHMENTS);
        attachments.push(target.msaa.as_ref().map_or(target.view, |image| image.view));
        if target.msaa.is_some() {
            attachments.push(target.view);
        }
        attachments.push(attachment.view);
        let framebuffer = unsafe {
            self.device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(passes.clear)
                    .attachments(&attachments)
                    .width(target.extent.width)
                    .height(target.extent.height)
                    .layers(1),
                None,
            )
        };
        let framebuffer = match framebuffer {
            Ok(framebuffer) => framebuffer,
            Err(error) => {
                attachment.destroy(&self.device);
                return Err(error.into());
            }
        };
        target.allocation_bytes += attachment.bytes;
        target.promoted = Some(Box::new(SceneDepth {
            attachment,
            framebuffer,
            passes,
            initialized: std::cell::Cell::new(false),
            color_append,
            order,
        }));
        Ok(())
    }

    pub fn disable_scene_depth(&mut self, target: &mut SceneTarget) -> Result<()> {
        self.wait_render()?;
        target.release_depth(&self.device);
        Ok(())
    }

    pub fn destroy_scene_target(&self, target: SceneTarget) {
        let _ = self.wait_render();
        target.destroy(&self.device);
    }

    pub fn render_scene(
        &mut self,
        target: &SceneTarget,
        clear: [f32; 4],
        quads: &[SceneQuad],
        textures: &[SceneTexture],
    ) -> Result<()> {
        let canvas = [target.extent.width as f32, target.extent.height as f32];
        self.render_scene_with_canvas(target, canvas, clear, quads, textures, &[])
    }

    pub fn render_scene_with_canvas(
        &mut self,
        target: &SceneTarget,
        canvas: [f32; 2],
        clear: [f32; 4],
        quads: &[SceneQuad],
        textures: &[SceneTexture],
        particles: &[ParticleDraw<'_>],
    ) -> Result<()> {
        self.begin_scene_batch()?;
        self.record_scene_with_canvas(target, canvas, clear, quads, textures, particles);
        self.submit_scene_batch()
    }

    pub fn begin_scene_batch(&mut self) -> Result<()> {
        self.ensure_scene_pipelines()?;
        self.begin_frame_cmd()?;
        self.scene_gpu_time_ns = None;
        if self.scene_timestamp_active {
            let pool = self.scene_query_pool.expect("timestamp pool checked");
            unsafe {
                self.device.cmd_reset_query_pool(self.cmd, pool, 0, 2);
                self.device.cmd_write_timestamp(
                    self.cmd,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    pool,
                    0,
                );
            }
        }
        Ok(())
    }

    pub fn record_scene(
        &mut self,
        target: &SceneTarget,
        clear: [f32; 4],
        quads: &[SceneQuad],
        textures: &[SceneTexture],
    ) {
        let canvas = [target.extent.width as f32, target.extent.height as f32];
        self.record_scene_with_canvas(target, canvas, clear, quads, textures, &[]);
    }

    fn grab_scene(&self, target: &SceneTarget, grab: SceneGrab, load_pass: vk::RenderPass) {
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let layers = vk::ImageSubresourceLayers::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .layer_count(1);
        let extent = vk::Extent3D {
            width: target.extent.width.min(grab.extent.width),
            height: target.extent.height.min(grab.extent.height),
            depth: 1,
        };
        unsafe {
            self.device.cmd_end_render_pass(self.cmd);
            let to_src = vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(target.image)
                .subresource_range(range);
            let to_dst = vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_READ)
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(grab.image)
                .subresource_range(range);
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                    | vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_src, to_dst],
            );
            self.device.cmd_copy_image(
                self.cmd,
                target.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                grab.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageCopy::default()
                    .src_subresource(layers)
                    .dst_subresource(layers)
                    .extent(extent)],
            );
            let ready = vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(grab.image)
                .subresource_range(range);
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[ready],
            );
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(load_pass)
                    .framebuffer(target.framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: target.extent,
                    }),
                vk::SubpassContents::INLINE,
            );
            self.device.cmd_set_scissor(
                self.cmd,
                0,
                &[vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: target.extent }],
            );
        }
    }

    fn draw_puppet(&self, mesh: PuppetDraw) {
        self.draw_range(mesh, &(0..mesh.count));
    }

    fn draw_particles(&self, batch: &ParticleDraw<'_>, target: &SceneTarget) {
        unsafe {
            self.device.cmd_set_viewport(
                self.cmd,
                0,
                &[target
                    .draw_viewport(batch.pipeline.hlsl, super::effect::EffectCoordinates::Canvas)],
            );
            self.device.cmd_bind_pipeline(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                batch.pipeline.pipeline,
            );
            self.device.cmd_bind_descriptor_sets(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                batch.pipeline.layout,
                0,
                &[batch.pipeline.set],
                &[],
            );
            self.device.cmd_bind_vertex_buffers(self.cmd, 0, &[batch.vertices], &[0]);
            self.device.cmd_bind_index_buffer(self.cmd, batch.indices, 0, vk::IndexType::UINT32);
            self.device.cmd_draw_indexed(self.cmd, batch.index_count, 1, 0, 0, 0);
            self.device.cmd_set_viewport(
                self.cmd,
                0,
                &[target.draw_viewport(false, super::effect::EffectCoordinates::Canvas)],
            );
        }
    }

    pub fn record_scene_with_canvas(
        &mut self,
        target: &SceneTarget,
        canvas: [f32; 2],
        clear: [f32; 4],
        quads: &[SceneQuad],
        textures: &[SceneTexture],
        particles: &[ParticleDraw<'_>],
    ) {
        self.record_scene_pass_with_canvas(
            target,
            canvas,
            (target.render_pass, clear),
            SceneLayers { quads, textures, particles, plain: &[] },
        );
    }

    pub fn record_scene_layers(
        &mut self,
        target: &SceneTarget,
        canvas: [f32; 2],
        clear: Option<[f32; 4]>,
        layers: SceneLayers<'_, '_>,
    ) -> Result<()> {
        let pass = match clear {
            Some(_) => target.render_pass,
            None => self.scene_pass_append_for(target.format, target.samples)?,
        };
        self.record_scene_pass_with_canvas(
            target,
            canvas,
            (pass, clear.unwrap_or_default()),
            layers,
        );
        Ok(())
    }

    pub fn record_scene_append_with_canvas(
        &mut self,
        target: &SceneTarget,
        canvas: [f32; 2],
        clear: [f32; 4],
        quads: &[SceneQuad],
        textures: &[SceneTexture],
        particles: &[ParticleDraw<'_>],
    ) -> Result<()> {
        let pass = self.scene_pass_append_for(target.format, target.samples)?;
        self.record_scene_pass_with_canvas(
            target,
            canvas,
            (pass, clear),
            SceneLayers { quads, textures, particles, plain: &[] },
        );
        Ok(())
    }

    fn switch_scene_depth(&self, target: &SceneTarget, active: &mut bool, wanted: bool) -> bool {
        let Some(depth) = &target.promoted else {
            return false;
        };
        if *active == wanted {
            return false;
        }
        let (pass, framebuffer) = if wanted {
            (depth.passes.append, depth.framebuffer)
        } else {
            (depth.color_append, target.framebuffer)
        };
        unsafe {
            self.device.cmd_end_render_pass(self.cmd);
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(pass)
                    .framebuffer(framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent: target.extent,
                    }),
                vk::SubpassContents::INLINE,
            );
        }
        *active = wanted;
        true
    }

    fn record_scene_pass_with_canvas(
        &mut self,
        target: &SceneTarget,
        canvas: [f32; 2],
        (render_pass, clear): (vk::RenderPass, [f32; 4]),
        SceneLayers { quads, textures, particles, plain }: SceneLayers<'_, '_>,
    ) {
        let canvas = [canvas[0].max(1.0), canvas[1].max(1.0)];
        let mut next_particle = 0usize;
        let effect_of = |quad: &SceneQuad, texture: &'_ SceneTexture| {
            texture.effect.filter(|_| !plain.contains(&quad.texture))
        };
        let grabs_effect = quads.iter().any(|quad| {
            textures.get(quad.texture).is_some_and(|texture| {
                effect_of(quad, texture).is_some_and(|effect| effect.grab.is_some())
            })
        });
        let load_pass = if grabs_effect || particles.iter().any(|batch| batch.grab.is_some()) {
            match self.scene_pass_load_for(target.format, target.samples) {
                Ok(pass) => Some(pass),
                Err(err) => {
                    tracing::warn!("skwd-wall-vk: scene grab pass unavailable: {err:#}");
                    None
                }
            }
        } else {
            None
        };
        let mut depth_active = target.promoted.is_some();
        let (render_pass, framebuffer, order) = target.promoted.as_ref().map_or(
            (render_pass, target.framebuffer, target.depth_order),
            |depth| {
                let pass = if render_pass == target.render_pass {
                    depth.passes.clear
                } else if depth.initialized.get() {
                    depth.passes.append
                } else {
                    depth.passes.initialize
                };
                depth.initialized.set(true);
                (pass, depth.framebuffer, Some(depth.order))
            },
        );
        unsafe {
            let clear_value = clear_values(clear, order);
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(render_pass)
                    .framebuffer(framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: target.extent,
                    })
                    .clear_values(&clear_value[..target.clear_value_count()]),
                vk::SubpassContents::INLINE,
            );
            self.device.cmd_set_viewport(
                self.cmd,
                0,
                &[target.draw_viewport(false, super::effect::EffectCoordinates::Canvas)],
            );
            self.device.cmd_set_scissor(
                self.cmd,
                0,
                &[vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: target.extent }],
            );
            let mut bound_blend = None;
            for (index, quad) in quads.iter().enumerate() {
                while next_particle < particles.len() && particles[next_particle].after <= index {
                    let batch = &particles[next_particle];
                    self.switch_scene_depth(target, &mut depth_active, false);
                    if let (Some(grab), Some(load_pass)) = (batch.grab, load_pass) {
                        self.grab_scene(target, grab.grab(), load_pass);
                    }
                    self.draw_particles(batch, target);
                    bound_blend = None;
                    next_particle += 1;
                }
                let Some(texture) = textures.get(quad.texture) else {
                    continue;
                };
                if texture.set == vk::DescriptorSet::null() {
                    continue;
                }
                let effect = effect_of(quad, texture);
                if self.switch_scene_depth(
                    target,
                    &mut depth_active,
                    effect.as_ref().is_some_and(|effect| effect.depth_pass),
                ) {
                    bound_blend = None;
                }
                if let Some(effect) = &effect {
                    if quad.tint[ALPHA_COMPONENT] > 0.0 {
                        if let (Some(grab), Some(load_pass)) = (effect.grab, load_pass) {
                            self.grab_scene(target, grab, load_pass);
                        }
                        for effect in std::iter::once(effect).chain(&texture.effect_tail) {
                            if let Some(projection) = effect.projection {
                                let matrix = projection.at(
                                    [quad.rect[0], quad.rect[1]],
                                    canvas,
                                    effect.hlsl && super::effect::d3d_clip_enabled(),
                                );
                                std::ptr::copy_nonoverlapping(
                                    matrix.as_ptr().cast::<u8>(),
                                    effect.uniforms.add(projection.offset),
                                    std::mem::size_of_val(&matrix),
                                );
                            }
                            self.device.cmd_set_viewport(
                                self.cmd,
                                0,
                                &[target.draw_viewport(effect.hlsl, effect.coordinates)],
                            );
                            self.device.cmd_bind_pipeline(
                                self.cmd,
                                vk::PipelineBindPoint::GRAPHICS,
                                if target.samples == vk::SampleCountFlags::TYPE_1
                                    && effect.single_sample != vk::Pipeline::null()
                                {
                                    effect.single_sample
                                } else {
                                    effect.pipeline
                                },
                            );
                            self.device.cmd_bind_descriptor_sets(
                                self.cmd,
                                vk::PipelineBindPoint::GRAPHICS,
                                effect.layout,
                                0,
                                &[effect.set],
                                &[],
                            );
                            match effect.geometry {
                                EffectGeometry::Quad { vertices, offset } => {
                                    self.device.cmd_bind_vertex_buffers(
                                        self.cmd,
                                        0,
                                        &[vertices],
                                        &[offset],
                                    );
                                    self.device.cmd_draw(self.cmd, QUAD_VERTEX_COUNT, 1, 0, 0);
                                }
                                EffectGeometry::Mesh(mesh) => {
                                    self.device.cmd_bind_vertex_buffers(
                                        self.cmd,
                                        0,
                                        &[mesh.vertices],
                                        &[0],
                                    );
                                    self.device.cmd_bind_index_buffer(
                                        self.cmd,
                                        mesh.indices,
                                        0,
                                        mesh.index_type,
                                    );
                                    self.device.cmd_draw_indexed(self.cmd, mesh.count, 1, 0, 0, 0);
                                }
                                EffectGeometry::Puppet(mesh) => self.draw_puppet(mesh),
                            }
                            self.device.cmd_set_viewport(
                                self.cmd,
                                0,
                                &[target.draw_viewport(
                                    false,
                                    super::effect::EffectCoordinates::Canvas,
                                )],
                            );
                        }
                        bound_blend = None;
                    }
                    continue;
                }
                if texture.empty_text {
                    continue;
                }
                let draw_kind = (
                    quad.blend,
                    texture.puppet.as_ref().map(|puppet| puppet.cull),
                    texture.text_atlas.is_some(),
                );
                if bound_blend != Some(draw_kind) {
                    let set = self.raster_pipelines
                        [usize::from(target.samples != vk::SampleCountFlags::TYPE_1)]
                    .as_ref()
                    .expect("scene raster pipelines");
                    self.device.cmd_bind_pipeline(
                        self.cmd,
                        vk::PipelineBindPoint::GRAPHICS,
                        if texture.text_atlas.is_some() {
                            set.text[quad.blend as usize]
                        } else if let Some(puppet) = &texture.puppet {
                            set.puppet_for(puppet.cull)[quad.blend as usize]
                        } else {
                            set.layers[quad.blend as usize]
                        },
                    );
                    bound_blend = Some(draw_kind);
                }
                self.device.cmd_bind_descriptor_sets(
                    self.cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipeline_layout_layer,
                    0,
                    &[texture.text_atlas.as_ref().map_or(texture.set, |atlas| atlas.set)],
                    &[],
                );
                let push = LayerPush {
                    rect: quad.rect,
                    uv: quad.uv,
                    tint: quad.tint,
                    canvas,
                    angle: quad.angle,
                    pad: ((u32::from(quad.blend == SceneBlend::Screen) * ALPHA_PREMULTIPLY)
                        | (u32::from(texture.premultiplied) * ALPHA_UNPREMULTIPLY)
                        | (u32::from(quad.uv_axes.is_some()) * UV_TRANSFORM)
                        | (u32::from(quad.uv_padding) * UV_PADDING)
                        | (u32::from(texture.text_mesh.is_some()) * TEXT_MASK))
                        as f32,
                    projection: quad.projection.unwrap_or([[0.0; 4]; 3]),
                    uv_axes: quad.uv_axes.unwrap_or(NO_UV_TRANSFORM),
                };
                self.device.cmd_push_constants(
                    self.cmd,
                    self.pipeline_layout_layer,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    std::slice::from_raw_parts(
                        (&raw const push).cast(),
                        std::mem::size_of::<LayerPush>(),
                    ),
                );
                if let (Some(mesh), Some(clip)) = (texture.puppet, &texture.clip) {
                    let pipeline = self.raster_pipelines
                        [usize::from(target.samples != vk::SampleCountFlags::TYPE_1)]
                    .as_ref()
                    .expect("scene raster pipelines")
                    .puppet_for(mesh.cull)[quad.blend as usize];
                    let draw = clip::ClipDraw {
                        target,
                        set: texture.set,
                        push: &push,
                        blend: quad.blend,
                        pipeline,
                    };
                    self.draw_clipped(&draw, mesh, &clip.borrow());
                } else if let Some(mesh) = texture.puppet {
                    self.draw_puppet(mesh);
                } else {
                    self.device.cmd_draw(self.cmd, QUAD_VERTEX_COUNT, 1, 0, 0);
                }
            }
            for batch in &particles[next_particle..] {
                self.switch_scene_depth(target, &mut depth_active, false);
                if let (Some(grab), Some(load_pass)) = (batch.grab, load_pass) {
                    self.grab_scene(target, grab.grab(), load_pass);
                }
                self.draw_particles(batch, target);
            }
            self.device.cmd_end_render_pass(self.cmd);
        }
    }

    pub fn submit_scene_batch(&mut self) -> Result<()> {
        unsafe {
            if self.scene_timestamp_active {
                self.device.cmd_write_timestamp(
                    self.cmd,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    self.scene_query_pool.expect("timestamp pool checked"),
                    1,
                );
            }
            self.device.end_command_buffer(self.cmd)?;
            let cmds = [self.cmd];
            self.submit_fenced(&[vk::SubmitInfo::default().command_buffers(&cmds)])?;
            self.wait_render()?;
            if self.scene_timestamp_active {
                let mut values = [0u64; 2];
                self.device.get_query_pool_results(
                    self.scene_query_pool.expect("timestamp pool checked"),
                    0,
                    &mut values,
                    vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
                )?;
                let elapsed = timestamp_delta_ns(
                    values[0],
                    values[1],
                    self.scene_timestamp_valid_bits,
                    self.scene_timestamp_period,
                );
                self.scene_gpu_time_ns =
                    Some(self.scene_gpu_time_ns.unwrap_or(0).saturating_add(elapsed));
            }
        }
        Ok(())
    }

    pub fn flush_scene_batch(&mut self) -> Result<()> {
        self.submit_scene_batch()?;
        let elapsed = self.scene_gpu_time_ns;
        self.begin_scene_batch()?;
        self.scene_gpu_time_ns = elapsed;
        Ok(())
    }

    pub fn take_scene_gpu_time_ns(&mut self) -> Option<u64> {
        self.scene_gpu_time_ns.take()
    }
}

#[cfg(test)]
#[path = "scene/tests.rs"]
mod tests;

impl Renderer {
    pub fn color_target_format(&self, format: vk::Format, fallback: vk::Format) -> vk::Format {
        let features =
            unsafe { self.instance.get_physical_device_format_properties(self.phys, format) }
                .optimal_tiling_features;
        let needed = vk::FormatFeatureFlags::COLOR_ATTACHMENT
            | vk::FormatFeatureFlags::COLOR_ATTACHMENT_BLEND
            | vk::FormatFeatureFlags::SAMPLED_IMAGE;
        if features.contains(needed) { format } else { fallback }
    }

    pub fn read_scene_target(&mut self, target: &SceneTarget) -> Result<(u32, u32, Vec<u8>)> {
        let converted = if matches!(
            target.format,
            vk::Format::R16G16B16A16_SFLOAT | vk::Format::R16G16B16A16_UNORM
        ) {
            Some(self.create_scene_target_fmt(
                target.extent.width,
                target.extent.height,
                false,
                vk::Format::R8G8B8A8_UNORM,
            )?)
        } else {
            None
        };
        let result = self.read_scene_pixels(target, converted.as_ref());
        if let Some(converted) = converted {
            self.destroy_scene_target(converted);
        }
        result
    }

    fn read_scene_pixels(
        &mut self,
        target: &SceneTarget,
        converted: Option<&SceneTarget>,
    ) -> Result<(u32, u32, Vec<u8>)> {
        let (width, height) = (target.extent.width, target.extent.height);
        let size = u64::from(width) * u64::from(height) * RGBA_CHANNELS as u64;
        let readback = self.create_readback_buf(size)?;
        unsafe {
            self.begin_frame_cmd()?;
            let range = SCENE_COLOR_RANGE;
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(target.image)
                    .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::SHADER_READ)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .subresource_range(range)],
            );
            if let Some(converted) = converted {
                self.record_readback_conversion(target, converted);
            }
            self.device.cmd_copy_image_to_buffer(
                self.cmd,
                converted.unwrap_or(target).image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                readback.buffer,
                &[vk::BufferImageCopy::default()
                    .image_subresource(SCENE_COLOR_LAYERS)
                    .image_extent(vk::Extent3D { width, height, depth: 1 })],
            );
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(target.image)
                    .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .subresource_range(range)],
            );
            self.device.end_command_buffer(self.cmd)?;
            let cmds = [self.cmd];
            self.submit_fenced(&[vk::SubmitInfo::default().command_buffers(&cmds)])?;
            self.wait_render()?;
            let mut out = vec![0u8; size as usize];
            std::ptr::copy_nonoverlapping(readback.ptr, out.as_mut_ptr(), size as usize);
            Ok((width, height, out))
        }
    }

    fn record_readback_conversion(&self, source: &SceneTarget, target: &SceneTarget) {
        let offsets = [
            vk::Offset3D::default(),
            vk::Offset3D { x: source.extent.width as i32, y: source.extent.height as i32, z: 1 },
        ];
        unsafe {
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(target.image)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .subresource_range(SCENE_COLOR_RANGE)],
            );
            self.device.cmd_blit_image(
                self.cmd,
                source.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                target.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageBlit::default()
                    .src_subresource(SCENE_COLOR_LAYERS)
                    .src_offsets(offsets)
                    .dst_subresource(SCENE_COLOR_LAYERS)
                    .dst_offsets(offsets)],
                vk::Filter::NEAREST,
            );
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(target.image)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .subresource_range(SCENE_COLOR_RANGE)],
            );
        }
    }
}

pub(super) fn clear_values(
    color: [f32; 4],
    order: Option<super::DepthOrder>,
) -> [vk::ClearValue; MAX_SCENE_ATTACHMENTS] {
    let depth = vk::ClearValue {
        depth_stencil: vk::ClearDepthStencilValue {
            depth: order.unwrap_or_default().clear(),
            stencil: 0,
        },
    };
    [vk::ClearValue { color: vk::ClearColorValue { float32: color } }, depth, depth]
}

#[cfg(test)]
thread_local! {
    static LIVE_TEXTURES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LIVE_TARGETS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LIVE_MESHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl SceneTarget {
    pub fn has_depth(&self) -> bool {
        self.depth_image().is_some()
    }

    pub(super) fn depth_image(&self) -> Option<(vk::Image, vk::ImageView)> {
        self.depth
            .as_ref()
            .or_else(|| self.promoted.as_ref().map(|depth| &depth.attachment))
            .map(|depth| (depth.image, depth.view))
    }

    pub(super) fn clear_value_count(&self) -> usize {
        if self.depth.is_none() && self.promoted.is_none() {
            1
        } else if self.samples == vk::SampleCountFlags::TYPE_1 {
            SINGLE_CLEAR_VALUES
        } else {
            MAX_SCENE_ATTACHMENTS
        }
    }
    pub fn destroy(mut self, device: &ash::Device) {
        self.device = None;
        self.release(device);
    }

    fn release_depth(&mut self, device: &ash::Device) {
        if let Some(depth) = self.promoted.take() {
            self.allocation_bytes -= depth.attachment.bytes;
            unsafe { device.destroy_framebuffer(depth.framebuffer, None) };
            depth.attachment.destroy(device);
        }
    }

    fn release(&mut self, device: &ash::Device) {
        unsafe {
            self.release_depth(device);
            device.destroy_framebuffer(self.framebuffer, None);
            if let Some(depth) = self.depth.take() {
                depth.destroy(device);
            }
            if let Some(msaa) = self.msaa.take() {
                msaa.destroy(device);
            }
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
        #[cfg(test)]
        LIVE_TARGETS.set(LIVE_TARGETS.get() - 1);
    }
}

pub struct PresentedTarget(SceneTarget);

impl PresentedTarget {
    pub fn new(target: SceneTarget) -> Self {
        Self(target)
    }

    pub fn destroy(self, device: &ash::Device) {
        let _ = crate::shared::wait_idle(device);
        self.0.destroy(device);
    }

    pub fn enable_depth(
        &mut self,
        renderer: &mut Renderer,
        order: super::DepthOrder,
    ) -> Result<()> {
        renderer.enable_scene_depth(&mut self.0, order)
    }

    pub fn disable_depth(&mut self, renderer: &mut Renderer) -> Result<()> {
        renderer.disable_scene_depth(&mut self.0)
    }

    pub fn set_viewport(&mut self, viewport: Option<super::SceneViewport>) {
        self.0.viewport = viewport;
    }
}

impl std::ops::Deref for PresentedTarget {
    type Target = SceneTarget;

    fn deref(&self) -> &SceneTarget {
        &self.0
    }
}

impl Drop for SceneTarget {
    fn drop(&mut self) {
        if let Some(device) = self.device.take() {
            let _ = crate::shared::wait_idle(&device);
            self.release(&device);
        }
    }
}

impl SceneTexture {
    fn release(&mut self, device: &ash::Device) {
        if let Some(mut atlas) = self.text_atlas.take() {
            atlas.device = None;
            atlas.release(device);
        }
        if let Some(mut mesh) = self.text_mesh.take() {
            mesh.release(device);
        }
        unsafe {
            if self.set != vk::DescriptorSet::null() && self.pool != vk::DescriptorPool::null() {
                let _ = device.free_descriptor_sets(self.pool, &[self.set]);
            }
            if self.image == vk::Image::null() {
                return;
            }
            #[cfg(test)]
            LIVE_TEXTURES.set(LIVE_TEXTURES.get() - 1);
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

impl Drop for SceneTexture {
    fn drop(&mut self) {
        if let Some(device) = self.device.take() {
            let _ = crate::shared::wait_idle(&device);
            self.release(&device);
        }
    }
}

impl SceneMesh {
    fn release(&mut self, device: &ash::Device) {
        #[cfg(test)]
        LIVE_MESHES.set(LIVE_MESHES.get() - 1);
        self.device = None;
        unsafe {
            if !self.mapped.is_null() {
                device.unmap_memory(self.memory);
            }
            device.destroy_buffer(self.buffer, None);
            device.free_memory(self.memory, None);
        }
    }
}

impl Drop for SceneMesh {
    fn drop(&mut self) {
        if let Some(device) = self.device.take() {
            let _ = crate::shared::wait_idle(&device);
            self.release(&device);
        }
    }
}

fn scene_target_spec(
    width: u32,
    height: u32,
    format: vk::Format,
    samples: vk::SampleCountFlags,
) -> super::images::Spec {
    super::images::Spec {
        extent: vk::Extent2D { width, height },
        format,
        levels: 1,
        samples,
        usage: if samples == vk::SampleCountFlags::TYPE_1 {
            vk::ImageUsageFlags::COLOR_ATTACHMENT
                | vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST
        } else {
            vk::ImageUsageFlags::COLOR_ATTACHMENT
        },
        aspect: vk::ImageAspectFlags::COLOR,
    }
}
