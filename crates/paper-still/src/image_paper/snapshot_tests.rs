use super::*;
use paper_runtime::mirror::{Layout, Memory, Plane};

#[test]
fn captures_cropped_pixels() {
    let path = std::env::temp_dir().join(format!("still-snapshot-{}", std::process::id()));
    let pixels = [0, 0, 0, 0, 3, 2, 1, 255, 6, 5, 4, 255, 0, 0, 0, 0];
    std::fs::write(&path, pixels).unwrap();
    let shown = Shown {
        memory: File::open(&path).unwrap().into(),
        layout: Layout {
            width: 3,
            height: 1,
            fourcc: AR24,
            modifier: 0,
            memory: Memory::Shm,
            planes: [Plane { stride: 16, offset: 0 }, Plane::default()],
            plane_count: 1,
        },
        source: [1, 0, 2, 1],
    };
    assert_eq!(shown.pixels().unwrap().as_raw(), &[1, 2, 3, 255, 4, 5, 6, 255]);
    let rgba =
        Shown { layout: Layout { fourcc: u32::from(Format::Abgr8888), ..shown.layout }, ..shown };
    assert_eq!(rgba.pixels().unwrap().as_raw(), &[3, 2, 1, 255, 6, 5, 4, 255]);
    let outside = Shown { source: [2, 0, 2, 1], ..rgba };
    assert!(outside.pixels().is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn captures_styled_buffer() {
    let path = std::env::temp_dir().join(format!("styled-snapshot-{}.png", std::process::id()));
    let source = image::RgbaImage::from_fn(32, 16, |x, _| {
        image::Rgba(if x < 16 { [220, 40, 80, 255] } else { [20, 200, 100, 255] })
    });
    source.save(&path).unwrap();
    let (width, height, styled) =
        super::super::decode::decode_image(path.to_str().unwrap(), 5.0, 44).unwrap();
    let buffer = path.with_extension("buffer");
    let mut packed = vec![0; styled.len()];
    super::super::shm_pixels::pack_pixels(&mut packed, &styled, Format::Argb8888);
    std::fs::write(&buffer, packed).unwrap();
    let shown = Shown {
        memory: File::open(&buffer).unwrap().into(),
        layout: Layout {
            width,
            height,
            fourcc: AR24,
            modifier: 0,
            memory: Memory::Shm,
            planes: [Plane { stride: width * 4, offset: 0 }, Plane::default()],
            plane_count: 1,
        },
        source: [0, 0, width, height],
    };
    assert_eq!(shown.pixels().unwrap().as_raw(), &styled);
    assert_ne!(styled, source.into_raw());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(buffer).unwrap();
}
