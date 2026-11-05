#version 450
#extension GL_GOOGLE_include_directive : require
layout(location = 0) out vec2 v_uv;
layout(location = 1) out float v_alpha;
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
    vec2 corner = vec2(float(gl_VertexIndex & 1), float((gl_VertexIndex >> 1) & 1));
    gl_Position = layer_position(corner - 0.5);
    v_uv = layer_uv(corner);
    v_alpha = 1.0;
}
