use std::sync::OnceLock;

use paper_geom::FillMode;

static FILL_MODE: OnceLock<FillMode> = OnceLock::new();

pub(crate) fn set_fill_mode(mode: FillMode) {
    let _ = FILL_MODE.set(mode);
}

pub(crate) fn fill_mode() -> FillMode {
    FILL_MODE.get().copied().unwrap_or_default()
}

fn flag_for(mode: FillMode) -> i32 {
    match mode {
        FillMode::Fit | FillMode::Center => 1,
        FillMode::Tile => 2,
        _ => 0,
    }
}

static CLAMP_OVERRIDE: OnceLock<Option<i32>> = OnceLock::new();

fn clamp_override() -> Option<i32> {
    *CLAMP_OVERRIDE.get_or_init(|| match std::env::var("SKWD_VK_SCENE_CLAMP").ok().as_deref() {
        Some("clamp") => Some(0),
        Some("border") => Some(1),
        Some("repeat") => Some(2),
        _ => None,
    })
}

pub(crate) fn fill_flag() -> i32 {
    clamp_override().unwrap_or_else(|| flag_for(fill_mode()))
}

fn mode_uv_for(
    source_width: u32,
    source_height: u32,
    surface_width: u32,
    surface_height: u32,
    mode: FillMode,
) -> [f32; 4] {
    let (scale, offset) =
        paper_geom::fill_uv_remap(source_width, source_height, surface_width, surface_height, mode);
    [scale[0], scale[1], offset[0], offset[1]]
}

pub(crate) fn mode_uv(
    source_width: u32,
    source_height: u32,
    surface_width: u32,
    surface_height: u32,
) -> [f32; 4] {
    mode_uv_for(source_width, source_height, surface_width, surface_height, fill_mode())
}

mod tests;
