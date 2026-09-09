#version 450
layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 frag;
layout(push_constant) uniform PC { vec2 uv_scale; vec2 uv_off; int fill; float blur; float dim; } pc;
layout(binding = 0) uniform sampler2D luma;
layout(binding = 1) uniform sampler2D chroma;
vec4 filtered(sampler2D source, vec2 uv) {
    if (pc.blur <= 0.0) return texture(source, uv);
    const vec2 offsets[48] = vec2[48](
        vec2(0.143986, 0.000000), vec2(-0.184861, 0.169348), vec2(0.028447, -0.324144),
        vec2(0.235530, 0.307208), vec2(-0.434628, -0.076880), vec2(0.414049, -0.263384),
        vec2(-0.139290, 0.518154), vec2(-0.267206, -0.514489), vec2(0.583213, 0.212988),
        vec2(-0.610458, 0.251988), vec2(0.296125, -0.632802), vec2(0.220230, 0.702129),
        vec2(-0.668124, -0.387191), vec2(0.789038, -0.173468), vec2(-0.484842, 0.689638),
        vec2(-0.112797, -0.870450), vec2(0.697458, 0.587818), vec2(-0.945504, 0.039100),
        vec2(0.694913, -0.691531), vec2(-0.046855, 1.013292), vec2(-0.671730, -0.804957),
        vec2(1.072899, 0.144357), vec2(-0.916820, 0.637900), vec2(0.252734, -1.123427),
        vec2(0.589879, 1.029418), vec2(-1.164014, -0.371352), vec2(1.141724, -0.527505),
        vec2(-0.499632, 1.193846), vec2(-0.450606, -1.252800), vec2(1.212167, 0.637081),
        vec2(-1.361830, 0.358974), vec2(0.783349, -1.218287), vec2(0.252320, 1.468181),
        vec2(-1.211596, -0.938328), vec2(1.571497, -0.130195), vec2(-1.102315, 1.191570),
        vec2(0.008156, -1.671870), vec2(1.157802, 1.276308), vec2(-1.770155, -0.164058),
        vec2(1.462598, -1.110059), vec2(-0.339947, 1.868649), vec2(-1.048437, -1.666072),
        vec2(1.972749, 0.540649), vec2(-1.897702, 0.973870), vec2(0.777101, -2.096096),
        vec2(0.890284, 2.186961), vec2(-2.284555, -1.082693), vec2(2.667690, -0.822477)
    );
    vec2 stepSize = vec2(pc.blur) / vec2(textureSize(luma, 0));
    vec4 color = vec4(0.0);
    for (int i = 0; i < 48; i++)
        color += texture(source, uv + offsets[i] * stepSize);
    return color / 48.0;
}
void main() {
    vec2 uv = v_uv;
    if (pc.fill == 1 && (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0))))) {
        frag = vec4(0.0, 0.0, 0.0, 1.0);
        return;
    }
    if (pc.fill == 2) uv = fract(uv);
    float y = filtered(luma, uv).r;
    vec2 c = filtered(chroma, uv).rg;
    // BT.709 limited range
    float yf = (y - 16.0 / 255.0) * (255.0 / 219.0);
    float u = c.x - 0.5;
    float v = c.y - 0.5;
    vec3 rgb = vec3(
        yf + 1.5748 * v,
        yf - 0.1873 * u - 0.4681 * v,
        yf + 1.8556 * u
    );
    frag = vec4(clamp(rgb, 0.0, 1.0) * (1.0 - pc.dim), 1.0);
}
