#![cfg(test)]

use super::*;

#[test]
fn exact_modifier_match() {
    let niri = vec![
        (FOURCC_XR24, 0),
        (FOURCC_XR24, 0x0300_0000_0060_6010),
        (FOURCC_NV12, 0),
        (FOURCC_NV12, 0x0300_0000_0060_6010),
    ];
    assert!(format_supported(&niri, FOURCC_XR24, 0));
    assert!(format_supported(&niri, FOURCC_NV12, 0));

    let kwin = vec![
        (FOURCC_XR24, 0x0300_0000_0060_6010),
        (FOURCC_XR24, 0x00ff_ffff_ffff_ffff),
        (FOURCC_NV12, 0),
    ];
    assert!(!format_supported(&kwin, FOURCC_XR24, 0));
    assert!(format_supported(&kwin, FOURCC_NV12, 0));
    assert!(!format_supported(&[], FOURCC_XR24, 0));
}

#[test]
fn tiled_modifier_order() {
    let first = 0x0300_0000_0060_6010;
    let second = 0x0300_0000_0060_6011;
    let formats = vec![
        (FOURCC_NV12, first),
        (FOURCC_NV12, 0),
        (FOURCC_NV12, second),
        (FOURCC_NV12, first),
        (FOURCC_NV12, DRM_MOD_INVALID),
    ];
    assert_eq!(preferred_tiled_modifiers(&formats, FOURCC_NV12), vec![first, second]);
}

#[test]
fn tiled_only_usable() {
    let modifier = 0x0300_0000_0060_6010;
    assert!(format_usable(&[(FOURCC_XR24, modifier)], FOURCC_XR24));
    assert!(!format_usable(&[(FOURCC_NV12, modifier)], FOURCC_XR24));
}

#[test]
fn xr24_export_tiling() {
    let tiled = 0x0300_0000_0060_6010;
    let formats = [(FOURCC_XR24, tiled), (FOURCC_XR24, DRM_MOD_INVALID)];
    assert_eq!(xr24_export_modifiers(&formats, false), (vec![tiled], Some(DRM_MOD_INVALID)));
    assert_eq!(xr24_export_modifiers(&formats, true), (vec![], Some(DRM_MOD_INVALID)));
    assert_eq!(xr24_export_modifiers(&[(FOURCC_XR24, tiled)], true), (vec![], None));
}
