#version 450
layout(push_constant) uniform PC { vec2 u_size; float u_t; float u_dry; int u_pass; };
layout(location = 0) in vec2 v_pos;
layout(location = 1) flat in vec4 v_p;
layout(location = 2) flat in vec4 v_t;
layout(location = 3) flat in vec3 v_w;
layout(location = 0) out vec4 frag;
const float PI = 3.14159265;
const float LENGTH_FLOOR = 1e-6;
const float PEN = 0.45;
const float HATCH_PEN = 0.6;
const float HATCH_FLAG = 0.5;
const float TAPER_FLOOR = 0.25;
const float TAPER_CURVE = 0.4;
const float PRESSURE_BASE = 0.9;
const float PRESSURE_SWING = 0.1;
const float PRESSURE_WAVES = 19.0;
const float PRESSURE_SEED = 731.0;
const float FINISH_DRY = 0.93;
const int COVERAGE_PASS = 0;
const float ATTRIBUTE_COVERAGE = 0.35;
const float HUE_SEED = 7.31;
const float HUE_WIDTH = 0.013;
void main() {
    vec2 p0 = v_p.xy;
    vec2 d = v_p.zw - p0;
    float f = clamp(dot(v_pos - p0, d) / max(dot(d, d), LENGTH_FLOOR), 0.0, 1.0);
    float dist = length(v_pos - (p0 + f * d));
    float arrival = mix(v_t.x, v_t.y, f);
    if (arrival > u_t) discard;
    float dry = max(clamp((u_t - arrival) / u_dry, 0.0, 1.0), smoothstep(FINISH_DRY, 1.0, u_t));
    float s = clamp(mix(v_t.z, v_t.w, f), 0.0, 1.0);
    bool hatch = v_w.y > HATCH_FLAG;
    float taper = hatch ? 1.0 : mix(TAPER_FLOOR, 1.0, pow(sin(PI * s), TAPER_CURVE));
    float pressure = hatch ? 1.0 : PRESSURE_BASE + PRESSURE_SWING * sin(s * PRESSURE_WAVES + v_t.x * PRESSURE_SEED);
    float fresh = v_w.x * (hatch ? HATCH_PEN : PEN) * taper * pressure;
    float w = mix(fresh, floor(v_w.x + 0.5), smoothstep(0.0, 1.0, dry));
    float cov = clamp(w * 0.5 + 0.5 - dist, 0.0, 1.0);
    if (cov <= 0.0) discard;
    if (u_pass == COVERAGE_PASS) {
        frag = vec4(cov, 0.0, 0.0, cov * (1.0 - dry));
        return;
    }
    if (cov < ATTRIBUTE_COVERAGE) discard;
    frag = vec4(0.0, fract(v_w.z * HUE_SEED + v_w.x * HUE_WIDTH), s, 0.0);
}
