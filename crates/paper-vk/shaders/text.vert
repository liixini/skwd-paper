#version 450
#extension GL_GOOGLE_include_directive : require
layout(location = 0) in vec2 a_corner;
layout(location = 1) in vec4 a_bounds;
layout(location = 2) in vec4 a_atlas;
layout(location = 3) in vec2 a_canvas;
layout(location = 0) out vec2 v_uv;
layout(location = 1) flat out vec4 v_bounds;
layout(location = 2) flat out vec4 v_atlas;
layout(location = 3) flat out vec2 v_canvas;
layout(location = 4) flat out uvec2 v_clip;
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
    vec2 origin = layer_uv_size(vec2(0.0), a_canvas);
    mat2 axes = mat2(layer_uv_size(vec2(1.0, 0.0), a_canvas) - origin,
                     layer_uv_size(vec2(0.0, 1.0), a_canvas) - origin);
    vec2 position = a_corner;
    bool identity = all(equal(origin, vec2(0.0))) && all(equal(axes[0], vec2(1.0, 0.0))) && all(equal(axes[1], vec2(0.0, 1.0)));
    bool edge = any(lessThanEqual(a_bounds.xy, vec2(0.0))) || any(greaterThanEqual(a_bounds.xy + a_bounds.zw, a_canvas));
    bool bounded = identity || (determinant(axes) != 0.0 && !edge);
    v_clip = uvec2(identity ? 0u : 1u, bounded ? 0u : 1u);
    if (bounded) {
        vec2 source = (a_bounds.xy + a_corner * a_bounds.zw) / a_canvas;
        position = inverse(axes) * (source - origin);
    }
    gl_Position = layer_position(position - 0.5);
    v_uv = position;
    v_bounds = a_bounds;
    v_atlas = a_atlas;
    v_canvas = a_canvas;
}
