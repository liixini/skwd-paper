use super::*;

fn halves(width: usize, height: usize) -> Outline {
    let mask = (0..width * height)
        .map(|index| if index % width < width / 2 { 0 } else { u8::MAX })
        .collect();
    Outline::new(width, height, mask).unwrap()
}

#[test]
fn stretch_maps_directly() {
    let outline = halves(100, 50);
    let frame = Frame { image: [100, 50], buffer: [200, 100], mode: FillMode::Stretch };
    let cut = outline.crop(frame, [0, 0], [200, 1]).unwrap();
    assert_eq!(cut[0], 0);
    assert_eq!(cut[199], u8::MAX);
    assert!(cut[90] == 0 && cut[110] == u8::MAX);
}

#[test]
fn fill_crops_sides() {
    let outline = halves(200, 100);
    let frame = Frame { image: [200, 100], buffer: [100, 100], mode: FillMode::Fill };
    let cut = outline.crop(frame, [0, 0], [100, 1]).unwrap();
    assert!(cut[40] == 0 && cut[60] == u8::MAX);
}

#[test]
fn fit_leaves_bars() {
    let outline = Outline::new(10, 10, vec![u8::MAX; 100]).unwrap();
    let frame = Frame { image: [10, 10], buffer: [200, 100], mode: FillMode::Fit };
    let cut = outline.crop(frame, [0, 50], [200, 1]).unwrap();
    assert_eq!(cut[10], 0);
    assert_eq!(cut[100], u8::MAX);
    assert_eq!(cut[190], 0);
}

#[test]
fn origin_offsets_crop() {
    let outline = halves(100, 10);
    let frame = Frame { image: [100, 10], buffer: [100, 10], mode: FillMode::Stretch };
    let cut = outline.crop(frame, [60, 0], [10, 1]).unwrap();
    assert!(cut.iter().all(|&level| level == u8::MAX));
}

#[test]
fn center_crops_large_image() {
    let outline = halves(200, 10);
    let frame = Frame { image: [200, 10], buffer: [100, 10], mode: FillMode::Center };
    let cut = outline.crop(frame, [0, 0], [100, 1]).unwrap();
    assert!(cut[40] == 0 && cut[60] == u8::MAX);
}

#[test]
fn center_pads_small_image() {
    let outline = Outline::new(10, 10, vec![u8::MAX; 100]).unwrap();
    let frame = Frame { image: [10, 10], buffer: [30, 30], mode: FillMode::Center };
    let cut = outline.crop(frame, [0, 15], [30, 1]).unwrap();
    assert_eq!((cut[5], cut[15], cut[25]), (0, u8::MAX, 0));
}

#[test]
fn tile_repeats() {
    let outline = halves(10, 10);
    let frame = Frame { image: [10, 10], buffer: [40, 10], mode: FillMode::Tile };
    let cut = outline.crop(frame, [0, 0], [40, 1]).unwrap();
    assert_eq!((cut[2], cut[8], cut[22], cut[28]), (0, u8::MAX, 0, u8::MAX));
}

#[test]
fn span_is_unmapped() {
    let frame = Frame { image: [10, 10], buffer: [10, 10], mode: FillMode::Span };
    assert!(halves(10, 10).crop(frame, [0, 0], [4, 4]).is_none());
}

#[test]
fn rejects_bad_mask() {
    assert!(Outline::new(4, 4, vec![0; 3]).is_none());
}

#[test]
fn missing_manifest_fails() {
    let manifest = Path::new("/nonexistent/manifest.json");
    assert!(Outline::load(manifest, Path::new("/nonexistent/source.png")).is_err());
}
