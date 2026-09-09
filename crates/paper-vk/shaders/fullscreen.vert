#version 450
layout(location = 0) out vec2 v_uv;
layout(push_constant) uniform PC { vec2 uv_scale; vec2 uv_off; int fill; float blur; float dim; } pc;
void main() {
    vec2 pos = vec2((gl_VertexIndex << 1) & 2, gl_VertexIndex & 2);
    gl_Position = vec4(pos * 2.0 - 1.0, 0.0, 1.0);
    v_uv = pos * pc.uv_scale + pc.uv_off;
}
