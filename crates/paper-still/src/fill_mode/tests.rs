#![cfg(test)]

use super::*;
use image::imageops::FilterType;

const RED: [u8; 4] = [255, 0, 0, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

fn solid(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
    let mut buf = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..(w * h) {
        buf.extend_from_slice(&color);
    }
    buf
}

fn px(pixels: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * w + x) * 4) as usize;
    [pixels[idx], pixels[idx + 1], pixels[idx + 2], pixels[idx + 3]]
}

#[test]
fn zero_surface_noop() {
    let pixels = solid(2, 2, RED);
    let (w, h, out) = apply_fill_mode(2, 2, &pixels.clone(), 0, 4, FillMode::Fill);
    assert_eq!((w, h), (2, 2));
    assert_eq!(out, pixels);
}

#[test]
fn zero_image_black() {
    let (w, h, out) = apply_fill_mode(0, 0, &Vec::new(), 3, 2, FillMode::Fill);
    assert_eq!((w, h), (3, 2));
    assert_eq!(out.len(), 3 * 2 * 4);
    for pixel in out.chunks_exact(4) {
        assert_eq!(pixel, BLACK);
    }
}

#[test]
fn stretch_dims() {
    let (w, h, out) = apply_fill_mode(4, 4, &solid(4, 4, RED), 8, 6, FillMode::Stretch);
    assert_eq!((w, h), (8, 6));
    assert_eq!(px(&out, 8, 0, 0), RED);
    assert_eq!(px(&out, 8, 7, 5), RED);
}

#[test]
fn fill_covers() {
    let (w, h, out) = apply_fill_mode(4, 2, &solid(4, 2, RED), 8, 8, FillMode::Fill);
    assert_eq!((w, h), (8, 8));
    for pixel in out.chunks_exact(4) {
        assert_eq!(pixel, RED);
    }
}

#[test]
fn fit_letterbox() {
    let (w, h, out) = apply_fill_mode(4, 2, &solid(4, 2, RED), 4, 4, FillMode::Fit);
    assert_eq!((w, h), (4, 4));
    assert_eq!(px(&out, 4, 0, 0), BLACK);
    assert_eq!(px(&out, 4, 3, 0), BLACK);
    assert_eq!(px(&out, 4, 0, 1), RED);
    assert_eq!(px(&out, 4, 2, 2), RED);
    assert_eq!(px(&out, 4, 0, 3), BLACK);
}

#[test]
fn center_pad() {
    let (w, h, out) = apply_fill_mode(2, 2, &solid(2, 2, RED), 4, 4, FillMode::Center);
    assert_eq!((w, h), (4, 4));
    assert_eq!(px(&out, 4, 0, 0), BLACK);
    assert_eq!(px(&out, 4, 1, 1), RED);
    assert_eq!(px(&out, 4, 2, 2), RED);
    assert_eq!(px(&out, 4, 3, 3), BLACK);
}

#[test]
fn center_crop() {
    let (w, h, out) = apply_fill_mode(8, 8, &solid(8, 8, RED), 4, 4, FillMode::Center);
    assert_eq!((w, h), (4, 4));
    for pixel in out.chunks_exact(4) {
        assert_eq!(pixel, RED);
    }
}

#[test]
fn tile_repeats() {
    let (w, h, out) = apply_fill_mode(2, 2, &solid(2, 2, RED), 5, 5, FillMode::Tile);
    assert_eq!((w, h), (5, 5));
    for pixel in out.chunks_exact(4) {
        assert_eq!(pixel, RED);
    }
}

fn gradient(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([
            (x * 255 / w.max(1)) as u8,
            (y * 255 / h.max(1)) as u8,
            ((x + y) * 255 / (w + h).max(1)) as u8,
            255,
        ])
    })
}

fn max_channel_diff(ours: &RgbaImage, theirs: &RgbaImage) -> u8 {
    ours.as_raw()
        .iter()
        .zip(theirs.as_raw().iter())
        .map(|(lhs, rhs)| lhs.abs_diff(*rhs))
        .max()
        .unwrap_or(0)
}

#[test]
fn identity_dims() {
    let img = gradient(16, 12);
    let pixels = img.as_raw().clone();
    for mode in [FillMode::Fill, FillMode::Fit, FillMode::Stretch, FillMode::Center, FillMode::Tile]
    {
        let (w, h, out) = apply_fill_mode(16, 12, &pixels.clone(), 16, 12, mode);
        assert_eq!((w, h), (16, 12));
        assert_eq!(out, pixels, "{mode:?}");
    }
}

fn view(img: &image::RgbaImage) -> super::SrcImage<'_> {
    super::SrcImage::from_raw(img.width(), img.height(), img.as_raw().as_slice()).unwrap()
}

#[test]
fn resample_downscale() {
    let img = gradient(64, 48);
    let ours = resample_view(&view(&img), 0, 0, 64, 48, 23, 17);
    let theirs = imageops::resize(&img, 23, 17, FilterType::Triangle);
    assert!(max_channel_diff(&ours, &theirs) <= 2);
}

#[test]
fn resample_upscale() {
    let img = gradient(9, 7);
    let ours = resample_view(&view(&img), 0, 0, 9, 7, 31, 22);
    let theirs = imageops::resize(&img, 31, 22, FilterType::Triangle);
    assert!(max_channel_diff(&ours, &theirs) <= 2);
}

#[test]
fn resample_crop_view() {
    let img = gradient(40, 30);
    let ours = resample_view(&view(&img), 8, 5, 24, 20, 12, 10);
    let cropped = imageops::crop_imm(&img, 8, 5, 24, 20).to_image();
    let theirs = imageops::resize(&cropped, 12, 10, FilterType::Triangle);
    assert!(max_channel_diff(&ours, &theirs) <= 2);
}

#[test]
fn opaque_black_full() {
    let buf = opaque_black(2, 3);
    assert_eq!(buf.len(), 2 * 3 * 4);
    for pixel in buf.chunks_exact(4) {
        assert_eq!(pixel, BLACK);
    }
}

fn horizontal_gradient(w: u32, h: u32) -> Vec<u8> {
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 4) as usize;
            buf[idx] = ((x * 255) / (w - 1).max(1)) as u8;
            buf[idx + 3] = 255;
        }
    }
    buf
}

#[test]
fn span_seam_continuity() {
    let (img_w, img_h) = (400, 100);
    let img = horizontal_gradient(img_w, img_h);
    let bounds = (0, 0, 400u32, 100u32);
    let (_, _, left) = span_pixels(img_w, img_h, &img, bounds, (0, 0, 200, 100), 200, 100);
    let (_, _, right) = span_pixels(img_w, img_h, &img, bounds, (200, 0, 200, 100), 200, 100);
    let left_last = px(&left, 200, 199, 50)[0] as i32;
    let right_first = px(&right, 200, 0, 50)[0] as i32;
    assert!((right_first - left_last).abs() <= 4);
    assert!(left_last > 100);
    let left_first = px(&left, 200, 0, 50)[0];
    let right_last = px(&right, 200, 199, 50)[0];
    assert!(left_first < 8);
    assert!(right_last > 247);
}

#[test]
fn span_physical_dims() {
    let img = horizontal_gradient(100, 100);
    let (w, h, out) = span_pixels(100, 100, &img, (0, 0, 100, 100), (0, 0, 50, 100), 100, 200);
    assert_eq!((w, h), (100, 200));
    assert_eq!(out.len(), 100 * 200 * 4);
    assert!(px(&out, 100, 99, 100)[0] < 135);
}

#[test]
fn span_degenerate_black() {
    let (w, h, out) = span_pixels(0, 0, &[], (0, 0, 100, 100), (0, 0, 50, 100), 64, 64);
    assert_eq!((w, h), (64, 64));
    assert!(out.chunks_exact(4).all(|pixel| pixel == [0, 0, 0, 255]));
}
