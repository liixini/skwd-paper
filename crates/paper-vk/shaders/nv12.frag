#version 450
layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 frag;
layout(push_constant) uniform PC { vec2 uv_scale; vec2 uv_off; int fill; } pc;
layout(binding = 0) uniform sampler2D luma;
layout(binding = 1) uniform sampler2D chroma;
void main() {
    vec2 uv = v_uv;
    if (pc.fill == 1 && (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0))))) {
        frag = vec4(0.0, 0.0, 0.0, 1.0);
        return;
    }
    if (pc.fill == 2) uv = fract(uv);
    float y = texture(luma, uv).r;
    vec2 c = texture(chroma, uv).rg;
    // BT.709 limited range
    float yf = (y - 16.0 / 255.0) * (255.0 / 219.0);
    float u = c.x - 0.5;
    float v = c.y - 0.5;
    vec3 rgb = vec3(
        yf + 1.5748 * v,
        yf - 0.1873 * u - 0.4681 * v,
        yf + 1.8556 * u
    );
    frag = vec4(clamp(rgb, 0.0, 1.0), 1.0);
}
