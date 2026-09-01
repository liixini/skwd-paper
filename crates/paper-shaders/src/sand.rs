pub const SAND_GRAIN_GW: i32 = 1920;
pub const SAND_GRAIN_GH: i32 = 1080;
pub const SAND_GRAIN_INSTANCES: i32 = SAND_GRAIN_GW * SAND_GRAIN_GH * 2;
pub const SAND_A_ONLY_BELOW: f32 = 0.37;
pub const SAND_B_ONLY_FROM: f32 = 0.54;

pub const SAND_STYLES: [&str; 9] = [
    "sand-bloom",
    "sand-maelstrom",
    "sand-murmuration",
    "sand-donut",
    "sand-globe",
    "sand-helix",
    "sand-galaxy",
    "sand-mobius",
    "sand-tornado",
];

pub const SAND_STYLE_IDS: [i32; 9] = [0, 4, 7, 8, 10, 11, 12, 13, 14];

pub fn sand_style_index(name: &str) -> Option<i32> {
    SAND_STYLES.iter().position(|style| *style == name).map(|idx| SAND_STYLE_IDS[idx])
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn sand_base_old(progress: f32) -> f32 {
    1.0 - smooth(0.26, 0.52, progress)
}

pub fn sand_base_new(progress: f32) -> f32 {
    smooth(0.52, 0.80, progress)
}

pub fn sand_window(style: i32, progress: f32) -> (i32, i32) {
    let half = SAND_GRAIN_INSTANCES / 2;
    if style >= 8 || progress < SAND_A_ONLY_BELOW {
        (0, half)
    } else if progress >= SAND_B_ONLY_FROM {
        (half, half)
    } else {
        (0, SAND_GRAIN_INSTANCES)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Gl330,
    Vulkan450,
}

pub const SAND_GRAIN_VERT_GL: &str = include_str!("../glsl/sand_grain.vert");
pub const SAND_BASE_FRAG_GL: &str = include_str!("../glsl/sand_base_gl.frag");
pub const SAND_BASE_FRAG_VK: &str = include_str!("../glsl/sand_base_vk.frag");
pub const SAND_GRAIN_FRAG_GL: &str = include_str!("../glsl/sand_grain_gl.frag");
pub const SAND_GRAIN_FRAG_VK: &str = include_str!("../glsl/sand_grain_vk.frag");

const GL_VERT_PROLOG: &str = "#version 330 core
uniform float u_progress;
uniform vec2 u_res;
uniform int u_style;
uniform int u_pid_base;
out vec2 v_uv;
out float v_alpha;
out float v_bright;
out float v_mix;
";

const VK_VERT_PROLOG: &str = "#version 450
layout(push_constant) uniform PC {
    vec2 u_res;
    float u_progress;
    int u_style;
    int u_pid_base;
    int u_fill;
    vec4 u_uv_a;
    vec4 u_uv_b;
};
layout(location = 0) out vec2 v_uv;
layout(location = 1) out float v_alpha;
layout(location = 2) out float v_bright;
layout(location = 3) out float v_mix;
";

pub fn sand_grain_vert(dialect: Dialect) -> String {
    match dialect {
        Dialect::Gl330 => SAND_GRAIN_VERT_GL.to_string(),
        Dialect::Vulkan450 => {
            let body = SAND_GRAIN_VERT_GL
                .strip_prefix(GL_VERT_PROLOG)
                .expect("sand_grain.vert prolog drifted from the composer");
            let body = body
                .replace("gl_VertexID", "gl_VertexIndex")
                // GL NDC Y points up; a positive-height Vulkan viewport points it down.
                .replace("1.0 - world8.y / u_res.y * 2.0", "world8.y / u_res.y * 2.0 - 1.0")
                .replace("1.0 - worldN.y / u_res.y * 2.0", "worldN.y / u_res.y * 2.0 - 1.0")
                .replace("1.0 - world.y / u_res.y * 2.0", "world.y / u_res.y * 2.0 - 1.0");
            format!("{VK_VERT_PROLOG}{body}")
        }
    }
}

pub fn sand_base_frag(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::Gl330 => SAND_BASE_FRAG_GL,
        Dialect::Vulkan450 => SAND_BASE_FRAG_VK,
    }
}

pub fn sand_grain_frag(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::Gl330 => SAND_GRAIN_FRAG_GL,
        Dialect::Vulkan450 => SAND_GRAIN_FRAG_VK,
    }
}

pub fn sand_grain_point_vert(dialect: Dialect) -> String {
    match dialect {
        Dialect::Gl330 => SAND_GRAIN_VERT_GL
            .replace(
                "uniform int u_pid_base;",
                "uniform int u_pid_base;\nuniform float u_point_scale;",
            )
            .replace("int pid = gl_VertexID / 6 + u_pid_base;", "int pid = gl_VertexID + u_pid_base;")
            .replace("vec2 off = offs[corner];", "vec2 off = vec2(0.0);")
            .replace(
                "vec2 size8 = u_res / vec2(float(GW), float(GH)) * 1.15;",
                "vec2 size8 = u_res / vec2(float(GW), float(GH)) * 1.15;\n        gl_PointSize = max(size8.x, size8.y) * u_point_scale;",
            )
            .replace(
                "vec2 sizeN = u_res / vec2(float(GW), float(GH)) * 1.15 * mix(1.0, psize, form);",
                "vec2 sizeN = u_res / vec2(float(GW), float(GH)) * 1.15 * mix(1.0, psize, form);\n        gl_PointSize = max(sizeN.x, sizeN.y) * u_point_scale;",
            )
            .replace(
                "vec2 size = u_res / vec2(float(GW), float(GH)) * 1.15;",
                "vec2 size = u_res / vec2(float(GW), float(GH)) * 1.15;\n    gl_PointSize = max(size.x, size.y) * u_point_scale;",
            ),
        Dialect::Vulkan450 => sand_grain_vert(Dialect::Vulkan450)
            .replace(
                "int pid = gl_VertexIndex / 6 + u_pid_base;",
                "int pid = gl_VertexIndex + u_pid_base;",
            )
            .replace("vec2 off = offs[corner];", "vec2 off = vec2(0.0);\n    gl_PointSize = 1.0;")
            .replace(
                "vec2 size8 = u_res / vec2(float(GW), float(GH)) * 1.15;",
                "vec2 size8 = u_res / vec2(float(GW), float(GH)) * 1.15;\n        gl_PointSize = max(size8.x, size8.y);",
            )
            .replace(
                "vec2 sizeN = u_res / vec2(float(GW), float(GH)) * 1.15 * mix(1.0, psize, form);",
                "vec2 sizeN = u_res / vec2(float(GW), float(GH)) * 1.15 * mix(1.0, psize, form);\n        gl_PointSize = max(sizeN.x, sizeN.y);",
            )
            .replace(
                "vec2 size = u_res / vec2(float(GW), float(GH)) * 1.15;",
                "vec2 size = u_res / vec2(float(GW), float(GH)) * 1.15;\n    gl_PointSize = max(size.x, size.y);",
            ),
    }
}

pub fn sand_grain_point_frag(dialect: Dialect) -> String {
    sand_grain_frag(dialect).replace(
        "vec2 uv = clamp(v_uv, vec2(0.0), vec2(1.0));",
        "vec2 uv = clamp(v_uv + (gl_PointCoord - 0.5) * vec2(1.15 / 1920.0, 1.15 / 1080.0), vec2(0.0), vec2(1.0));",
    )
}
