uniform sampler2D g_Texture0;

void main() {
    ivec2 pixel = ivec2(gl_FragCoord.xy);
#if DEPTH_SAMPLES > 1
    float depth = 0.0;
    for (int sampleIndex = 0; sampleIndex < DEPTH_SAMPLES; ++sampleIndex) {
        depth = max(depth, texelFetch(g_Texture0, pixel, sampleIndex).r);
    }
#else
    float depth = texelFetch(g_Texture0, pixel, 0).r;
#endif
    gl_FragColor = vec4(depth);
}
