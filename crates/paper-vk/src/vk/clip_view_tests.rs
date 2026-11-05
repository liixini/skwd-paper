use super::*;

fn screen(view: ClipView, canvas: (f32, f32), [x, y]: [f32; 2]) -> [f32; 2] {
    let mut rows =
        [[2.0 / canvas.0, 0.0, 0.0, -1.0], [0.0, 2.0 / canvas.1, 0.0, -1.0], [0.0, 0.0, 0.0, 1.0]];
    view.apply_rows(&mut rows, false);
    let clip = [
        rows[0][0] * x + rows[0][1] * y + rows[0][3],
        rows[1][0] * x + rows[1][1] * y + rows[1][3],
    ];
    [(clip[0] + 1.0) * canvas.0 * 0.5, (clip[1] + 1.0) * canvas.1 * 0.5]
}

#[test]
fn camera_object_mapping() {
    let canvas = (1920.0, 1080.0);
    let camera = |zoom: f32, origin: [f32; 2], roll: f32| {
        ClipView::new(CameraView { zoom: 1.0, object_zoom: zoom, origin, roll }, canvas)
    };
    for (view, point, expected) in [
        (camera(2.0, [0.0; 2], 0.0), [1460.0, 540.0], [1960.0, 540.0]),
        (camera(1.0, [100.0, 50.0], 0.0), [960.0, 540.0], [860.0, 490.0]),
        (camera(2.0, [100.0, 50.0], 0.0), [960.0, 540.0], [760.0, 440.0]),
        (camera(1.0, [0.0; 2], 0.3), [960.0, 540.0], [1076.7, 232.2]),
        (camera(1.0, [100.0, 50.0], 0.3), [960.0, 540.0], [966.4, 214.0]),
    ] {
        let [x, y] = screen(view, canvas, point);
        assert!(
            (x - expected[0]).abs() < 0.5 && (y - expected[1]).abs() < 0.5,
            "{x} {y} {expected:?}"
        );
    }
}

#[test]
fn zoom_matches_scale() {
    let canvas = (1920.0, 1080.0);
    assert_eq!(ClipView::new(CameraView::UNIT, canvas), ClipView::IDENTITY);
}
