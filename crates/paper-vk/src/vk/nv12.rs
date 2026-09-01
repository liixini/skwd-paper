use crate::ffmpeg_vulkan;
use crate::vk::Nv12Export;
use anyhow::{Context, Result, anyhow};
use ash::vk;
use std::cell::Cell;

const COLOR_RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};

struct QueueLock(u32);

impl QueueLock {
    fn acquire(family: u32) -> Self {
        crate::shared::lock_queue_family(family);
        Self(family)
    }
}

impl Drop for QueueLock {
    fn drop(&mut self) {
        crate::shared::unlock_queue_family(self.0);
    }
}

pub struct Nv12Presenter {
    instance: ash::Instance,
    device: ash::Device,
    phys: vk::PhysicalDevice,
    ext_mem_fd: ash::khr::external_memory_fd::Device,
    queue: vk::Queue,
    queue_family: u32,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    fence: vk::Fence,
    in_flight: Cell<bool>,
}

impl Nv12Presenter {
    pub fn new_shared(
        parts: (ash::Instance, vk::PhysicalDevice, ash::Device, u32, vk::Queue),
    ) -> Result<Self> {
        let (instance, phys, device, queue_family, queue) = parts;
        let ext_mem_fd = ash::khr::external_memory_fd::Device::new(&instance, &device);
        let command_pool = unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(queue_family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )
        }
        .context("create NV12 transfer command pool")?;
        let command_buffer = match unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
        } {
            Ok(buffers) => buffers[0],
            Err(err) => {
                unsafe { device.destroy_command_pool(command_pool, None) };
                return Err(err).context("allocate NV12 transfer command buffer");
            }
        };
        let fence = match unsafe {
            device.create_fence(
                &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )
        } {
            Ok(fence) => fence,
            Err(err) => {
                unsafe { device.destroy_command_pool(command_pool, None) };
                return Err(err).context("create NV12 transfer fence");
            }
        };
        Ok(Self {
            instance,
            device,
            phys,
            ext_mem_fd,
            queue,
            queue_family,
            command_pool,
            command_buffer,
            fence,
            in_flight: Cell::new(false),
        })
    }

    pub fn create_nv12_export(
        &self,
        width: u32,
        height: u32,
        modifiers: &[u64],
        allow_linear: bool,
    ) -> Result<Nv12Export> {
        if !modifiers.is_empty() {
            match self.create_nv12_export_tiling(
                width,
                height,
                vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT,
                modifiers,
            ) {
                Ok(export) => return Ok(export),
                Err(err) => {
                    tracing::info!(
                        "skwd-wall-vk: tiled NV12 presentation unavailable ({err:#}), using linear"
                    );
                }
            }
        }
        if !allow_linear {
            return Err(anyhow!("no importable NV12 presentation modifier"));
        }
        self.create_nv12_export_tiling(width, height, vk::ImageTiling::LINEAR, &[])
    }

    fn create_nv12_export_tiling(
        &self,
        width: u32,
        height: u32,
        tiling: vk::ImageTiling,
        modifiers: &[u64],
    ) -> Result<Nv12Export> {
        unsafe {
            let mut export = Nv12Export::empty(&self.device);
            let mut external = vk::ExternalMemoryImageCreateInfo::default()
                .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let mut modifier = vk::ImageDrmFormatModifierListCreateInfoEXT::default()
                .drm_format_modifiers(modifiers);
            let mut image = vk::ImageCreateInfo::default().push_next(&mut external);
            if tiling == vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT {
                image = image.push_next(&mut modifier);
            }
            export.image = self
                .device
                .create_image(
                    &image
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
                        .extent(vk::Extent3D { width, height, depth: 1 })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(tiling)
                        .usage(vk::ImageUsageFlags::TRANSFER_DST)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .context("create nv12 export image")?;
            if tiling == vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT {
                let modifiers =
                    ash::ext::image_drm_format_modifier::Device::new(&self.instance, &self.device);
                let mut properties = vk::ImageDrmFormatModifierPropertiesEXT::default();
                modifiers
                    .get_image_drm_format_modifier_properties(export.image, &mut properties)
                    .context("query nv12 export modifier")?;
                export.modifier = properties.drm_format_modifier;
            }
            let requirements = self.device.get_image_memory_requirements(export.image);
            let memory = self.instance.get_physical_device_memory_properties(self.phys);
            let memory_type = (0..memory.memory_type_count)
                .find(|&index| {
                    requirements.memory_type_bits & (1 << index) != 0
                        && memory.memory_types[index as usize]
                            .property_flags
                            .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                })
                .or_else(|| {
                    (0..memory.memory_type_count)
                        .find(|&index| requirements.memory_type_bits & (1 << index) != 0)
                })
                .ok_or_else(|| anyhow!("no memory type for nv12 export"))?;
            let mut external = vk::ExportMemoryAllocateInfo::default()
                .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(export.image);
            export.memory = self
                .device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .push_next(&mut external)
                        .push_next(&mut dedicated)
                        .allocation_size(requirements.size)
                        .memory_type_index(memory_type),
                    None,
                )
                .context("allocate nv12 export memory")?;
            self.device.bind_image_memory(export.image, export.memory, 0)?;
            let plane0 = self.device.get_image_subresource_layout(
                export.image,
                vk::ImageSubresource {
                    aspect_mask: vk::ImageAspectFlags::PLANE_0,
                    mip_level: 0,
                    array_layer: 0,
                },
            );
            let plane1 = self.device.get_image_subresource_layout(
                export.image,
                vk::ImageSubresource {
                    aspect_mask: vk::ImageAspectFlags::PLANE_1,
                    mip_level: 0,
                    array_layer: 0,
                },
            );
            export.fd = self
                .ext_mem_fd
                .get_memory_fd(
                    &vk::MemoryGetFdInfoKHR::default()
                        .memory(export.memory)
                        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT),
                )
                .context("export nv12 memory fd")?;
            export.plane0_offset = plane0.offset as u32;
            export.plane0_stride = plane0.row_pitch as u32;
            export.plane1_offset = plane1.offset as u32;
            export.plane1_stride = plane1.row_pitch as u32;
            Ok(export)
        }
    }

    pub fn copy_frame(
        &self,
        export: &mut Nv12Export,
        frame: &ffmpeg_the_third::frame::Video,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.begin_frame()?;
        let frame =
            ffmpeg_vulkan::Frame::from_video(frame).ok_or_else(|| anyhow!("no AVVkFrame"))?.lock();
        if !frame.held() {
            return Err(anyhow!("cannot lock AVVkFrame"));
        }
        let image_count = frame.image_count();
        if image_count != 1 {
            return Err(anyhow!(
                "NV12 copy requires one multiplane AVVkFrame image, got {image_count}"
            ));
        }
        let format = frame.format(0);
        if format != vk::Format::G8_B8R8_2PLANE_420_UNORM {
            return Err(anyhow!(
                "NV12 copy requires an NV12 AVVkFrame image, got {}",
                format.as_raw()
            ));
        }
        let semaphores = [frame.semaphore(0)];
        if semaphores[0] == vk::Semaphore::null() {
            return Err(anyhow!("NV12 copy requires an AVVkFrame timeline semaphore"));
        }
        let wait_values = [frame.semaphore_value(0)];
        let signal_values = [wait_values[0]
            .checked_add(1)
            .ok_or_else(|| anyhow!("AVVkFrame semaphore value overflow"))?];
        let acquire = export.owner.acquire(export.ready, self.queue_family);
        let release = export.owner.release(self.queue_family);
        unsafe {
            let barriers = [
                vk::ImageMemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .old_layout(frame.layout(0))
                    .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(frame.image(0))
                    .subresource_range(COLOR_RANGE),
                vk::ImageMemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::empty())
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .old_layout(if export.ready {
                        vk::ImageLayout::GENERAL
                    } else {
                        vk::ImageLayout::UNDEFINED
                    })
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .src_queue_family_index(acquire.src)
                    .dst_queue_family_index(acquire.dst)
                    .image(export.image)
                    .subresource_range(COLOR_RANGE),
            ];
            self.device.cmd_pipeline_barrier(
                self.command_buffer,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barriers,
            );
            for (aspect, divisor) in
                [(vk::ImageAspectFlags::PLANE_0, 1), (vk::ImageAspectFlags::PLANE_1, 2)]
            {
                let subresource = vk::ImageSubresourceLayers {
                    aspect_mask: aspect,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                };
                self.device.cmd_copy_image(
                    self.command_buffer,
                    frame.image(0),
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    export.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::ImageCopy {
                        src_subresource: subresource,
                        src_offset: vk::Offset3D::default(),
                        dst_subresource: subresource,
                        dst_offset: vk::Offset3D::default(),
                        extent: vk::Extent3D {
                            width: width / divisor,
                            height: height / divisor,
                            depth: 1,
                        },
                    }],
                );
            }
            let release = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::MEMORY_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(release.src)
                .dst_queue_family_index(release.dst)
                .image(export.image)
                .subresource_range(COLOR_RANGE)];
            self.device.cmd_pipeline_barrier(
                self.command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &release,
            );
            self.device.end_command_buffer(self.command_buffer)?;
            let commands = [self.command_buffer];
            let stages = [vk::PipelineStageFlags::ALL_COMMANDS];
            let mut timeline = vk::TimelineSemaphoreSubmitInfo::default()
                .wait_semaphore_values(&wait_values)
                .signal_semaphore_values(&signal_values);
            let submit = vk::SubmitInfo::default()
                .push_next(&mut timeline)
                .wait_semaphores(&semaphores)
                .wait_dst_stage_mask(&stages)
                .signal_semaphores(&semaphores)
                .command_buffers(&commands);
            self.device.reset_fences(&[self.fence])?;
            let _lock = QueueLock::acquire(self.queue_family);
            self.device.queue_submit(self.queue, &[submit], self.fence)?;
        }
        self.in_flight.set(true);
        frame.commit(
            0,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            vk::AccessFlags::TRANSFER_READ,
            signal_values[0],
        );
        self.wait()?;
        export.ready = true;
        Ok(())
    }

    fn begin_frame(&self) -> Result<()> {
        self.wait()?;
        unsafe {
            self.device.begin_command_buffer(
                self.command_buffer,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
        }
        Ok(())
    }

    fn wait(&self) -> Result<()> {
        if self.in_flight.get() {
            let result = unsafe { self.device.wait_for_fences(&[self.fence], true, u64::MAX) };
            self.in_flight.set(false);
            result?;
        }
        Ok(())
    }
}

impl Drop for Nv12Presenter {
    fn drop(&mut self) {
        unsafe {
            let _ = self.wait();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
        }
    }
}
