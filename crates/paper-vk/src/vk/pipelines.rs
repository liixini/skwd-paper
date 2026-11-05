use anyhow::{Context, Result};
use ash::vk;

use super::Renderer;

const SHADER_ENTRY: &std::ffi::CStr = c"main";
const PUPPET_UV_OFFSET: u32 = std::mem::size_of::<[f32; 2]>() as u32;
const PUPPET_ALPHA_OFFSET: u32 =
    std::mem::size_of::<[f32; super::scene::PUPPET_ALPHA_FLOAT]>() as u32;
const TEXT_SHADERS: [&[u8]; 2] = [
    include_bytes!(concat!(env!("OUT_DIR"), "/text.vert.spv")),
    include_bytes!(concat!(env!("OUT_DIR"), "/text.frag.spv")),
];

const CLIP_SHADERS: [&[u8]; 5] = [
    include_bytes!(concat!(env!("OUT_DIR"), "/layer.vert.spv")),
    include_bytes!(concat!(env!("OUT_DIR"), "/puppet_clip.vert.spv")),
    include_bytes!(concat!(env!("OUT_DIR"), "/clip_mask.frag.spv")),
    include_bytes!(concat!(env!("OUT_DIR"), "/clip_fold.frag.spv")),
    include_bytes!(concat!(env!("OUT_DIR"), "/layer_clip.frag.spv")),
];

#[cfg(all(test, feature = "shared-device"))]
mod tests;

struct PipelineBuild<'a> {
    device: &'a ash::Device,
    modules: ShaderModules<'a>,
    pipelines: Vec<vk::Pipeline>,
    pass: vk::RenderPass,
    layout: vk::PipelineLayout,
}

impl<'a> PipelineBuild<'a> {
    fn new(device: &'a ash::Device) -> Self {
        Self {
            device,
            modules: ShaderModules::new(device),
            pipelines: Vec::new(),
            pass: vk::RenderPass::null(),
            layout: vk::PipelineLayout::null(),
        }
    }

    fn module(&mut self, bytes: &[u8]) -> Result<vk::ShaderModule> {
        #[cfg(test)]
        pipeline_checkpoint()?;
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect();
        self.modules.create(&words)
    }

    fn pipeline(&mut self, info: vk::GraphicsPipelineCreateInfo<'_>) -> Result<vk::Pipeline> {
        #[cfg(test)]
        pipeline_checkpoint()?;
        let created = unsafe {
            self.device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None)
        };
        match created {
            Ok(pipelines) => {
                let pipeline = pipelines[0];
                self.pipelines.extend(pipelines);
                Ok(pipeline)
            }
            Err((pipelines, err)) => {
                self.pipelines.extend(pipelines);
                Err(err.into())
            }
        }
    }
}

impl Drop for PipelineBuild<'_> {
    fn drop(&mut self) {
        unsafe {
            for pipeline in self.pipelines.drain(..) {
                self.device.destroy_pipeline(pipeline, None);
            }
            self.modules.clear();
            self.device.destroy_pipeline_layout(self.layout, None);
            self.device.destroy_render_pass(self.pass, None);
        }
    }
}

#[cfg(test)]
thread_local! {
    pub(super) static PIPELINE_FAILURE: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn pipeline_checkpoint() -> Result<()> {
    PIPELINE_FAILURE.with(|remaining| match remaining.get() {
        Some(0) => {
            remaining.set(None);
            Err(anyhow::anyhow!("injected pipeline creation failure"))
        }
        Some(count) => {
            remaining.set(Some(count - 1));
            Ok(())
        }
        None => Ok(()),
    })
}

impl Renderer {
    pub(crate) fn create_render_pass(
        device: &ash::Device,
        format: vk::Format,
        headless: bool,
    ) -> Result<vk::RenderPass> {
        unsafe {
            let attachment = [vk::AttachmentDescription::default()
                .format(format)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(if headless {
                    vk::ImageLayout::GENERAL
                } else {
                    vk::ImageLayout::PRESENT_SRC_KHR
                })];
            let color_ref = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let subpass = [vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&color_ref)];
            let dependencies = [vk::SubpassDependency::default()
                .src_subpass(0)
                .dst_subpass(vk::SUBPASS_EXTERNAL)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .dst_stage_mask(vk::PipelineStageFlags::TRANSFER)
                .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ)];
            let render_pass = device.create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(&attachment)
                    .subpasses(&subpass)
                    .dependencies(if headless { &dependencies } else { &[] }),
                None,
            )?;
            Ok(render_pass)
        }
    }

    pub(crate) fn create_pipelines(
        device: &ash::Device,
        extent: vk::Extent2D,
        render_pass: vk::RenderPass,
        desc_layout: vk::DescriptorSetLayout,
    ) -> Result<(vk::PipelineLayout, vk::Pipeline, vk::PipelineLayout)> {
        unsafe {
            let layouts = [desc_layout];
            let push = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .size(48)];
            let pipeline_layout = device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&layouts)
                    .push_constant_ranges(&push),
                None,
            )?;

            let pipeline = create_video_pipeline(device, extent, render_pass, pipeline_layout)?;

            let sand_push = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .size(super::draw::EFFECT_PUSH_BYTES)];
            let sand_sets = [desc_layout, desc_layout];
            let pipeline_layout_sand = device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&sand_sets)
                    .push_constant_ranges(&sand_push),
                None,
            )?;

            Ok((pipeline_layout, pipeline, pipeline_layout_sand))
        }
    }

    pub(crate) fn ensure_transition_pipelines(&mut self) -> Result<()> {
        if self.pipeline_fade != vk::Pipeline::null() {
            return Ok(());
        }
        let device = self.device.clone();
        let extent = self.extent;
        let render_pass = self.render_pass;
        unsafe {
            self.pipeline_fade = build_pipeline(
                &device,
                &PipelineSpec {
                    vert: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vert.spv")),
                    frag: include_bytes!(concat!(env!("OUT_DIR"), "/nv12.frag.spv")),
                    topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                    blend: vk::PipelineColorBlendAttachmentState::default()
                        .color_write_mask(vk::ColorComponentFlags::RGBA)
                        .blend_enable(true)
                        .src_color_blend_factor(vk::BlendFactor::CONSTANT_ALPHA)
                        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_CONSTANT_ALPHA)
                        .color_blend_op(vk::BlendOp::ADD)
                        .src_alpha_blend_factor(vk::BlendFactor::ONE)
                        .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
                        .alpha_blend_op(vk::BlendOp::ADD),
                    dynamic: &[vk::DynamicState::BLEND_CONSTANTS],
                    layout: self.pipeline_layout,
                    render_pass,
                    extent,
                    vertex: None,
                },
            )?;

            self.pipeline_sand = build_pipeline(
                &device,
                &PipelineSpec {
                    vert: include_bytes!(concat!(env!("OUT_DIR"), "/sand_grain.vert.spv")),
                    frag: include_bytes!(concat!(env!("OUT_DIR"), "/sand_grain.frag.spv")),
                    topology: vk::PrimitiveTopology::POINT_LIST,
                    blend: particle_blend(),
                    dynamic: &[],
                    layout: self.pipeline_layout_sand,
                    render_pass,
                    extent,
                    vertex: None,
                },
            )?;

            self.pipeline_base = build_pipeline(
                &device,
                &PipelineSpec {
                    vert: include_bytes!(concat!(env!("OUT_DIR"), "/sand_base.vert.spv")),
                    frag: include_bytes!(concat!(env!("OUT_DIR"), "/sand_base.frag.spv")),
                    topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                    blend: vk::PipelineColorBlendAttachmentState::default()
                        .color_write_mask(vk::ColorComponentFlags::RGBA),
                    dynamic: &[],
                    layout: self.pipeline_layout_sand,
                    render_pass,
                    extent,
                    vertex: None,
                },
            )?;
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub fn retire_effect(&mut self, effect: usize) -> Result<()> {
        self.wait_render()?;
        if let Some(pipeline) = self.effect_pipes.remove(&effect) {
            unsafe {
                self.device.destroy_pipeline(pipeline, None);
            }
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub fn effect_pipeline(&mut self, fx: usize) -> Result<vk::Pipeline> {
        if let Some(pipe) = self.effect_pipes.get(&fx) {
            return Ok(*pipe);
        }
        let layout = if paper_shaders::effect_requires_environment(fx) {
            self.transition_layout()?
        } else {
            self.pipeline_layout_sand
        };
        let spv =
            EFFECT_SPV.get(fx).ok_or_else(|| anyhow::anyhow!("effect index {fx} out of range"))?;
        let pipe = unsafe {
            build_pipeline(
                &self.device,
                &PipelineSpec {
                    vert: include_bytes!(concat!(env!("OUT_DIR"), "/sand_base.vert.spv")),
                    frag: spv,
                    topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                    blend: vk::PipelineColorBlendAttachmentState::default()
                        .color_write_mask(vk::ColorComponentFlags::RGBA),
                    dynamic: &[],
                    layout,
                    render_pass: self.render_pass,
                    extent: self.extent,
                    vertex: None,
                },
            )?
        };
        self.effect_pipes.insert(fx, pipe);
        Ok(pipe)
    }
}

#[cfg(feature = "shared-device")]
include!(concat!(env!("OUT_DIR"), "/effects_gen.rs"));

pub(super) struct ShaderModules<'a> {
    device: &'a ash::Device,
    handles: Vec<vk::ShaderModule>,
}

impl<'a> ShaderModules<'a> {
    pub(super) fn new(device: &'a ash::Device) -> Self {
        Self { device, handles: Vec::new() }
    }

    pub(super) fn create(&mut self, words: &[u32]) -> Result<vk::ShaderModule> {
        let module = unsafe {
            self.device
                .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(words), None)
                .context("shader module")?
        };
        self.handles.push(module);
        Ok(module)
    }

    fn clear(&mut self) {
        for module in self.handles.drain(..) {
            unsafe { self.device.destroy_shader_module(module, None) };
        }
    }
}

impl Drop for ShaderModules<'_> {
    fn drop(&mut self) {
        self.clear();
    }
}

fn create_video_pipeline(
    device: &ash::Device,
    extent: vk::Extent2D,
    render_pass: vk::RenderPass,
    layout: vk::PipelineLayout,
) -> Result<vk::Pipeline> {
    unsafe {
        build_pipeline(
            device,
            &PipelineSpec {
                vert: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vert.spv")),
                frag: include_bytes!(concat!(env!("OUT_DIR"), "/nv12.frag.spv")),
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                blend: vk::PipelineColorBlendAttachmentState::default()
                    .color_write_mask(vk::ColorComponentFlags::RGBA),
                dynamic: &[],
                layout,
                render_pass,
                extent,
                vertex: None,
            },
        )
    }
}

fn particle_blend() -> vk::PipelineColorBlendAttachmentState {
    vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::RGBA)
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
}

pub(super) struct PipelineSpec<'a> {
    pub vert: &'a [u8],
    pub frag: &'a [u8],
    pub topology: vk::PrimitiveTopology,
    pub blend: vk::PipelineColorBlendAttachmentState,
    pub dynamic: &'a [vk::DynamicState],
    pub layout: vk::PipelineLayout,
    pub render_pass: vk::RenderPass,
    pub extent: vk::Extent2D,
    pub vertex: Option<&'a vk::PipelineVertexInputStateCreateInfo<'a>>,
}

fn raster_pipeline_info<'a>(
    viewport: &'a vk::PipelineViewportStateCreateInfo<'a>,
    raster: &'a vk::PipelineRasterizationStateCreateInfo<'a>,
    samples: &'a vk::PipelineMultisampleStateCreateInfo<'a>,
) -> vk::GraphicsPipelineCreateInfo<'a> {
    vk::GraphicsPipelineCreateInfo::default()
        .viewport_state(viewport)
        .rasterization_state(raster)
        .multisample_state(samples)
}

fn shader_stages(
    vertex: vk::ShaderModule,
    fragment: vk::ShaderModule,
) -> [vk::PipelineShaderStageCreateInfo<'static>; 2] {
    [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vertex)
            .name(SHADER_ENTRY),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fragment)
            .name(SHADER_ENTRY),
    ]
}

pub(super) unsafe fn build_pipeline(
    device: &ash::Device,
    spec: &PipelineSpec<'_>,
) -> Result<vk::Pipeline> {
    let mut candidate = PipelineBuild::new(device);
    let vert = candidate.module(spec.vert)?;
    let frag = candidate.module(spec.frag)?;
    let stages = shader_stages(vert, frag);
    let empty = vk::PipelineVertexInputStateCreateInfo::default();
    let vi = spec.vertex.unwrap_or(&empty);
    let ia = vk::PipelineInputAssemblyStateCreateInfo::default().topology(spec.topology);
    let viewport = [vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: spec.extent.width as f32,
        height: spec.extent.height as f32,
        min_depth: 0.0,
        max_depth: 1.0,
    }];
    let scissor = [vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: spec.extent }];
    let vp = vk::PipelineViewportStateCreateInfo::default().viewports(&viewport).scissors(&scissor);
    let rs = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .line_width(1.0);
    let ms = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let blend_att = [spec.blend];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_att);
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(spec.dynamic);
    let depth = vk::PipelineDepthStencilStateCreateInfo::default();
    let mut info = raster_pipeline_info(&vp, &rs, &ms)
        .stages(&stages)
        .vertex_input_state(vi)
        .input_assembly_state(&ia)
        .color_blend_state(&blend)
        .depth_stencil_state(&depth)
        .layout(spec.layout)
        .render_pass(spec.render_pass)
        .subpass(0);
    if !spec.dynamic.is_empty() {
        info = info.dynamic_state(&dynamic);
    }
    let pipeline = candidate.pipeline(info)?;
    candidate.pipelines.clear();
    Ok(pipeline)
}

#[derive(Clone, Copy)]
pub(super) struct DepthPasses {
    pub clear: vk::RenderPass,
    pub append: vk::RenderPass,
    pub initialize: vk::RenderPass,
}

#[derive(Clone, Copy)]
pub(super) struct SceneRasterPipelines {
    pub layers: [vk::Pipeline; super::SceneBlend::ALL.len()],
    pub text: [vk::Pipeline; super::SceneBlend::ALL.len()],
    pub puppet: [vk::Pipeline; super::SceneBlend::ALL.len()],
    pub culled_puppet: [vk::Pipeline; super::SceneBlend::ALL.len()],
}

impl SceneRasterPipelines {
    pub fn puppet_for(&self, cull: bool) -> &[vk::Pipeline; super::SceneBlend::ALL.len()] {
        if cull { &self.culled_puppet } else { &self.puppet }
    }
}

pub(super) fn create_scene_render_pass(
    device: &ash::Device,
    config: super::SceneConfig,
    samples: vk::SampleCountFlags,
) -> Result<vk::RenderPass> {
    create_scene_pass(device, config, samples, vk::ImageLayout::UNDEFINED, false)
}

pub(super) fn create_scene_render_pass_load(
    device: &ash::Device,
    config: super::SceneConfig,
    samples: vk::SampleCountFlags,
) -> Result<vk::RenderPass> {
    create_scene_pass(device, config, samples, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, true)
}

#[expect(clippy::too_many_lines, reason = "one Vulkan scene render pass definition")]
fn create_scene_pass(
    device: &ash::Device,
    config: super::SceneConfig,
    samples: vk::SampleCountFlags,
    initial_layout: vk::ImageLayout,
    depth_load: bool,
) -> Result<vk::RenderPass> {
    let load = initial_layout != vk::ImageLayout::UNDEFINED;
    unsafe {
        let mut attachment = vec![
            vk::AttachmentDescription::default()
                .format(config.format)
                .samples(samples)
                .load_op(if load {
                    vk::AttachmentLoadOp::LOAD
                } else {
                    vk::AttachmentLoadOp::CLEAR
                })
                .store_op(vk::AttachmentStoreOp::STORE)
                .initial_layout(if samples == vk::SampleCountFlags::TYPE_1 {
                    initial_layout
                } else if load {
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL
                } else {
                    vk::ImageLayout::UNDEFINED
                })
                .final_layout(if samples == vk::SampleCountFlags::TYPE_1 {
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
                } else {
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL
                }),
        ];
        if samples != vk::SampleCountFlags::TYPE_1 {
            attachment.push(
                vk::AttachmentDescription::default()
                    .format(config.format)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
            );
        }
        if config.depth.is_some() {
            attachment.push(
                vk::AttachmentDescription::default()
                    .format(super::depth::FORMAT)
                    .samples(samples)
                    .load_op(if depth_load {
                        vk::AttachmentLoadOp::LOAD
                    } else {
                        vk::AttachmentLoadOp::CLEAR
                    })
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .initial_layout(if depth_load {
                        vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL
                    } else {
                        vk::ImageLayout::UNDEFINED
                    })
                    .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
            );
        }
        let depth_ref = vk::AttachmentReference::default()
            .attachment(if samples == vk::SampleCountFlags::TYPE_1 { 1 } else { 2 })
            .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
        let color_ref = [vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
        let mut subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&color_ref);
        let resolve_ref = [vk::AttachmentReference::default()
            .attachment(1)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
        if samples != vk::SampleCountFlags::TYPE_1 {
            subpass = subpass.resolve_attachments(&resolve_ref);
        }
        if config.depth.is_some() {
            subpass = subpass.depth_stencil_attachment(&depth_ref);
        }
        let subpass = [subpass];
        let dependencies = [
            vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(
                    vk::PipelineStageFlags::VERTEX_SHADER
                        | vk::PipelineStageFlags::FRAGMENT_SHADER
                        | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                        | vk::PipelineStageFlags::TRANSFER
                        | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                )
                .dst_stage_mask(
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                        | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                )
                .src_access_mask(
                    vk::AccessFlags::SHADER_READ
                        | vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                        | vk::AccessFlags::TRANSFER_READ
                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                )
                .dst_access_mask(
                    vk::AccessFlags::COLOR_ATTACHMENT_READ
                        | vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                ),
            vk::SubpassDependency::default()
                .src_subpass(0)
                .dst_subpass(vk::SUBPASS_EXTERNAL)
                .src_stage_mask(
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                        | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                )
                .dst_stage_mask(
                    vk::PipelineStageFlags::VERTEX_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
                )
                .src_access_mask(
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                )
                .dst_access_mask(vk::AccessFlags::SHADER_READ),
        ];
        device
            .create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(&attachment)
                    .subpasses(&subpass)
                    .dependencies(&dependencies),
                None,
            )
            .context("scene render pass")
    }
}

struct PuppetInput {
    binding: [vk::VertexInputBindingDescription; 1],
    attributes: [vk::VertexInputAttributeDescription; 3],
}

impl PuppetInput {
    fn new() -> Self {
        let binding = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(std::mem::size_of::<[f32; super::scene::PUPPET_VERTEX_FLOATS]>() as u32)
            .input_rate(vk::VertexInputRate::VERTEX)];
        Self { binding, attributes: puppet_attributes() }
    }

    fn state(&self) -> vk::PipelineVertexInputStateCreateInfo<'_> {
        vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&self.binding)
            .vertex_attribute_descriptions(&self.attributes)
    }
}

fn puppet_attributes() -> [vk::VertexInputAttributeDescription; 3] {
    [
        vk::VertexInputAttributeDescription::default()
            .location(0)
            .binding(0)
            .format(vk::Format::R32G32_SFLOAT)
            .offset(0),
        vk::VertexInputAttributeDescription::default()
            .location(1)
            .binding(0)
            .format(vk::Format::R32G32_SFLOAT)
            .offset(PUPPET_UV_OFFSET),
        vk::VertexInputAttributeDescription::default()
            .location(2)
            .binding(0)
            .format(vk::Format::R32_SFLOAT)
            .offset(PUPPET_ALPHA_OFFSET),
    ]
}

const PUPPET_FRONT_FACE: vk::FrontFace = vk::FrontFace::COUNTER_CLOCKWISE;

fn create_scene_raster(
    device: &ash::Device,
    pass: vk::RenderPass,
    layout: vk::PipelineLayout,
    samples: vk::SampleCountFlags,
) -> Result<SceneRasterPipelines> {
    let mut candidate = PipelineBuild::new(device);
    let vert = candidate.module(include_bytes!(concat!(env!("OUT_DIR"), "/layer.vert.spv")))?;
    let frag = candidate.module(include_bytes!(concat!(env!("OUT_DIR"), "/layer.frag.spv")))?;
    let puppet_vert =
        candidate.module(include_bytes!(concat!(env!("OUT_DIR"), "/puppet.vert.spv")))?;
    let [text_vertex, text_fragment] = TEXT_SHADERS;
    let text_vert = candidate.module(text_vertex)?;
    let text_frag = candidate.module(text_fragment)?;
    let stages = shader_stages(vert, frag);
    let puppet_stages = shader_stages(puppet_vert, frag);
    let text_stages = shader_stages(text_vert, text_frag);
    let vi = vk::PipelineVertexInputStateCreateInfo::default();
    let ia = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_STRIP);
    let viewport = [vk::Viewport::default()];
    let scissor = [vk::Rect2D::default()];
    let vp = vk::PipelineViewportStateCreateInfo::default().viewports(&viewport).scissors(&scissor);
    let rs = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .line_width(1.0);
    let ms = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(samples);
    let depth = vk::PipelineDepthStencilStateCreateInfo::default();
    let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);
    let blend_attachment = [super::scene::scene_blend_state(super::SceneBlend::Alpha)];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachment);
    let layer_info = raster_pipeline_info(&vp, &rs, &ms)
        .stages(&stages)
        .vertex_input_state(&vi)
        .input_assembly_state(&ia)
        .color_blend_state(&blend)
        .depth_stencil_state(&depth)
        .dynamic_state(&dynamic)
        .layout(layout)
        .render_pass(pass)
        .subpass(0);
    let mut layers = [vk::Pipeline::null(); super::SceneBlend::ALL.len()];
    for scene_blend in super::SceneBlend::ALL {
        let coverage = ms.alpha_to_coverage_enable(scene_blend == super::SceneBlend::AlphaCoverage);
        let attachment = [super::scene::scene_blend_state(scene_blend)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachment);
        layers[scene_blend as usize] = candidate
            .pipeline(layer_info.multisample_state(&coverage).color_blend_state(&blend))?;
    }
    let puppet_input = PuppetInput::new();
    let puppet_vi = puppet_input.state();
    let triangle = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let culled = rs.cull_mode(vk::CullModeFlags::BACK).front_face(PUPPET_FRONT_FACE);
    let mut puppet = [vk::Pipeline::null(); super::SceneBlend::ALL.len()];
    let mut culled_puppet = [vk::Pipeline::null(); super::SceneBlend::ALL.len()];
    for scene_blend in super::SceneBlend::ALL {
        let coverage = ms.alpha_to_coverage_enable(scene_blend == super::SceneBlend::AlphaCoverage);
        let attachment = [super::scene::scene_blend_state(scene_blend)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachment);
        let info = layer_info
            .multisample_state(&coverage)
            .stages(&puppet_stages)
            .vertex_input_state(&puppet_vi)
            .input_assembly_state(&triangle)
            .color_blend_state(&blend);
        puppet[scene_blend as usize] = candidate.pipeline(info)?;
        culled_puppet[scene_blend as usize] =
            candidate.pipeline(info.rasterization_state(&culled))?;
    }
    let text_bind = [vk::VertexInputBindingDescription::default()
        .binding(0)
        .stride(super::scene::text::TEXT_VERTEX_BYTES)
        .input_rate(vk::VertexInputRate::VERTEX)];
    let text_attrs = super::scene::text::vertex_attributes();
    let text_vi = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&text_bind)
        .vertex_attribute_descriptions(&text_attrs);
    let mut text = [vk::Pipeline::null(); super::SceneBlend::ALL.len()];
    for scene_blend in super::SceneBlend::ALL {
        let coverage = ms.alpha_to_coverage_enable(scene_blend == super::SceneBlend::AlphaCoverage);
        let attachment = [super::scene::text::blend_state(scene_blend)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachment);
        text[scene_blend as usize] = candidate.pipeline(
            layer_info
                .multisample_state(&coverage)
                .stages(&text_stages)
                .vertex_input_state(&text_vi)
                .input_assembly_state(&triangle)
                .color_blend_state(&blend),
        )?;
    }
    candidate.pipelines.clear();
    Ok(SceneRasterPipelines { layers, text, puppet, culled_puppet })
}

#[derive(Clone, Copy)]
pub(super) struct ClipPipelines {
    pub mask: vk::Pipeline,
    pub fold: vk::Pipeline,
    pub clipped: [vk::Pipeline; super::SceneBlend::ALL.len()],
    pub culled_clipped: [vk::Pipeline; super::SceneBlend::ALL.len()],
}

impl ClipPipelines {
    pub fn clipped_for(&self, cull: bool) -> &[vk::Pipeline; super::SceneBlend::ALL.len()] {
        if cull { &self.culled_clipped } else { &self.clipped }
    }

    pub fn all(self) -> impl Iterator<Item = vk::Pipeline> {
        [self.mask, self.fold].into_iter().chain(self.clipped).chain(self.culled_clipped)
    }
}

fn fold_blend() -> vk::PipelineColorBlendAttachmentState {
    vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::RGBA)
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::DST_COLOR)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE)
        .alpha_blend_op(vk::BlendOp::ADD)
}

fn create_clip_raster(
    device: &ash::Device,
    [scene, mask]: [vk::RenderPass; 2],
    layout: vk::PipelineLayout,
    samples: vk::SampleCountFlags,
) -> Result<ClipPipelines> {
    let mut candidate = PipelineBuild::new(device);
    let [layer_vert, clip_vert, mask_frag, fold_frag, clipped_frag] = CLIP_SHADERS;
    let layer_vert = candidate.module(layer_vert)?;
    let clip_vert = candidate.module(clip_vert)?;
    let mask_frag = candidate.module(mask_frag)?;
    let fold_frag = candidate.module(fold_frag)?;
    let clipped_frag = candidate.module(clipped_frag)?;
    let mask_stages = shader_stages(clip_vert, mask_frag);
    let fold_stages = shader_stages(layer_vert, fold_frag);
    let clipped_stages = shader_stages(clip_vert, clipped_frag);
    let viewport = [vk::Viewport::default()];
    let scissor = [vk::Rect2D::default()];
    let vp = vk::PipelineViewportStateCreateInfo::default().viewports(&viewport).scissors(&scissor);
    let rs = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .line_width(1.0);
    let culled = rs.cull_mode(vk::CullModeFlags::BACK).front_face(PUPPET_FRONT_FACE);
    let single = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let ms = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(samples);
    let depth = vk::PipelineDepthStencilStateCreateInfo::default();
    let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);
    let puppet_input = PuppetInput::new();
    let puppet_vi = puppet_input.state();
    let triangle = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let strip = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_STRIP);
    let empty_vi = vk::PipelineVertexInputStateCreateInfo::default();
    let alpha = [super::scene::scene_blend_state(super::SceneBlend::Alpha)];
    let alpha = vk::PipelineColorBlendStateCreateInfo::default().attachments(&alpha);
    let mask_info = raster_pipeline_info(&vp, &rs, &single)
        .stages(&mask_stages)
        .vertex_input_state(&puppet_vi)
        .input_assembly_state(&triangle)
        .color_blend_state(&alpha)
        .depth_stencil_state(&depth)
        .dynamic_state(&dynamic)
        .layout(layout)
        .render_pass(mask)
        .subpass(0);
    let mask_pipeline = candidate.pipeline(mask_info.rasterization_state(&culled))?;
    let fold = [fold_blend()];
    let fold = vk::PipelineColorBlendStateCreateInfo::default().attachments(&fold);
    let fold_pipeline = candidate.pipeline(
        mask_info
            .stages(&fold_stages)
            .vertex_input_state(&empty_vi)
            .input_assembly_state(&strip)
            .color_blend_state(&fold),
    )?;
    let mut clipped = [vk::Pipeline::null(); super::SceneBlend::ALL.len()];
    let mut culled_clipped = [vk::Pipeline::null(); super::SceneBlend::ALL.len()];
    for scene_blend in super::SceneBlend::ALL {
        let coverage = ms.alpha_to_coverage_enable(scene_blend == super::SceneBlend::AlphaCoverage);
        let attachment = [super::scene::scene_blend_state(scene_blend)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachment);
        let info = mask_info
            .stages(&clipped_stages)
            .multisample_state(&coverage)
            .color_blend_state(&blend)
            .render_pass(scene);
        clipped[scene_blend as usize] = candidate.pipeline(info)?;
        culled_clipped[scene_blend as usize] =
            candidate.pipeline(info.rasterization_state(&culled))?;
    }
    candidate.pipelines.clear();
    Ok(ClipPipelines { mask: mask_pipeline, fold: fold_pipeline, clipped, culled_clipped })
}

impl Renderer {
    pub(super) fn scene_video_pipeline(&mut self) -> Result<vk::Pipeline> {
        if self.pipeline_scene_video == vk::Pipeline::null() {
            let pass = self.scene_pass_for(self.format, vk::SampleCountFlags::TYPE_1)?;
            self.pipeline_scene_video =
                create_video_pipeline(&self.device, self.extent, pass, self.pipeline_layout)?;
        }
        Ok(self.pipeline_scene_video)
    }

    pub(super) fn depth_passes_for(
        &mut self,
        config: super::SceneConfig,
        samples: vk::SampleCountFlags,
    ) -> Result<DepthPasses> {
        if let Some((_, passes)) =
            self.depth_passes.iter().find(|(known, _)| *known == (config, samples))
        {
            return Ok(*passes);
        }
        let clear = create_scene_render_pass(&self.device, config, samples)?;
        let append = match create_scene_pass(
            &self.device,
            config,
            samples,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            true,
        ) {
            Ok(pass) => pass,
            Err(error) => {
                unsafe { self.device.destroy_render_pass(clear, None) };
                return Err(error);
            }
        };
        let initialize = match create_scene_pass(
            &self.device,
            config,
            samples,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            false,
        ) {
            Ok(pass) => pass,
            Err(error) => {
                unsafe {
                    self.device.destroy_render_pass(clear, None);
                    self.device.destroy_render_pass(append, None);
                }
                return Err(error);
            }
        };
        let passes = DepthPasses { clear, append, initialize };
        self.depth_passes.push(((config, samples), passes));
        Ok(passes)
    }

    pub fn scene_pass_for(
        &mut self,
        format: vk::Format,
        samples: vk::SampleCountFlags,
    ) -> Result<vk::RenderPass> {
        self.ensure_scene_pipelines()?;
        if format == self.scene_config.format && samples == vk::SampleCountFlags::TYPE_1 {
            return Ok(self.scene_pass);
        }
        let key = (format, samples);
        if let Some((_, pass)) = self.format_passes.iter().find(|(known, _)| *known == key) {
            return Ok(*pass);
        }
        let pass = create_scene_render_pass(
            &self.device,
            super::SceneConfig { format, ..self.scene_config },
            samples,
        )?;
        self.format_passes.push((key, pass));
        Ok(pass)
    }

    pub fn scene_pass_load_for(
        &mut self,
        format: vk::Format,
        samples: vk::SampleCountFlags,
    ) -> Result<vk::RenderPass> {
        self.ensure_scene_pipelines()?;
        let key = (format, samples);
        if let Some((_, pass)) = self.format_passes_load.iter().find(|(known, _)| *known == key) {
            return Ok(*pass);
        }
        let pass = create_scene_render_pass_load(
            &self.device,
            super::SceneConfig { format, ..self.scene_config },
            samples,
        )?;
        self.format_passes_load.push((key, pass));
        Ok(pass)
    }

    pub fn scene_pass_append_for(
        &mut self,
        format: vk::Format,
        samples: vk::SampleCountFlags,
    ) -> Result<vk::RenderPass> {
        self.ensure_scene_pipelines()?;
        let key = (format, samples);
        if let Some((_, pass)) = self.format_passes_append.iter().find(|(known, _)| *known == key) {
            return Ok(*pass);
        }
        let pass = create_scene_pass(
            &self.device,
            super::SceneConfig { format, ..self.scene_config },
            samples,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            true,
        )?;
        self.format_passes_append.push((key, pass));
        Ok(pass)
    }

    pub(crate) fn ensure_scene_pipelines(&mut self) -> Result<()> {
        if self.raster_pipelines[0].is_some() {
            return Ok(());
        }
        self.ensure_rgba_pipelines()?;
        let pass = create_scene_render_pass(
            &self.device,
            self.scene_config,
            vk::SampleCountFlags::TYPE_1,
        )?;
        let layouts = [self.desc_layout];
        let push = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .size(super::scene::LAYER_PUSH_BYTES)];
        let layout = unsafe {
            self.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&layouts)
                    .push_constant_ranges(&push),
                None,
            )
        };
        let layout = match layout {
            Ok(layout) => layout,
            Err(error) => {
                unsafe { self.device.destroy_render_pass(pass, None) };
                return Err(error.into());
            }
        };
        let set = create_scene_raster(&self.device, pass, layout, vk::SampleCountFlags::TYPE_1);
        let set = match set {
            Ok(set) => set,
            Err(error) => {
                unsafe {
                    self.device.destroy_pipeline_layout(layout, None);
                    self.device.destroy_render_pass(pass, None);
                }
                return Err(error);
            }
        };
        self.scene_pass = pass;
        self.pipeline_layout_layer = layout;
        self.raster_pipelines[0] = Some(set);
        Ok(())
    }

    pub(crate) fn ensure_scene_msaa_pipelines(&mut self) -> Result<()> {
        if self.raster_pipelines[1].is_some() {
            return Ok(());
        }
        self.ensure_scene_pipelines()?;
        let pass = self.scene_pass_for(self.scene_config.format, vk::SampleCountFlags::TYPE_4)?;
        let set = create_scene_raster(
            &self.device,
            pass,
            self.pipeline_layout_layer,
            vk::SampleCountFlags::TYPE_4,
        )?;
        self.raster_pipelines[1] = Some(set);
        Ok(())
    }

    pub(super) fn ensure_clip_pipelines(
        &mut self,
        samples: vk::SampleCountFlags,
    ) -> Result<ClipPipelines> {
        let slot = usize::from(samples != vk::SampleCountFlags::TYPE_1);
        if let Some(pipelines) = self.clip_pipelines[slot] {
            return Ok(pipelines);
        }
        let scene = self.scene_pass_for(self.scene_config.format, samples)?;
        let mask = self
            .depth_passes_for(
                super::SceneConfig { format: super::scene::clip::MASK_FORMAT, depth: None },
                vk::SampleCountFlags::TYPE_1,
            )?
            .clear;
        if self.pipeline_layout_clip == vk::PipelineLayout::null() {
            let layouts = [self.desc_layout, self.desc_layout];
            let push = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .size(super::scene::LAYER_PUSH_BYTES)];
            self.pipeline_layout_clip = unsafe {
                self.device.create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&layouts)
                        .push_constant_ranges(&push),
                    None,
                )?
            };
        }
        let pipelines =
            create_clip_raster(&self.device, [scene, mask], self.pipeline_layout_clip, samples)?;
        self.clip_pipelines[slot] = Some(pipelines);
        Ok(pipelines)
    }

    pub(crate) fn ensure_rgba_pipelines(&mut self) -> Result<()> {
        if self.pipeline_rgba != vk::Pipeline::null() {
            return Ok(());
        }
        let device = self.device.clone();
        let extent = self.extent;
        let mut candidate = PipelineBuild::new(&device);
        unsafe {
            let pipeline = build_pipeline(
                &device,
                &PipelineSpec {
                    vert: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vert.spv")),
                    frag: include_bytes!(concat!(env!("OUT_DIR"), "/rgba.frag.spv")),
                    topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                    blend: vk::PipelineColorBlendAttachmentState::default()
                        .color_write_mask(vk::ColorComponentFlags::RGBA),
                    dynamic: &[],
                    layout: self.pipeline_layout,
                    render_pass: self.render_pass,
                    extent,
                    vertex: None,
                },
            )?;
            candidate.pipelines.push(pipeline);
            let fade = build_pipeline(
                &device,
                &PipelineSpec {
                    vert: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vert.spv")),
                    frag: include_bytes!(concat!(env!("OUT_DIR"), "/rgba.frag.spv")),
                    topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                    blend: vk::PipelineColorBlendAttachmentState::default()
                        .color_write_mask(vk::ColorComponentFlags::RGBA)
                        .blend_enable(true)
                        .src_color_blend_factor(vk::BlendFactor::CONSTANT_ALPHA)
                        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_CONSTANT_ALPHA)
                        .color_blend_op(vk::BlendOp::ADD)
                        .src_alpha_blend_factor(vk::BlendFactor::ONE)
                        .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
                        .alpha_blend_op(vk::BlendOp::ADD),
                    dynamic: &[vk::DynamicState::BLEND_CONSTANTS],
                    layout: self.pipeline_layout,
                    render_pass: self.render_pass,
                    extent,
                    vertex: None,
                },
            )?;
            candidate.pipelines.push(fade);
            self.pipeline_rgba = pipeline;
            self.pipeline_rgba_fade = fade;
            candidate.pipelines.clear();
        }
        Ok(())
    }
}
