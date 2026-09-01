#![cfg(test)]

use super::{choose_shm_format, decode_image, pack_pixels};
use paper_control::StillCommand;
use wayland_client::protocol::wl_shm::Format;

fn write_solid_png(w: u32, h: u32, px: [u8; 4], name: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("skwd-still-img-{}-{name}.png", std::process::id()));
    image::RgbaImage::from_pixel(w, h, image::Rgba(px)).save(&path).unwrap();
    path
}

#[test]
fn still_line_parse() {
    let cmd: StillCommand = serde_json::from_str(r#"{"path":"/w/a.png"}"#).unwrap();
    assert_eq!(cmd.path, "/w/a.png");
    assert!(cmd.slide.is_none() && cmd.preload.is_empty());
    let with_extras: StillCommand =
        serde_json::from_str(r#"{"path":"/w/a.png","mute":true,"volume":50}"#).unwrap();
    assert_eq!(with_extras.path, "/w/a.png");
    let slide: StillCommand =
        serde_json::from_str(r#"{"path":"/w/a.png","slide":"down","duration_ms":250}"#).unwrap();
    assert_eq!(slide.slide.as_deref(), Some("down"));
    assert_eq!(slide.duration_ms, Some(250));
    let preload: StillCommand =
        serde_json::from_str(r#"{"path":"","preload":["/w/a.png","/w/b.png"]}"#).unwrap();
    assert!(preload.path.is_empty());
    assert_eq!(preload.preload.len(), 2);
    for line in [r#"{"path":7}"#, "{"] {
        assert!(serde_json::from_str::<StillCommand>(line).is_err());
    }
}

#[test]
fn compose_tall_slide() {
    let old = [1u8; 8];
    let new = [2u8; 8];
    let mut canvas = [0u8; 16];
    super::compose_tall(&mut canvas, &old, &new, true);
    assert_eq!(&canvas[..8], &old);
    assert_eq!(&canvas[8..], &new);
    super::compose_tall(&mut canvas, &old, &new, false);
    assert_eq!(&canvas[..8], &new);
    assert_eq!(&canvas[8..], &old);

    assert_eq!(super::slide_source_y(true, 100, 0.0), 0.0);
    assert_eq!(super::slide_source_y(true, 100, 1.0), 100.0);
    assert_eq!(super::slide_source_y(false, 100, 0.0), 100.0);
    assert_eq!(super::slide_source_y(false, 100, 1.0), 0.0);

    let eased = super::ease_out_cubic(0.5);
    assert!(eased > 0.5 && eased < 1.0);
    assert_eq!(super::ease_out_cubic(0.0), 0.0);
    assert_eq!(super::ease_out_cubic(1.0), 1.0);
    assert_eq!(super::ease_out_cubic(7.0), 1.0);
}

#[test]
fn decode_passthrough() {
    let path = write_solid_png(4, 3, [200, 100, 50, 255], "plain");
    let (w, h, bytes) = decode_image(path.to_str().unwrap(), 0.0, 0).unwrap();
    assert_eq!((w, h), (4, 3));
    assert_eq!(&bytes[..4], &[200, 100, 50, 255]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn dim_scales_rgb() {
    let path = write_solid_png(2, 2, [200, 100, 50, 255], "dim50");
    let (_, _, bytes) = decode_image(path.to_str().unwrap(), 0.0, 50).unwrap();
    assert_eq!(&bytes[..4], &[100, 50, 25, 255]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn dim_saturates() {
    let path = write_solid_png(2, 2, [200, 100, 50, 255], "dimover");
    let (_, _, at100) = decode_image(path.to_str().unwrap(), 0.0, 100).unwrap();
    assert_eq!(&at100[..4], &[0, 0, 0, 255]);
    let (_, _, over) = decode_image(path.to_str().unwrap(), 0.0, 150).unwrap();
    assert_eq!(&over[..4], &[0, 0, 0, 255]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn blur_caps_size() {
    let path = write_solid_png(3200, 100, [40, 120, 220, 255], "blurcap");
    let (w, h, bytes) = decode_image(path.to_str().unwrap(), 2.0, 0).unwrap();
    assert_eq!((w, h), (1600, 50));
    let center = ((h / 2) * w + w / 2) as usize * 4;
    let px = &bytes[center..center + 4];
    for (idx, want) in [40u8, 120, 220, 255].iter().enumerate() {
        assert!(px[idx].abs_diff(*want) <= 3);
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn blur_small_untouched() {
    let path = write_solid_png(120, 80, [10, 20, 30, 255], "blursmall");
    let (w, h, _) = decode_image(path.to_str().unwrap(), 3.0, 0).unwrap();
    assert_eq!((w, h), (120, 80));
    let _ = std::fs::remove_file(path);
}

#[test]
fn decode_missing_err() {
    assert!(decode_image("/nonexistent/skwd-test.png", 0.0, 0).is_err());
}

#[test]
fn shm_format_choice() {
    assert_eq!(
        choose_shm_format(&[Format::Argb8888, Format::Abgr8888, Format::Xrgb8888]),
        Format::Abgr8888
    );
    assert_eq!(choose_shm_format(&[Format::Argb8888, Format::Xrgb8888]), Format::Argb8888);
    assert_eq!(choose_shm_format(&[]), Format::Argb8888);
}

#[test]
fn pack_formats() {
    let rgba = [10u8, 20, 30, 40, 50, 60, 70, 80];

    let mut abgr = [0u8; 8];
    pack_pixels(&mut abgr, &rgba, Format::Abgr8888);
    assert_eq!(abgr, rgba);

    let mut argb = [0u8; 8];
    pack_pixels(&mut argb, &rgba, Format::Argb8888);
    assert_eq!(argb, [30, 20, 10, 40, 70, 60, 50, 80]);
}

#[test]
fn physical_size_ratio() {
    use super::lifecycle::physical_size;
    use wayland_client::protocol::wl_output::Transform;

    assert_eq!(
        physical_size((2560, 1440), Some((2560, 1440)), Some((2560, 1440)), Transform::Normal, 1),
        (2560, 1440)
    );
    assert_eq!(
        physical_size((1440, 2560), Some((1440, 2560)), Some((3840, 2160)), Transform::_90, 2),
        (2160, 3840)
    );
    assert_eq!(
        physical_size((1920, 1080), Some((1920, 1080)), Some((3840, 2160)), Transform::Normal, 2),
        (3840, 2160)
    );
    assert_eq!(
        physical_size(
            (1440, 2560),
            Some((1440, 2560)),
            Some((3840, 2160)),
            Transform::Flipped270,
            2
        ),
        (2160, 3840)
    );
    assert_eq!(physical_size((1600, 900), None, None, Transform::Normal, 2), (3200, 1800));
    assert_eq!(
        physical_size((1600, 900), Some((0, 0)), Some((1600, 900)), Transform::Normal, 1),
        (1600, 900)
    );
}
