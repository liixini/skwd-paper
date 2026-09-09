use anyhow::{Context, Result};
use ash::vk;

use super::Renderer;

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
            let render_pass = device.create_render_pass(
                &vk::RenderPassCreateInfo::default().attachments(&attachment).subpasses(&subpass),
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
                .size(28)];
            let pipeline_layout = device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&layouts)
                    .push_constant_ranges(&push),
                None,
            )?;

            let pipeline = build_pipeline(
                device,
                &PipelineSpec {
                    vert: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vert.spv")),
                    frag: include_bytes!(concat!(env!("OUT_DIR"), "/nv12.frag.spv")),
                    topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                    blend: vk::PipelineColorBlendAttachmentState::default()
                        .color_write_mask(vk::ColorComponentFlags::RGBA),
                    dynamic: &[],
                    layout: pipeline_layout,
                    render_pass,
                    extent,
                },
            )?;

            let sand_push = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .size(64)];
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
                },
            )?;

            self.pipeline_sand = build_pipeline(
                &device,
                &PipelineSpec {
                    vert: include_bytes!(concat!(env!("OUT_DIR"), "/sand_grain.vert.spv")),
                    frag: include_bytes!(concat!(env!("OUT_DIR"), "/sand_grain.frag.spv")),
                    topology: vk::PrimitiveTopology::POINT_LIST,
                    blend: vk::PipelineColorBlendAttachmentState::default()
                        .color_write_mask(vk::ColorComponentFlags::RGBA)
                        .blend_enable(true)
                        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                        .color_blend_op(vk::BlendOp::ADD)
                        .src_alpha_blend_factor(vk::BlendFactor::ONE)
                        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                        .alpha_blend_op(vk::BlendOp::ADD),
                    dynamic: &[],
                    layout: self.pipeline_layout_sand,
                    render_pass,
                    extent,
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
                },
            )?;
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub fn effect_pipeline(&mut self, fx: usize) -> Result<vk::Pipeline> {
        if let Some(pipe) = self.effect_pipes.get(&fx) {
            return Ok(*pipe);
        }
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
                    layout: self.pipeline_layout_sand,
                    render_pass: self.render_pass,
                    extent: self.extent,
                },
            )?
        };
        self.effect_pipes.insert(fx, pipe);
        Ok(pipe)
    }
}

#[cfg(feature = "shared-device")]
include!(concat!(env!("OUT_DIR"), "/effects_gen.rs"));

fn make_module(device: &ash::Device, bytes: &[u8]) -> Result<vk::ShaderModule> {
    let words: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        .collect();
    unsafe {
        device
            .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
            .context("shader module")
    }
}

struct PipelineSpec<'a> {
    vert: &'a [u8],
    frag: &'a [u8],
    topology: vk::PrimitiveTopology,
    blend: vk::PipelineColorBlendAttachmentState,
    dynamic: &'a [vk::DynamicState],
    layout: vk::PipelineLayout,
    render_pass: vk::RenderPass,
    extent: vk::Extent2D,
}

unsafe fn build_pipeline(device: &ash::Device, spec: &PipelineSpec<'_>) -> Result<vk::Pipeline> {
    let vert = make_module(device, spec.vert)?;
    let frag = make_module(device, spec.frag)?;
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vert)
            .name(c"main"),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(frag)
            .name(c"main"),
    ];
    let vi = vk::PipelineVertexInputStateCreateInfo::default();
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
    let mut info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vi)
        .input_assembly_state(&ia)
        .viewport_state(&vp)
        .rasterization_state(&rs)
        .multisample_state(&ms)
        .color_blend_state(&blend)
        .layout(spec.layout)
        .render_pass(spec.render_pass)
        .subpass(0);
    if !spec.dynamic.is_empty() {
        info = info.dynamic_state(&dynamic);
    }
    unsafe {
        let pipeline = device
            .create_graphics_pipelines(vk::PipelineCache::null(), &[info], None)
            .map_err(|(_, err)| err)?[0];
        device.destroy_shader_module(vert, None);
        device.destroy_shader_module(frag, None);
        Ok(pipeline)
    }
}

impl Renderer {
    pub(crate) fn ensure_scene_pipelines(&mut self) -> Result<()> {
        if self.pipeline_layer != vk::Pipeline::null() {
            return Ok(());
        }
        let device = self.device.clone();
        unsafe {
            let attachment = [vk::AttachmentDescription::default()
                .format(vk::Format::R8G8B8A8_UNORM)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let color_ref = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let subpass = [vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&color_ref)];
            let dependencies = [
                vk::SubpassDependency::default()
                    .src_subpass(vk::SUBPASS_EXTERNAL)
                    .dst_subpass(0)
                    .src_stage_mask(
                        vk::PipelineStageFlags::FRAGMENT_SHADER
                            | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    )
                    .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                    .src_access_mask(
                        vk::AccessFlags::SHADER_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                    )
                    .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
                vk::SubpassDependency::default()
                    .src_subpass(0)
                    .dst_subpass(vk::SUBPASS_EXTERNAL)
                    .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                    .dst_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
                    .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ),
            ];
            self.scene_pass = device.create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(&attachment)
                    .subpasses(&subpass)
                    .dependencies(&dependencies),
                None,
            )?;

            let layouts = [self.desc_layout];
            let push = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .size(64)];
            self.pipeline_layout_layer = device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&layouts)
                    .push_constant_ranges(&push),
                None,
            )?;
            let vert =
                make_module(&device, include_bytes!(concat!(env!("OUT_DIR"), "/layer.vert.spv")))?;
            let frag =
                make_module(&device, include_bytes!(concat!(env!("OUT_DIR"), "/layer.frag.spv")))?;
            let puppet_vert =
                make_module(&device, include_bytes!(concat!(env!("OUT_DIR"), "/puppet.vert.spv")))?;
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(vert)
                    .name(c"main"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(frag)
                    .name(c"main"),
            ];
            let vi = vk::PipelineVertexInputStateCreateInfo::default();
            let ia = vk::PipelineInputAssemblyStateCreateInfo::default()
                .topology(vk::PrimitiveTopology::TRIANGLE_STRIP);
            let viewport = [vk::Viewport::default()];
            let scissor = [vk::Rect2D::default()];
            let vp = vk::PipelineViewportStateCreateInfo::default()
                .viewports(&viewport)
                .scissors(&scissor);
            let rs = vk::PipelineRasterizationStateCreateInfo::default()
                .polygon_mode(vk::PolygonMode::FILL)
                .cull_mode(vk::CullModeFlags::NONE)
                .line_width(1.0);
            let ms = vk::PipelineMultisampleStateCreateInfo::default()
                .rasterization_samples(vk::SampleCountFlags::TYPE_1);
            let att = [vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA)
                .blend_enable(true)
                .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .alpha_blend_op(vk::BlendOp::ADD)];
            let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&att);
            let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
            let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);
            self.pipeline_layer = device
                .create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    &[vk::GraphicsPipelineCreateInfo::default()
                        .stages(&stages)
                        .vertex_input_state(&vi)
                        .input_assembly_state(&ia)
                        .viewport_state(&vp)
                        .rasterization_state(&rs)
                        .multisample_state(&ms)
                        .color_blend_state(&blend)
                        .dynamic_state(&dynamic)
                        .layout(self.pipeline_layout_layer)
                        .render_pass(self.scene_pass)
                        .subpass(0)],
                    None,
                )
                .map_err(|(_, err)| err)?[0];
            let att_add = [vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA)
                .blend_enable(true)
                .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                .dst_color_blend_factor(vk::BlendFactor::ONE)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ZERO)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE)
                .alpha_blend_op(vk::BlendOp::ADD)];
            let blend_add = vk::PipelineColorBlendStateCreateInfo::default().attachments(&att_add);
            self.pipeline_layer_add = device
                .create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    &[vk::GraphicsPipelineCreateInfo::default()
                        .stages(&stages)
                        .vertex_input_state(&vi)
                        .input_assembly_state(&ia)
                        .viewport_state(&vp)
                        .rasterization_state(&rs)
                        .multisample_state(&ms)
                        .color_blend_state(&blend_add)
                        .dynamic_state(&dynamic)
                        .layout(self.pipeline_layout_layer)
                        .render_pass(self.scene_pass)
                        .subpass(0)],
                    None,
                )
                .map_err(|(_, err)| err)?[0];
            let att_copy = [vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA)];
            let blend_copy =
                vk::PipelineColorBlendStateCreateInfo::default().attachments(&att_copy);
            self.pipeline_layer_copy = device
                .create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    &[vk::GraphicsPipelineCreateInfo::default()
                        .stages(&stages)
                        .vertex_input_state(&vi)
                        .input_assembly_state(&ia)
                        .viewport_state(&vp)
                        .rasterization_state(&rs)
                        .multisample_state(&ms)
                        .color_blend_state(&blend_copy)
                        .dynamic_state(&dynamic)
                        .layout(self.pipeline_layout_layer)
                        .render_pass(self.scene_pass)
                        .subpass(0)],
                    None,
                )
                .map_err(|(_, err)| err)?[0];
            let att_screen = [vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA)
                .blend_enable(true)
                .src_color_blend_factor(vk::BlendFactor::ONE_MINUS_DST_COLOR)
                .dst_color_blend_factor(vk::BlendFactor::ONE)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ZERO)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE)
                .alpha_blend_op(vk::BlendOp::ADD)];
            let blend_screen =
                vk::PipelineColorBlendStateCreateInfo::default().attachments(&att_screen);
            self.pipeline_layer_screen = device
                .create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    &[vk::GraphicsPipelineCreateInfo::default()
                        .stages(&stages)
                        .vertex_input_state(&vi)
                        .input_assembly_state(&ia)
                        .viewport_state(&vp)
                        .rasterization_state(&rs)
                        .multisample_state(&ms)
                        .color_blend_state(&blend_screen)
                        .dynamic_state(&dynamic)
                        .layout(self.pipeline_layout_layer)
                        .render_pass(self.scene_pass)
                        .subpass(0)],
                    None,
                )
                .map_err(|(_, err)| err)?[0];
            let puppet_stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(puppet_vert)
                    .name(c"main"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(frag)
                    .name(c"main"),
            ];
            let puppet_bind = [vk::VertexInputBindingDescription::default()
                .binding(0)
                .stride(16)
                .input_rate(vk::VertexInputRate::VERTEX)];
            let puppet_attrs = [
                vk::VertexInputAttributeDescription::default()
                    .location(0)
                    .binding(0)
                    .format(vk::Format::R32G32_SFLOAT)
                    .offset(0),
                vk::VertexInputAttributeDescription::default()
                    .location(1)
                    .binding(0)
                    .format(vk::Format::R32G32_SFLOAT)
                    .offset(8),
            ];
            let puppet_vi = vk::PipelineVertexInputStateCreateInfo::default()
                .vertex_binding_descriptions(&puppet_bind)
                .vertex_attribute_descriptions(&puppet_attrs);
            let puppet_ia = vk::PipelineInputAssemblyStateCreateInfo::default()
                .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
            self.pipeline_puppet = device
                .create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    &[vk::GraphicsPipelineCreateInfo::default()
                        .stages(&puppet_stages)
                        .vertex_input_state(&puppet_vi)
                        .input_assembly_state(&puppet_ia)
                        .viewport_state(&vp)
                        .rasterization_state(&rs)
                        .multisample_state(&ms)
                        .color_blend_state(&blend)
                        .dynamic_state(&dynamic)
                        .layout(self.pipeline_layout_layer)
                        .render_pass(self.scene_pass)
                        .subpass(0)],
                    None,
                )
                .map_err(|(_, err)| err)?[0];
            device.destroy_shader_module(vert, None);
            device.destroy_shader_module(puppet_vert, None);
            device.destroy_shader_module(frag, None);
        }
        self.ensure_rgba_pipelines()
    }

    pub(crate) fn ensure_rgba_pipelines(&mut self) -> Result<()> {
        if self.pipeline_rgba != vk::Pipeline::null() {
            return Ok(());
        }
        let device = self.device.clone();
        let extent = self.extent;
        unsafe {
            self.pipeline_rgba = build_pipeline(
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
                },
            )?;
            self.pipeline_rgba_fade = build_pipeline(
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
                },
            )?;
        }
        Ok(())
    }
}
