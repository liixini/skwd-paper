#version 450
#extension GL_GOOGLE_include_directive : require
layout(location = 0) in vec2 v_uv;
layout(location = 1) flat in vec4 v_bounds;
layout(location = 2) flat in vec4 v_atlas;
layout(location = 3) flat in vec2 v_canvas;
layout(location = 4) flat in uvec2 v_clip;
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
#include "layer_geometry.glsl"
const uint ALPHA_PREMULTIPLY = 1u;
void main() {
    if (v_clip.x != 0u && (any(lessThan(v_uv, vec2(0.0))) || any(greaterThanEqual(v_uv, vec2(1.0))))) {
        discard;
    }
    vec2 source = clamp(layer_uv_size(v_uv, v_canvas) * v_canvas, vec2(0.5), v_canvas - 0.5);
    vec2 local = source - v_bounds.xy;
    if (v_clip.y != 0u && (any(lessThan(local, vec2(0.0))) || any(greaterThanEqual(local, v_bounds.zw)))) {
        discard;
    }
    vec2 atlas = (v_atlas.xy + local * v_atlas.zw / v_bounds.zw) / vec2(textureSize(tex, 0));
    float alpha = texture(tex, atlas).a * pc.tint.a;
    vec3 rgb = pc.tint.rgb;
    frag = (uint(pc.pad) & ALPHA_PREMULTIPLY) != 0u ? vec4(rgb * alpha, alpha) : vec4(rgb, alpha);
}
