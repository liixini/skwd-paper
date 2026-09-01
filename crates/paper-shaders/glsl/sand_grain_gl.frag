#version 330 core
in vec2 v_uv;
in float v_alpha;
in float v_bright;
in float v_mix;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
out vec4 frag;
void main() {
    if (v_alpha < 0.004) {
        discard;
    }
    vec2 uv = clamp(v_uv, vec2(0.0), vec2(1.0));
    float k = clamp(v_mix, 0.0, 1.0);
    vec3 rgb;
    if (k < 0.001) {
        rgb = texture(u_tex_old, uv).rgb;
    } else if (k > 0.999) {
        rgb = texture(u_tex_new, uv).rgb;
    } else {
        rgb = mix(texture(u_tex_old, uv).rgb, texture(u_tex_new, uv).rgb, k);
    }
    frag = vec4(rgb * v_bright, v_alpha);
}
