use super::*;

const CORNER: Pin = Pin { anchor: [1.0, 1.0], offset: [0.0, 0.0] };

#[test]
fn snaps_to_whole_pixels() {
    assert_eq!(snap(1.0), 1.0);
    assert_eq!(snap(2.0), 1.0);
    assert_eq!(snap(1.5), 2.0);
    assert_eq!(snap(1.25), 4.0);
    assert_eq!(snap(1.137), 1.0);
}

#[test]
fn corner_stays_inside() {
    let outputs =
        [([2048, 1152], [2560, 1440]), ([1366, 768], [1366, 768]), ([1280, 720], [1920, 1080])];
    for (logical, buffer) in outputs {
        for frame in [[301.0, 77.0], [400.0, 280.0], [150.0, 70.0]] {
            let place = place(logical, buffer, frame, CORNER, None).unwrap();
            for ((origin, size), limit) in place.origin.iter().zip(place.size).zip(buffer) {
                assert!(*origin as usize + size <= limit as usize, "{logical:?} {frame:?}");
            }
        }
    }
}

#[test]
fn canvas_matches_cut() {
    for scale in [1.0, 1.1, 1.15, 1.25, 1.5, 2.0] {
        let logical = [1600, 900];
        let buffer = [(1600.0 * scale) as u32, (900.0 * scale) as u32];
        let place = place(logical, buffer, [301.0, 77.0], CORNER, None).unwrap();
        let canvas = crate::Canvas::window(place.size, place.ink, place.shift);
        assert_eq!([canvas.width, canvas.height], place.size, "scale {scale}");
    }
}

#[test]
fn oversized_widget_clips() {
    let place = place([400, 300], [400, 300], [731.0, 731.0], CORNER, None).unwrap();
    assert_eq!(place.position, [0, 0]);
    assert_eq!(place.size, [400, 300]);
    assert_eq!(place.shift, [331.0, 431.0]);
}

#[test]
fn height_fills_share() {
    let centre = Pin { anchor: [0.5, 0.5], offset: [0.0, 0.0] };
    let place = place([2560, 1440], [2560, 1440], [400.0, 280.0], centre, Some(0.8)).unwrap();
    assert!((place.ink - 1152.0 / 280.0).abs() < 1e-4);
    assert_eq!(place.size[1], 1152);
    assert_eq!(place.position[1], 144);
}

#[test]
fn giant_centres_past_edges() {
    let centre = Pin { anchor: [0.5, 0.5], offset: [0.0, 0.0] };
    let place = place([768, 1366], [768, 1366], [400.0, 280.0], centre, Some(1.0)).unwrap();
    assert_eq!(place.size, [768, 1366]);
    assert!(place.shift[0] > 0.0);
}

#[test]
fn fractional_frames_are_whole() {
    let place = place([2048, 1152], [2560, 1440], [301.0, 77.0], CORNER, None).unwrap();
    let axes = place.frame.iter().zip(place.size).zip(place.origin.iter().zip(place.position));
    for ((frame, size), (origin, position)) in axes {
        let physical = *frame as f32 * place.ink;
        assert_eq!(physical, physical.round());
        assert_eq!(size, physical as usize);
        assert_eq!(*origin as f32, position as f32 * place.ink);
    }
}

#[test]
fn offsets_move_widget() {
    let pin = Pin { anchor: [0.0, 0.0], offset: [40.0, 12.0] };
    let place = place([1920, 1080], [1920, 1080], [100.0, 50.0], pin, None).unwrap();
    assert_eq!(place.position, [40, 12]);
}

#[test]
fn unconfigured_output_skips() {
    assert!(place([0, 0], [0, 0], [10.0, 10.0], CORNER, None).is_none());
}

#[test]
fn bounds_skip_clipping() {
    let place = place([400, 300], [400, 300], [731.0, 731.0], CORNER, None).unwrap();
    assert_eq!(place.bounds, [-331.0, -431.0, 731.0, 731.0]);
}

#[test]
fn anchors_by_third() {
    for (centre, anchor) in [(100.0, 0.0), (533.0, 0.0), (534.0, 0.5), (1066.0, 0.5), (1067.0, 1.0)]
    {
        assert_eq!(third(centre, 1600.0), anchor, "centre {centre}");
    }
}

#[test]
fn pins_offsets() {
    let (left, share) = pin([200.0, 160.0, 400.0, 280.0], [1600.0, 900.0]);
    assert_eq!(left.anchor, [0.0, 0.5]);
    assert_eq!(left.offset, [200.0, 160.0 - 0.5 * (900.0 - 280.0)]);
    assert!((share - 280.0 / 900.0).abs() < 1e-6);
    let (right, _) = pin([1200.0, 600.0, 400.0, 280.0], [1600.0, 900.0]);
    assert_eq!(right.anchor, [1.0, 1.0]);
    assert_eq!(right.offset, [0.0, -20.0]);
}

#[test]
fn pin_clamps_height() {
    assert_eq!(pin([0.0, 0.0, 40.0, 10.0], [1600.0, 900.0]).1, 0.05);
    assert_eq!(pin([0.0, 0.0, 4000.0, 2000.0], [1600.0, 900.0]).1, 1.0);
}

#[test]
fn pin_round_trips() {
    let logical = [1600, 900];
    let output = logical.map(|side| side as f32);
    let pins = [
        Pin { anchor: [0.0, 0.0], offset: [48.0, 36.0] },
        Pin { anchor: [0.5, 0.5], offset: [-12.0, 20.0] },
        CORNER,
    ];
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let buffer = logical.map(|side| (side as f32 * scale) as u32);
        for start in pins {
            let first = place(logical, buffer, [400.0, 280.0], start, Some(0.3)).unwrap();
            let (back, share) = pin(first.bounds, output);
            let again = place(logical, buffer, [400.0, 280.0], back, Some(share)).unwrap();
            assert_eq!(again, first, "scale {scale} {start:?}");
        }
    }
}

#[test]
fn sized_matches_place() {
    let (logical, buffer) = ([2048, 1152], [2560, 1440]);
    let placed = place(logical, buffer, [301.0, 77.0], CORNER, Some(0.27)).unwrap();
    let size = sized(logical, buffer, [301.0, 77.0], Some(0.27)).unwrap();
    assert_eq!(size, [placed.bounds[2], placed.bounds[3]]);
}
