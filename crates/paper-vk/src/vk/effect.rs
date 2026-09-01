use crate::vk::Renderer;
use anyhow::{Context, Result, anyhow};
use ash::vk;
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

pub struct EffectPipeline {
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
    set_layout: vk::DescriptorSetLayout,
    set: vk::DescriptorSet,
    ubo: vk::Buffer,
    ubo_memory: vk::DeviceMemory,
    ubo_mapped: *mut u8,
    ubo_size: usize,
    pub sampler_count: u32,
}

pub struct QuadBuffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
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
        label: &str,
    ) -> Result<EffectPipeline> {
        self.ensure_scene_pipelines()?;
        let pool = self.effect_pool()?;
        let vert_words = paper_scene::shader::compile(&vertex.source, Stage::Vertex, label)?;
        let frag_words = paper_scene::shader::compile(&fragment.source, Stage::Fragment, label)?;
        let (sampler_count, ubo_size) = effect_caps(vertex, fragment)?;

        unsafe {
            let mut bindings = vec![
                vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
            ];
            for slot in 0..sampler_count {
                bindings.push(
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(slot + 1)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
                );
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
            let bind_desc = [vk::VertexInputBindingDescription::default()
                .binding(0)
                .stride(20)
                .input_rate(vk::VertexInputRate::VERTEX)];
            let attrs = [
                vk::VertexInputAttributeDescription::default()
                    .location(0)
                    .binding(0)
                    .format(vk::Format::R32G32B32_SFLOAT)
                    .offset(0),
                vk::VertexInputAttributeDescription::default()
                    .location(1)
                    .binding(0)
                    .format(vk::Format::R32G32_SFLOAT)
                    .offset(12),
            ];
            let vi = vk::PipelineVertexInputStateCreateInfo::default()
                .vertex_binding_descriptions(&bind_desc)
                .vertex_attribute_descriptions(&attrs);
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
                .color_write_mask(vk::ColorComponentFlags::RGBA)];
            let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&att);
            let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
            let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);
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
                        .render_pass(self.scene_pass)
                        .subpass(0)],
                    None,
                )
                .map_err(|(_, err)| anyhow!("effect pipeline {label}: {err}"))?[0];
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

    pub fn record_effect_pass(
        &mut self,
        pipe: &EffectPipeline,
        quad: &QuadBuffer,
        target: &crate::vk::SceneTarget,
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
                writes.push(
                    vk::WriteDescriptorSet::default()
                        .dst_set(pipe.set)
                        .dst_binding(slot as u32 + 1)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(info),
                );
            }
            if !writes.is_empty() {
                self.device.update_descriptor_sets(&writes, &[]);
            }

            let clear =
                [vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 0.0] } }];
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.scene_pass)
                    .framebuffer(target.framebuffer())
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: target.extent,
                    })
                    .clear_values(&clear),
                vk::SubpassContents::INLINE,
            );
            self.device.cmd_set_viewport(
                self.cmd,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: target.extent.width as f32,
                    height: target.extent.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
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
