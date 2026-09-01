#version 450
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
    vec2 corner = vec2(float(gl_VertexIndex & 1), float((gl_VertexIndex >> 1) & 1));
    vec2 scaled = (corner - 0.5) * pc.rect.zw;
    float s = sin(pc.angle);
    float c = cos(pc.angle);
    vec2 rotated = vec2(scaled.x * c - scaled.y * s, scaled.x * s + scaled.y * c);
    vec2 pos = (pc.rect.xy + rotated) / pc.canvas;
    gl_Position = vec4(pos * 2.0 - 1.0, 0.0, 1.0);
    v_uv = pc.uv.xy + corner * pc.uv.zw;
}
