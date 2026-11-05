#version 450
layout(location = 0) in vec2 v_uv;
layout(location = 1) in float v_alpha;
layout(location = 2) in vec2 v_mask_uv;
layout(location = 0) out vec4 frag;
layout(push_constant) uniform PC {
    vec4 rect;
    vec4 uv;
    vec4 tint;
    vec2 canvas;
    float angle;
    float pad;
    vec4 projection_x;
    vec4 projection_y;
    vec4 projection_w;
    vec4 uv_axes;
} pc;
layout(binding = 0) uniform sampler2D tex;
layout(set = 1, binding = 0) uniform sampler2D mask_tex;
const uint CLIP_INVERT = 32u;
const float OVERLAP_POWER = 4.0;
void main() {
    float albedo = texture(tex, v_uv).a;
    float mask = texture(mask_tex, v_mask_uv).r;
    float alpha = mix(pow(albedo, OVERLAP_POWER), albedo, mask) * v_alpha;
    float coverage = mask * alpha;
    float invert = float((uint(pc.pad) & CLIP_INVERT) != 0u);
    frag = vec4(mix(coverage, 1.0 - coverage, invert), 0.0, 0.0, alpha);
}
