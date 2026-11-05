#version 450
layout(location = 0) out vec4 frag;
layout(binding = 0) uniform sampler2D tex;
void main() {
    frag = texelFetch(tex, ivec2(gl_FragCoord.xy), 0);
}
