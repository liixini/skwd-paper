#![cfg(test)]

use super::*;
use paper_geom::cover_uv;

#[test]
fn fill_uv_matches_cover() {
    for (source_width, source_height, target_width, target_height) in
        [(3440, 1440, 2560, 1440), (1920, 1080, 2560, 1440), (1080, 1920, 1920, 1080)]
    {
        assert_eq!(
            mode_uv_for(source_width, source_height, target_width, target_height, FillMode::Fill),
            cover_uv(source_width, source_height, target_width, target_height)
        );
    }
}

#[test]
fn cover_uv_center_identity() {
    let [scale_x, scale_y, offset_x, offset_y] = cover_uv(1920, 1080, 1080, 1920);
    let (x, _, width, _) = paper_geom::fill_crop_rect(1920, 1080, 1080, 1920);
    assert_eq!(scale_x, width as f32 / 1920.0);
    assert_eq!(scale_y, 1.0);
    assert_eq!(offset_x, x as f32 / 1920.0);
    // Odd discarded-pixel counts keep the extra pixel at the far edge in both renderers.
    assert!((scale_x * 0.5 + offset_x - 0.5).abs() <= 0.5 / 1920.0);
    assert_eq!(offset_y, 0.0);

    assert_eq!(cover_uv(1000, 1000, 500, 500), [1.0, 1.0, 0.0, 0.0]);
    assert_eq!(cover_uv(1920, 1080, 3840, 2160), [1.0, 1.0, 0.0, 0.0]);
}

#[test]
fn fit_and_stretch() {
    let [scale_x, scale_y, offset_x, offset_y] = mode_uv_for(3440, 1440, 2560, 1440, FillMode::Fit);
    assert_eq!([scale_x, offset_x], [1.0, 0.0]);
    assert!(scale_y > 1.0);
    assert!(offset_y < 0.0);
    assert!(offset_y + scale_y > 1.0);

    assert_eq!(mode_uv_for(3440, 1440, 2560, 1440, FillMode::Stretch), [1.0, 1.0, 0.0, 0.0]);
}

#[test]
fn shader_flags_map_fill_modes() {
    assert_eq!(flag_for(FillMode::Fill), 0);
    assert_eq!(flag_for(FillMode::Span), 0);
    assert_eq!(flag_for(FillMode::Stretch), 0);
    assert_eq!(flag_for(FillMode::Fit), 1);
    assert_eq!(flag_for(FillMode::Center), 1);
    assert_eq!(flag_for(FillMode::Tile), 2);
}
