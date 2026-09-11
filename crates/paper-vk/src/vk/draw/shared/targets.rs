use super::super::model::Src;
use super::model::{AvvkFrame, WaitSems, avvk_frame};
use crate::vk::{ExportImage, FrameImages, ReadbackBuf, RenderTarget, Renderer};
use anyhow::{Context, Result, anyhow};
use ash::vk;
use ash::vk::Handle;

impl Renderer {
    #[cfg(feature = "shared-device")]
    pub fn render_video_texture(
        &mut self,
        target: &crate::vk::SceneTarget,
        src: &Src,
    ) -> Result<()> {
        if target.extent != self.extent || target.format != self.format {
            return Err(anyhow!("scene video target does not match its renderer"));
        }
        self.wait_frame_complete()?;
        let avf = Self::resolve_src(src)?;
        let mut waits = WaitSems::new();
        if let Some(avf) = &avf {
            avf.push_wait_sems(&mut waits);
        }
        self.begin_frame_cmd()?;
        self.acquire_imports(&[src]);
        self.bind_src(self.desc_set, src, &avf)?;
        if let Some(avf) = &avf {
            self.sample_barrier(&[avf.read_barrier()], vk::PipelineStageFlags::FRAGMENT_SHADER);
        }
        let clear =
            [vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] } }];
        unsafe {
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(target.render_pass)
                    .framebuffer(target.framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent: target.extent,
                    })
                    .clear_values(&clear),
                vk::SubpassContents::INLINE,
            );
        }
        self.draw_fullscreen(self.pipeline, self.desc_set, &src.texture_uv([1.0, 1.0, 0.0, 0.0]));
        unsafe { self.device.cmd_end_render_pass(self.cmd) };
        self.release_imports(&[src]);
        self.end_and_submit(&waits)?;
        Self::note_imports(&[src]);
        if let Some(avf) = avf {
            avf.commit_sampled();
        }
        Ok(())
    }

    pub fn wait_frame_complete(&self) -> Result<()> {
        unsafe { self.device.wait_for_fences(&[self.fence], true, u64::MAX)? };
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub fn render_pattern_to(
        &mut self,
        rt: &RenderTarget,
        export: &mut ExportImage,
        bar_x: u32,
    ) -> Result<()> {
        self.begin_frame_cmd()?;
        if export.direct_render {
            self.acquire_direct_export(export);
        }
        unsafe {
            let black = [vk::ClearValue {
                color: vk::ClearColorValue { float32: [0.05, 0.05, 0.05, 1.0] },
            }];
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.render_pass)
                    .framebuffer(rt.framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: self.extent,
                    })
                    .clear_values(&black),
                vk::SubpassContents::INLINE,
            );
            self.device.cmd_end_render_pass(self.cmd);
            let white =
                [vk::ClearValue { color: vk::ClearColorValue { float32: [1.0, 1.0, 1.0, 1.0] } }];
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.render_pass)
                    .framebuffer(rt.framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: bar_x as i32, y: 0 },
                        extent: vk::Extent2D { width: 60, height: self.extent.height },
                    })
                    .clear_values(&white),
                vk::SubpassContents::INLINE,
            );
            self.device.cmd_end_render_pass(self.cmd);
            if export.direct_render {
                self.release_direct_export(export);
            } else {
                self.copy_rt_to_export(rt, export);
            }
            self.device.end_command_buffer(self.cmd)?;
            let cmds = [self.cmd];
            self.device.reset_fences(&[self.fence])?;
            {
                let _qg = self.queue_guard();
                self.device.queue_submit(
                    self.queue,
                    &[vk::SubmitInfo::default().command_buffers(&cmds)],
                    self.fence,
                )?;
            }
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        export.ready = true;
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    fn resolve_src<'a>(src: &Src<'a>) -> Result<Option<AvvkFrame<'a>>> {
        match src {
            Src::Avvk(frame) => Ok(Some(avvk_frame(frame)?)),
            Src::Imported(_) | Src::Views(_, _) | Src::Rgba(_) => Ok(None),
        }
    }

    #[cfg(feature = "shared-device")]
    fn imported_frames<'a>(srcs: &[&'a Src<'a>]) -> Vec<&'a FrameImages> {
        let mut frames: Vec<&FrameImages> = Vec::new();
        for src in srcs {
            let Src::Imported(frame) = src else {
                continue;
            };
            if !frames.iter().any(|known| known.luma_img == frame.luma_img) {
                frames.push(*frame);
            }
        }
        frames
    }

    #[cfg(feature = "shared-device")]
    fn acquire_imports(&self, srcs: &[&Src]) {
        let frames = Self::imported_frames(srcs);
        if frames.is_empty() {
            return;
        }
        let barriers = frames
            .iter()
            .flat_map(|frame| {
                [frame.luma_img, frame.chroma_img].map(|image| {
                    vk::ImageMemoryBarrier::default()
                        .src_access_mask(vk::AccessFlags::empty())
                        .dst_access_mask(vk::AccessFlags::SHADER_READ)
                        .old_layout(if frame.ready.get() {
                            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
                        } else {
                            vk::ImageLayout::UNDEFINED
                        })
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                        .dst_queue_family_index(self.queue_family)
                        .image(image)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            base_mip_level: 0,
                            level_count: 1,
                            base_array_layer: 0,
                            layer_count: 1,
                        })
                })
            })
            .collect::<Vec<_>>();
        self.sample_barrier(
            &barriers,
            vk::PipelineStageFlags::VERTEX_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
        );
    }

    #[cfg(feature = "shared-device")]
    fn release_imports(&self, srcs: &[&Src]) {
        let frames = Self::imported_frames(srcs);
        if frames.is_empty() {
            return;
        }
        let barriers = frames
            .iter()
            .flat_map(|frame| {
                [frame.luma_img, frame.chroma_img].map(|image| {
                    vk::ImageMemoryBarrier::default()
                        .src_access_mask(vk::AccessFlags::SHADER_READ)
                        .dst_access_mask(vk::AccessFlags::empty())
                        .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .src_queue_family_index(self.queue_family)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                        .image(image)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            base_mip_level: 0,
                            level_count: 1,
                            base_array_layer: 0,
                            layer_count: 1,
                        })
                })
            })
            .collect::<Vec<_>>();
        unsafe {
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::VERTEX_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barriers,
            );
        }
    }

    #[cfg(feature = "shared-device")]
    fn note_imports(srcs: &[&Src]) {
        for frame in Self::imported_frames(srcs) {
            frame.ready.set(true);
        }
    }

    #[cfg(feature = "shared-device")]
    fn present_pipeline(&self, src: &Src) -> vk::Pipeline {
        match src {
            Src::Rgba(_) => self.pipeline_rgba,
            _ => self.pipeline,
        }
    }

    #[cfg(feature = "shared-device")]
    fn fade_pipeline(&self, src: &Src) -> vk::Pipeline {
        match src {
            Src::Rgba(_) => self.pipeline_rgba_fade,
            _ => self.pipeline_fade,
        }
    }

    #[cfg(feature = "shared-device")]
    fn ensure_src_pipelines(&mut self, srcs: &[&Src]) -> Result<()> {
        if srcs.iter().any(|src| matches!(src, Src::Rgba(_))) {
            self.ensure_rgba_pipelines()?;
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    fn bind_src(
        &mut self,
        set: vk::DescriptorSet,
        src: &Src,
        avf: &Option<AvvkFrame<'_>>,
    ) -> Result<()> {
        match (src, avf) {
            (Src::Imported(frame), _) => {
                self.write_view_set(set, frame.luma_view, frame.chroma_view);
                Ok(())
            }
            (Src::Views(lv, cv), _) => {
                self.write_view_set(set, *lv, *cv);
                Ok(())
            }
            (Src::Rgba(view), _) => {
                self.write_view_set(set, *view, *view);
                Ok(())
            }
            (Src::Avvk(_), Some(avf)) => self.write_plane_set(set, avf.image),
            (Src::Avvk(_), None) => Err(anyhow!("unresolved avvk src")),
        }
    }

    #[cfg(feature = "shared-device")]
    pub fn render_to(
        &mut self,
        rt: &RenderTarget,
        export: &mut ExportImage,
        src: &Src,
        uv: [f32; 4],
    ) -> Result<()> {
        let uv = src.texture_uv(uv);
        self.ensure_src_pipelines(&[src])?;
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let avf = Self::resolve_src(src)?;
        let mut waits = WaitSems::new();
        if let Some(avf) = &avf {
            avf.push_wait_sems(&mut waits);
        }
        self.begin_frame_cmd()?;
        if export.direct_render {
            self.acquire_direct_export(export);
        }
        self.acquire_imports(&[src]);
        self.bind_src(self.desc_set, src, &avf)?;
        if let Some(avf) = &avf {
            self.sample_barrier(&[avf.read_barrier()], vk::PipelineStageFlags::FRAGMENT_SHADER);
        }
        self.begin_clear_pass(rt.framebuffer);
        self.draw_fullscreen(self.present_pipeline(src), self.desc_set, &uv);
        unsafe { self.device.cmd_end_render_pass(self.cmd) };
        self.release_imports(&[src]);
        if export.direct_render {
            self.release_direct_export(export);
        } else {
            self.copy_rt_to_export(rt, export);
        }
        self.end_and_submit(&waits)?;
        Self::note_imports(&[src]);
        export.ready = true;
        if let Some(avf) = avf {
            avf.commit_sampled();
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub fn render_fade_to(
        &mut self,
        rt: &RenderTarget,
        export: &mut ExportImage,
        src_a: &Src,
        uv_a: [f32; 4],
        src_b: &Src,
        uv_b: [f32; 4],
        mix: f32,
    ) -> Result<()> {
        let uv_a = src_a.texture_uv(uv_a);
        let uv_b = src_b.texture_uv(uv_b);
        self.ensure_transition_pipelines()?;
        self.ensure_src_pipelines(&[src_a, src_b])?;
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let (avf_a, avf_b) = (Self::resolve_src(src_a)?, Self::resolve_src(src_b)?);
        let mut waits = WaitSems::new();
        for avf in avf_a.iter().chain(&avf_b) {
            avf.push_wait_sems(&mut waits);
        }
        self.begin_frame_cmd()?;
        if export.direct_render {
            self.acquire_direct_export(export);
        }
        self.acquire_imports(&[src_a, src_b]);
        self.bind_src(self.desc_set, src_a, &avf_a)?;
        self.bind_src(self.desc_set_b, src_b, &avf_b)?;
        let barriers: Vec<_> = avf_a.iter().chain(&avf_b).map(AvvkFrame::read_barrier).collect();
        if !barriers.is_empty() {
            self.sample_barrier(&barriers, vk::PipelineStageFlags::FRAGMENT_SHADER);
        }
        self.begin_clear_pass(rt.framebuffer);
        self.draw_fullscreen(self.present_pipeline(src_a), self.desc_set, &uv_a);
        let mix = mix.clamp(0.0, 1.0);
        unsafe { self.device.cmd_set_blend_constants(self.cmd, &[mix, mix, mix, mix]) };
        self.draw_fullscreen(self.fade_pipeline(src_b), self.desc_set_b, &uv_b);
        unsafe { self.device.cmd_end_render_pass(self.cmd) };
        self.release_imports(&[src_a, src_b]);
        if export.direct_render {
            self.release_direct_export(export);
        } else {
            self.copy_rt_to_export(rt, export);
        }
        self.end_and_submit(&waits)?;
        Self::note_imports(&[src_a, src_b]);
        export.ready = true;
        for avf in avf_a.into_iter().chain(avf_b) {
            avf.commit_sampled();
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub fn render_effect_to(
        &mut self,
        rt: &RenderTarget,
        export: &mut ExportImage,
        src_a: &Src,
        uv_a: [f32; 4],
        src_b: &Src,
        uv_b: [f32; 4],
        progress: f32,
        fx: usize,
    ) -> Result<()> {
        let uv_a = src_a.texture_uv(uv_a);
        let uv_b = src_b.texture_uv(uv_b);
        let pipe = self.effect_pipeline(fx)?;
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let (avf_a, avf_b) = (Self::resolve_src(src_a)?, Self::resolve_src(src_b)?);
        let mut waits = WaitSems::new();
        for avf in avf_a.iter().chain(&avf_b) {
            avf.push_wait_sems(&mut waits);
        }
        self.begin_frame_cmd()?;
        if export.direct_render {
            self.acquire_direct_export(export);
        }
        self.acquire_imports(&[src_a, src_b]);
        self.bind_src(self.desc_set, src_a, &avf_a)?;
        self.bind_src(self.desc_set_b, src_b, &avf_b)?;
        let barriers: Vec<_> = avf_a.iter().chain(&avf_b).map(AvvkFrame::read_barrier).collect();
        if !barriers.is_empty() {
            self.sample_barrier(&barriers, vk::PipelineStageFlags::FRAGMENT_SHADER);
        }
        self.begin_clear_pass(rt.framebuffer);
        #[repr(C)]
        struct FxPush {
            res: [f32; 2],
            progress: f32,
            style: i32,
            pid_base: i32,
            fill: i32,
            rgba_a: i32,
            rgba_b: i32,
            uv_a: [f32; 4],
            uv_b: [f32; 4],
        }
        let push = FxPush {
            res: [self.extent.width as f32, self.extent.height as f32],
            progress: progress.clamp(0.0, 1.0),
            style: fx as i32,
            pid_base: 0,
            fill: crate::fill_flag(),
            rgba_a: i32::from(matches!(src_a, Src::Rgba(_))),
            rgba_b: i32::from(matches!(src_b, Src::Rgba(_))),
            uv_a,
            uv_b,
        };
        unsafe {
            self.device.cmd_bind_descriptor_sets(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout_sand,
                0,
                &[self.desc_set, self.desc_set_b],
                &[],
            );
            self.device.cmd_push_constants(
                self.cmd,
                self.pipeline_layout_sand,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                std::slice::from_raw_parts((&raw const push).cast(), 64),
            );
            self.device.cmd_bind_pipeline(self.cmd, vk::PipelineBindPoint::GRAPHICS, pipe);
            self.device.cmd_draw(self.cmd, 3, 1, 0, 0);
            self.device.cmd_end_render_pass(self.cmd);
        }
        self.release_imports(&[src_a, src_b]);
        if export.direct_render {
            self.release_direct_export(export);
        } else {
            self.copy_rt_to_export(rt, export);
        }
        self.end_and_submit(&waits)?;
        Self::note_imports(&[src_a, src_b]);
        export.ready = true;
        for avf in avf_a.into_iter().chain(avf_b) {
            avf.commit_sampled();
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub fn render_sand_to(
        &mut self,
        rt: &RenderTarget,
        export: &mut ExportImage,
        src_a: &Src,
        uv_a: [f32; 4],
        src_b: &Src,
        uv_b: [f32; 4],
        progress: f32,
        style: i32,
    ) -> Result<()> {
        let uv_a = src_a.texture_uv(uv_a);
        let uv_b = src_b.texture_uv(uv_b);
        self.ensure_transition_pipelines()?;
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let (avf_a, avf_b) = (Self::resolve_src(src_a)?, Self::resolve_src(src_b)?);
        let mut waits = WaitSems::new();
        for avf in avf_a.iter().chain(&avf_b) {
            avf.push_wait_sems(&mut waits);
        }
        self.begin_frame_cmd()?;
        if export.direct_render {
            self.acquire_direct_export(export);
        }
        self.acquire_imports(&[src_a, src_b]);
        self.bind_src(self.desc_set, src_a, &avf_a)?;
        self.bind_src(self.desc_set_b, src_b, &avf_b)?;
        let barriers: Vec<_> = avf_a.iter().chain(&avf_b).map(AvvkFrame::read_barrier).collect();
        if !barriers.is_empty() {
            self.sample_barrier(
                &barriers,
                vk::PipelineStageFlags::VERTEX_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
            );
        }
        self.begin_clear_pass(rt.framebuffer);
        #[repr(C)]
        struct SandPush {
            res: [f32; 2],
            progress: f32,
            style: i32,
            pid_base: i32,
            fill: i32,
            rgba_a: i32,
            rgba_b: i32,
            uv_a: [f32; 4],
            uv_b: [f32; 4],
        }
        let (base, count) = paper_shaders::sand_window(style, progress);
        let push = SandPush {
            res: [self.extent.width as f32, self.extent.height as f32],
            progress,
            style,
            pid_base: base,
            fill: crate::fill_flag(),
            rgba_a: i32::from(matches!(src_a, Src::Rgba(_))),
            rgba_b: i32::from(matches!(src_b, Src::Rgba(_))),
            uv_a,
            uv_b,
        };
        unsafe {
            self.device.cmd_bind_descriptor_sets(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout_sand,
                0,
                &[self.desc_set, self.desc_set_b],
                &[],
            );
            self.device.cmd_push_constants(
                self.cmd,
                self.pipeline_layout_sand,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                std::slice::from_raw_parts((&raw const push).cast(), 64),
            );
        }
        unsafe {
            self.device.cmd_bind_pipeline(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_base,
            );
            self.device.cmd_draw(self.cmd, 3, 1, 0, 0);
        }
        unsafe {
            self.device.cmd_bind_pipeline(
                self.cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_sand,
            );
            self.device.cmd_draw(self.cmd, count as u32, 1, 0, 0);
            self.device.cmd_end_render_pass(self.cmd);
        }
        self.release_imports(&[src_a, src_b]);
        if export.direct_render {
            self.release_direct_export(export);
        } else {
            self.copy_rt_to_export(rt, export);
        }
        self.end_and_submit(&waits)?;
        Self::note_imports(&[src_a, src_b]);
        export.ready = true;
        for avf in avf_a.into_iter().chain(avf_b) {
            avf.commit_sampled();
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub fn read_export_to(
        &mut self,
        export: &ExportImage,
        rb: &ReadbackBuf,
        width: u32,
        height: u32,
    ) -> Result<()> {
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            self.device.begin_command_buffer(
                self.cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            let range = vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            };
            let to_src = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                .old_layout(vk::ImageLayout::GENERAL)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(export.image)
                .subresource_range(range)];
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &to_src,
            );
            self.device.cmd_copy_image_to_buffer(
                self.cmd,
                export.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                rb.buffer,
                &[vk::BufferImageCopy {
                    buffer_offset: 0,
                    buffer_row_length: 0,
                    buffer_image_height: 0,
                    image_subresource: vk::ImageSubresourceLayers {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        mip_level: 0,
                        base_array_layer: 0,
                        layer_count: 1,
                    },
                    image_offset: vk::Offset3D::default(),
                    image_extent: vk::Extent3D { width, height, depth: 1 },
                }],
            );
            let back = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                .dst_access_mask(vk::AccessFlags::MEMORY_READ)
                .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(export.image)
                .subresource_range(range)];
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &back,
            );
            self.device.end_command_buffer(self.cmd)?;
            let cmds = [self.cmd];
            self.device.reset_fences(&[self.fence])?;
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

    #[cfg(feature = "shared-device")]
    pub fn retire_plane_views(&mut self) -> Result<()> {
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            for (_, (lv, cv)) in self.plane_views.drain() {
                self.device.destroy_image_view(lv, None);
                self.device.destroy_image_view(cv, None);
            }
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub fn wait_render(&self) -> Result<()> {
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub(in crate::vk::draw) fn plane_views_for(
        &mut self,
        image: vk::Image,
    ) -> Result<(vk::ImageView, vk::ImageView)> {
        if let Some(views) = self.plane_views.get(&image.as_raw()) {
            return Ok(*views);
        }
        let make = |format: vk::Format, aspect: vk::ImageAspectFlags| -> Result<vk::ImageView> {
            unsafe {
                self.device
                    .create_image_view(
                        &vk::ImageViewCreateInfo::default()
                            .image(image)
                            .view_type(vk::ImageViewType::TYPE_2D)
                            .format(format)
                            .subresource_range(vk::ImageSubresourceRange {
                                aspect_mask: aspect,
                                base_mip_level: 0,
                                level_count: 1,
                                base_array_layer: 0,
                                layer_count: 1,
                            }),
                        None,
                    )
                    .context("plane view")
            }
        };
        let lv = make(vk::Format::R8_UNORM, vk::ImageAspectFlags::PLANE_0)?;
        let cv = make(vk::Format::R8G8_UNORM, vk::ImageAspectFlags::PLANE_1)?;
        self.plane_views.insert(image.as_raw(), (lv, cv));
        Ok((lv, cv))
    }
}
