use super::model::avvk_frame;
use crate::vk::Renderer;
use anyhow::{Context, Result};
use ash::vk;

impl Renderer {
    #[cfg(feature = "shared-device")]
    pub fn draw_avvk(
        &mut self,
        frame: &ffmpeg_the_third::frame::Video,
        uv: [f32; 4],
    ) -> Result<()> {
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            let (idx, _) = match self.swapchain_fns.acquire_next_image(
                self.swapchain,
                u64::MAX,
                self.acquire_sem,
                vk::Fence::null(),
            ) {
                Ok(pair) => pair,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.recreate_swapchain()?;
                    return Ok(());
                }
                Err(err) => return Err(err).context("acquire"),
            };

            let vkf = avvk_frame(frame)?;
            let count = vkf.frame.image_count().max(1);
            let image = vkf.image;
            let old_layout = vkf.layout;
            let (lv, cv) = self.plane_views_for(image)?;

            let infos_l = [vk::DescriptorImageInfo::default()
                .sampler(self.sampler)
                .image_view(lv)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let infos_c = [vk::DescriptorImageInfo::default()
                .sampler(self.sampler)
                .image_view(cv)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            self.device.update_descriptor_sets(
                &[
                    vk::WriteDescriptorSet::default()
                        .dst_set(self.desc_set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&infos_l),
                    vk::WriteDescriptorSet::default()
                        .dst_set(self.desc_set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&infos_c),
                ],
                &[],
            );

            self.device.begin_command_buffer(
                self.cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            let barrier = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .old_layout(old_layout)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })];
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barrier,
            );

            let clear =
                [vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] } }];
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.render_pass)
                    .framebuffer(self.framebuffers[idx as usize])
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: self.extent,
                    })
                    .clear_values(&clear),
                vk::SubpassContents::INLINE,
            );
            self.device.cmd_bind_pipeline(self.cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            self.device.cmd_bind_descriptor_sets(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.desc_set],
                &[],
            );
            self.push_blit(uv);
            self.device.cmd_draw(self.cmd, 3, 1, 0, 0);
            self.device.cmd_end_render_pass(self.cmd);
            self.device.end_command_buffer(self.cmd)?;

            let mut waits = vec![self.acquire_sem];
            let mut wait_values = vec![0u64];
            for plane in 0..count {
                waits.push(vkf.frame.semaphore(plane));
                wait_values.push(vkf.frame.semaphore_value(plane));
            }
            let stages = vec![vk::PipelineStageFlags::ALL_COMMANDS; waits.len()];
            let cmds = [self.cmd];
            let signals = [self.render_sem, vkf.frame.semaphore(0)];
            let signal_values = [0u64, vkf.signal];
            let mut timeline = vk::TimelineSemaphoreSubmitInfo::default()
                .wait_semaphore_values(&wait_values)
                .signal_semaphore_values(&signal_values);
            self.device.reset_fences(&[self.fence])?;
            {
                let _qg = self.queue_guard();
                self.device.queue_submit(
                    self.queue,
                    &[vk::SubmitInfo::default()
                        .push_next(&mut timeline)
                        .wait_semaphores(&waits)
                        .wait_dst_stage_mask(&stages)
                        .command_buffers(&cmds)
                        .signal_semaphores(&signals)],
                    self.fence,
                )?;
            }
            vkf.commit_sampled();
            let swapchains = [self.swapchain];
            let indices = [idx];
            let render_done = [self.render_sem];
            let present = {
                let _qg = self.queue_guard();
                self.swapchain_fns.queue_present(
                    self.queue,
                    &vk::PresentInfoKHR::default()
                        .wait_semaphores(&render_done)
                        .swapchains(&swapchains)
                        .image_indices(&indices),
                )
            };
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            match present {
                Ok(_) | Err(vk::Result::SUBOPTIMAL_KHR) => {}
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.recreate_swapchain()?,
                Err(err) => return Err(err).context("present"),
            }
            Ok(())
        }
    }
}
