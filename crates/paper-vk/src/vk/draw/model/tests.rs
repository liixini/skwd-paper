use super::{EFFECT_PUSH_BYTES, EffectPush, remap_texture_uv};

#[test]
fn h264_row_padding() {
    assert_eq!(
        remap_texture_uv([1.0, 1.0, 0.0, 0.0], (1920, 1088), (1920, 1080), (0, 0)),
        [1.0, 1080.0 / 1088.0, 0.0, 0.0]
    );
}

#[test]
fn visible_region_crop() {
    let uv = remap_texture_uv([0.8, 0.75, 0.1, 0.125], (2048, 1088), (1920, 1080), (8, 0));
    for (actual, expected) in
        uv.into_iter().zip([0.75, 0.75 * 1080.0 / 1088.0, 200.0 / 2048.0, 135.0 / 1088.0])
    {
        assert!((actual - expected).abs() < 1e-6);
    }
}

#[test]
fn push_matches_glsl() {
    assert_eq!(std::mem::offset_of!(EffectPush, uv_a), 32);
    assert_eq!(std::mem::offset_of!(EffectPush, colours), 64);
    assert_eq!(std::mem::offset_of!(EffectPush, clock), 72);
    assert_eq!(std::mem::offset_of!(EffectPush, ink_style), 76);
    assert_eq!(EFFECT_PUSH_BYTES, 80);
}
