use crate::vk::Renderer;
use anyhow::{Context, Result, anyhow};
use ash::vk;

pub struct SceneTexture {
    image: vk::Image,
    memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub sampler: vk::Sampler,
    set: vk::DescriptorSet,
}

pub struct SceneTarget {
    pub(super) image: vk::Image,
    pub(super) memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub sampler: vk::Sampler,
    pub(super) framebuffer: vk::Framebuffer,
    pub extent: vk::Extent2D,
    pub allocation_bytes: u64,
    pub repeat: bool,
}

pub struct SceneMesh {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    mapped: *mut f32,
    vertices: usize,
    index_offset: vk::DeviceSize,
    index_count: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SceneBlend {
    Alpha,
    Add,
    Copy,
    Screen,
}

#[derive(Clone)]
pub struct SceneQuad {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub tint: [f32; 4],
    pub angle: f32,
    pub texture: usize,
    pub blend: SceneBlend,
}

#[repr(C)]
struct LayerPush {
    rect: [f32; 4],
    uv: [f32; 4],
    tint: [f32; 4],
    canvas: [f32; 2],
    angle: f32,
    pad: f32,
}

fn timestamp_delta_ns(start: u64, end: u64, valid_bits: u32, period: f64) -> u64 {
    let mask = if valid_bits >= 64 { u64::MAX } else { (1u64 << valid_bits) - 1 };
    let ticks = end.wrapping_sub(start) & mask;
    (ticks as f64 * period).round() as u64
}

impl SceneTarget {
    pub(super) fn framebuffer(&self) -> ash::vk::Framebuffer {
        self.framebuffer
    }
}

impl Renderer {
    pub fn create_scene_mesh(
        &self,
        mesh: &paper_scene::puppet::Mesh,
        size: (f32, f32),
    ) -> Result<SceneMesh> {
        let width = size.0.abs().max(1.0);
        let height = size.1.abs().max(1.0);
        let vertices: Vec<[f32; 4]> = mesh
            .vertices
            .iter()
            .map(|vertex| {
                [
                    vertex.position[0] / width,
                    -vertex.position[1] / height,
                    vertex.uv[0],
                    vertex.uv[1],
                ]
            })
            .collect();
        let vertex_bytes = std::mem::size_of_val(vertices.as_slice()) as u64;
        let index_bytes = std::mem::size_of_val(mesh.indices.as_slice()) as u64;
        let size = vertex_bytes
            .checked_add(index_bytes)
            .ok_or_else(|| anyhow!("puppet buffer size overflow"))?;
        unsafe {
            let buffer = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER)
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
            std::ptr::copy_nonoverlapping(
                vertices.as_ptr().cast::<u8>(),
                ptr.cast::<u8>(),
                vertex_bytes as usize,
            );
            std::ptr::copy_nonoverlapping(
                mesh.indices.as_ptr().cast::<u8>(),
                ptr.cast::<u8>().add(vertex_bytes as usize),
                index_bytes as usize,
            );
            Ok(SceneMesh {
                buffer,
                memory,
                mapped: ptr.cast(),
                vertices: vertices.len(),
                index_offset: vertex_bytes,
                index_count: u32::try_from(mesh.indices.len())?,
            })
        }
    }

    pub fn destroy_scene_mesh(&self, mesh: SceneMesh) {
        unsafe {
            self.device.unmap_memory(mesh.memory);
            self.device.destroy_buffer(mesh.buffer, None);
            self.device.free_memory(mesh.memory, None);
        }
    }

    pub fn update_scene_mesh(
        &self,
        mesh: &SceneMesh,
        positions: &[[f32; 3]],
        size: (f32, f32),
    ) -> Result<()> {
        if positions.len() != mesh.vertices {
            return Err(anyhow!("puppet position count {} != {}", positions.len(), mesh.vertices));
        }
        let width = size.0.abs().max(1.0);
        let height = size.1.abs().max(1.0);
        unsafe {
            for (index, position) in positions.iter().enumerate() {
                mesh.mapped.add(index * 4).write(position[0] / width);
                mesh.mapped.add(index * 4 + 1).write(-position[1] / height);
            }
        }
        Ok(())
    }

    pub fn render_scene_mesh(
        &mut self,
        target: &SceneTarget,
        texture: &SceneTexture,
        mesh: &SceneMesh,
    ) -> Result<()> {
        self.begin_scene_batch()?;
        self.record_scene_mesh(target, texture, mesh);
        self.submit_scene_batch()
    }

    pub fn record_scene_mesh(
        &mut self,
        target: &SceneTarget,
        texture: &SceneTexture,
        mesh: &SceneMesh,
    ) {
        let canvas = [target.extent.width as f32, target.extent.height as f32];
        unsafe {
            let clear =
                [vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 0.0] } }];
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.scene_pass)
                    .framebuffer(target.framebuffer)
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
                    width: canvas[0],
                    height: canvas[1],
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            self.device.cmd_set_scissor(
                self.cmd,
                0,
                &[vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: target.extent }],
            );
            self.device.cmd_bind_pipeline(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_puppet,
            );
            self.device.cmd_bind_descriptor_sets(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout_layer,
                0,
                &[texture.set],
                &[],
            );
            self.device.cmd_bind_vertex_buffers(self.cmd, 0, &[mesh.buffer], &[0]);
            self.device.cmd_bind_index_buffer(
                self.cmd,
                mesh.buffer,
                mesh.index_offset,
                vk::IndexType::UINT16,
            );
            let push = LayerPush {
                rect: [canvas[0] * 0.5, canvas[1] * 0.5, canvas[0], canvas[1]],
                uv: [0.0; 4],
                tint: [1.0; 4],
                canvas,
                angle: 0.0,
                pad: 0.0,
            };
            self.device.cmd_push_constants(
                self.cmd,
                self.pipeline_layout_layer,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                std::slice::from_raw_parts((&raw const push).cast(), 64),
            );
            self.device.cmd_draw_indexed(self.cmd, mesh.index_count, 1, 0, 0, 0);
            self.device.cmd_end_render_pass(self.cmd);
        }
    }

    pub(super) fn effect_pool(&mut self) -> Result<vk::DescriptorPool> {
        if self.fx_pool == vk::DescriptorPool::null() {
            let sizes = [
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(512),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(4096),
            ];
            self.fx_pool = unsafe {
                self.device.create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default().max_sets(512).pool_sizes(&sizes),
                    None,
                )?
            };
        }
        Ok(self.fx_pool)
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

    pub fn ensure_scene_pool(&mut self, textures: u32) -> Result<()> {
        if self.scene_pool != vk::DescriptorPool::null() {
            return Ok(());
        }
        let sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(textures.max(1) * 2)];
        self.scene_pool = unsafe {
            self.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                    .max_sets(textures.max(1))
                    .pool_sizes(&sizes),
                None,
            )?
        };
        Ok(())
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
        let sampler = match (clamp, nearest) {
            (true, false) => self.sampler,
            (false, false) => self.sampler_repeat,
            (true, true) => self.sampler_nearest,
            (false, true) => self.sampler_nearest_repeat,
        };
        let expect = width as usize * height as usize * 4;
        if rgba.len() < expect {
            return Err(anyhow!("texture payload {} < {expect}", rgba.len()));
        }
        unsafe {
            let image = self
                .device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(vk::Format::R8G8B8A8_UNORM)
                        .extent(vk::Extent3D { width, height, depth: 1 })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
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
                    .size(expect as u64)
                    .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let sreqs = self.device.get_buffer_memory_requirements(staging);
            let smem = self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(sreqs.size)
                    .memory_type_index(self.host_visible_index(sreqs)?),
                None,
            )?;
            self.device.bind_buffer_memory(staging, smem, 0)?;
            let ptr = self
                .device
                .map_memory(smem, 0, expect as u64, vk::MemoryMapFlags::empty())
                .context("tex map")?;
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), ptr.cast::<u8>(), expect);
            self.device.unmap_memory(smem);

            self.begin_frame_cmd()?;
            let range = vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            };
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(image)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .subresource_range(range)],
            );
            self.device.cmd_copy_buffer_to_image(
                self.cmd,
                staging,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::BufferImageCopy::default()
                    .image_subresource(vk::ImageSubresourceLayers {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        mip_level: 0,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
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
                    .image(image)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .subresource_range(range)],
            );
            self.device.end_command_buffer(self.cmd)?;
            let cmds = [self.cmd];
            self.device.reset_fences(&[self.fence])?;
            {
                let _guard = self.queue_guard();
                self.device
                    .queue_submit(
                        self.queue,
                        &[vk::SubmitInfo::default().command_buffers(&cmds)],
                        self.fence,
                    )
                    .context("tex submit")?;
            }
            self.device.wait_for_fences(&[self.fence], true, u64::MAX).context("tex fence")?;
            self.device.destroy_buffer(staging, None);
            self.device.free_memory(smem, None);

            let view = self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .subresource_range(range),
                None,
            )?;
            let layouts = [self.desc_layout];
            let set = self.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.scene_pool)
                    .set_layouts(&layouts),
            )?[0];
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
            Ok(SceneTexture { image, memory, view, sampler, set })
        }
    }

    pub fn create_view_slot(&mut self) -> Result<SceneTexture> {
        let layouts = [self.desc_layout];
        let set = unsafe {
            self.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.scene_pool)
                    .set_layouts(&layouts),
            )?[0]
        };
        Ok(SceneTexture {
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            sampler: self.sampler,
            set,
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

    pub fn destroy_scene_texture(&self, texture: SceneTexture) {
        unsafe {
            if texture.set != vk::DescriptorSet::null()
                && self.scene_pool != vk::DescriptorPool::null()
            {
                let _ = self.device.free_descriptor_sets(self.scene_pool, &[texture.set]);
            }
            if texture.image == vk::Image::null() {
                return;
            }
            self.device.destroy_image_view(texture.view, None);
            self.device.destroy_image(texture.image, None);
            self.device.free_memory(texture.memory, None);
        }
    }

    pub fn create_scene_target(&mut self, width: u32, height: u32) -> Result<SceneTarget> {
        self.create_scene_target_opts(width, height, false)
    }

    pub fn create_scene_target_repeat(&mut self, width: u32, height: u32) -> Result<SceneTarget> {
        self.create_scene_target_opts(width, height, true)
    }

    fn create_scene_target_opts(
        &mut self,
        width: u32,
        height: u32,
        repeat: bool,
    ) -> Result<SceneTarget> {
        self.ensure_scene_pipelines()?;
        let sampler = if repeat { self.sampler_repeat } else { self.sampler };
        unsafe {
            let image = self.device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .extent(vk::Extent3D { width, height, depth: 1 })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(
                        vk::ImageUsageFlags::COLOR_ATTACHMENT
                            | vk::ImageUsageFlags::SAMPLED
                            | vk::ImageUsageFlags::TRANSFER_SRC,
                    )
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )?;
            let reqs = self.device.get_image_memory_requirements(image);
            let memory = self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(reqs.size)
                    .memory_type_index(self.device_local_index(reqs)?),
                None,
            )?;
            self.device.bind_image_memory(image, memory, 0)?;
            let view = self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    }),
                None,
            )?;
            let atts = [view];
            let framebuffer = self.device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(self.scene_pass)
                    .attachments(&atts)
                    .width(width)
                    .height(height)
                    .layers(1),
                None,
            )?;
            Ok(SceneTarget {
                image,
                memory,
                view,
                sampler,
                framebuffer,
                extent: vk::Extent2D { width, height },
                allocation_bytes: reqs.size,
                repeat,
            })
        }
    }

    pub fn destroy_scene_target(&self, target: SceneTarget) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_framebuffer(target.framebuffer, None);
            self.device.destroy_image_view(target.view, None);
            self.device.destroy_image(target.image, None);
            self.device.free_memory(target.memory, None);
        }
    }

    pub fn render_scene(
        &mut self,
        target: &SceneTarget,
        clear: [f32; 4],
        quads: &[SceneQuad],
        textures: &[SceneTexture],
    ) -> Result<()> {
        let canvas = [target.extent.width as f32, target.extent.height as f32];
        self.render_scene_with_canvas(target, canvas, clear, quads, textures)
    }

    pub fn render_scene_with_canvas(
        &mut self,
        target: &SceneTarget,
        canvas: [f32; 2],
        clear: [f32; 4],
        quads: &[SceneQuad],
        textures: &[SceneTexture],
    ) -> Result<()> {
        self.begin_scene_batch()?;
        self.record_scene_with_canvas(target, canvas, clear, quads, textures);
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
        self.record_scene_with_canvas(target, canvas, clear, quads, textures);
    }

    pub fn record_scene_with_canvas(
        &mut self,
        target: &SceneTarget,
        canvas: [f32; 2],
        clear: [f32; 4],
        quads: &[SceneQuad],
        textures: &[SceneTexture],
    ) {
        let canvas = [canvas[0].max(1.0), canvas[1].max(1.0)];
        unsafe {
            let clear_value = [vk::ClearValue { color: vk::ClearColorValue { float32: clear } }];
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.scene_pass)
                    .framebuffer(target.framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: target.extent,
                    })
                    .clear_values(&clear_value),
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
            let mut bound_blend = None;
            for quad in quads {
                let Some(texture) = textures.get(quad.texture) else {
                    continue;
                };
                if bound_blend != Some(quad.blend) {
                    self.device.cmd_bind_pipeline(
                        self.cmd,
                        vk::PipelineBindPoint::GRAPHICS,
                        match quad.blend {
                            SceneBlend::Alpha => self.pipeline_layer,
                            SceneBlend::Add => self.pipeline_layer_add,
                            SceneBlend::Copy => self.pipeline_layer_copy,
                            SceneBlend::Screen => self.pipeline_layer_screen,
                        },
                    );
                    bound_blend = Some(quad.blend);
                }
                self.device.cmd_bind_descriptor_sets(
                    self.cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipeline_layout_layer,
                    0,
                    &[texture.set],
                    &[],
                );
                let push = LayerPush {
                    rect: quad.rect,
                    uv: quad.uv,
                    tint: quad.tint,
                    canvas,
                    angle: quad.angle,
                    pad: if quad.blend == SceneBlend::Screen { 1.0 } else { 0.0 },
                };
                self.device.cmd_push_constants(
                    self.cmd,
                    self.pipeline_layout_layer,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    std::slice::from_raw_parts((&raw const push).cast(), 64),
                );
                self.device.cmd_draw(self.cmd, 4, 1, 0, 0);
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
            self.device.reset_fences(&[self.fence])?;
            {
                let _guard = self.queue_guard();
                self.device.queue_submit(
                    self.queue,
                    &[vk::SubmitInfo::default().command_buffers(&cmds)],
                    self.fence,
                )?;
            }
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            if self.scene_timestamp_active {
                let mut values = [0u64; 2];
                self.device.get_query_pool_results(
                    self.scene_query_pool.expect("timestamp pool checked"),
                    0,
                    &mut values,
                    vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
                )?;
                self.scene_gpu_time_ns = Some(timestamp_delta_ns(
                    values[0],
                    values[1],
                    self.scene_timestamp_valid_bits,
                    self.scene_timestamp_period,
                ));
            }
        }
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
    pub fn read_scene_target(&mut self, target: &SceneTarget) -> Result<(u32, u32, Vec<u8>)> {
        let (width, height) = (target.extent.width, target.extent.height);
        let size = u64::from(width) * u64::from(height) * 4;
        // Keep the readback allocation under the same drop-owned lifecycle as
        // presentation readbacks. Every command-recording, submission, and
        // wait failure below can return early; the buffer must not depend on
        // reaching a manual cleanup tail.
        let readback = self.create_readback_buf(size)?;
        unsafe {
            self.begin_frame_cmd()?;
            let range = vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            };
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
            self.device.cmd_copy_image_to_buffer(
                self.cmd,
                target.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                readback.buffer,
                &[vk::BufferImageCopy::default()
                    .image_subresource(vk::ImageSubresourceLayers {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        mip_level: 0,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
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
            self.device.reset_fences(&[self.fence])?;
            {
                let _guard = self.queue_guard();
                self.device.queue_submit(
                    self.queue,
                    &[vk::SubmitInfo::default().command_buffers(&cmds)],
                    self.fence,
                )?;
            }
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            let mut out = vec![0u8; size as usize];
            std::ptr::copy_nonoverlapping(readback.ptr, out.as_mut_ptr(), size as usize);
            Ok((width, height, out))
        }
    }
}
