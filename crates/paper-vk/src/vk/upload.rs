use anyhow::{Context, Result, anyhow};
use ash::vk;

use super::{FrameImages, FrameViews, Renderer};
use crate::decode::PlaneDesc;
use crate::dmabuf::DRM_MOD_INVALID;

pub(crate) fn import_modifier_gate(modifier_ext: bool, luma: u64, chroma: u64) -> Result<()> {
    if !modifier_ext {
        return Err(anyhow!("Vulkan device has no DRM format modifier support"));
    }
    for (plane, modifier) in [("luma", luma), ("chroma", chroma)] {
        if modifier == DRM_MOD_INVALID {
            return Err(anyhow!("{plane} plane has no explicit DRM format modifier"));
        }
    }
    Ok(())
}

pub(crate) fn unlisted_import_modifiers(
    listed: &[Vec<u64>; 2],
    luma: u64,
    chroma: u64,
) -> Vec<(&'static str, u64)> {
    [("luma", luma), ("chroma", chroma)]
        .into_iter()
        .zip(listed)
        .filter(|((_, modifier), listed)| !listed.contains(modifier))
        .map(|(entry, _)| entry)
        .collect()
}

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

pub struct FrameStream {
    device: Option<ash::Device>,
    staging: vk::Buffer,
    staging_mem: vk::DeviceMemory,
    staging_ptr: *mut u8,
    image: vk::Image,
    memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub extent: vk::Extent2D,
}

const RGBA_BYTES: u64 = 4;

#[cfg(test)]
thread_local! {
    static LIVE_STREAMS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LIVE_UPLOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LIVE_IMPORTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl FrameStream {
    #[cfg(test)]
    pub fn live_allocations() -> usize {
        LIVE_STREAMS.get()
    }
}

impl UploadPath {
    #[cfg(test)]
    pub fn live_allocations() -> usize {
        LIVE_UPLOADS.get()
    }
}

impl FrameImages {
    #[cfg(test)]
    pub fn live_allocations() -> usize {
        LIVE_IMPORTS.get()
    }
}

impl Drop for FrameStream {
    fn drop(&mut self) {
        let Some(device) = self.device.take() else { return };
        let _ = crate::shared::wait_idle(&device);
        unsafe {
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
            device.destroy_buffer(self.staging, None);
            device.free_memory(self.staging_mem, None);
        }
        #[cfg(test)]
        LIVE_STREAMS.set(LIVE_STREAMS.get() - 1);
    }
}
const CHROMA_BYTES: u64 = 2;

impl Renderer {
    unsafe fn upload_image(
        &self,
        format: vk::Format,
        width: u32,
        height: u32,
    ) -> Result<(vk::Image, vk::DeviceMemory, vk::ImageView)> {
        unsafe {
            let img = self.device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D { width, height, depth: 1 })
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
                    .subresource_range(super::images::color_range(0, 1)),
                None,
            )?;
            Ok((img, mem, view))
        }
    }

    unsafe fn upload_staging(&self, size: u64) -> Result<(vk::Buffer, vk::DeviceMemory, *mut u8)> {
        unsafe {
            let staging = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
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
            Ok((staging, staging_mem, staging_ptr))
        }
    }

    pub fn create_upload_path(&self, video_w: u32, video_h: u32) -> Result<UploadPath> {
        unsafe {
            let (cw, ch) = (video_w.div_ceil(2), video_h.div_ceil(2));
            let (luma_img, luma_mem, luma_view) =
                self.upload_image(vk::Format::R8_UNORM, video_w, video_h)?;
            let (chroma_img, chroma_mem, chroma_view) =
                self.upload_image(vk::Format::R8G8_UNORM, cw, ch)?;
            let staging_size = (u64::from(video_w) * u64::from(video_h))
                + (u64::from(cw) * u64::from(ch) * CHROMA_BYTES);
            let (staging, staging_mem, staging_ptr) = self.upload_staging(staging_size)?;
            #[cfg(test)]
            LIVE_UPLOADS.set(LIVE_UPLOADS.get() + 1);
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
        #[cfg(test)]
        LIVE_UPLOADS.set(LIVE_UPLOADS.get() - 1);
        let _ = crate::shared::wait_idle(&self.device);
        unsafe {
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

    pub fn create_frame_stream(&self, width: u32, height: u32) -> Result<FrameStream> {
        unsafe {
            let (image, memory, view) =
                self.upload_image(vk::Format::R8G8B8A8_UNORM, width, height)?;
            let (staging, staging_mem, staging_ptr) =
                self.upload_staging(u64::from(width) * u64::from(height) * RGBA_BYTES)?;
            #[cfg(test)]
            LIVE_STREAMS.set(LIVE_STREAMS.get() + 1);
            Ok(FrameStream {
                device: Some(self.device.clone()),
                staging,
                staging_mem,
                staging_ptr,
                image,
                memory,
                view,
                extent: vk::Extent2D { width, height },
            })
        }
    }

    pub fn upload_frame(&self, stream: &FrameStream, rgba: &[u8]) -> Result<()> {
        let vk::Extent2D { width, height } = stream.extent;
        let size = u64::from(width) * u64::from(height) * RGBA_BYTES;
        if rgba.len() as u64 != size {
            return Err(anyhow!("frame holds {} of {size} bytes", rgba.len()));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), stream.staging_ptr, rgba.len());
            self.submit_copies(
                stream.staging,
                &[(stream.image, 0, vk::Extent3D { width, height, depth: 1 })],
            )
        }
    }

    pub fn upload_video(
        &self,
        upload: &UploadPath,
        frame: &ffmpeg_the_third::frame::Video,
    ) -> Result<()> {
        self.upload_nv12(upload, frame.data(0), frame.stride(0), frame.data(1), frame.stride(1))
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
            self.submit_copies(
                up.staging,
                &[
                    (up.luma_img, 0, vk::Extent3D { width: lw, height: lh, depth: 1 }),
                    (
                        up.chroma_img,
                        u64::from(lw * lh),
                        vk::Extent3D { width: cw, height: ch, depth: 1 },
                    ),
                ],
            )
        }
    }

    unsafe fn submit_copies(
        &self,
        staging: vk::Buffer,
        copies: &[(vk::Image, u64, vk::Extent3D)],
    ) -> Result<()> {
        self.begin_frame_cmd()?;
        unsafe {
            let transition = |old, new, src, dst| -> Vec<_> {
                copies
                    .iter()
                    .map(|(img, _, _)| {
                        vk::ImageMemoryBarrier::default()
                            .src_access_mask(src)
                            .dst_access_mask(dst)
                            .old_layout(old)
                            .new_layout(new)
                            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                            .image(*img)
                            .subresource_range(super::images::color_range(0, 1))
                    })
                    .collect()
            };
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &transition(
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::empty(),
                    vk::AccessFlags::TRANSFER_WRITE,
                ),
            );
            for (img, offset, extent) in copies {
                self.device.cmd_copy_buffer_to_image(
                    self.cmd,
                    staging,
                    *img,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::BufferImageCopy {
                        buffer_offset: *offset,
                        buffer_row_length: 0,
                        buffer_image_height: 0,
                        image_subresource: super::images::color_layers(0),
                        image_offset: vk::Offset3D::default(),
                        image_extent: *extent,
                    }],
                );
            }
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &transition(
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::AccessFlags::SHADER_READ,
                ),
            );
            self.device.end_command_buffer(self.cmd)?;
            let cmds = [self.cmd];
            self.submit_fenced(&[vk::SubmitInfo::default().command_buffers(&cmds)])?;
            self.wait_render()?;
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
        import_modifier_gate(self.drm_modifier_ext, luma.modifier, chroma.modifier)?;
        let unlisted =
            unlisted_import_modifiers(&self.import_modifiers, luma.modifier, chroma.modifier);
        if !unlisted.is_empty()
            && !self.import_modifier_warned.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            tracing::warn!(
                "skwd-wall-vk: VAAPI frame modifiers not listed by Vulkan for single-plane import: {}",
                unlisted
                    .iter()
                    .map(|(plane, modifier)| format!("{plane}={modifier:#018x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
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
        #[cfg(test)]
        LIVE_IMPORTS.set(LIVE_IMPORTS.get() + 1);
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
        #[cfg(test)]
        LIVE_IMPORTS.set(LIVE_IMPORTS.get() - 1);
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

#[cfg(test)]
mod tests;
