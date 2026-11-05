uniform sampler2D g_Texture0;
uniform vec2 g_Resolution;
const float CENTER_WEIGHT = 0.5;
const float SIDE_WEIGHT = 0.25;
void main() {
    vec2 uv = gl_FragCoord.xy / g_Resolution;
    vec3 color = texSample2D(g_Texture0, uv).rgb;
#if BLUR
#if VERTICAL
    vec2 step = vec2(0.0, 1.0 / g_Resolution.y);
#else
    vec2 step = vec2(1.0 / g_Resolution.x, 0.0);
#endif
    color = color * CENTER_WEIGHT + (texSample2D(g_Texture0, uv + step).rgb + texSample2D(g_Texture0, uv - step).rgb) * SIDE_WEIGHT;
#endif
    gl_FragColor = vec4(color, 1.0);
}
