use super::*;

#[test]
fn history_level_count() {
    for (size, levels) in
        [([1280, 720], 7), ([640, 360], 6), ([1280, 512], 6), ([16, 16], 1), ([17, 32], 2)]
    {
        assert_eq!(
            SceneHistory::level_count(vk::Extent2D { width: size[0], height: size[1] }),
            levels
        );
    }
    assert_eq!(SceneHistory::MIP_INFO, 1.0);
}
