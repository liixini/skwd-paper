#version 450
layout(push_constant) uniform PC { vec2 u_size; float u_t; float u_dry; int u_pass; };
layout(location = 0) in vec4 a_p;
layout(location = 1) in vec4 a_t;
layout(location = 2) in vec3 a_w;
layout(location = 0) out vec2 v_pos;
layout(location = 1) flat out vec4 v_p;
layout(location = 2) flat out vec4 v_t;
layout(location = 3) flat out vec3 v_w;
const float MARGIN = 2.0;
const float DIRECTION_FLOOR = 1e-4;
const vec4 CULLED = vec4(2.0, 2.0, 0.0, 1.0);
void main() {
    v_p = a_p;
    v_t = a_t;
    v_w = a_w;
    v_pos = a_p.xy;
    if (a_t.x > u_t) {
        gl_Position = CULLED;
        return;
    }
    vec2 corner = vec2(float(gl_VertexIndex & 1), float((gl_VertexIndex >> 1) & 1)) * 2.0 - 1.0;
    vec2 d = a_p.zw - a_p.xy;
    float l = length(d);
    vec2 dir = l > DIRECTION_FLOOR ? d / l : vec2(1.0, 0.0);
    vec2 nrm = vec2(-dir.y, dir.x);
    float r = a_w.x * 0.5 + MARGIN;
    vec2 base = corner.x < 0.0 ? a_p.xy : a_p.zw;
    v_pos = base + dir * corner.x * r + nrm * corner.y * r;
    gl_Position = vec4(v_pos / u_size * 2.0 - 1.0, 0.0, 1.0);
}
