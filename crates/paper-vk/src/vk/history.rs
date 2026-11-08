use super::{Renderer, SceneTarget, images};
use anyhow::{Result, ensure};
use ash::vk;

const MIN_MIP_EXTENT: u32 = 16;

#[cfg(test)]
mod tests;

#[cfg(test)]
thread_local! {
    static LIVE_HISTORY: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub struct SceneHistory {
    device: ash::Device,
    storage: Option<images::Storage>,
    sampler: vk::Sampler,
    pub extent: vk::Extent2D,
    pub levels: u32,
    format: vk::Format,
}

impl SceneHistory {
    pub const MIP_INFO: f32 = 1.0;

    pub fn level_count(extent: vk::Extent2D) -> u32 {
        let mut minor = extent.width.min(extent.height);
        let mut levels = 1;
        while minor > MIN_MIP_EXTENT {
            minor /= 2;
            levels += 1;
        }
        levels
    }

    pub fn sample(&self) -> (vk::ImageView, vk::Sampler, vk::Extent2D) {
        (self.storage().view, self.sampler, self.extent)
    }

    pub fn allocation_bytes(&self) -> u64 {
        self.storage().bytes
    }

    fn storage(&self) -> &images::Storage {
        self.storage.as_ref().expect("live scene history")
    }
}

impl Drop for SceneHistory {
    fn drop(&mut self) {
        let Some(storage) = self.storage.take() else { return };
        let _ = crate::shared::wait_idle(&self.device);
        storage.destroy(&self.device);
        #[cfg(test)]
        LIVE_HISTORY.set(LIVE_HISTORY.get() - 1);
    }
}

impl Renderer {
    #[cfg(test)]
    pub fn scene_history_live_allocations() -> usize {
        LIVE_HISTORY.get()
    }

    pub fn create_scene_history(&mut self, extent: vk::Extent2D) -> Result<SceneHistory> {
        let format = self.scene_format();
        let levels = SceneHistory::level_count(extent);
        let storage = images::Storage::new(
            self,
            &images::Spec {
                extent,
                format,
                levels,
                samples: vk::SampleCountFlags::TYPE_1,
                usage: vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::TRANSFER_DST,
                aspect: vk::ImageAspectFlags::COLOR,
            },
        )?;
        #[cfg(test)]
        LIVE_HISTORY.set(LIVE_HISTORY.get() + 1);
        let history = SceneHistory {
            device: self.device.clone(),
            storage: Some(storage),
            sampler: self.sampler_mip,
            extent,
            levels,
            format,
        };
        self.initialize_scene_history(&history)?;
        Ok(history)
    }

    fn initialize_scene_history(&mut self, history: &SceneHistory) -> Result<()> {
        let clear = if history.format == vk::Format::R8G8B8A8_UNORM {
            super::scene::CLEAR_COLOR
        } else {
            [0.0, 0.0, 0.0, 1.0]
        };
        self.begin_scene_batch()?;
        unsafe {
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(history.storage().image)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .subresource_range(images::color_range(0, history.levels))],
            );
            self.device.cmd_clear_color_image(
                self.cmd,
                history.storage().image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &vk::ClearColorValue { float32: clear },
                &[images::color_range(0, 1)],
            );
        }
        self.record_mips(history.storage().image, history.extent, history.levels);
        self.submit_scene_batch()
    }

    pub fn record_scene_history(&self, source: &SceneTarget, history: &SceneHistory) -> Result<()> {
        ensure!(
            source.extent == history.extent && source.format == history.format,
            "scene history source changed"
        );
        let transition = |image, range, layout, access| {
            vk::ImageMemoryBarrier::default()
                .image(image)
                .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .new_layout(layout)
                .src_access_mask(
                    vk::AccessFlags::SHADER_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                )
                .dst_access_mask(access)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .subresource_range(range)
        };
        unsafe {
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::ALL_GRAPHICS,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[
                    transition(
                        source.image,
                        images::color_range(0, 1),
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        vk::AccessFlags::TRANSFER_READ,
                    ),
                    transition(
                        history.storage().image,
                        images::color_range(0, history.levels),
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                    ),
                ],
            );
            self.device.cmd_copy_image(
                self.cmd,
                source.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                history.storage().image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageCopy::default()
                    .src_subresource(images::color_layers(0))
                    .dst_subresource(images::color_layers(0))
                    .extent(vk::Extent3D {
                        width: history.extent.width,
                        height: history.extent.height,
                        depth: 1,
                    })],
            );
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_GRAPHICS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(source.image)
                    .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .subresource_range(images::color_range(0, 1))],
            );
        }
        self.record_mips(history.storage().image, history.extent, history.levels);
        Ok(())
    }
}
