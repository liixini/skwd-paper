#version 330 core
in vec2 v_uv;
uniform float u_progress;
uniform int u_style;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
out vec4 frag;
const int GW = 1920;
const int GH = 1080;
uint uh(uint x) {
    x ^= x >> 16u;
    x *= 0x7feb352du;
    x ^= x >> 15u;
    x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}
float hp(uint id, uint salt) {
    return float(uh(id * 0x9e3779b9u + salt * 0x85ebca6bu)) / 4294967296.0;
}
void main() {
    vec2 uvp = clamp(v_uv, vec2(0.0), vec2(1.0));
    vec2 dc = uvp - 0.5;
    int gx = int(min(uvp.x * float(GW), float(GW) - 1.0));
    int gy = int(min(uvp.y * float(GH), float(GH) - 1.0));
    uint id = uint(gy * GW + gx);
    float h = hp(id, 1u);
    float nd = mix(
        clamp(max(abs(dc.x), abs(dc.y)) * 2.0, 0.0, 1.0),
        clamp(length(dc) * 1.4142136, 0.0, 1.0),
        0.25
    );
    float rim = pow(nd, 1.5);
    float local;
    if (u_style == 7) {
        float lift = (uvp.x * 0.65 + h * 0.2) * 0.2895;
        float settle = 0.495 + 0.175 * uvp.x + 0.054 * h;
        local = (u_progress - lift) / max(settle - lift, 0.02);
    } else if (u_style == 11) {
        float lift = mix(rim, h, 0.35) * 0.16;
        float land = 0.62 + hp(id, 2u) * 0.16;
        local = (u_progress - lift) / max(land + 0.18 - lift, 0.02);
    } else {
        float start = (rim * 0.80 + h * 0.20) * 0.60;
        local = (u_progress - start) / 0.40;
    }
    if (local <= 0.0) {
        frag = vec4(texture(u_tex_old, uvp).rgb, 1.0);
    } else if (local >= 1.0) {
        frag = vec4(texture(u_tex_new, uvp).rgb, 1.0);
    } else {
        discard;
    }
}
