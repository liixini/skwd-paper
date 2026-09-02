#![cfg(feature = "shared-device")]

use super::{
    ExportImage, ExportOwner, Nv12Export, ReadbackBuf, RenderTarget, single_plane_modifiers,
    xr24_layout_aspect,
};
use ash::vk;

#[test]
fn export_types_drop() {
    assert!(std::mem::needs_drop::<ExportImage>());
    assert!(std::mem::needs_drop::<Nv12Export>());
    assert!(std::mem::needs_drop::<ReadbackBuf>());
    assert!(std::mem::needs_drop::<RenderTarget>());
}

#[test]
fn queue_ownership_transfers() {
    let local = 7;
    assert_eq!(
        ExportOwner::Foreign.acquire(true, local),
        super::QueueTransfer { src: vk::QUEUE_FAMILY_FOREIGN_EXT, dst: local }
    );
    assert_eq!(
        ExportOwner::Foreign.release(local),
        super::QueueTransfer { src: local, dst: vk::QUEUE_FAMILY_FOREIGN_EXT }
    );
    assert_eq!(
        ExportOwner::External.acquire(true, local),
        super::QueueTransfer { src: vk::QUEUE_FAMILY_EXTERNAL, dst: local }
    );
    assert_eq!(
        ExportOwner::External.release(local),
        super::QueueTransfer { src: local, dst: vk::QUEUE_FAMILY_EXTERNAL }
    );
}

#[test]
fn first_use_local() {
    let local = 3;
    let ignored =
        super::QueueTransfer { src: vk::QUEUE_FAMILY_IGNORED, dst: vk::QUEUE_FAMILY_IGNORED };
    assert_eq!(ExportOwner::Foreign.acquire(false, local), ignored);
    assert_eq!(ExportOwner::External.acquire(false, local), ignored);
    assert_eq!(ExportOwner::None.acquire(true, local), ignored);
    assert_eq!(ExportOwner::None.release(local), ignored);
}

#[test]
fn xr24_modifier_selection_excludes_auxiliary_planes() {
    let available = [
        vk::DrmFormatModifierPropertiesEXT::default()
            .drm_format_modifier(8)
            .drm_format_modifier_plane_count(3),
        vk::DrmFormatModifierPropertiesEXT::default()
            .drm_format_modifier(9)
            .drm_format_modifier_plane_count(1),
        vk::DrmFormatModifierPropertiesEXT::default()
            .drm_format_modifier(2)
            .drm_format_modifier_plane_count(1),
    ];
    assert_eq!(single_plane_modifiers(&[8, 9, 2, 7], &available), [9, 2]);
}

#[test]
fn xr24_modifier_layout_uses_memory_plane_aspect() {
    assert!(
        xr24_layout_aspect(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            == vk::ImageAspectFlags::MEMORY_PLANE_0_EXT
    );
    assert!(xr24_layout_aspect(vk::ImageTiling::LINEAR) == vk::ImageAspectFlags::COLOR);
}
