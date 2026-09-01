#![cfg(test)]

use super::*;

#[test]
fn gl_dialect_source() {
    let vert = sand_grain_vert(Dialect::Gl330);
    assert!(vert.starts_with("#version 330 core"));
    assert!(
        vert.contains("int pid = gl_VertexID / 6 + u_pid_base;")
            && vert.contains("int corner = gl_VertexID % 6;")
    );
    assert_eq!(sand_grain_frag(Dialect::Gl330).matches("sampler2D").count(), 2);
}

#[test]
fn vulkan_dialect_transform() {
    let vert = sand_grain_vert(Dialect::Vulkan450);
    assert!(vert.starts_with("#version 450"));
    assert!(!vert.contains("gl_VertexID\n") && !vert.contains("gl_VertexID "));
    assert!(vert.contains("int pid = gl_VertexIndex / 6 + u_pid_base;"));
    assert!(vert.contains("push_constant"));
    let gl_body = sand_grain_vert(Dialect::Gl330);
    let math_marker = "vec2 guide = mix(mix(p0, waist, ease), mix(waist, p2, ease), ease);";
    assert!(vert.contains(math_marker) && gl_body.contains(math_marker));
    let frag = sand_grain_frag(Dialect::Vulkan450);
    assert!(frag.contains("nv12(") && frag.contains("set = 1"));
    assert!(frag.contains("u_uv_a") && frag.contains("u_uv_b"));
}

#[test]
fn grain_grid_consts() {
    let vert = sand_grain_vert(Dialect::Gl330);
    assert!(vert.contains(&format!("const int GW = {SAND_GRAIN_GW}")));
    assert!(vert.contains(&format!("const int GH = {SAND_GRAIN_GH}")));
    assert_eq!(SAND_GRAIN_INSTANCES, SAND_GRAIN_GW * SAND_GRAIN_GH * 2);
}

#[test]
fn population_window() {
    let vert = sand_grain_vert(Dialect::Gl330);
    assert!(
        vert.contains("float pb = (u_progress - 0.20) / 0.62;")
            && vert.contains("tt = clamp((pb - 0.04) * 2.3")
            && vert.contains("v_alpha = smoothstep(0.02, 0.15, tt);"),
        "re-derive SAND_A_ONLY_BELOW"
    );
    assert!(
        vert.contains("tt = clamp(u_progress / 0.55 * 1.9")
            && vert.contains("v_alpha = 1.0 - smoothstep(0.80, 0.99, tt);"),
        "re-derive SAND_B_ONLY_FROM"
    );
    const { assert!(SAND_A_ONLY_BELOW <= 0.3768 && SAND_B_ONLY_FROM >= 0.5327) };
    let half = SAND_GRAIN_INSTANCES / 2;
    assert_eq!(sand_window(0, 0.0), (0, half));
    assert_eq!(sand_window(0, 0.36), (0, half));
    assert_eq!(sand_window(0, 0.45), (0, SAND_GRAIN_INSTANCES));
    assert_eq!(sand_window(0, 0.54), (half, half));
    assert_eq!(sand_window(0, 1.0), (half, half));
    for style in [8, 9] {
        for progress in [0.0f32, 0.3, 0.5, 0.7, 1.0] {
            assert_eq!(sand_window(style, progress), (0, half));
        }
    }
}

#[test]
fn style_table_index() {
    for (idx, name) in SAND_STYLES.iter().enumerate() {
        assert_eq!(sand_style_index(name), Some(SAND_STYLE_IDS[idx]));
    }
    assert_eq!(sand_style_index("sand-ribbons"), None);
    assert_eq!(sand_style_index("sand-geyser"), None);
    assert_eq!(sand_style_index("pixelate"), None);
    assert_eq!(sand_style_index("fade"), None);
}

#[test]
fn base_curve_reveal() {
    assert_eq!(sand_base_old(0.0), 1.0);
    assert_eq!(sand_base_new(0.0), 0.0);
    assert_eq!(sand_base_old(1.0), 0.0);
    assert_eq!(sand_base_new(1.0), 1.0);
    assert!(sand_base_old(0.24) > 0.99);
    assert!(sand_base_new(0.86) > 0.99);
    let mut dark = 0;
    for step in 0..=1000 {
        let progress = step as f32 / 1000.0;
        assert!(sand_base_old(progress) >= sand_base_old(progress + 0.001) - 1e-4);
        assert!(sand_base_new(progress) <= sand_base_new(progress + 0.001) + 1e-4);
        if sand_base_old(progress) < 0.15 && sand_base_new(progress) < 0.15 {
            dark += 1;
        }
    }
    assert!(dark <= 180, "dark window got {dark}");
}

#[test]
fn cyclone_timing_shared() {
    let vert = SAND_GRAIN_VERT_GL;
    for frag in [SAND_BASE_FRAG_GL, SAND_BASE_FRAG_VK] {
        for token in ["(rim * 0.80 + h * 0.20) * 0.60", "/ 0.40", "hp(id, 1u)", "pow(nd, 1.5)"] {
            assert!(vert.contains(token) && frag.contains(token), "missing {token}");
        }
    }
    for frag in [SAND_BASE_FRAG_GL, SAND_BASE_FRAG_VK] {
        assert!(frag.contains("discard"));
        assert!(frag.contains("local <= 0.0") && frag.contains("local >= 1.0"));
    }
    assert!(vert.contains("raw8 <= 0.0 || raw8 >= 1.0"));
}

#[test]
fn snapshot_sand_shader_source() {
    insta::assert_snapshot!("sand_grain_vert_gl330", sand_grain_vert(Dialect::Gl330));
    insta::assert_snapshot!("sand_grain_vert_vulkan450", sand_grain_vert(Dialect::Vulkan450));
    insta::assert_snapshot!("sand_grain_frag_gl330", sand_grain_frag(Dialect::Gl330));
    insta::assert_snapshot!("sand_grain_frag_vulkan450", sand_grain_frag(Dialect::Vulkan450));
    insta::assert_snapshot!("sand_base_frag_gl330", sand_base_frag(Dialect::Gl330));
    insta::assert_snapshot!("sand_base_frag_vulkan450", sand_base_frag(Dialect::Vulkan450));
}

fn helix_const(name: &str) -> f32 {
    let src = sand_grain_vert(Dialect::Gl330);
    let needle = format!("const float {name} = ");
    let line = src
        .lines()
        .find(|line| line.contains(&needle))
        .unwrap_or_else(|| panic!("missing const {name}"));
    line.split('=').nth(1).unwrap().trim().trim_end_matches(';').parse().unwrap()
}

#[test]
fn helix_gathers_before_scatter() {
    let hold_start = helix_const("HELIX_LIFT_SPREAD") + helix_const("HELIX_FLY");
    let hold_end = helix_const("HELIX_HOLD_END");
    assert!(hold_start < hold_end, "{hold_start} vs {hold_end}");
    assert!(hold_end + helix_const("HELIX_LAND_SPREAD") + helix_const("HELIX_FLY") < 1.0);
}

fn coil_s(
    progress: f32,
    tail: f32,
    smear_sign: f32,
    gathered: f32,
    scattered: f32,
    strand: f32,
) -> f32 {
    let flow = (gathered - 1.0 + scattered) * helix_const("HELIX_INTAKE");
    progress * helix_const("HELIX_HEAD_RATE")
        - helix_const("HELIX_HEAD_LAG")
        - tail * helix_const("HELIX_TRAIL")
        + smear_sign * helix_const("HELIX_SMEAR") * 0.5
        + flow
        + strand * helix_const("HELIX_STRAND_LAG")
}

const PERSP_MIN: f32 = 0.84;

fn coil_screen_y(
    progress: f32,
    tail: f32,
    smear_sign: f32,
    gathered: f32,
    scattered: f32,
    strand: f32,
) -> f32 {
    let s = coil_s(progress, tail, smear_sign, gathered, scattered, strand);
    (s - 0.5) * helix_const("HELIX_COIL_SPAN") * PERSP_MIN
}

#[test]
fn grains_cross_screen_centre() {
    let band = helix_const("HELIX_FADE_BAND");

    let latest_lift = helix_const("HELIX_LIFT_SPREAD");
    let leaves_at = coil_screen_y(latest_lift, 0.0, 1.0, 0.0, 0.0, 1.0);
    assert!(leaves_at <= -band, "lift y={leaves_at} band={band}");

    let earliest_land = helix_const("HELIX_HOLD_END") + helix_const("HELIX_FLY");
    let lands_at = coil_screen_y(earliest_land, 1.0, -1.0, 1.0, 1.0, 0.0);
    assert!(lands_at >= band, "land y={lands_at} band={band}");
}

#[test]
fn strands_cross_apart() {
    let band = helix_const("HELIX_FADE_BAND");
    let cross = |strand: f32| {
        (0..2000)
            .map(|step| step as f32 / 2000.0)
            .find(|progress| coil_screen_y(*progress, 0.0, 0.0, 1.0, 0.0, strand) >= band)
            .expect("no colour crossing")
    };
    let (lead, trail) = (cross(1.0), cross(0.0));
    let gap = trail - lead;
    assert!(gap > 0.10, "strand gap {gap}");
    let bright_spread = helix_const("HELIX_TRAIL") * 0.38 * helix_const("HELIX_COIL_SPAN");
    assert!(band * 2.0 < bright_spread * 0.75, "band {band} spread {bright_spread}");
}

#[test]
fn helix_base_matches_grain() {
    let base = sand_base_frag(Dialect::Gl330);
    let branch = base.split("u_style == 11").nth(1).expect("no helix branch");
    let branch = &branch[..branch.find("} else").unwrap_or(branch.len())];
    for (name, value) in [
        ("HELIX_LIFT_SPREAD", helix_const("HELIX_LIFT_SPREAD")),
        ("HELIX_HOLD_END", helix_const("HELIX_HOLD_END")),
        ("HELIX_LAND_SPREAD", helix_const("HELIX_LAND_SPREAD")),
        ("HELIX_FLY", helix_const("HELIX_FLY")),
    ] {
        assert!(branch.contains(&format!("{value:.2}")), "missing {name} ({value})");
    }
}

#[test]
fn point_variants_generated() {
    let vert = sand_grain_point_vert(Dialect::Gl330);
    assert!(!vert.contains("gl_VertexID / 6"));
    assert!(vert.contains("vec2 off = vec2(0.0);"));
    assert_eq!(vert.matches("gl_PointSize").count(), 3);
    assert_eq!(vert.matches("u_point_scale").count(), 4);
    let frag = sand_grain_point_frag(Dialect::Gl330);
    assert!(frag.contains("gl_PointCoord"));
    assert!(!frag.contains("vec2 uv = clamp(v_uv, vec2(0.0), vec2(1.0));"));
}

#[test]
fn point_variants_generated_vk() {
    let vert = sand_grain_point_vert(Dialect::Vulkan450);
    assert!(!vert.contains("gl_VertexID"));
    assert!(!vert.contains("gl_VertexIndex / 6"));
    assert!(vert.contains("int pid = gl_VertexIndex + u_pid_base;"));
    assert!(!vert.contains("u_point_scale"));
    assert_eq!(vert.matches("gl_PointSize").count(), 4);
    assert!(vert.contains("vec2 off = vec2(0.0);\n    gl_PointSize = 1.0;"));
    let frag = sand_grain_point_frag(Dialect::Vulkan450);
    assert!(frag.contains("gl_PointCoord"));
    assert!(frag.contains("nv12("));
    assert!(!frag.contains("vec2 uv = clamp(v_uv, vec2(0.0), vec2(1.0));"));
}

#[test]
fn vulkan_grain_viewport_y() {
    let gl = sand_grain_vert(Dialect::Gl330);
    assert_eq!(gl.matches("1.0 - world").count(), 3);

    let vk = sand_grain_vert(Dialect::Vulkan450);
    assert!(!vk.contains("1.0 - world"));
    for position in ["world8.y", "worldN.y", "world.y"] {
        assert!(vk.contains(&format!("{position} / u_res.y * 2.0 - 1.0")), "{position}");
    }
}

#[test]
fn vk_shaders_carry_fill_guard() {
    let fx = effect_frag_vk(EFFECTS[0].1);
    assert!(fx.contains("int u_fill;"));
    assert!(fx.contains("fill_oob") && fx.contains("fill_wrap"));
    assert!(fx.contains("int u_rgba_a;") && fx.contains("int u_rgba_b;"));
    assert!(fx.contains("if (u_rgba_a == 1)") && fx.contains("if (u_rgba_b == 1)"));
    assert!(sand_grain_point_vert(Dialect::Vulkan450).contains("int u_fill;"));
    for frag in [SAND_BASE_FRAG_VK, SAND_GRAIN_FRAG_VK] {
        assert!(frag.contains("int u_fill;"));
        assert!(frag.contains("u_fill == 1") && frag.contains("u_fill == 2"));
    }
    assert!(!SAND_BASE_FRAG_GL.contains("u_fill"));
    assert!(!SAND_GRAIN_FRAG_GL.contains("u_fill"));
}
