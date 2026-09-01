#![cfg(test)]

use super::resolve_viewport;
use crate::model::{ColorMatrix, FrameRate, VideoInfo};

fn video(width: i32, height: i32) -> VideoInfo {
    VideoInfo::new(width, height, FrameRate { numerator: 30, denominator: 1 }, ColorMatrix::Bt709)
        .unwrap()
}

#[test]
fn zero_configure_fallback() {
    assert_eq!(resolve_viewport(video(1920, 1080), 0, 0), (1920, 1080, (0, 0, 1920, 1080)));
}

#[test]
fn matching_aspect_full_frame() {
    assert_eq!(resolve_viewport(video(1920, 1080), 3840, 2160), (3840, 2160, (0, 0, 1920, 1080)));
}

#[test]
fn wider_output_crops_height() {
    assert_eq!(resolve_viewport(video(1920, 1080), 2560, 1080), (2560, 1080, (0, 135, 1920, 810)));
}

#[test]
fn taller_output_crops_width() {
    assert_eq!(resolve_viewport(video(1920, 1080), 1080, 1920), (1080, 1920, (656, 0, 608, 1080)));
}
