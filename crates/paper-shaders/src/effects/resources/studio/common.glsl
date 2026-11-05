#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
uniform vec2 u_res;

const float STUDIO_PI = 3.141592653589793;
const float STUDIO_FIRST = 0.015;
const float STUDIO_LAST = 0.985;
const float STUDIO_SETTLE_SPLIT = 0.72;
const float STUDIO_SETTLE_PEAK = 1.025;
const vec3 STUDIO_IVORY = vec3(0.94509804, 0.92156863, 0.86274510);
const vec3 STUDIO_INK = vec3(0.12549020, 0.12549020, 0.11764706);
const vec3 STUDIO_GRAY_WEIGHTS = vec3(0.2126, 0.7152, 0.0722);

float studio_span(float t, vec2 interval) {
    return clamp((t - interval.x) / (interval.y - interval.x), 0.0, 1.0);
}

float studio_ease(float t) {
    float p = clamp(t, 0.0, 1.0);
    return p < 0.5 ? 4.0 * p * p * p : 1.0 - pow(-2.0 * p + 2.0, 3.0) * 0.5;
}

float studio_out(float t) {
    return 1.0 - pow(1.0 - clamp(t, 0.0, 1.0), 3.0);
}

float studio_settle(float t) {
    float p = clamp(t, 0.0, 1.0);
    return p < STUDIO_SETTLE_SPLIT
        ? studio_out(p / STUDIO_SETTLE_SPLIT) * STUDIO_SETTLE_PEAK
        : mix(STUDIO_SETTLE_PEAK, 1.0, studio_ease((p - STUDIO_SETTLE_SPLIT) / (1.0 - STUDIO_SETTLE_SPLIT)));
}

float studio_pulse(float t, vec3 interval) {
    return t < interval.y ? studio_out(studio_span(t, interval.xy)) : 1.0 - studio_ease(studio_span(t, interval.yz));
}

vec2 studio_turn(vec2 p, float angle) {
    float co = cos(angle);
    float si = sin(angle);
    return vec2(co * p.x - si * p.y, si * p.x + co * p.y);
}

vec2 studio_camera(vec2 uv, float scale, vec2 offset) {
    return (uv - vec2(0.5) - offset) / scale + vec2(0.5);
}

vec4 studio_old(vec2 uv, float scale, vec2 offset) {
    return texture(u_tex_old, studio_camera(uv, scale, offset));
}

vec4 studio_new(vec2 uv, float scale, vec2 offset) {
    return texture(u_tex_new, studio_camera(uv, scale, offset));
}

vec3 studio_mono(vec3 color, float contrast) {
    float gray = dot(color, STUDIO_GRAY_WEIGHTS);
    return clamp(vec3((gray - 0.5) * contrast + 0.5), 0.0, 1.0);
}

vec4 studio_over(vec4 base, vec4 top, float alpha) {
    return mix(base, top, clamp(alpha, 0.0, 1.0));
}
