use anyhow::{Result, anyhow};
use ash::vk;

#[cfg(feature = "shared-device")]
pub(super) const COLOR_RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};

#[cfg(feature = "shared-device")]
pub(super) struct AvvkFrame<'a> {
    pub(super) frame: crate::ffmpeg_vulkan::LockedFrame<'a>,
    pub(super) image: vk::Image,
    pub(super) layout: vk::ImageLayout,
    pub(super) signal: u64,
}

#[cfg(feature = "shared-device")]
pub(super) fn avvk_frame(frame: &ffmpeg_the_third::frame::Video) -> Result<AvvkFrame<'_>> {
    let frame = crate::ffmpeg_vulkan::Frame::from_video(frame)
        .ok_or_else(|| anyhow!("no AVVkFrame"))?
        .lock();
    let signal = frame
        .semaphore_value(0)
        .checked_add(1)
        .ok_or_else(|| anyhow!("AVVkFrame semaphore value overflow"))?;
    Ok(AvvkFrame { image: frame.image(0), layout: frame.layout(0), signal, frame })
}

#[cfg(feature = "shared-device")]
impl AvvkFrame<'_> {
    pub(super) fn push_wait_sems(&self, waits: &mut WaitSems) {
        for idx in 0..self.frame.image_count().max(1) {
            waits.sems[waits.len] = self.frame.semaphore(idx);
            waits.values[waits.len] = self.frame.semaphore_value(idx);
            waits.len += 1;
        }
        waits.signal_sems[waits.signal_len] = self.frame.semaphore(0);
        waits.signal_values[waits.signal_len] = self.signal;
        waits.signal_len += 1;
    }

    pub(super) fn read_barrier(&self) -> vk::ImageMemoryBarrier<'static> {
        vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .old_layout(self.layout)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.image)
            .subresource_range(COLOR_RANGE)
    }

    pub(super) fn commit_sampled(self) {
        let signal = self.signal;
        self.frame.commit(
            0,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            vk::AccessFlags::SHADER_READ,
            signal,
        );
    }
}

#[cfg(feature = "shared-device")]
pub(super) struct WaitSems {
    pub(super) sems: [vk::Semaphore; 16],
    pub(super) values: [u64; 16],
    pub(super) len: usize,
    pub(super) signal_sems: [vk::Semaphore; 4],
    pub(super) signal_values: [u64; 4],
    pub(super) signal_len: usize,
}

#[cfg(feature = "shared-device")]
impl WaitSems {
    pub(super) fn new() -> Self {
        Self {
            sems: [vk::Semaphore::null(); 16],
            values: [0; 16],
            len: 0,
            signal_sems: [vk::Semaphore::null(); 4],
            signal_values: [0; 4],
            signal_len: 0,
        }
    }

    pub(super) fn signal_sems(&self) -> &[vk::Semaphore] {
        &self.signal_sems[..self.signal_len]
    }

    pub(super) fn signal_values(&self) -> &[u64] {
        &self.signal_values[..self.signal_len]
    }

    pub(super) fn sems(&self) -> &[vk::Semaphore] {
        &self.sems[..self.len]
    }

    pub(super) fn values(&self) -> &[u64] {
        &self.values[..self.len]
    }
}
