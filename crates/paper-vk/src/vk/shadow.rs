use super::{MeshGeometry, Renderer, images, pipelines::ShaderModules};
use anyhow::{Result, ensure};
use ash::vk;
use paper_scene::matrix::Mat4;
use paper_scene::shader::{self, Stage};
use std::collections::BTreeMap;

pub(super) const SLOPE_BIAS: f32 = -4.0;

const TILE_ERROR: &str = "shadow tile outside atlas";
const STRIDE_ERROR: &str = "shadow vertex stride invalid";

const ENTRY: &std::ffi::CStr = c"main";
const LABEL: &str = "shadow atlas";
const POSITION_BYTES: u32 = size_of::<[f32; 3]>() as u32;
const MATRIX_BYTES: u32 = size_of::<Mat4>() as u32;
const VERTEX: &str = "#version 450\nlayout(location=0) in vec3 position;\nlayout(push_constant) uniform Projection { mat4 matrix; };\nvoid main(){gl_Position=matrix*vec4(position,1.0);}";
const FRAGMENT: &str = "#version 450\nvoid main(){}";
const ALPHA_VERTEX: &str = "#version 450\nlayout(location=0) in vec3 position;\nlayout(location=1) in vec2 uv;\nlayout(location=0) out vec2 texcoord;\nlayout(push_constant) uniform Projection { mat4 matrix; };\nvoid main(){gl_Position=matrix*vec4(position,1.0);texcoord=uv;}";
const ALPHA_FRAGMENT: &str = "#version 450\nlayout(set=0,binding=0) uniform sampler2D albedo;\nlayout(location=0) in vec2 texcoord;\nvoid main(){if(texture(albedo,texcoord).a<0.5)discard;}";
const UV_BYTES: u32 = size_of::<[f32; 2]>() as u32;

#[cfg(test)]
thread_local! {
    static LIVE_ATLASES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy, PartialEq)]
pub struct ShadowAlpha {
    pub offset: u32,
    pub set: vk::DescriptorSet,
}

#[derive(PartialEq)]
pub struct ShadowDraw {
    pub program: Option<(vk::Pipeline, vk::PipelineLayout, vk::DescriptorSet)>,
    pub geometry: MeshGeometry,
    pub stride: u32,
    pub alpha: Option<ShadowAlpha>,
    pub projection: Mat4,
    pub tile: vk::Rect2D,
    pub revision: u64,
}

#[derive(Clone, Copy)]
pub struct ShadowSample {
    pub view: vk::ImageView,
    pub ordinary: vk::Sampler,
    pub comparison: vk::Sampler,
}

impl ShadowSample {
    pub fn comparison(self) -> (vk::ImageView, vk::Sampler) {
        (self.view, self.comparison)
    }
}

pub struct ShadowAtlas {
    device: ash::Device,
    image: Option<images::Storage>,
    pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    layout: vk::PipelineLayout,
    pipelines: BTreeMap<(u32, Option<u32>), vk::Pipeline>,
    extent: vk::Extent2D,
    #[cfg(test)]
    sampler: vk::Sampler,
    comparison: vk::Sampler,
    ordinary: vk::Sampler,
}

impl ShadowAtlas {
    #[cfg(test)]
    pub fn live_allocations() -> usize {
        LIVE_ATLASES.get()
    }

    pub fn new(renderer: &Renderer, extent: vk::Extent2D) -> Result<Self> {
        let image = images::Storage::new(
            renderer,
            &images::Spec {
                extent,
                format: super::depth::FORMAT,
                levels: 1,
                samples: vk::SampleCountFlags::TYPE_1,
                usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
                aspect: vk::ImageAspectFlags::DEPTH,
            },
        )?;
        #[cfg(test)]
        LIVE_ATLASES.set(LIVE_ATLASES.get() + 1);
        let mut atlas = Self {
            device: renderer.device.clone(),
            image: Some(image),
            pass: vk::RenderPass::null(),
            framebuffer: vk::Framebuffer::null(),
            layout: vk::PipelineLayout::null(),
            pipelines: BTreeMap::new(),
            extent,
            #[cfg(test)]
            sampler: renderer.sampler_nearest,
            comparison: vk::Sampler::null(),
            ordinary: vk::Sampler::null(),
        };
        atlas.create_pass(renderer.desc_layout)?;
        atlas.comparison = unsafe {
            renderer.device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                    .border_color(vk::BorderColor::FLOAT_TRANSPARENT_BLACK)
                    .compare_enable(true)
                    .compare_op(vk::CompareOp::GREATER),
                None,
            )?
        };
        atlas.ordinary = unsafe {
            renderer.device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                    .border_color(vk::BorderColor::FLOAT_TRANSPARENT_BLACK),
                None,
            )?
        };
        Ok(atlas)
    }

    fn create_pass(&mut self, descriptor_layout: vk::DescriptorSetLayout) -> Result<()> {
        let attachments = [vk::AttachmentDescription::default()
            .format(super::depth::FORMAT)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let depth = vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
        let subpasses = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .depth_stencil_attachment(&depth)];
        let dependencies = [vk::SubpassDependency::default()
            .src_subpass(0)
            .dst_subpass(vk::SUBPASS_EXTERNAL)
            .src_stage_mask(vk::PipelineStageFlags::LATE_FRAGMENT_TESTS)
            .dst_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
            .src_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)];
        unsafe {
            self.pass = self.device.create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(&attachments)
                    .subpasses(&subpasses)
                    .dependencies(&dependencies),
                None,
            )?;
            self.framebuffer = self.device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(self.pass)
                    .attachments(&[self.image.as_ref().unwrap().view])
                    .width(self.extent.width)
                    .height(self.extent.height)
                    .layers(1),
                None,
            )?;
            self.layout = self.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[descriptor_layout])
                    .push_constant_ranges(&[vk::PushConstantRange::default()
                        .stage_flags(vk::ShaderStageFlags::VERTEX)
                        .size(MATRIX_BYTES)]),
                None,
            )?;
        }
        Ok(())
    }

    pub fn render_pass(&self) -> vk::RenderPass {
        self.pass
    }

    pub fn samples(&self) -> ShadowSample {
        ShadowSample {
            view: self.image.as_ref().unwrap().view,
            ordinary: self.ordinary,
            comparison: self.comparison,
        }
    }

    #[cfg(test)]
    pub fn sample(&self) -> (vk::ImageView, vk::Sampler) {
        (self.image.as_ref().unwrap().view, self.sampler)
    }

    pub fn allocation_bytes(&self) -> u64 {
        self.image.as_ref().unwrap().bytes
    }

    fn pipeline(&mut self, stride: u32, alpha: Option<u32>) -> Result<vk::Pipeline> {
        if let Some(pipeline) = self.pipelines.get(&(stride, alpha)) {
            return Ok(*pipeline);
        }
        ensure!(stride >= POSITION_BYTES, STRIDE_ERROR);
        ensure!(alpha.is_none_or(|offset| offset + UV_BYTES <= stride), STRIDE_ERROR);
        let vertex = shader::compile(
            if alpha.is_some() { ALPHA_VERTEX } else { VERTEX },
            Stage::Vertex,
            LABEL,
        )?;
        let fragment = shader::compile(
            if alpha.is_some() { ALPHA_FRAGMENT } else { FRAGMENT },
            Stage::Fragment,
            LABEL,
        )?;
        let mut modules = ShaderModules::new(&self.device);
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(modules.create(&vertex)?)
                .name(ENTRY),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(modules.create(&fragment)?)
                .name(ENTRY),
        ];
        let bindings = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(stride)
            .input_rate(vk::VertexInputRate::VERTEX)];
        let mut attributes = vec![
            vk::VertexInputAttributeDescription::default()
                .binding(0)
                .location(0)
                .format(vk::Format::R32G32B32_SFLOAT)
                .offset(0),
        ];
        if let Some(offset) = alpha {
            attributes.push(
                vk::VertexInputAttributeDescription::default()
                    .binding(0)
                    .location(1)
                    .format(vk::Format::R32G32_SFLOAT)
                    .offset(offset),
            );
        }
        let input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&bindings)
            .vertex_attribute_descriptions(&attributes);
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport =
            vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .depth_bias_enable(true)
            .depth_bias_slope_factor(SLOPE_BIAS)
            .line_width(1.0);
        let samples = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let depth = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::GREATER);
        let blend = vk::PipelineColorBlendStateCreateInfo::default();
        let dynamic = vk::PipelineDynamicStateCreateInfo::default()
            .dynamic_states(&[vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR]);
        let spec = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&samples)
            .depth_stencil_state(&depth)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(self.layout)
            .render_pass(self.pass)
            .subpass(0);
        let created = unsafe {
            self.device.create_graphics_pipelines(vk::PipelineCache::null(), &[spec], None)
        };
        let pipeline = match created {
            Ok(pipelines) => pipelines[0],
            Err((pipelines, error)) => {
                for pipeline in pipelines {
                    unsafe {
                        self.device.destroy_pipeline(pipeline, None);
                    }
                }
                return Err(error.into());
            }
        };
        self.pipelines.insert((stride, alpha), pipeline);
        Ok(pipeline)
    }

    pub fn record(&mut self, renderer: &Renderer, draws: &[ShadowDraw]) -> Result<()> {
        for draw in draws {
            if draw.program.is_none() {
                self.pipeline(draw.stride, draw.alpha.map(|alpha| alpha.offset))?;
            }
            ensure!(
                draw.tile.offset.x >= 0
                    && draw.tile.offset.y >= 0
                    && draw.tile.offset.x as u32 + draw.tile.extent.width <= self.extent.width
                    && draw.tile.offset.y as u32 + draw.tile.extent.height <= self.extent.height,
                TILE_ERROR
            );
        }
        unsafe {
            let clear = [vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue { depth: 0.0, stencil: 0 },
            }];
            self.device.cmd_begin_render_pass(
                renderer.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.pass)
                    .framebuffer(self.framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent: self.extent,
                    })
                    .clear_values(&clear),
                vk::SubpassContents::INLINE,
            );
            for draw in draws {
                let viewport = vk::Viewport {
                    x: draw.tile.offset.x as f32,
                    y: (draw.tile.offset.y as u32 + draw.tile.extent.height) as f32,
                    width: draw.tile.extent.width as f32,
                    height: -(draw.tile.extent.height as f32),
                    min_depth: 0.0,
                    max_depth: 1.0,
                };
                self.device.cmd_set_viewport(renderer.cmd, 0, &[viewport]);
                self.device.cmd_set_scissor(renderer.cmd, 0, &[draw.tile]);
                let (pipeline, layout, set) = draw.program.map_or_else(
                    || {
                        (
                            self.pipelines[&(draw.stride, draw.alpha.map(|alpha| alpha.offset))],
                            self.layout,
                            draw.alpha.map(|alpha| alpha.set),
                        )
                    },
                    |(pipeline, layout, set)| (pipeline, layout, Some(set)),
                );
                self.device.cmd_bind_pipeline(
                    renderer.cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipeline,
                );
                if let Some(set) = set {
                    self.device.cmd_bind_descriptor_sets(
                        renderer.cmd,
                        vk::PipelineBindPoint::GRAPHICS,
                        layout,
                        0,
                        &[set],
                        &[],
                    );
                }
                let bytes = std::slice::from_raw_parts(
                    draw.projection.as_ptr().cast(),
                    MATRIX_BYTES as usize,
                );
                self.device.cmd_push_constants(
                    renderer.cmd,
                    layout,
                    vk::ShaderStageFlags::VERTEX,
                    0,
                    bytes,
                );
                self.device.cmd_bind_vertex_buffers(
                    renderer.cmd,
                    0,
                    &[draw.geometry.vertices],
                    &[0],
                );
                self.device.cmd_bind_index_buffer(
                    renderer.cmd,
                    draw.geometry.indices,
                    0,
                    draw.geometry.index_type,
                );
                self.device.cmd_draw_indexed(renderer.cmd, draw.geometry.count, 1, 0, 0, 0);
            }
            self.device.cmd_end_render_pass(renderer.cmd);
        }
        Ok(())
    }
}

impl Drop for ShadowAtlas {
    fn drop(&mut self) {
        #[cfg(test)]
        LIVE_ATLASES.set(LIVE_ATLASES.get() - 1);
        unsafe {
            for (_, pipeline) in std::mem::take(&mut self.pipelines) {
                self.device.destroy_pipeline(pipeline, None);
            }
            self.device.destroy_sampler(self.comparison, None);
            self.device.destroy_sampler(self.ordinary, None);
            self.device.destroy_pipeline_layout(self.layout, None);
            self.device.destroy_framebuffer(self.framebuffer, None);
            self.device.destroy_render_pass(self.pass, None);
        }
        if let Some(image) = self.image.take() {
            image.destroy(&self.device);
        }
    }
}
