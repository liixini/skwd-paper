use super::*;

fn stroke(start: f32, end: f32, points: Vec<[f32; 2]>) -> DrawingStroke {
    DrawingStroke { start, end, width: 7.0, hatch: false, points }
}

fn spec(ink: DrawingInk) -> StrokeSpec {
    StrokeSpec { extent: vk::Extent2D { width: 16, height: 16 }, ink, seconds: 6.5 }
}

#[test]
fn segment_arrivals() {
    let mut hatch = stroke(0.5, 1.0, vec![[0.0, 0.0], [4.0, 0.0]]);
    hatch.hatch = true;
    hatch.width = 24.0;
    let floats = segments(&[stroke(0.2, 0.6, vec![[0.0, 0.0], [3.0, 0.0], [3.0, 1.0]]), hatch]);
    assert_eq!(
        floats,
        [
            [0.0, 0.0, 3.0, 0.0, 0.2, 0.5, 0.0, 0.75, 7.0, 0.0, 0.2],
            [3.0, 0.0, 3.0, 1.0, 0.5, 0.6, 0.75, 1.0, 7.0, 0.0, 0.2],
            [0.0, 0.0, 4.0, 0.0, 0.5, 1.0, 0.0, 1.0, 24.0, 1.0, 0.5],
        ]
        .concat()
    );
}

#[test]
fn push_matches_glsl() {
    assert_eq!(std::mem::offset_of!(StrokePush, size), 0);
    assert_eq!(std::mem::offset_of!(StrokePush, t), 8);
    assert_eq!(std::mem::offset_of!(StrokePush, dry), 12);
    assert_eq!(std::mem::offset_of!(StrokePush, pass), 16);
    assert_eq!(size_of::<StrokePush>(), 20);
}

#[test]
fn still_stroke_arrives() {
    let floats = segments(&[stroke(0.3, 0.4, vec![[2.0, 2.0], [2.0, 2.0]])]);
    assert_eq!(floats[4..8], [0.3, 0.3, 0.0, 0.0]);
}

#[test]
#[ignore = "requires a Vulkan device"]
fn strokes_release_resources() {
    let shared = crate::shared::test_device();
    let renderer = crate::shared::renderer(&shared, 16, 16).unwrap();
    let strokes = [stroke(0.0, 1.0, vec![[0.0, 8.0], [16.0, 8.0]])];
    let buffers = Renderer::effect_live_allocations()[2];
    for ink in [DrawingInk::Plain, DrawingInk::Rainbow] {
        let made = renderer.create_drawing_strokes(&strokes, spec(ink)).unwrap();
        assert_eq!(live_strokes(), 1);
        assert_eq!(made.attributes != vk::Pipeline::null(), ink == DrawingInk::Rainbow);
        drop(made);
        assert_eq!(live_strokes(), 0);
    }
    for step in 0..3 {
        super::super::pipelines::PIPELINE_FAILURE.set(Some(step));
        assert!(renderer.create_drawing_strokes(&strokes, spec(DrawingInk::Plain)).is_err());
        super::super::pipelines::PIPELINE_FAILURE.set(None);
        assert_eq!(live_strokes(), 0, "failed step {step}");
    }
    let empty = vk::Extent2D { width: 0, height: 0 };
    let invalid = StrokeSpec { extent: empty, ..spec(DrawingInk::Plain) };
    assert!(renderer.create_drawing_strokes(&strokes, invalid).is_err());
    assert_eq!(live_strokes(), 0);
    assert_eq!(Renderer::effect_live_allocations()[2], buffers);
}
