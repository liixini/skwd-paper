use super::{import_modifier_gate, unlisted_import_modifiers};
use crate::dmabuf::{DRM_MOD_INVALID, DRM_MOD_LINEAR};

const TILED: u64 = 0x0200_0000_0000_0442;

#[test]
fn gate_rejects_devices_without_modifier_extension() {
    let error = import_modifier_gate(false, DRM_MOD_LINEAR, DRM_MOD_LINEAR).unwrap_err();
    assert!(error.to_string().contains("no DRM format modifier support"));
}

#[test]
fn gate_rejects_invalid_modifier_on_either_plane() {
    assert!(import_modifier_gate(true, DRM_MOD_INVALID, DRM_MOD_LINEAR).is_err());
    assert!(import_modifier_gate(true, DRM_MOD_LINEAR, DRM_MOD_INVALID).is_err());
}

#[test]
fn gate_accepts_explicit_modifiers() {
    assert!(import_modifier_gate(true, DRM_MOD_LINEAR, DRM_MOD_LINEAR).is_ok());
    assert!(import_modifier_gate(true, TILED, TILED).is_ok());
    assert!(import_modifier_gate(true, TILED, DRM_MOD_LINEAR).is_ok());
}

#[test]
fn unlisted_check_is_per_plane_and_only_advisory() {
    let listed = [vec![DRM_MOD_LINEAR, TILED], vec![DRM_MOD_LINEAR]];
    assert!(unlisted_import_modifiers(&listed, DRM_MOD_LINEAR, DRM_MOD_LINEAR).is_empty());
    assert_eq!(unlisted_import_modifiers(&listed, TILED, TILED), [("chroma", TILED)]);
    assert_eq!(
        unlisted_import_modifiers(&[Vec::new(), Vec::new()], TILED, DRM_MOD_LINEAR),
        [("luma", TILED), ("chroma", DRM_MOD_LINEAR)]
    );
}
