pub(crate) const FOURCC_XR24: u32 = 0x3432_5258;
pub(crate) const FOURCC_NV12: u32 = 0x3231_564e;
pub(crate) const DRM_MOD_LINEAR: u64 = 0;
pub(crate) const DRM_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

pub(crate) fn format_supported(formats: &[(u32, u64)], fourcc: u32, modifier: u64) -> bool {
    formats
        .iter()
        .any(|&(format, candidate_modifier)| format == fourcc && candidate_modifier == modifier)
}

pub(crate) fn format_usable(formats: &[(u32, u64)], fourcc: u32) -> bool {
    formats.iter().any(|&(format, _)| format == fourcc)
}

pub(crate) fn preferred_modifier(formats: &[(u32, u64)], fourcc: u32) -> u64 {
    if format_supported(formats, fourcc, DRM_MOD_LINEAR) {
        DRM_MOD_LINEAR
    } else if format_supported(formats, fourcc, DRM_MOD_INVALID) {
        DRM_MOD_INVALID
    } else {
        DRM_MOD_LINEAR
    }
}

pub(crate) fn preferred_tiled_modifiers(formats: &[(u32, u64)], fourcc: u32) -> Vec<u64> {
    let mut modifiers = Vec::new();
    for &(format, modifier) in formats {
        if format == fourcc
            && modifier != DRM_MOD_LINEAR
            && modifier != DRM_MOD_INVALID
            && !modifiers.contains(&modifier)
        {
            modifiers.push(modifier);
        }
    }
    modifiers
}

pub(crate) fn xr24_export_modifiers(
    formats: &[(u32, u64)],
    force_linear: bool,
) -> (Vec<u64>, Option<u64>) {
    let tiled =
        if force_linear { Vec::new() } else { preferred_tiled_modifiers(formats, FOURCC_XR24) };
    let linear = [DRM_MOD_LINEAR, DRM_MOD_INVALID]
        .into_iter()
        .find(|modifier| format_supported(formats, FOURCC_XR24, *modifier));
    (tiled, linear)
}

mod tests;
