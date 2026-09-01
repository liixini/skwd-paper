#version 450
layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 frag;
layout(push_constant) uniform PC { vec2 uv_scale; vec2 uv_off; int fill; } pc;
layout(binding = 0) uniform sampler2D src;
void main() {
    vec2 uv = v_uv;
    if (pc.fill == 1 && (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0))))) {
        frag = vec4(0.0, 0.0, 0.0, 1.0);
        return;
    }
    if (pc.fill == 2) uv = fract(uv);
    frag = vec4(texture(src, uv).rgb, 1.0);
}
