use super::{SceneConfig, SceneLayers, SceneQuad, SceneTarget, SceneTexture};
use crate::vk::{RenderTarget, Renderer};
use anyhow::Result;
use ash::vk;

pub(crate) struct Overlay {
    pub(crate) textures: Vec<SceneTexture>,
    pub(crate) quads: Vec<SceneQuad>,
    pass: vk::RenderPass,
}

impl Renderer {
    pub(crate) fn output_target(&mut self) -> Result<SceneTarget> {
        self.configure_scene(SceneConfig { format: self.format, depth: None })?;
        self.create_scene_target(self.extent.width, self.extent.height)
    }

    pub(crate) fn take_overlay(&mut self) -> Option<Overlay> {
        self.overlay.take()
    }

    pub(crate) fn restore_overlay(&mut self, overlay: Option<Overlay>) {
        self.overlay = overlay;
    }

    pub(crate) fn sample_output(&mut self, target: &SceneTarget) -> Result<()> {
        self.begin_scene_batch()?;
        let barrier = vk::ImageMemoryBarrier::default()
            .image(target.image)
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .subresource_range(super::SCENE_COLOR_RANGE);
        unsafe {
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
        }
        self.submit_scene_batch()?;
        self.wait_render()
    }

    pub(crate) fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }

    pub(crate) fn set_overlay(
        &mut self,
        textures: Vec<SceneTexture>,
        quads: Vec<SceneQuad>,
    ) -> Result<()> {
        self.wait_render()?;
        if quads.is_empty() {
            self.overlay = None;
            return Ok(());
        }
        self.configure_scene(SceneConfig { format: self.format, depth: None })?;
        let pass = self.scene_pass_append_for(self.format, vk::SampleCountFlags::TYPE_1)?;
        self.overlay = Some(Overlay { textures, quads, pass });
        Ok(())
    }

    pub(crate) fn record_overlay(&mut self, target: &RenderTarget) {
        let Some(overlay) = self.overlay.take() else { return };
        let borrowed = SceneTarget {
            promoted: None,
            device: None,
            depth: None,
            msaa: None,
            samples: vk::SampleCountFlags::TYPE_1,
            depth_order: None,
            viewport: None,
            image: target.image,
            memory: vk::DeviceMemory::null(),
            view: target.view,
            sampler: vk::Sampler::null(),
            framebuffer: target.framebuffer,
            extent: self.extent,
            allocation_bytes: 0,
            repeat: false,
            format: self.format,
            render_pass: overlay.pass,
        };
        self.overlay_barrier(target.image, false);
        self.record_scene_pass_with_canvas(
            &borrowed,
            [self.extent.width as f32, self.extent.height as f32],
            (overlay.pass, super::CLEAR_COLOR),
            SceneLayers {
                quads: &overlay.quads,
                textures: &overlay.textures,
                particles: &[],
                plain: &[],
            },
        );
        self.overlay_barrier(target.image, true);
        self.overlay = Some(overlay);
    }

    fn overlay_barrier(&self, image: vk::Image, finished: bool) {
        let (old, new) = if finished {
            (vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, vk::ImageLayout::GENERAL)
        } else {
            (vk::ImageLayout::GENERAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        };
        let barrier = vk::ImageMemoryBarrier::default()
            .image(image)
            .old_layout(old)
            .new_layout(new)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .dst_access_mask(
                vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            )
            .subresource_range(super::SCENE_COLOR_RANGE);
        unsafe {
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
        }
    }
}
