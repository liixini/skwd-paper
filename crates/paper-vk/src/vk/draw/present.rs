use super::super::{FrameImages, FrameViews, Renderer};
use super::model::QueueGuard;
use anyhow::{Context, Result};
use ash::vk;

impl Renderer {
    pub(in crate::vk) fn queue_guard(&self) -> QueueGuard {
        #[cfg(feature = "shared-device")]
        crate::shared::lock_queue_family(self.queue_family);
        QueueGuard {
            #[cfg(feature = "shared-device")]
            family: self.queue_family,
        }
    }

    pub fn draw(&mut self, frame: &FrameImages, uv: [f32; 4]) -> Result<()> {
        let views = FrameViews {
            luma_view: frame.luma_view,
            chroma_view: frame.chroma_view,
            pre_barrier: true,
        };
        let imgs = [frame.luma_img, frame.chroma_img];
        self.draw_with(&views, Some(imgs), uv)
    }

    pub(crate) fn draw_inner(&mut self, views: &FrameViews, uv: [f32; 4]) -> Result<()> {
        self.draw_with(views, None, uv)
    }

    fn draw_with(
        &mut self,
        views: &FrameViews,
        barrier_imgs: Option<[vk::Image; 2]>,
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

            let infos_l = [vk::DescriptorImageInfo::default()
                .sampler(self.sampler)
                .image_view(views.luma_view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let infos_c = [vk::DescriptorImageInfo::default()
                .sampler(self.sampler)
                .image_view(views.chroma_view)
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

            if views.pre_barrier
                && let Some(imgs) = barrier_imgs
            {
                let barriers = imgs.map(|img| {
                    vk::ImageMemoryBarrier::default()
                        .src_access_mask(vk::AccessFlags::empty())
                        .dst_access_mask(vk::AccessFlags::SHADER_READ)
                        .old_layout(vk::ImageLayout::UNDEFINED)
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                        .dst_queue_family_index(self.queue_family)
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
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &barriers,
                );
            }

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

            let waits = [self.acquire_sem];
            let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            let cmds = [self.cmd];
            let signals = [self.render_sem];
            self.device.reset_fences(&[self.fence])?;
            {
                let _qg = self.queue_guard();
                self.device.queue_submit(
                    self.queue,
                    &[vk::SubmitInfo::default()
                        .wait_semaphores(&waits)
                        .wait_dst_stage_mask(&stages)
                        .command_buffers(&cmds)
                        .signal_semaphores(&signals)],
                    self.fence,
                )?;
            }

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
                Ok(_) | Err(vk::Result::SUBOPTIMAL_KHR | vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    if matches!(present, Err(vk::Result::ERROR_OUT_OF_DATE_KHR)) {
                        self.recreate_swapchain()?;
                    }
                    Ok(())
                }
                Err(err) => Err(err).context("present"),
            }
        }
    }

    #[cfg(feature = "shared-device")]
    pub(in crate::vk) fn begin_frame_cmd(&self) -> Result<()> {
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            self.device.begin_command_buffer(
                self.cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub(super) fn write_view_set(
        &mut self,
        set: vk::DescriptorSet,
        lv: vk::ImageView,
        cv: vk::ImageView,
    ) {
        let infos_l = [vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(lv)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let infos_c = [vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(cv)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        unsafe {
            self.device.update_descriptor_sets(
                &[
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&infos_l),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&infos_c),
                ],
                &[],
            );
        }
    }

    #[cfg(feature = "shared-device")]
    pub(super) fn write_plane_set(
        &mut self,
        set: vk::DescriptorSet,
        image: vk::Image,
    ) -> Result<()> {
        let (lv, cv) = self.plane_views_for(image)?;
        let infos_l = [vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(lv)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let infos_c = [vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(cv)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        unsafe {
            self.device.update_descriptor_sets(
                &[
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&infos_l),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&infos_c),
                ],
                &[],
            );
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub(super) fn sample_barrier(
        &self,
        barriers: &[vk::ImageMemoryBarrier],
        dst: vk::PipelineStageFlags,
    ) {
        unsafe {
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::ALL_COMMANDS,
                dst,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                barriers,
            );
        }
    }

    #[cfg(feature = "shared-device")]
    pub(super) fn begin_clear_pass(&self, fb: vk::Framebuffer) {
        let clear =
            [vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] } }];
        unsafe {
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.render_pass)
                    .framebuffer(fb)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: self.extent,
                    })
                    .clear_values(&clear),
                vk::SubpassContents::INLINE,
            );
        }
    }

    pub(super) fn push_blit(&self, uv: [f32; 4]) {
        #[repr(C)]
        struct BlitPush {
            uv: [f32; 4],
            fill: i32,
            blur: f32,
            dim: f32,
        }
        let (blur, dim) = crate::surface::effects();
        let push = BlitPush { uv, fill: crate::fill_flag(), blur, dim };
        unsafe {
            self.device.cmd_push_constants(
                self.cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                std::slice::from_raw_parts((&raw const push).cast(), 28),
            );
        }
    }

    #[cfg(feature = "shared-device")]
    pub(super) fn draw_fullscreen(
        &self,
        pipeline: vk::Pipeline,
        set: vk::DescriptorSet,
        uv: &[f32; 4],
    ) {
        unsafe {
            self.device.cmd_bind_pipeline(self.cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
            self.device.cmd_bind_descriptor_sets(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[set],
                &[],
            );
            self.push_blit(*uv);
            self.device.cmd_draw(self.cmd, 3, 1, 0, 0);
        }
    }
}
