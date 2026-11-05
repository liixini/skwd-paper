use super::SceneViewport;

#[test]
fn centered_cover_view() {
    let view = SceneViewport::cover([256.0, 256.0], [1280, 720]).unwrap();
    assert_eq!(view.source, [0.0, 56.0, 256.0, 144.0]);
    assert_eq!(view.authored_viewport(), [0.0, -280.0, 1280.0, 1280.0]);
    assert!(view.same_outputs(&[(1280, 720), (1280, 720)]));
    assert!(!view.same_outputs(&[(1280, 720), (1920, 1080)]));
    assert!(!view.same_outputs(&[(720, 1280)]));
}

#[test]
fn square_cover_view() {
    let view = SceneViewport::cover([256.0, 256.0], [720, 720]).unwrap();
    assert_eq!(view.source, [0.0, 0.0, 256.0, 256.0]);
    assert_eq!(view.authored_viewport(), [0.0, 0.0, 720.0, 720.0]);
}

#[test]
fn invalid_view_dimensions() {
    for (canvas, raster) in
        [([0.0, 256.0], [1280, 720]), ([f32::NAN, 256.0], [1280, 720]), ([256.0, 256.0], [0, 720])]
    {
        assert!(SceneViewport::cover(canvas, raster).is_none());
    }
}

#[test]
fn device_viewport_limits() {
    let limits = ash::vk::PhysicalDeviceLimits {
        max_viewport_dimensions: [32768, 32768],
        viewport_bounds_range: [-32768.0, 32767.0],
        ..Default::default()
    };
    assert!(SceneViewport::cover([256.0, 256.0], [1280, 720]).unwrap().supported(&limits));
    assert!(!SceneViewport::cover([256.0, 25600.0], [1280, 720]).unwrap().supported(&limits));
    assert!(!SceneViewport::cover([25600.0, 256.0], [720, 1280]).unwrap().supported(&limits));
    assert!(!SceneViewport::cover([256.0, 256.0], [1280, 720]).unwrap().same_outputs(&[]));
    assert!(SceneViewport::cover([1.0, 1.0], [1280, 720]).unwrap().supported(&limits));
}

#[test]
fn whole_canvas_uncropped() {
    let whole = SceneViewport::whole([1280.0, 720.0]);
    assert_eq!(whole.authored_viewport(), [0.0, 0.0, 1280.0, 720.0]);
    assert!(whole.uncropped());
    assert!(!SceneViewport::cover([1280.0, 1280.0], [1280, 720]).unwrap().uncropped());
}
