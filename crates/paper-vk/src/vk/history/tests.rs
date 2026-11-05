use super::*;

#[test]
fn history_storage_bytes() {
    for (extent, bytes) in [([1, 1], 4), ([5, 3], 60), ([1, 8], 32), ([1280, 720], 4_914_800)] {
        let extent = vk::Extent2D { width: extent[0], height: extent[1] };
        assert_eq!(SceneHistory::expected_bytes(extent, vk::Format::R8G8B8A8_UNORM), Some(bytes));
        assert_eq!(
            SceneHistory::expected_bytes(extent, vk::Format::R16G16B16A16_SFLOAT),
            Some(bytes * 2)
        );
    }
    for (size, levels) in
        [([1280, 720], 7), ([640, 360], 6), ([1280, 512], 6), ([16, 16], 1), ([17, 32], 2)]
    {
        assert_eq!(
            SceneHistory::level_count(vk::Extent2D { width: size[0], height: size[1] }),
            levels
        );
    }
    assert_eq!(SceneHistory::MIP_INFO, 1.0);
    for (extent, format) in [
        (vk::Extent2D { width: 0, height: 1 }, vk::Format::R8G8B8A8_UNORM),
        (vk::Extent2D { width: u32::MAX, height: u32::MAX }, vk::Format::R16G16B16A16_SFLOAT),
        (vk::Extent2D { width: 1, height: 1 }, vk::Format::D32_SFLOAT),
    ] {
        assert!(SceneHistory::expected_bytes(extent, format).is_none());
    }
}
