pub(in crate::effects) const SMOKE_FRAG: &str = "#version 330 core
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
    for (int i = 0; i < 6; i++) {
        v += amp * noise(p);
        p *= 2.0;
        amp *= 0.5;
    }
    return v;
}
vec2 warp_r;
float warpedFbm(vec2 p, float t) {
    vec2 q = vec2(fbm(p), fbm(p + vec2(5.2, 1.3)));
    warp_r = vec2(fbm(p + 6.0 * q + vec2(1.7, 9.2) + 0.25 * t),
                  fbm(p + 6.0 * q + vec2(8.3, 2.8) + 0.22 * t));
    return fbm(p + 5.0 * warp_r);
}
void main() {
    float p = u_progress;
    vec2 uv = v_uv;
    float t = p * 12.0;
    float fluid = warpedFbm(uv * 2.0, t);
    vec2 center = uv - 0.5;
    float dist = length(center * vec2(1.0, 0.7));
    float visibility = (1.0 - dist) * 1.2 + fluid * 0.7;
    float reveal_progress = p * 2.5 - 0.4;
    float reveal_mask = smoothstep(visibility - 0.4, visibility + 0.4, reveal_progress);
    float distort_strength = sin(p * 3.14159) * 0.35;
    vec2 warped_uv = uv + (warp_r - 0.5) * distort_strength;
    vec4 a = texture(u_tex_old, warped_uv);
    vec4 b = texture(u_tex_new, warped_uv);
    frag = mix(a, b, reveal_mask);
}
";

pub(in crate::effects) const CHROMATIC_BLOOM_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
void main() {
    float p = u_progress;
    float intensity = sin(p * 3.14159);
    vec2 c = v_uv - vec2(0.5);
    vec2 dir = c * intensity * 0.15;
    vec3 oldc = vec3(
        texture(u_tex_old, v_uv + dir).r,
        texture(u_tex_old, v_uv).g,
        texture(u_tex_old, v_uv - dir).b
    );
    vec3 newc = vec3(
        texture(u_tex_new, v_uv + dir).r,
        texture(u_tex_new, v_uv).g,
        texture(u_tex_new, v_uv - dir).b
    );
    vec3 mixed = mix(oldc, newc, smoothstep(0.4, 0.6, p));
    vec3 bloom = vec3(0.0);
    for (int i = 1; i <= 4; i++) {
        float r = float(i) * 0.01 * intensity;
        bloom += texture(u_tex_new, v_uv + vec2(r, 0.0)).rgb;
        bloom += texture(u_tex_new, v_uv - vec2(r, 0.0)).rgb;
        bloom += texture(u_tex_new, v_uv + vec2(0.0, r)).rgb;
        bloom += texture(u_tex_new, v_uv - vec2(0.0, r)).rgb;
    }
    bloom /= 16.0;
    mixed = mix(mixed, mixed + bloom * 0.5, intensity);
    frag = vec4(mixed, 1.0);
}
";

pub(in crate::effects) const INKWELL_DROP_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
void main() {
    float p = u_progress;
    vec2 impact = vec2(0.35, 0.4);
    vec2 c = v_uv - impact;
    c.x *= 1.7777;
    float d = length(c);
    float front = p * 1.5;
    float ring1 = sin((d - front) * 80.0) * exp(-abs(d - front) * 6.0);
    float ring2 = sin((d - front + 0.08) * 80.0) * exp(-abs(d - front + 0.08) * 8.0) * 0.6;
    float ring3 = sin((d - front + 0.16) * 80.0) * exp(-abs(d - front + 0.16) * 10.0) * 0.4;
    float ripple = (ring1 + ring2 + ring3) * 0.05 * (1.0 - p);
    vec2 dir = (d > 0.001) ? normalize(c) : vec2(0.0);
    vec2 distorted = v_uv + dir * ripple;
    vec4 a = texture(u_tex_old, distorted);
    vec4 b = texture(u_tex_new, distorted);
    float reveal = smoothstep(0.05, -0.02, d - front);
    vec4 mixed = mix(a, b, reveal);
    float crest = exp(-abs(d - front) * 25.0) * (1.0 - p);
    mixed.rgb += vec3(0.6, 0.75, 0.95) * crest * 0.5;
    frag = mixed;
}
";

pub(in crate::effects) const PIXELFADE_WAVE_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
void main() {
    float p = u_progress;
    float wave_x = (v_uv.x + v_uv.y) * 0.5;
    float wave_p = smoothstep(0.0, 1.0, p * 1.6 - wave_x * 0.6);
    float bump = sin(wave_p * 3.14159);
    float blocks = mix(800.0, 8.0, bump);
    vec2 q = floor(v_uv * blocks) / blocks + 0.5 / blocks;
    vec4 a = texture(u_tex_old, q);
    vec4 b = texture(u_tex_new, q);
    frag = mix(a, b, smoothstep(0.0, 1.0, wave_p));
}
";

pub(in crate::effects) const SOFT_WARP_FADE_FRAG: &str = "#version 330 core
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
    float strength = sin(p * 3.14159) * 0.025;
    vec2 warp = vec2(
        noise(v_uv * 3.0 + vec2(0.0, p * 0.5)),
        noise(v_uv * 3.0 + vec2(p * 0.5, 0.0))
    ) - 0.5;
    vec2 uv_warped = v_uv + warp * strength;
    vec4 a = texture(u_tex_old, uv_warped);
    vec4 b = texture(u_tex_new, uv_warped);
    float t = smoothstep(0.05, 0.95, p);
    t = t * t * (3.0 - 2.0 * t);
    frag = mix(a, b, t);
}
";

pub(in crate::effects) const ZOOM_BLUR_PULL_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const float PI = 3.14159265358979;
const float strength_v = 0.4;
float Linear_ease(float begin, float change, float duration, float time) {
    return change * time / duration + begin;
}
float Exponential_easeInOut(float begin, float change, float duration, float time) {
    if (time == 0.0) return begin;
    if (time == duration) return begin + change;
    time = time / (duration / 2.0);
    if (time < 1.0) return change / 2.0 * pow(2.0, 10.0 * (time - 1.0)) + begin;
    return change / 2.0 * (-pow(2.0, -10.0 * (time - 1.0)) + 2.0) + begin;
}
float Sinusoidal_easeInOut(float begin, float change, float duration, float time) {
    return -change / 2.0 * (cos(PI * time / duration) - 1.0) + begin;
}
float rand(vec2 co) {
    return fract(sin(dot(co.xy, vec2(12.9898, 78.233))) * 43758.5453);
}
vec4 crossFade(vec2 uv, float dissolve) {
    return mix(texture(u_tex_old, uv), texture(u_tex_new, uv), dissolve);
}
void main() {
    vec2 texCoord = v_uv;
    vec2 center = vec2(Linear_ease(0.25, 0.5, 1.0, u_progress), 0.5);
    float dissolve = Exponential_easeInOut(0.0, 1.0, 1.0, u_progress);
    float strength = Sinusoidal_easeInOut(0.0, strength_v, 0.5, u_progress);
    if (strength < 0.005) {
        frag = crossFade(texCoord, dissolve);
        return;
    }
    vec4 color = vec4(0.0);
    float total = 0.0;
    vec2 toCenter = center - texCoord;
    float offset = rand(v_uv);
    for (float t = 0.0; t <= 12.0; t++) {
        float percent = (t + offset) / 12.0;
        float weight = 4.0 * (percent - percent * percent);
        color += crossFade(texCoord + toCenter * percent * strength, dissolve) * weight;
        total += weight;
    }
    frag = color / total;
}
";

pub(in crate::effects) const MOSAIC_TUMBLE_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform sampler2D u_tex_thumb_0;
uniform sampler2D u_tex_thumb_1;
uniform sampler2D u_tex_thumb_2;
uniform sampler2D u_tex_thumb_3;
uniform sampler2D u_tex_thumb_4;
uniform sampler2D u_tex_thumb_5;
uniform sampler2D u_tex_thumb_6;
uniform sampler2D u_tex_thumb_7;
uniform sampler2D u_tex_thumb_8;
uniform sampler2D u_tex_thumb_9;
uniform sampler2D u_tex_thumb_10;
uniform sampler2D u_tex_thumb_11;
uniform sampler2D u_tex_thumb_12;
uniform sampler2D u_tex_thumb_13;
uniform sampler2D u_tex_thumb_14;
uniform sampler2D u_tex_thumb_15;
uniform sampler2D u_tex_thumb_16;
uniform sampler2D u_tex_thumb_17;
uniform sampler2D u_tex_thumb_18;
uniform sampler2D u_tex_thumb_19;
uniform int u_thumb_count;
uniform float u_progress;
const float PI = 3.14159265358979323;
const int endx = 2;
const int endy = -1;
float Rand(vec2 v) { return fract(sin(dot(v.xy, vec2(12.9898, 78.233))) * 43758.5453); }
vec2 Rotate(vec2 v, float a) {
    mat2 rm = mat2(cos(a), -sin(a), sin(a), cos(a));
    return rm * v;
}
float CosInterpolation(float x) { return -cos(x * PI) / 2.0 + 0.5; }
vec4 sample_thumb(int idx, vec2 uv) {
    if (idx ==  0) return texture(u_tex_thumb_0,  uv);
    if (idx ==  1) return texture(u_tex_thumb_1,  uv);
    if (idx ==  2) return texture(u_tex_thumb_2,  uv);
    if (idx ==  3) return texture(u_tex_thumb_3,  uv);
    if (idx ==  4) return texture(u_tex_thumb_4,  uv);
    if (idx ==  5) return texture(u_tex_thumb_5,  uv);
    if (idx ==  6) return texture(u_tex_thumb_6,  uv);
    if (idx ==  7) return texture(u_tex_thumb_7,  uv);
    if (idx ==  8) return texture(u_tex_thumb_8,  uv);
    if (idx ==  9) return texture(u_tex_thumb_9,  uv);
    if (idx == 10) return texture(u_tex_thumb_10, uv);
    if (idx == 11) return texture(u_tex_thumb_11, uv);
    if (idx == 12) return texture(u_tex_thumb_12, uv);
    if (idx == 13) return texture(u_tex_thumb_13, uv);
    if (idx == 14) return texture(u_tex_thumb_14, uv);
    if (idx == 15) return texture(u_tex_thumb_15, uv);
    if (idx == 16) return texture(u_tex_thumb_16, uv);
    if (idx == 17) return texture(u_tex_thumb_17, uv);
    if (idx == 18) return texture(u_tex_thumb_18, uv);
    return texture(u_tex_thumb_19, uv);
}
void main() {
    vec2 p = v_uv - 0.5;
    vec2 rp = p;
    float rpr = (u_progress * 2.0 - 1.0);
    float z = -(rpr * rpr * 2.0) + 3.0;
    float az = abs(z);
    rp *= az;
    rp += mix(vec2(0.5, 0.5), vec2(float(endx) + 0.5, float(endy) + 0.5),
              CosInterpolation(u_progress) * CosInterpolation(u_progress));
    vec2 mrp = mod(rp, 1.0);
    vec2 crp = rp;
    int cx = int(floor(crp.x));
    int cy = int(floor(crp.y));
    bool onEnd = cx == endx && cy == endy;
    bool onStart = cx == 0 && cy == 0;
    if (onEnd) {
        frag = texture(u_tex_new, mrp);
    } else if (onStart) {
        frag = texture(u_tex_old, mrp);
    } else if (u_thumb_count > 0) {
        int idx = int(Rand(floor(crp) + vec2(7.3, 1.1)) * float(u_thumb_count));
        if (idx >= u_thumb_count) idx = u_thumb_count - 1;
        frag = sample_thumb(idx, mrp);
    } else {
        float ang = float(int(Rand(floor(crp)) * 4.0)) * 0.5 * PI;
        vec2 rotated = vec2(0.5) + Rotate(mrp - vec2(0.5), ang);
        if (Rand(floor(crp)) > 0.5) {
            frag = texture(u_tex_new, rotated);
        } else {
            frag = texture(u_tex_old, rotated);
        }
    }
}
";
