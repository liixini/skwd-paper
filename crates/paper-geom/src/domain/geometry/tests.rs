#![cfg(test)]

use super::*;

#[test]
fn fill_crop_center() {
    assert_eq!(fill_crop_rect(1920, 1080, 1920, 1080), (0, 0, 1920, 1080));
    let (cx, cy, cw, ch) = fill_crop_rect(3840, 1080, 1920, 1080);
    assert_eq!((cy, ch), (0, 1080));
    assert_eq!(cw, 1920);
    assert_eq!(cx, (3840 - 1920) / 2);
    let (cx, cy, cw, ch) = fill_crop_rect(1080, 1920, 1080, 540);
    assert_eq!((cx, cw), (0, 1080));
    assert_eq!(ch, 540);
    assert_eq!(cy, (1920 - 540) / 2);
}

#[test]
fn fit_size_aspect() {
    assert_eq!(fit_scaled_size(1920, 1080, 1920, 1080), (1920, 1080));
    let (w, h) = fit_scaled_size(3840, 1080, 1920, 1080);
    assert_eq!(w, 1920);
    assert_eq!(h, 540);
    let (w, h) = fit_scaled_size(1080, 1920, 1920, 1080);
    assert_eq!(h, 1080);
    assert!((w as f32 / h as f32 - 1080.0 / 1920.0).abs() < 0.01);
}

#[test]
fn cover_uv_landscape() {
    let [sx, sy, ox, oy] = cover_uv(1920, 1080, 1080, 1920);
    assert!(sx < 1.0 && sy == 1.0);
    assert!((ox - (1.0 - sx) * 0.5).abs() < 1e-6 && oy == 0.0);
}

#[test]
fn cover_uv_portrait() {
    let [sx, sy, _, oy] = cover_uv(1080, 1920, 1920, 1080);
    assert!(sy < 1.0 && sx == 1.0);
    assert!((oy - (1.0 - sy) * 0.5).abs() < 1e-6);
}

#[test]
fn cover_uv_identity() {
    assert_eq!(cover_uv(1920, 1080, 3840, 2160), [1.0, 1.0, 0.0, 0.0]);
}

#[test]
fn cover_dims_aspect() {
    assert_eq!(cover_dims(1920, 1080, 640, 360), (640, 360));
    let (cw, ch) = cover_dims(1080, 1920, 640, 360);
    assert!(cw >= 640 && ch >= 360);
    assert!((cw as f64 / ch as f64 - 1080.0 / 1920.0).abs() < 0.01);
}

#[test]
fn fill_uv_modes() {
    assert_eq!(fill_uv_remap(100, 100, 200, 100, FillMode::Stretch), ([1.0, 1.0], [0.0, 0.0]));
    let ([sx, sy], [ox, oy]) = fill_uv_remap(3840, 1080, 1920, 1080, FillMode::Fill);
    assert!(sx < 1.0 && sy == 1.0 && ox > 0.0 && oy == 0.0);
    let ([sx, sy], [ox, oy]) = fill_uv_remap(3840, 1080, 1920, 1080, FillMode::Fit);
    assert!(sy > 1.0 && sx == 1.0 && oy < 0.0 && ox == 0.0);
    let ([sx, sy], _) = fill_uv_remap(960, 540, 1920, 1080, FillMode::Tile);
    assert_eq!([sx, sy], [2.0, 2.0]);
}

#[test]
fn fill_mode_parse() {
    assert_eq!("fill".parse::<FillMode>(), Ok(FillMode::Fill));
    assert_eq!("tile".parse::<FillMode>(), Ok(FillMode::Tile));
    assert_eq!("span".parse::<FillMode>(), Ok(FillMode::Span));
    assert!("banana".parse::<FillMode>().is_err());
}

#[test]
fn fill_mode_wire_tokens() {
    let cases = FillMode::ALL.into_iter().zip(["fill", "fit", "stretch", "center", "tile", "span"]);
    assert_eq!(FillMode::default(), FillMode::Fill);
    for (mode, token) in cases {
        assert_eq!(mode.as_str(), token);
        assert_eq!(serde_json::to_string(&mode).unwrap(), format!("\"{token}\""));
        assert_eq!(serde_json::from_str::<FillMode>(&format!("\"{token}\"")).unwrap(), mode);
    }
}

#[test]
fn desktop_bounds_union() {
    assert_eq!(desktop_bounds(&[]), None);
    assert_eq!(desktop_bounds(&[(0, 0, 1920, 1080)]), Some((0, 0, 1920, 1080)));
    assert_eq!(
        desktop_bounds(&[(1920, 0, 2560, 1440), (0, 0, 1920, 1080), (4480, 0, 2560, 1440)]),
        Some((0, 0, 7040, 1440))
    );
    assert_eq!(
        desktop_bounds(&[(-1920, -500, 1920, 1080), (0, 0, 2560, 1440)]),
        Some((-1920, -500, 4480, 1940))
    );
    assert_eq!(desktop_bounds(&[(0, 0, 0, 0), (5, 5, 10, 10)]), Some((5, 5, 10, 10)));
}

#[test]
fn span_crop_partition() {
    let bounds = (0, 0, 3840, 1080);
    let left = span_crop_rect(3840, 1080, bounds, (0, 0, 1920, 1080));
    let right = span_crop_rect(3840, 1080, bounds, (1920, 0, 1920, 1080));
    assert_eq!(left, (0, 0, 1920, 1080));
    assert_eq!(right, (1920, 0, 1920, 1080));
}

#[test]
fn span_crop_aspect() {
    let bounds = (0, 0, 3840, 1080);
    let left = span_crop_rect(3840, 2160, bounds, (0, 0, 1920, 1080));
    let right = span_crop_rect(3840, 2160, bounds, (1920, 0, 1920, 1080));
    assert_eq!(left.1, right.1);
    assert_eq!(left.1, (2160 - 1080) / 2);
    assert_eq!((left.2, left.3), (1920, 1080));
    assert_eq!(right.0, 1920);
}

#[test]
fn span_crop_upscale() {
    let bounds = (0, 0, 3840, 1080);
    let left = span_crop_rect(1920, 540, bounds, (0, 0, 1920, 1080));
    let right = span_crop_rect(1920, 540, bounds, (1920, 0, 1920, 1080));
    assert_eq!(left, (0, 0, 960, 540));
    assert_eq!(right.0, 960);
    assert_eq!(right.2, 960);
}

#[test]
fn span_crop_offset() {
    let bounds = (-1920, 0, 3840, 1080);
    let left = span_crop_rect(3840, 1080, bounds, (-1920, 0, 1920, 1080));
    assert_eq!(left, (0, 0, 1920, 1080));
    let rect = span_crop_rect(0, 0, bounds, (0, 0, 1920, 1080));
    assert!(rect.2 > 0 && rect.3 > 0);
}
