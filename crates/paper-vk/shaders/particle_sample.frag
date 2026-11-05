uniform sampler2D g_Texture0;
uniform sampler2D g_Texture1;
uniform vec2 g_SourceSize;
uniform float g_HasMask;
varying vec2 v_TexCoord;
const float TAP_OFFSET = 2.0;
const float TAP_WEIGHT = 0.25;
void main() {
    vec2 offset = vec2(TAP_OFFSET) / g_SourceSize;
    vec4 color = texSample2D(g_Texture0, v_TexCoord + vec2(-offset.x, -offset.y));
    color += texSample2D(g_Texture0, v_TexCoord + vec2(offset.x, -offset.y));
    color += texSample2D(g_Texture0, v_TexCoord + vec2(-offset.x, offset.y));
    color += texSample2D(g_Texture0, v_TexCoord + vec2(offset.x, offset.y));
    color *= TAP_WEIGHT;
    color.a *= mix(1.0, texSample2D(g_Texture1, v_TexCoord).r, g_HasMask);
    gl_FragColor = color;
}
