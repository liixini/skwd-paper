use super::*;

#[test]
fn mixed_outputs_require_the_largest_sampling_scale() {
    let source = (15360, 8640);
    assert_eq!(texture_size(source, &[(1920, 1080), (3840, 2160)], FillMode::Fill), (3840, 2160));
    assert_eq!(texture_size(source, &[(3840, 2160), (1920, 1080)], FillMode::Fill), (3840, 2160));
    assert_eq!(texture_size(source, &[(1920, 1080), (2160, 3840)], FillMode::Fill), (6827, 3840));
    assert_eq!(texture_size(source, &[(1920, 1080), (2160, 3840)], FillMode::Fit), (2160, 1215));
}

#[test]
fn native_size_modes_and_small_sources_keep_their_pixels() {
    for mode in [FillMode::Center, FillMode::Tile] {
        assert_eq!(texture_size((7680, 4320), &[(1920, 1080)], mode), (7680, 4320));
    }
    for mode in FillMode::ALL {
        assert_eq!(texture_size((640, 360), &[(3840, 2160)], mode), (640, 360));
    }
    assert_eq!(texture_size((7680, 4320), &[], FillMode::Fill), (7680, 4320));
}

#[test]
fn stretch_satisfies_both_landscape_and_portrait_targets() {
    assert_eq!(
        texture_size((15360, 8640), &[(3840, 2160), (2160, 3840)], FillMode::Stretch),
        (3840, 3840)
    );
}

#[test]
fn one_decode_supplies_rgba_and_a_compatible_frame() {
    ff::init().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("alpha.png");
    let image =
        image::RgbaImage::from_fn(127, 65, |x, y| image::Rgba([x as u8, y as u8, 240, 123]));
    image.save(&path).unwrap();
    let mut dec = StillDecoder::open(path.to_str().unwrap(), &[(63, 31)], FillMode::Fit).unwrap();
    let (frame, _) = dec.next().unwrap();
    std::fs::remove_file(&path).unwrap();
    let pixels = frame.still_pixels().unwrap();
    assert_eq!(pixels.source, (127, 65));
    assert_eq!(pixels.image.dimensions(), (61, 31));
    assert_eq!(frame.format(), ff::format::Pixel::NV12);
    assert_eq!(frame.width(), 61);
    assert_eq!(frame.height(), 31);
    assert_eq!(pixels.image.get_pixel(20, 20).0[3], 123);
    let cloned = frame.clone();
    assert!(Arc::ptr_eq(frame.still.as_ref().unwrap(), cloned.still.as_ref().unwrap()));
    assert!(dec.next().is_err());
}

#[test]
fn dimension_probe_does_not_decode_pixel_payload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("header.png");
    image::RgbaImage::new(3840, 2160).save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let idat = bytes.windows(4).position(|part| part == b"IDAT").unwrap();
    std::fs::write(&path, &bytes[..idat + 4]).unwrap();
    assert_eq!(super::super::probe_dims(path.to_str().unwrap()), Some((3840, 2160)));
    assert!(image::open(&path).is_err());
}
