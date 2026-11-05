use super::resources::{
    CHROMATIC_BLOOM_FRAG, CIRCLE_CROP_FRAG, COLOUR_DISTANCE_FRAG, CROP_RELAY_FRAG, CROSSFADE_FRAG,
    CROSSHATCH_FRAG, CROSSWARP_FRAG, DIRECTIONAL_FRAG, DIRECTIONAL_SCALED_FRAG,
    DIRECTIONAL_WIPE_FRAG, FADECOLOR_FRAG, GLITCH_DISPLACE_FRAG, GLITCH_FRAG, HEAT_MELT_FRAG,
    INK_SPLASH_FRAG, INKWELL_DROP_FRAG, IRIS_FRAG, MORPH_FRAG, MOSAIC_TUMBLE_FRAG,
    PAPER_HEART_FRAG, PARAMETRIC_GLITCH_FRAG, PERLIN_FRAG, PHOTO_CUTOUT_FRAG, PIXELATE_FRAG,
    PIXELFADE_WAVE_FRAG, PLASMA_FLOW_FRAG, POLKA_DOTS_CURTAIN_FRAG, PUZZLE_RIGHT_FRAG,
    RANDOMSQUARES_FRAG, SLAT_TURN_FRAG, SMOKE_FRAG, SOFT_WARP_FADE_FRAG, SUNLIT_ARCADE_FRAG,
    VORONOI_SHATTER_FRAG, ZOOM_BLUR_PULL_FRAG,
};
use paper_control::transitions::{CROP_RELAY, PAPER_HEART, PHOTO_CUTOUT, SLAT_TURN, SUNLIT_ARCADE};

pub const RANDOM_SHADER: &str = "random";

pub const RANDOM_EFFECT_COUNT: usize = 30;

pub const EFFECTS: [(&str, &str); 36] = [
    ("pixelate", PIXELATE_FRAG),
    ("iris", IRIS_FRAG),
    ("glitch", GLITCH_FRAG),
    ("voronoi-shatter", VORONOI_SHATTER_FRAG),
    ("heat-melt", HEAT_MELT_FRAG),
    ("plasma-flow", PLASMA_FLOW_FRAG),
    ("ink-splash", INK_SPLASH_FRAG),
    ("smoke", SMOKE_FRAG),
    ("chromatic-bloom", CHROMATIC_BLOOM_FRAG),
    ("inkwell-drop", INKWELL_DROP_FRAG),
    ("pixelfade-wave", PIXELFADE_WAVE_FRAG),
    ("soft-warp-fade", SOFT_WARP_FADE_FRAG),
    ("zoom-blur-pull", ZOOM_BLUR_PULL_FRAG),
    ("mosaic-tumble", MOSAIC_TUMBLE_FRAG),
    ("crosswarp", CROSSWARP_FRAG),
    ("morph", MORPH_FRAG),
    ("circle-crop", CIRCLE_CROP_FRAG),
    ("colour-distance", COLOUR_DISTANCE_FRAG),
    ("crossfade", CROSSFADE_FRAG),
    ("directional", DIRECTIONAL_FRAG),
    ("directional-scaled", DIRECTIONAL_SCALED_FRAG),
    ("glitch-displace", GLITCH_DISPLACE_FRAG),
    ("polka-dots-curtain", POLKA_DOTS_CURTAIN_FRAG),
    ("puzzle-right", PUZZLE_RIGHT_FRAG),
    ("crosshatch", CROSSHATCH_FRAG),
    ("directional-wipe", DIRECTIONAL_WIPE_FRAG),
    ("fadecolor", FADECOLOR_FRAG),
    ("parametric-glitch", PARAMETRIC_GLITCH_FRAG),
    ("perlin", PERLIN_FRAG),
    ("randomsquares", RANDOMSQUARES_FRAG),
    (crate::DRAWING_SHADER, include_str!("../../glsl/drawing.frag")),
    (PHOTO_CUTOUT, PHOTO_CUTOUT_FRAG),
    (SLAT_TURN, SLAT_TURN_FRAG),
    (CROP_RELAY, CROP_RELAY_FRAG),
    (SUNLIT_ARCADE, SUNLIT_ARCADE_FRAG),
    (PAPER_HEART, PAPER_HEART_FRAG),
];

pub fn effect_index(name: &str) -> Option<usize> {
    EFFECTS.iter().position(|(entry, _)| *entry == name)
}

pub fn effect_requires_environment(index: usize) -> bool {
    EFFECTS.get(index).is_some_and(|(name, _)| matches!(*name, SLAT_TURN | SUNLIT_ARCADE))
}

pub fn resolve_shader(name: &str) -> &str {
    if name != RANDOM_SHADER {
        return name;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    let sand = crate::SAND_STYLES.len();
    let pick = nanos as usize % (sand + RANDOM_EFFECT_COUNT);
    if pick < sand { crate::SAND_STYLES[pick] } else { EFFECTS[pick - sand].0 }
}
