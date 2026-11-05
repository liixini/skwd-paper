#![deny(unsafe_code)]

mod effects;
mod sand;

pub use effects::{
    EFFECTS, RANDOM_EFFECT_COUNT, STUDIO_ENVIRONMENT_RGBA, STUDIO_ENVIRONMENT_SIZE, effect_frag_vk,
    effect_index, effect_requires_environment, resolve_shader,
};
pub use sand::{
    Dialect, SAND_A_ONLY_BELOW, SAND_B_ONLY_FROM, SAND_BASE_FRAG_GL, SAND_BASE_FRAG_VK,
    SAND_GRAIN_FRAG_GL, SAND_GRAIN_FRAG_VK, SAND_GRAIN_GH, SAND_GRAIN_GW, SAND_GRAIN_INSTANCES,
    SAND_GRAIN_VERT_GL, SAND_STYLE_IDS, SAND_STYLES, sand_base_frag, sand_base_new, sand_base_old,
    sand_grain_frag, sand_grain_point_frag, sand_grain_point_vert, sand_grain_vert,
    sand_style_index, sand_window,
};

mod tests;

mod drawing;
pub use drawing::{DRAWING_BUILD_END, DRAWING_DRY_SECONDS, DRAWING_PAPER_START, DRAWING_SHADER};
