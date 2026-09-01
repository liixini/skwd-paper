use super::model::{COLOR_RANGE, WaitSems};
use crate::ffmpeg_vulkan;
use crate::vk::{ExportImage, RenderTarget, Renderer, SceneTarget};
use anyhow::{Result, anyhow};
use ash::vk;

impl Renderer {
    #[cfg(feature = "shared-device")]
    pub fn scene_export_blit_supported(&self) -> bool {
        unsafe {
            let src = self
                .instance
                .get_physical_device_format_properties(self.phys, vk::Format::R8G8B8A8_UNORM);
            let dst = self
                .instance
                .get_physical_device_format_properties(self.phys, vk::Format::B8G8R8A8_UNORM);
            src.optimal_tiling_features.contains(vk::FormatFeatureFlags::BLIT_SRC)
                && dst.linear_tiling_features.contains(vk::FormatFeatureFlags::BLIT_DST)
        }
    }

    #[cfg(feature = "shared-device")]
    pub fn blit_scene_to_export(
        &mut self,
        scene: &SceneTarget,
        export: &mut ExportImage,
    ) -> Result<()> {
        if scene.extent != self.extent {
            return Err(anyhow!(
                "direct scene blit extent {}x{} != presenter {}x{}",
                scene.extent.width,
                scene.extent.height,
                self.extent.width,
                self.extent.height
            ));
        }
        self.begin_frame_cmd()?;
        let acquire = export.owner.acquire(export.ready, self.queue_family);
        let release = export.owner.release(self.queue_family);
        unsafe {
            let pre = [
                vk::ImageMemoryBarrier::default()
                    .src_access_mask(
                        vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::SHADER_READ,
                    )
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(scene.image)
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
                self.cmd,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &pre,
            );
            let subresource = vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            };
            let far =
                vk::Offset3D { x: self.extent.width as i32, y: self.extent.height as i32, z: 1 };
            self.device.cmd_blit_image(
                self.cmd,
                scene.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                export.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageBlit {
                    src_subresource: subresource,
                    src_offsets: [vk::Offset3D::default(), far],
                    dst_subresource: subresource,
                    dst_offsets: [vk::Offset3D::default(), far],
                }],
                vk::Filter::NEAREST,
            );
            let post = [
                vk::ImageMemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(scene.image)
                    .subresource_range(COLOR_RANGE),
                vk::ImageMemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::MEMORY_READ)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::GENERAL)
                    .src_queue_family_index(release.src)
                    .dst_queue_family_index(release.dst)
                    .image(export.image)
                    .subresource_range(COLOR_RANGE),
            ];
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &post,
            );
        }
        self.end_and_submit(&WaitSems::new())?;
        export.ready = true;
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub fn copy_avvk_to_nv12(
        &mut self,
        export: &mut crate::vk::Nv12Export,
        frame: &ffmpeg_the_third::frame::Video,
        width: u32,
        height: u32,
    ) -> Result<()> {
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
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
        self.begin_frame_cmd()?;
        let acquire = export.owner.acquire(export.ready, self.queue_family);
        let release = export.owner.release(self.queue_family);
        unsafe {
            let pre = [
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
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &pre,
            );
            for (aspect, div) in
                [(vk::ImageAspectFlags::PLANE_0, 1), (vk::ImageAspectFlags::PLANE_1, 2)]
            {
                let sub = vk::ImageSubresourceLayers {
                    aspect_mask: aspect,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                };
                self.device.cmd_copy_image(
                    self.cmd,
                    frame.image(0),
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    export.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::ImageCopy {
                        src_subresource: sub,
                        src_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
                        dst_subresource: sub,
                        dst_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
                        extent: vk::Extent3D { width: width / div, height: height / div, depth: 1 },
                    }],
                );
            }
            let post = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::MEMORY_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(release.src)
                .dst_queue_family_index(release.dst)
                .image(export.image)
                .subresource_range(COLOR_RANGE)];
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &post,
            );
            self.device.end_command_buffer(self.cmd)?;
            let commands = [self.cmd];
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
            let _guard = self.queue_guard();
            self.device.queue_submit(self.queue, &[submit], self.fence)?;
        }
        frame.commit(
            0,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            vk::AccessFlags::TRANSFER_READ,
            signal_values[0],
        );
        export.ready = true;
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub(super) fn copy_rt_to_export(&self, rt: &RenderTarget, export: &ExportImage) {
        unsafe {
            let acquire = export.owner.acquire(export.ready, self.queue_family);
            let release = export.owner.release(self.queue_family);
            let pre_copy = [vk::ImageMemoryBarrier::default()
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
                .subresource_range(COLOR_RANGE)];
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &pre_copy,
            );
            let sub = vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            };
            self.device.cmd_copy_image(
                self.cmd,
                rt.image,
                vk::ImageLayout::GENERAL,
                export.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageCopy {
                    src_subresource: sub,
                    src_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
                    dst_subresource: sub,
                    dst_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
                    extent: vk::Extent3D {
                        width: self.extent.width,
                        height: self.extent.height,
                        depth: 1,
                    },
                }],
            );
            let post_copy = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::MEMORY_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(release.src)
                .dst_queue_family_index(release.dst)
                .image(export.image)
                .subresource_range(COLOR_RANGE)];
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &post_copy,
            );
        }
    }

    #[cfg(feature = "shared-device")]
    pub(super) fn acquire_direct_export(&self, export: &ExportImage) {
        let transfer = export.owner.acquire(export.ready, self.queue_family);
        if transfer.src == vk::QUEUE_FAMILY_IGNORED {
            return;
        }
        unsafe {
            let barriers = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                .old_layout(vk::ImageLayout::GENERAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(transfer.src)
                .dst_queue_family_index(transfer.dst)
                .image(export.image)
                .subresource_range(COLOR_RANGE)];
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barriers,
            );
        }
    }

    #[cfg(feature = "shared-device")]
    pub(super) fn release_direct_export(&self, export: &ExportImage) {
        let transfer = export.owner.release(self.queue_family);
        if transfer.dst == vk::QUEUE_FAMILY_IGNORED {
            return;
        }
        unsafe {
            let barriers = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                .dst_access_mask(vk::AccessFlags::empty())
                .old_layout(vk::ImageLayout::GENERAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(transfer.src)
                .dst_queue_family_index(transfer.dst)
                .image(export.image)
                .subresource_range(COLOR_RANGE)];
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barriers,
            );
        }
    }

    #[cfg(feature = "shared-device")]
    pub(super) fn end_and_submit(&self, waits: &WaitSems) -> Result<()> {
        unsafe {
            self.device.end_command_buffer(self.cmd)?;
            let cmds = [self.cmd];
            let stages = [vk::PipelineStageFlags::ALL_COMMANDS; 16];
            let mut timeline = vk::TimelineSemaphoreSubmitInfo::default()
                .wait_semaphore_values(waits.values())
                .signal_semaphore_values(waits.signal_values());
            self.device.reset_fences(&[self.fence])?;
            {
                let _qg = self.queue_guard();
                self.device.queue_submit(
                    self.queue,
                    &[vk::SubmitInfo::default()
                        .push_next(&mut timeline)
                        .wait_semaphores(waits.sems())
                        .wait_dst_stage_mask(&stages[..waits.len])
                        .signal_semaphores(waits.signal_sems())
                        .command_buffers(&cmds)],
                    self.fence,
                )?;
            }
        }
        Ok(())
    }
}
