#version 450
layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 frag;
layout(push_constant) uniform PC {
    vec4 rect;
    vec4 uv;
    vec4 tint;
    vec2 canvas;
    float angle;
    float pad;
} pc;
layout(binding = 0) uniform sampler2D tex;
void main() {
    vec4 texel = texture(tex, v_uv);
    vec3 rgb = texel.rgb * pc.tint.rgb;
    float alpha = texel.a * pc.tint.a;
    frag = pc.pad > 0.5 ? vec4(rgb * alpha, alpha) : vec4(rgb, alpha);
}
