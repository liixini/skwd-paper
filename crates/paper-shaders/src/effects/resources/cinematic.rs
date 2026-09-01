pub(in crate::effects) const CROSSFADE_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
void main() {
    vec4 a = texture(u_tex_old, v_uv);
    vec4 b = texture(u_tex_new, v_uv);
    frag = mix(a, b, smoothstep(0.0, 1.0, u_progress));
}
";

pub(in crate::effects) const PIXELATE_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
void main() {
    float bump = 1.0 - abs(u_progress - 0.5) * 2.0;
    float blocks = mix(800.0, 12.0, bump);
    vec2 q = floor(v_uv * blocks) / blocks + 0.5 / blocks;
    vec4 a = texture(u_tex_old, q);
    vec4 b = texture(u_tex_new, q);
    frag = mix(a, b, smoothstep(0.0, 1.0, u_progress));
}
";

pub(in crate::effects) const IRIS_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
void main() {
    vec2 c = v_uv - vec2(0.5);
    c.x *= 1.7777;
    float d = length(c);
    float r = u_progress * 1.2;
    float feather = 0.05;
    float t = smoothstep(r + feather, r - feather, d);
    float edge = exp(-abs(d - r) * 100.0);
    vec3 chrom = vec3(
        texture(u_tex_new, v_uv + vec2(0.012, 0.0) * edge).r,
        texture(u_tex_new, v_uv).g,
        texture(u_tex_new, v_uv - vec2(0.012, 0.0) * edge).b
    );
    vec4 a = texture(u_tex_old, v_uv);
    frag = mix(a, vec4(chrom, 1.0), t);
}
";

pub(in crate::effects) const GLITCH_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
void main() {
    vec2 p = v_uv;
    vec2 block = floor(p.xy / vec2(16.0));
    vec2 uv_noise = block / vec2(64.0);
    uv_noise += floor(vec2(u_progress) * vec2(1200.0, 3500.0)) / vec2(64.0);
    vec2 dist = u_progress > 0.0 ? (fract(uv_noise) - 0.5) * 0.3 * (1.0 - u_progress) : vec2(0.0);
    vec2 red = p + dist * 0.2;
    vec2 green = p + dist * 0.3;
    vec2 blue = p + dist * 0.5;
    frag = vec4(
        mix(texture(u_tex_old, red), texture(u_tex_new, red), u_progress).r,
        mix(texture(u_tex_old, green), texture(u_tex_new, green), u_progress).g,
        mix(texture(u_tex_old, blue), texture(u_tex_new, blue), u_progress).b,
        1.0
    );
}
";

pub(in crate::effects) const VORONOI_SHATTER_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
vec2 hash2(vec2 p) {
    return fract(sin(vec2(dot(p, vec2(127.1, 311.7)),
                          dot(p, vec2(269.5, 183.3)))) * 43758.5453);
}
void main() {
    float scale = 14.0;
    vec2 p = v_uv * scale;
    vec2 g = floor(p);
    vec2 f = fract(p);
    float min_d = 100.0;
    vec2 cell = g;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            vec2 nb = vec2(float(x), float(y));
            vec2 q = nb + hash2(g + nb) - f;
            float d = dot(q, q);
            if (d < min_d) { min_d = d; cell = g + nb; }
        }
    }
    vec2 dir = normalize(hash2(cell) - 0.5 + vec2(0.0001));
    float seed = hash2(cell).x;
    float shard_p = smoothstep(seed * 0.5, seed * 0.5 + 0.5, u_progress);
    vec2 displaced = v_uv - dir * shard_p * 1.5;
    vec4 a = texture(u_tex_old, displaced);
    vec4 b = texture(u_tex_new, v_uv);
    frag = mix(a, b, smoothstep(0.0, 0.5, shard_p));
}
";

pub(in crate::effects) const HEAT_MELT_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const bool direction_v = true;
const float l_threshold = 0.65;
const bool above_v = false;
float rand(vec2 co) { return fract(sin(dot(co.xy, vec2(12.9898, 78.233))) * 43758.5453); }
vec3 mod289v3(vec3 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec2 mod289v2(vec2 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec3 permute(vec3 x) { return mod289v3(((x * 34.0) + 1.0) * x); }
float snoise(vec2 v) {
    const vec4 C = vec4(0.211324865405187, 0.366025403784439, -0.577350269189626, 0.024390243902439);
    vec2 i = floor(v + dot(v, C.yy));
    vec2 x0 = v - i + dot(i, C.xx);
    vec2 i1 = (x0.x > x0.y) ? vec2(1.0, 0.0) : vec2(0.0, 1.0);
    vec4 x12 = x0.xyxy + C.xxzz;
    x12.xy -= i1;
    i = mod289v2(i);
    vec3 p = permute(permute(i.y + vec3(0.0, i1.y, 1.0)) + i.x + vec3(0.0, i1.x, 1.0));
    vec3 m = max(0.5 - vec3(dot(x0, x0), dot(x12.xy, x12.xy), dot(x12.zw, x12.zw)), 0.0);
    m = m * m;
    m = m * m;
    vec3 x = 2.0 * fract(p * C.www) - 1.0;
    vec3 h = abs(x) - 0.5;
    vec3 ox = floor(x + 0.5);
    vec3 a0 = x - ox;
    m *= 1.79284291400159 - 0.85373472095314 * (a0 * a0 + h * h);
    vec3 g;
    g.x = a0.x * x0.x + h.x * x0.y;
    g.yz = a0.yz * x12.xz + h.yz * x12.yw;
    return 130.0 * dot(m, g);
}
float luminance(vec4 color) { return color.r * 0.299 + color.g * 0.587 + color.b * 0.114; }
void main() {
    vec2 center = vec2(1.0, direction_v ? 1.0 : 0.0);
    vec2 p = v_uv;
    if (u_progress == 0.0) { frag = texture(u_tex_old, p); return; }
    if (u_progress == 1.0) { frag = texture(u_tex_new, p); return; }
    float x = u_progress;
    float dist = distance(center, p) - u_progress * exp(snoise(vec2(p.x, 0.0)));
    float r = x - rand(vec2(p.x, 0.1));
    float m;
    if (above_v) {
        m = (dist <= r && luminance(texture(u_tex_old, p)) > l_threshold) ? 1.0 : (u_progress * u_progress * u_progress);
    } else {
        m = (dist <= r && luminance(texture(u_tex_old, p)) < l_threshold) ? 1.0 : (u_progress * u_progress * u_progress);
    }
    frag = mix(texture(u_tex_old, p), texture(u_tex_new, p), m);
}
";

pub(in crate::effects) const PLASMA_FLOW_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
float hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}
float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x),
               mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
void main() {
    float p = u_progress;
    vec2 flow = vec2(
        noise(v_uv * 5.0 + vec2(p * 2.0, 0.0)),
        noise(v_uv * 5.0 + vec2(0.0, p * 2.0))
    ) - 0.5;
    float intensity = sin(p * 3.14159) * 0.18;
    vec2 distorted = v_uv + flow * intensity;
    vec4 a = texture(u_tex_old, distorted);
    vec4 b = texture(u_tex_new, distorted);
    frag = mix(a, b, smoothstep(0.2, 0.8, p));
}
";

pub(in crate::effects) const INK_SPLASH_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
float hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}
float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x),
               mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
float fbm(vec2 p) {
    float v = 0.0;
    float amp = 0.5;
    for (int i = 0; i < 5; i++) {
        v += amp * noise(p);
        p *= 2.1;
        amp *= 0.5;
    }
    return v;
}
void main() {
    float p = u_progress;
    float blob = fbm(v_uv * 3.5);
    float fingers = fbm(v_uv * 14.0);
    float distortion = (blob - 0.5) * 0.5 + (fingers - 0.5) * 0.18;
    vec2 c = v_uv - vec2(0.5);
    c.x *= 1.7777;
    float d = length(c);
    float splash_d = d + distortion;
    float boundary = p * 1.7 - 0.15;
    float diff = splash_d - boundary;
    float reveal = smoothstep(0.04, -0.04, diff);
    float edge_outer = smoothstep(0.16, 0.02, diff);
    float edge_inner = smoothstep(0.02, -0.04, diff);
    float edge = edge_outer * (1.0 - edge_inner);
    vec4 a = texture(u_tex_old, v_uv);
    vec4 b = texture(u_tex_new, v_uv);
    vec4 mixed = mix(a, b, reveal);
    vec3 ink = vec3(0.03, 0.01, 0.06);
    mixed.rgb = mix(mixed.rgb, ink, edge * 0.95);
    float fingers_pre = smoothstep(0.25, 0.05, diff) * (1.0 - reveal);
    mixed.rgb = mix(mixed.rgb, ink, fingers_pre * fingers * 0.4);
    frag = mixed;
}
";
