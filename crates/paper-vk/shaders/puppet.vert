#version 450
#extension GL_GOOGLE_include_directive : require
layout(location = 0) in vec2 a_position;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in float a_alpha;
layout(location = 0) out vec2 v_uv;
layout(location = 1) out float v_alpha;
#ifdef CLIPPED
layout(location = 2) out vec2 v_mask_uv;
#endif
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
#include "layer_geometry.glsl"
void main() {
    gl_Position = layer_position(a_position);
    v_uv = layer_uv(a_uv);
    v_alpha = a_alpha;
#ifdef CLIPPED
    v_mask_uv = a_uv;
#endif
}
