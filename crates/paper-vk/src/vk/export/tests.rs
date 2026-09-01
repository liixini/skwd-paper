#![cfg(feature = "shared-device")]

use super::{ExportImage, ExportOwner, Nv12Export, ReadbackBuf, RenderTarget};
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
