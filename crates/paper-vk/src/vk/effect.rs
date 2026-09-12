use crate::vk::Renderer;
use anyhow::{Context, Result, anyhow};
use ash::vk;
use paper_scene::hlsl::{HlslPair, SAMPLER_BINDING_SHIFT, TEXTURE_BINDING_SHIFT};
use paper_scene::shader::{Stage, Translated};

const MAX_EFFECT_SAMPLERS: u32 = 16;
const MAX_EFFECT_UBO_BYTES: usize = 16 * 1024;

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
    pub(crate) pipeline: vk::Pipeline,
    pub(crate) layout: vk::PipelineLayout,
    set_layout: vk::DescriptorSetLayout,
    pub(crate) set: vk::DescriptorSet,
    ubo: vk::Buffer,
    ubo_memory: vk::DeviceMemory,
    ubo_mapped: *mut u8,
    ubo_size: usize,
    pub sampler_count: u32,
    pub hlsl: bool,
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

fn compile_stages(
    vertex: &Translated,
    fragment: &Translated,
    hlsl: Option<&HlslPair>,
    label: &str,
) -> Result<(Vec<u32>, Vec<u32>, bool)> {
    if let Some(pair) = hlsl
        && paper_scene::hlsl::available()
        && !hlsl_skipped(label)
    {
        let compiled =
            paper_scene::hlsl::compile(&pair.vertex, Stage::Vertex, label).and_then(|vert| {
                paper_scene::hlsl::compile(&pair.fragment, Stage::Fragment, label)
                    .map(|frag| (vert, frag))
            });
        match compiled {
            Ok((vert, frag)) => return Ok((vert, frag, true)),
            Err(err) => {
                tracing::info!("skwd-wall-vk: hlsl path fell back to glsl for {label}: {err:#}")
            }
        }
    }
    let vert_words = paper_scene::shader::compile(&vertex.source, Stage::Vertex, label)?;
    let frag_words = paper_scene::shader::compile(&fragment.source, Stage::Fragment, label)?;
    Ok((vert_words, frag_words, false))
}

pub struct QuadBuffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
}

pub struct DynBuffer {
    pub buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    ptr: *mut u8,
    pub size: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PipelineKind {
    Effect,
    Particles(paper_scene::particles::ParticleBlend),
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
    pub fn create_quad_buffer(&self, width: u32, height: u32) -> Result<QuadBuffer> {
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        self.upload_quad([
            0.0, h, 0.0, 0.0, 0.0, //
            w, h, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 1.0, //
            w, 0.0, 0.0, 1.0, 1.0,
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

    fn upload_quad(&self, quad: [f32; 20]) -> Result<QuadBuffer> {
        unsafe {
            let size = std::mem::size_of_val(&quad) as u64;
            let buffer = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let reqs = self.device.get_buffer_memory_requirements(buffer);
            let memory = self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(reqs.size)
                    .memory_type_index(self.host_visible_index(reqs)?),
                None,
            )?;
            self.device.bind_buffer_memory(buffer, memory, 0)?;
            let ptr = self.device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty())?;
            std::ptr::copy_nonoverlapping(quad.as_ptr(), ptr.cast::<f32>(), quad.len());
            self.device.unmap_memory(memory);
            Ok(QuadBuffer { buffer, memory })
        }
    }

    pub fn destroy_quad_buffer(&self, quad: QuadBuffer) {
        unsafe {
            self.device.destroy_buffer(quad.buffer, None);
            self.device.free_memory(quad.memory, None);
        }
    }

    pub fn create_effect_pipeline(
        &mut self,
        vertex: &Translated,
        fragment: &Translated,
        hlsl: Option<&HlslPair>,
        label: &str,
    ) -> Result<EffectPipeline> {
        self.create_effect_pipeline_for(vertex, fragment, hlsl, label, vk::Format::R8G8B8A8_UNORM)
    }

    pub fn create_effect_pipeline_for(
        &mut self,
        vertex: &Translated,
        fragment: &Translated,
        hlsl: Option<&HlslPair>,
        label: &str,
        format: vk::Format,
    ) -> Result<EffectPipeline> {
        let render_pass = self.scene_pass_for(format)?;
        self.create_pipeline_kind(vertex, fragment, hlsl, label, PipelineKind::Effect, render_pass)
    }

    pub fn create_particle_pipeline(
        &mut self,
        vertex: &Translated,
        fragment: &Translated,
        hlsl: Option<&HlslPair>,
        label: &str,
        blend: paper_scene::particles::ParticleBlend,
    ) -> Result<EffectPipeline> {
        self.ensure_scene_pipelines()?;
        let render_pass = self.scene_pass;
        self.create_pipeline_kind(
            vertex,
            fragment,
            hlsl,
            label,
            PipelineKind::Particles(blend),
            render_pass,
        )
    }

    pub fn create_dyn_buffer(&self, size: usize, usage: vk::BufferUsageFlags) -> Result<DynBuffer> {
        unsafe {
            let buffer = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size.max(4) as u64)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let reqs = self.device.get_buffer_memory_requirements(buffer);
            let allocation = (|| -> Result<_> {
                Ok(self.device.allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(reqs.size)
                        .memory_type_index(self.host_visible_index(reqs)?),
                    None,
                )?)
            })();
            let memory = match allocation {
                Ok(memory) => memory,
                Err(error) => {
                    self.device.destroy_buffer(buffer, None);
                    return Err(error);
                }
            };
            let mapping = self.device.bind_buffer_memory(buffer, memory, 0).and_then(|()| {
                self.device.map_memory(memory, 0, size.max(4) as u64, vk::MemoryMapFlags::empty())
            });
            let ptr = match mapping {
                Ok(ptr) => ptr.cast::<u8>(),
                Err(error) => {
                    self.device.destroy_buffer(buffer, None);
                    self.device.free_memory(memory, None);
                    return Err(error.into());
                }
            };
            Ok(DynBuffer { buffer, memory, ptr, size: size.max(4) })
        }
    }

    pub fn write_dyn_buffer(&self, buffer: &DynBuffer, bytes: &[u8]) {
        let len = bytes.len().min(buffer.size);
        if len > 0 && !buffer.ptr.is_null() {
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.ptr, len) };
        }
    }

    pub fn destroy_dyn_buffer(&self, buffer: DynBuffer) {
        unsafe {
            self.device.unmap_memory(buffer.memory);
            self.device.destroy_buffer(buffer.buffer, None);
            self.device.free_memory(buffer.memory, None);
        }
    }

    fn create_pipeline_kind(
        &mut self,
        vertex: &Translated,
        fragment: &Translated,
        hlsl: Option<&HlslPair>,
        label: &str,
        kind: PipelineKind,
        render_pass: vk::RenderPass,
    ) -> Result<EffectPipeline> {
        self.ensure_scene_pipelines()?;
        let pool = self.effect_pool()?;
        let shaders_started = std::time::Instant::now();
        let (vert_words, frag_words, hlsl) = compile_stages(vertex, fragment, hlsl, label)?;
        let shaders_ms = shaders_started.elapsed().as_secs_f64() * 1000.0;
        let (sampler_count, ubo_size) = effect_caps(vertex, fragment)?;

        unsafe {
            let stages_all = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
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
            let set_layout = self.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?;
            let layouts = [set_layout];
            let layout = self.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts),
                None,
            )?;

            let buffer = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(ubo_size as u64)
                    .usage(vk::BufferUsageFlags::UNIFORM_BUFFER)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let reqs = self.device.get_buffer_memory_requirements(buffer);
            let ubo_memory = self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(reqs.size)
                    .memory_type_index(self.host_visible_index(reqs)?),
                None,
            )?;
            self.device.bind_buffer_memory(buffer, ubo_memory, 0)?;
            let mapped = self
                .device
                .map_memory(ubo_memory, 0, ubo_size as u64, vk::MemoryMapFlags::empty())?
                .cast::<u8>();

            let set = self.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&layouts),
            )?[0];
            let infos = [vk::DescriptorBufferInfo::default()
                .buffer(buffer)
                .offset(0)
                .range(ubo_size as u64)];
            self.device.update_descriptor_sets(
                &[vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&infos)],
                &[],
            );

            let vert_module = self.device.create_shader_module(
                &vk::ShaderModuleCreateInfo::default().code(&vert_words),
                None,
            )?;
            let frag_module = self.device.create_shader_module(
                &vk::ShaderModuleCreateInfo::default().code(&frag_words),
                None,
            )?;
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(vert_module)
                    .name(c"main"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(frag_module)
                    .name(c"main"),
            ];
            let stride = match kind {
                PipelineKind::Effect => 20,
                PipelineKind::Particles(_) => {
                    4 * paper_scene::particles::SPRITE_FLOATS_PER_VERTEX as u32
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
                PipelineKind::Effect => vec![
                    attr(0, vk::Format::R32G32B32_SFLOAT, 0),
                    attr(1, vk::Format::R32G32_SFLOAT, 12),
                ],
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
                PipelineKind::Effect => vk::PrimitiveTopology::TRIANGLE_STRIP,
                PipelineKind::Particles(_) => vk::PrimitiveTopology::TRIANGLE_LIST,
            });
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
            let att = [match kind {
                PipelineKind::Effect => vk::PipelineColorBlendAttachmentState::default()
                    .color_write_mask(vk::ColorComponentFlags::RGBA),
                PipelineKind::Particles(blend) => particle_blend_state(blend),
            }];
            let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&att);
            let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
            let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);
            let pipeline_started = std::time::Instant::now();
            let pipeline = self
                .device
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
                        .layout(layout)
                        .render_pass(render_pass)
                        .subpass(0)],
                    None,
                )
                .map_err(|(_, err)| anyhow!("effect pipeline {label}: {err}"))?[0];
            tracing::debug!(
                label,
                shaders_ms,
                pipeline_ms = pipeline_started.elapsed().as_secs_f64() * 1000.0,
                "scene pipeline setup"
            );
            self.device.destroy_shader_module(vert_module, None);
            self.device.destroy_shader_module(frag_module, None);

            Ok(EffectPipeline {
                pipeline,
                layout,
                set_layout,
                set,
                ubo: buffer,
                ubo_memory,
                ubo_mapped: mapped,
                ubo_size,
                sampler_count,
                hlsl,
            })
        }
    }

    pub fn destroy_effect_pipeline(&self, pipe: EffectPipeline) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_pipeline(pipe.pipeline, None);
            self.device.destroy_pipeline_layout(pipe.layout, None);
            self.device.destroy_descriptor_set_layout(pipe.set_layout, None);
            self.device.unmap_memory(pipe.ubo_memory);
            self.device.destroy_buffer(pipe.ubo, None);
            self.device.free_memory(pipe.ubo_memory, None);
        }
    }

    pub fn run_effect_pass(
        &mut self,
        pipe: &EffectPipeline,
        quad: &QuadBuffer,
        target: &crate::vk::SceneTarget,
        inputs: &[(vk::ImageView, vk::Sampler)],
        uniforms: &[u8],
    ) -> Result<()> {
        self.begin_scene_batch()?;
        self.record_effect_pass(pipe, quad, target, inputs, uniforms);
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
            let mut writes = Vec::new();
            let mut infos = Vec::new();
            let mut slots = Vec::new();
            for slot in 0..pipe.sampler_count as usize {
                let Some((view, sampler)) =
                    inputs.get(slot).copied().or_else(|| inputs.first().copied())
                else {
                    continue;
                };
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
        target: &crate::vk::SceneTarget,
        inputs: &[(vk::ImageView, vk::Sampler)],
        uniforms: &[u8],
    ) {
        self.write_effect_inputs(pipe, inputs, uniforms);
        unsafe {
            let clear =
                [vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 0.0] } }];
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(target.render_pass)
                    .framebuffer(target.framebuffer())
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: target.extent,
                    })
                    .clear_values(&clear),
                vk::SubpassContents::INLINE,
            );
            self.device.cmd_set_viewport(self.cmd, 0, &[viewport(target.extent, pipe.hlsl)]);
            self.device.cmd_set_scissor(
                self.cmd,
                0,
                &[vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: target.extent }],
            );
            self.device.cmd_bind_pipeline(self.cmd, vk::PipelineBindPoint::GRAPHICS, pipe.pipeline);
            self.device.cmd_bind_descriptor_sets(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.layout,
                0,
                &[pipe.set],
                &[],
            );
            self.device.cmd_bind_vertex_buffers(self.cmd, 0, &[quad.buffer], &[0]);
            self.device.cmd_draw(self.cmd, 4, 1, 0, 0);
            self.device.cmd_end_render_pass(self.cmd);
        }
    }
}

#[cfg(test)]
mod tests;
