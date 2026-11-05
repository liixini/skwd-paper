use super::Renderer;
use anyhow::{Result, ensure};
use ash::vk;

#[cfg(test)]
thread_local! {
    static ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn allocation_count() -> usize {
    ALLOCATIONS.get()
}

pub(super) struct Spec {
    pub extent: vk::Extent2D,
    pub format: vk::Format,
    pub levels: u32,
    pub samples: vk::SampleCountFlags,
    pub usage: vk::ImageUsageFlags,
    pub aspect: vk::ImageAspectFlags,
}

pub(super) struct Storage {
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub bytes: u64,
}

impl Storage {
    pub fn new(renderer: &Renderer, spec: &Spec) -> Result<Self> {
        let device = &renderer.device;
        let image = spec.create(renderer)?;
        let mut storage =
            Self { image, memory: vk::DeviceMemory::null(), view: vk::ImageView::null(), bytes: 0 };
        let setup = (|| -> Result<()> {
            unsafe {
                let requirements = device.get_image_memory_requirements(image);
                storage.memory = device.allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(requirements.size)
                        .memory_type_index(renderer.device_local_index(requirements)?),
                    None,
                )?;
                #[cfg(test)]
                ALLOCATIONS.set(ALLOCATIONS.get() + 1);
                device.bind_image_memory(image, storage.memory, 0)?;
                storage.view = device.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(spec.format)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: spec.aspect,
                            base_mip_level: 0,
                            level_count: spec.levels,
                            base_array_layer: 0,
                            layer_count: 1,
                        }),
                    None,
                )?;
                storage.bytes = requirements.size;
            }
            Ok(())
        })();
        if let Err(error) = setup {
            storage.destroy(device);
            return Err(error);
        }
        Ok(storage)
    }

    pub fn destroy(self, device: &ash::Device) {
        unsafe {
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

pub(super) fn color_range(base: u32, count: u32) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: base,
        level_count: count,
        base_array_layer: 0,
        layer_count: 1,
    }
}

pub(super) fn color_layers(level: u32) -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        mip_level: level,
        base_array_layer: 0,
        layer_count: 1,
    }
}

impl Renderer {
    pub(super) fn record_mips(&self, image: vk::Image, extent: vk::Extent2D, levels: u32) {
        let ready = |base, count, layout, access| {
            vk::ImageMemoryBarrier::default()
                .image(image)
                .old_layout(layout)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(access)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .subresource_range(color_range(base, count))
        };
        let (mut width, mut height) = (extent.width as i32, extent.height as i32);
        unsafe {
            for level in 1..levels {
                self.device.cmd_pipeline_barrier(
                    self.cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[vk::ImageMemoryBarrier::default()
                        .image(image)
                        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .subresource_range(color_range(level - 1, 1))],
                );
                let (next_width, next_height) = ((width / 2).max(1), (height / 2).max(1));
                let blit = vk::ImageBlit::default()
                    .src_subresource(color_layers(level - 1))
                    .src_offsets([
                        vk::Offset3D::default(),
                        vk::Offset3D { x: width, y: height, z: 1 },
                    ])
                    .dst_subresource(color_layers(level))
                    .dst_offsets([
                        vk::Offset3D::default(),
                        vk::Offset3D { x: next_width, y: next_height, z: 1 },
                    ]);
                self.device.cmd_blit_image(
                    self.cmd,
                    image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[blit],
                    vk::Filter::LINEAR,
                );
                (width, height) = (next_width, next_height);
            }
            let barriers = [
                ready(
                    0,
                    levels - 1,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    vk::AccessFlags::TRANSFER_READ,
                ),
                ready(
                    levels - 1,
                    1,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::TRANSFER_WRITE,
                ),
            ];
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_GRAPHICS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                if levels > 1 { &barriers } else { &barriers[1..] },
            );
        }
    }
}

impl Spec {
    fn create(&self, renderer: &Renderer) -> Result<vk::Image> {
        let maximum = u32::BITS - self.extent.width.max(self.extent.height).leading_zeros();
        ensure!(self.extent.width > 0 && self.extent.height > 0, "empty image extent");
        ensure!((1..=maximum).contains(&self.levels), "invalid image mip count");
        let device = &renderer.device;
        unsafe {
            device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(self.format)
                        .extent(vk::Extent3D {
                            width: self.extent.width,
                            height: self.extent.height,
                            depth: 1,
                        })
                        .mip_levels(self.levels)
                        .array_layers(1)
                        .samples(self.samples)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(self.usage)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .map_err(Into::into)
        }
    }

    pub fn allocation_size(&self, renderer: &Renderer) -> Result<u64> {
        let image = self.create(renderer)?;
        unsafe {
            let bytes = renderer.device.get_image_memory_requirements(image).size;
            renderer.device.destroy_image(image, None);
            Ok(bytes)
        }
    }
}
