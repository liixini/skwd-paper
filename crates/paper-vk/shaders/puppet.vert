#version 450
layout(location = 0) in vec2 a_position;
layout(location = 1) in vec2 a_uv;
layout(location = 0) out vec2 v_uv;
layout(push_constant) uniform PC {
    vec4 rect;
    vec4 uv;
    vec4 tint;
    vec2 canvas;
    float angle;
    float pad;
} pc;
void main() {
    vec2 scaled = a_position * pc.rect.zw;
    float s = sin(pc.angle);
    float c = cos(pc.angle);
    vec2 rotated = vec2(scaled.x * c - scaled.y * s, scaled.x * s + scaled.y * c);
    vec2 pos = (pc.rect.xy + rotated) / pc.canvas;
    gl_Position = vec4(pos * 2.0 - 1.0, 0.0, 1.0);
    v_uv = a_uv;
}
