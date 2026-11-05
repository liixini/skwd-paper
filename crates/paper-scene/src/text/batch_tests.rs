use super::*;

#[test]
fn isolates_glyph_samples() {
    let glyphs = vec![
        Glyph { bounds: [0, 0, 2, 1], bitmap: vec![0, 154] },
        Glyph { bounds: [1, 0, 2, 1], bitmap: vec![0, 0] },
    ];
    let batch = Batch::pack([3, 1], glyphs).unwrap();
    assert_eq!(batch.quads.len(), 2);
    for quad in &batch.quads {
        let [x, y, width, _] = quad.uv;
        let pixels = &batch.pixels.levels[0];
        for edge in [x - 1, x + width] {
            let at = (y * pixels.width + edge) as usize * RGBA_CHANNELS;
            assert_eq!(pixels.data[at + RGBA_CHANNELS - 1], 0);
        }
    }
    assert_eq!(batch.quads[0].bounds, [0, 0, 2, 1]);
    assert_eq!(batch.quads[1].bounds, [1, 0, 2, 1]);
}

#[test]
fn reuses_glyph_bitmaps() {
    let glyphs =
        [0, 1, 2].into_iter().map(|x| Glyph { bounds: [x, 0, 1, 1], bitmap: vec![128] }).collect();
    let batch = Batch::pack([3, 1], glyphs).unwrap();
    assert_eq!(batch.quads.len(), 3);
    assert_eq!(batch.pixels.width(), 3);
    assert_eq!(batch.pixels.height(), 3);
    assert!(batch.quads.iter().all(|quad| quad.uv == [1, 1, 1, 1]));
}

#[test]
fn clips_glyph_geometry() {
    let glyphs = vec![Glyph { bounds: [-1, -1, 3, 3], bitmap: (1..=9).collect() }];
    let batch = Batch::pack([1, 1], glyphs).unwrap();
    assert_eq!(batch.quads[0].bounds, [0, 0, 1, 1]);
    assert_eq!(batch.quads[0].uv, [1, 1, 1, 1]);
    assert_eq!(batch.pixels.levels[0].data[19], 5);
}

#[test]
fn retains_canvas_edge() {
    let width = super::super::MAX_TEXT_EDGE;
    let batch = Batch::pack(
        [width, 1],
        vec![Glyph { bounds: [0, 0, i64::from(width), 1], bitmap: vec![128; width as usize] }],
    )
    .unwrap();
    assert_eq!(batch.canvas, [width, 1]);
    assert_eq!(batch.quads[0].bounds, [0, 0, width as i32, 1]);
    assert_eq!(batch.pixels.width(), width + 2);
}

#[test]
fn distinguishes_glyph_layouts() {
    let make =
        |x| Batch::pack([2, 1], vec![Glyph { bounds: [x, 0, 1, 1], bitmap: vec![128] }]).unwrap();
    let first = make(0);
    let second = make(1);
    assert_eq!(first.pixels, second.pixels);
    assert_ne!(first, second);
}
