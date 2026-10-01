use super::*;
use std::path::{Path, PathBuf};

const RED: [u8; 4] = [220, 25, 25, 255];
const GREEN: [u8; 4] = [25, 200, 40, 255];
const BLUE: [u8; 4] = [20, 30, 220, 255];

fn clip(directory: &Path) -> PathBuf {
    let path = directory.join("clip.gif");
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = image::codecs::gif::GifEncoder::new(file);
    for color in [RED, GREEN, BLUE] {
        encoder
            .encode_frame(image::Frame::from_parts(
                image::RgbaImage::from_pixel(32, 16, image::Rgba(color)),
                0,
                0,
                image::Delay::from_numer_denom_ms(500, 1),
            ))
            .unwrap();
    }
    path
}

fn near(pixel: &image::Rgba<u8>, color: [u8; 4]) -> bool {
    pixel.0.iter().zip(color).all(|(&actual, expected)| actual.abs_diff(expected) <= 12)
}

#[test]
fn picks_the_frame_near_the_target_clamped_to_half_the_clip() {
    let directory = tempfile::tempdir().unwrap();
    let path = clip(directory.path());
    let path = path.to_str().unwrap();
    let start = rgba_near(path, 0.0, (64, 64)).unwrap();
    assert!(near(start.get_pixel(4, 4), RED), "{:?}", start.get_pixel(4, 4));
    let later = rgba_near(path, 0.5, (64, 64)).unwrap();
    assert!(near(later.get_pixel(4, 4), GREEN), "{:?}", later.get_pixel(4, 4));
    let clamped = rgba_near(path, 30.0, (64, 64)).unwrap();
    assert!(near(clamped.get_pixel(4, 4), BLUE), "{:?}", clamped.get_pixel(4, 4));
}

#[test]
fn fits_inside_bounds_without_upscaling() {
    let directory = tempfile::tempdir().unwrap();
    let path = clip(directory.path());
    let path = path.to_str().unwrap();
    assert_eq!(rgba_near(path, 0.0, (8, 8)).unwrap().dimensions(), (8, 4));
    assert_eq!(rgba_near(path, 0.0, (1280, 720)).unwrap().dimensions(), (32, 16));
}

#[test]
fn rejects_undecodable_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("broken.mp4");
    std::fs::write(&path, b"not a video").unwrap();
    assert!(rgba_near(path.to_str().unwrap(), 1.0, (64, 64)).is_err());
}
