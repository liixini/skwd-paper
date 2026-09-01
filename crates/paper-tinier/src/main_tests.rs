use super::*;
use crate::model::{ColorMatrix, FrameRate};

#[test]
fn wake_coalesces() {
    let wake = control_wake().unwrap();
    assert_eq!(drain(wake.as_raw_fd()), Ok(()));
    notify(wake.as_raw_fd());
    notify(wake.as_raw_fd());
    assert_eq!(drain(wake.as_raw_fd()), Ok(()));
    assert_eq!(drain(wake.as_raw_fd()), Ok(()));
}

#[test]
fn freeze_frame_pixels() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("frame.ppm");
    let video =
        VideoInfo::new(2, 1, FrameRate { numerator: 30, denominator: 1 }, ColorMatrix::Bt709)
            .unwrap();
    write_freeze(path.to_str().unwrap(), &[0, 0, 255, 0, 0, 255, 0, 0], video).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"P6\n2 1\n255\n\xff\0\0\0\xff\0");
    assert!(write_freeze(path.to_str().unwrap(), &[0; 8], video).is_err());
    let error_path = directory.path().join("frame.error");
    write_freeze_error(error_path.to_str().unwrap(), "capture failed").unwrap();
    assert_eq!(std::fs::read_to_string(error_path).unwrap(), "capture failed");
}

#[test]
fn stream_frame_scaling_and_channel_order() {
    let source = [0, 0, 255, 0, 0, 255, 0, 0];
    let frame = compose_stream_frame(&source, 2, 1, 4, 1, paper_geom::FillMode::Stretch);
    assert_eq!(frame, [255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255,]);
}

#[test]
fn stream_fit_letterboxes() {
    let source = [255, 0, 0, 0];
    let frame = compose_stream_frame(&source, 1, 1, 3, 1, paper_geom::FillMode::Fit);
    assert_eq!(frame, [0, 0, 0, 255, 0, 0, 255, 255, 0, 0, 0, 255]);
}
