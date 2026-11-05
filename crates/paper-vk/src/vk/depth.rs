use super::{Renderer, images};
use anyhow::Result;
use ash::vk;

pub(super) const FORMAT: vk::Format = vk::Format::D32_SFLOAT;
pub(super) const PIXEL_BYTES: u64 = 4;
pub(super) type Attachment = images::Storage;

pub(super) fn spec(width: u32, height: u32, samples: vk::SampleCountFlags) -> images::Spec {
    images::Spec {
        extent: vk::Extent2D { width, height },
        format: FORMAT,
        levels: 1,
        samples,
        usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
        aspect: vk::ImageAspectFlags::DEPTH,
    }
}

pub(super) fn attachment(
    renderer: &Renderer,
    width: u32,
    height: u32,
    samples: vk::SampleCountFlags,
) -> Result<Attachment> {
    images::Storage::new(renderer, &spec(width, height, samples))
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum DepthOrder {
    #[default]
    Less,
    Greater,
}

impl DepthOrder {
    pub(super) fn compare(self) -> vk::CompareOp {
        match self {
            Self::Less => vk::CompareOp::LESS,
            Self::Greater => vk::CompareOp::GREATER,
        }
    }

    pub(super) fn clear(self) -> f32 {
        match self {
            Self::Less => 1.0,
            Self::Greater => 0.0,
        }
    }
}
