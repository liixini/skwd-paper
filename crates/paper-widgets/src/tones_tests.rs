use super::*;

const PALETTE: Palette =
    Palette { text: [236, 235, 231], accent: [90, 110, 200], shade: [18, 19, 22] };

#[test]
fn keeps_light_text() {
    let tones = Tones::over(&PALETTE, 0.02);
    assert_eq!(tones.text.rgb, PALETTE.text);
    assert_eq!(tones.shade.rgb, PALETTE.shade);
}

#[test]
fn flips_on_light_ground() {
    let tones = Tones::over(&PALETTE, 0.8);
    assert_eq!(tones.text.rgb, PALETTE.shade);
}

#[test]
fn keeps_better_ink() {
    for ground in (0..=100).map(|step| step as f32 / 100.0) {
        let tones = Tones::over(&PALETTE, ground);
        let chosen = contrast(luminance(tones.text.rgb), ground);
        let other = contrast(luminance(tones.shade.rgb), ground);
        assert!(chosen >= TEXT_FLOOR || chosen >= other, "ground {ground}");
    }
}

#[test]
fn accent_meets_floor() {
    for ground in [0.0, 0.05, 0.2, 0.5, 0.9] {
        let accent = Tones::over(&PALETTE, ground).accent.rgb;
        assert!(contrast(luminance(accent), ground) >= ACCENT_FLOOR - 0.01, "ground {ground}");
    }
}

#[test]
fn luminance_spans_unit() {
    assert_eq!(luminance([0; 3]), 0.0);
    assert!((luminance([u8::MAX; 3]) - 1.0).abs() < 1e-4);
}

#[test]
fn ground_averages_area() {
    let mut rgba = vec![0; 8 * 8 * 4];
    for pixel in rgba.chunks_mut(4).take(32) {
        pixel.copy_from_slice(&[u8::MAX; 4]);
    }
    assert!((ground(&rgba, 8, [0, 0, 8, 4]) - 1.0).abs() < 1e-4);
    assert_eq!(ground(&rgba, 8, [0, 4, 8, 4]), 0.0);
    assert_eq!(ground(&rgba, 8, [0, 9, 8, 4]), 0.0);
}
