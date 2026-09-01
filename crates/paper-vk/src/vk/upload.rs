use anyhow::{Context, Result, anyhow};
use ash::vk;

use super::{FrameImages, FrameViews, Renderer};
use crate::decode::PlaneDesc;

pub struct UploadPath {
    staging: vk::Buffer,
    staging_mem: vk::DeviceMemory,
    staging_ptr: *mut u8,
    luma_img: vk::Image,
    luma_mem: vk::DeviceMemory,
    pub luma_view: vk::ImageView,
    chroma_img: vk::Image,
    chroma_mem: vk::DeviceMemory,
    pub chroma_view: vk::ImageView,
    luma_size: (u32, u32),
    chroma_size: (u32, u32),
}

impl Renderer {
    pub fn create_upload_path(&self, video_w: u32, video_h: u32) -> Result<UploadPath> {
        unsafe {
            let (cw, ch) = (video_w.div_ceil(2), video_h.div_ceil(2));
            let make_img = |format: vk::Format,
                            w: u32,
                            h: u32|
             -> Result<(vk::Image, vk::DeviceMemory, vk::ImageView)> {
                let img = self.device.create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(format)
                        .extent(vk::Extent3D { width: w, height: h, depth: 1 })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )?;
                let reqs = self.device.get_image_memory_requirements(img);
                let mem_props = self.instance.get_physical_device_memory_properties(self.phys);
                let idx = (0..mem_props.memory_type_count)
                    .find(|&i| {
                        reqs.memory_type_bits & (1 << i) != 0
                            && mem_props.memory_types[i as usize]
                                .property_flags
                                .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                    })
                    .ok_or_else(|| anyhow!("no device-local memory type"))?;
                let mem = self.device.allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(reqs.size)
                        .memory_type_index(idx),
                    None,
                )?;
                self.device.bind_image_memory(img, mem, 0)?;
                let view = self.device.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(img)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            base_mip_level: 0,
                            level_count: 1,
                            base_array_layer: 0,
                            layer_count: 1,
                        }),
                    None,
                )?;
                Ok((img, mem, view))
            };
            let (luma_img, luma_mem, luma_view) = make_img(vk::Format::R8_UNORM, video_w, video_h)?;
            let (chroma_img, chroma_mem, chroma_view) = make_img(vk::Format::R8G8_UNORM, cw, ch)?;

            let staging_size = (video_w as u64 * video_h as u64) + (cw as u64 * ch as u64 * 2);
            let staging = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(staging_size)
                    .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let reqs = self.device.get_buffer_memory_requirements(staging);
            let mem_props = self.instance.get_physical_device_memory_properties(self.phys);
            let idx = (0..mem_props.memory_type_count)
                .find(|&i| {
                    reqs.memory_type_bits & (1 << i) != 0
                        && mem_props.memory_types[i as usize].property_flags.contains(
                            vk::MemoryPropertyFlags::HOST_VISIBLE
                                | vk::MemoryPropertyFlags::HOST_COHERENT,
                        )
                })
                .ok_or_else(|| anyhow!("no host-visible memory type"))?;
            let staging_mem = self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(reqs.size)
                    .memory_type_index(idx),
                None,
            )?;
            self.device.bind_buffer_memory(staging, staging_mem, 0)?;
            let staging_ptr = self
                .device
                .map_memory(staging_mem, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())?
                .cast();

            Ok(UploadPath {
                staging,
                staging_mem,
                staging_ptr,
                luma_img,
                luma_mem,
                luma_view,
                chroma_img,
                chroma_mem,
                chroma_view,
                luma_size: (video_w, video_h),
                chroma_size: (cw, ch),
            })
        }
    }

    pub fn destroy_upload_path(&self, up: UploadPath) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_image_view(up.luma_view, None);
            self.device.destroy_image_view(up.chroma_view, None);
            self.device.destroy_image(up.luma_img, None);
            self.device.destroy_image(up.chroma_img, None);
            self.device.free_memory(up.luma_mem, None);
            self.device.free_memory(up.chroma_mem, None);
            self.device.destroy_buffer(up.staging, None);
            self.device.free_memory(up.staging_mem, None);
        }
    }

    pub fn upload_nv12(
        &self,
        up: &UploadPath,
        luma: &[u8],
        luma_pitch: usize,
        chroma: &[u8],
        chroma_pitch: usize,
    ) -> Result<()> {
        unsafe {
            let (lw, lh) = up.luma_size;
            let (cw, ch) = up.chroma_size;
            let mut dst = up.staging_ptr;
            for row in 0..lh as usize {
                std::ptr::copy_nonoverlapping(
                    luma.as_ptr().add(row * luma_pitch),
                    dst,
                    lw as usize,
                );
                dst = dst.add(lw as usize);
            }
            let chroma_base = up.staging_ptr.add((lw * lh) as usize);
            let mut dst = chroma_base;
            for row in 0..ch as usize {
                std::ptr::copy_nonoverlapping(
                    chroma.as_ptr().add(row * chroma_pitch),
                    dst,
                    cw as usize * 2,
                );
                dst = dst.add(cw as usize * 2);
            }

            self.device.begin_command_buffer(
                self.cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            let to_dst = [up.luma_img, up.chroma_img].map(|img| {
                vk::ImageMemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::empty())
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(img)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
            });
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &to_dst,
            );
            let sub = vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            };
            self.device.cmd_copy_buffer_to_image(
                self.cmd,
                up.staging,
                up.luma_img,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::BufferImageCopy {
                    buffer_offset: 0,
                    buffer_row_length: 0,
                    buffer_image_height: 0,
                    image_subresource: sub,
                    image_offset: vk::Offset3D::default(),
                    image_extent: vk::Extent3D { width: lw, height: lh, depth: 1 },
                }],
            );
            self.device.cmd_copy_buffer_to_image(
                self.cmd,
                up.staging,
                up.chroma_img,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::BufferImageCopy {
                    buffer_offset: (lw * lh) as u64,
                    buffer_row_length: 0,
                    buffer_image_height: 0,
                    image_subresource: sub,
                    image_offset: vk::Offset3D::default(),
                    image_extent: vk::Extent3D { width: cw, height: ch, depth: 1 },
                }],
            );
            let to_read = [up.luma_img, up.chroma_img].map(|img| {
                vk::ImageMemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(img)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
            });
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &to_read,
            );
            self.device.end_command_buffer(self.cmd)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            self.device.reset_fences(&[self.fence])?;
            let cmds = [self.cmd];
            let guard = self.queue_guard();
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().command_buffers(&cmds)],
                self.fence,
            )?;
            drop(guard);
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            Ok(())
        }
    }

    pub fn draw_views(
        &mut self,
        luma_view: vk::ImageView,
        chroma_view: vk::ImageView,
        uv: [f32; 4],
        pre_barrier: bool,
    ) -> Result<()> {
        let views = FrameViews { luma_view, chroma_view, pre_barrier };
        self.draw_inner(&views, uv)
    }

    pub fn import_nv12(
        &self,
        video_w: u32,
        video_h: u32,
        luma: &PlaneDesc,
        chroma: &PlaneDesc,
    ) -> Result<FrameImages> {
        if !self.supports_foreign_import() {
            return Err(anyhow!("Vulkan device has no foreign queue-family support"));
        }
        let (luma_img, luma_mem, luma_view) =
            self.import_plane(vk::Format::R8_UNORM, video_w, video_h, luma)?;
        let (chroma_img, chroma_mem, chroma_view) = match self.import_plane(
            vk::Format::R8G8_UNORM,
            video_w.div_ceil(2),
            video_h.div_ceil(2),
            chroma,
        ) {
            Ok(chroma) => chroma,
            Err(error) => {
                unsafe {
                    self.device.destroy_image_view(luma_view, None);
                    self.device.destroy_image(luma_img, None);
                    self.device.free_memory(luma_mem, None);
                }
                return Err(error);
            }
        };
        Ok(FrameImages {
            luma_img,
            luma_mem,
            luma_view,
            chroma_img,
            chroma_mem,
            chroma_view,
            ready: std::cell::Cell::new(false),
        })
    }

    fn import_plane(
        &self,
        format: vk::Format,
        w: u32,
        h: u32,
        plane: &PlaneDesc,
    ) -> Result<(vk::Image, vk::DeviceMemory, vk::ImageView)> {
        unsafe {
            let plane_layout = [vk::SubresourceLayout {
                offset: plane.offset as u64,
                size: 0,
                row_pitch: plane.pitch as u64,
                array_pitch: 0,
                depth_pitch: 0,
            }];
            let mut modifier_info = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
                .drm_format_modifier(plane.modifier)
                .plane_layouts(&plane_layout);
            let mut ext_mem = vk::ExternalMemoryImageCreateInfo::default()
                .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let image = self
                .device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .push_next(&mut modifier_info)
                        .push_next(&mut ext_mem)
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(format)
                        .extent(vk::Extent3D { width: w, height: h, depth: 1 })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                        .usage(vk::ImageUsageFlags::SAMPLED)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .context("create dmabuf image")?;

            let fd = libc::dup(plane.fd);
            if fd < 0 {
                self.device.destroy_image(image, None);
                return Err(anyhow!("dup dmabuf fd failed"));
            }
            let mut fd_props = vk::MemoryFdPropertiesKHR::default();
            if let Err(error) = self.ext_mem_fd.get_memory_fd_properties(
                vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
                fd,
                &mut fd_props,
            ) {
                let _ = libc::close(fd);
                self.device.destroy_image(image, None);
                return Err(error).context("memory fd properties");
            }
            let reqs = self.device.get_image_memory_requirements(image);
            let type_bits = reqs.memory_type_bits & fd_props.memory_type_bits;
            let mem_props = self.instance.get_physical_device_memory_properties(self.phys);
            let Some(type_index) =
                (0..mem_props.memory_type_count).find(|&i| type_bits & (1 << i) != 0)
            else {
                let _ = libc::close(fd);
                self.device.destroy_image(image, None);
                return Err(anyhow!("no compatible memory type for dmabuf"));
            };

            let mut import = vk::ImportMemoryFdInfoKHR::default()
                .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
                .fd(fd);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
            let mem = match self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .push_next(&mut import)
                    .push_next(&mut dedicated)
                    .allocation_size((plane.object_size as u64).max(reqs.size))
                    .memory_type_index(type_index),
                None,
            ) {
                Ok(mem) => mem,
                Err(error) => {
                    let _ = libc::close(fd);
                    self.device.destroy_image(image, None);
                    return Err(error).context("import dmabuf memory");
                }
            };
            if let Err(error) = self.device.bind_image_memory(image, mem, 0) {
                self.device.destroy_image(image, None);
                self.device.free_memory(mem, None);
                return Err(error).context("bind dmabuf memory");
            }

            let view = match self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    }),
                None,
            ) {
                Ok(view) => view,
                Err(error) => {
                    self.device.destroy_image(image, None);
                    self.device.free_memory(mem, None);
                    return Err(error).context("create dmabuf image view");
                }
            };
            Ok((image, mem, view))
        }
    }

    pub fn destroy_frame(&self, f: FrameImages) {
        unsafe {
            self.device.destroy_image_view(f.luma_view, None);
            self.device.destroy_image_view(f.chroma_view, None);
            self.device.destroy_image(f.luma_img, None);
            self.device.destroy_image(f.chroma_img, None);
            self.device.free_memory(f.luma_mem, None);
            self.device.free_memory(f.chroma_mem, None);
        }
    }
}
