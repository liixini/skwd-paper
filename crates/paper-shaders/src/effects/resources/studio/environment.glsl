uniform sampler2D u_studio_environment;

const vec2 STUDIO_SIZE = vec2(768.0, 1024.0);
const float STUDIO_BYTE_MAX = 255.0;
const float STUDIO_BYTE_RADIX = 256.0;
const float STUDIO_HALF_MANTISSA = 1024.0;
const float STUDIO_HALF_BIAS = 15.0;
const float STUDIO_HALF_SUBNORMAL = -24.0;
const float STUDIO_MIN_MIP = 4.0;
const float STUDIO_MAX_MIP = 8.0;
const float STUDIO_MIN_TILE = 16.0;
const float STUDIO_FACE_ROW = 3.0;
const float STUDIO_TILE_STRIDE = 4.0;
const int STUDIO_ROUGHNESS_STOPS = 5;
const vec2 STUDIO_ROUGHNESS[STUDIO_ROUGHNESS_STOPS] = vec2[STUDIO_ROUGHNESS_STOPS](vec2(1.0, -2.0), vec2(0.8, -1.0), vec2(0.4, 2.0), vec2(0.305, 3.0), vec2(0.21, 4.0));
const float STUDIO_ROUGHNESS_SCALE = 1.16;
const vec4 STUDIO_DFG_LOW = vec4(-1.0, -0.0275, -0.572, 0.022);
const vec4 STUDIO_DFG_HIGH = vec4(1.0, 0.0425, 1.04, -0.04);
const float STUDIO_DFG_EXP = -9.28;
const vec2 STUDIO_DFG_SCALE = vec2(-1.04, 1.04);
const float STUDIO_FRESNEL_AVERAGE = 0.047619;

float studio_half(vec2 bytes) {
    float bits = bytes.x + bytes.y * STUDIO_BYTE_RADIX;
    float exponent = floor(bits / STUDIO_HALF_MANTISSA);
    float mantissa = bits - exponent * STUDIO_HALF_MANTISSA;
    return exponent == 0.0 ? mantissa * exp2(STUDIO_HALF_SUBNORMAL)
        : (1.0 + mantissa / STUDIO_HALF_MANTISSA) * exp2(exponent - STUDIO_HALF_BIAS);
}

vec3 studio_texel(ivec2 position) {
    ivec2 cell = clamp(position, ivec2(0), ivec2(STUDIO_SIZE) - 1);
    vec4 first = floor(texelFetch(u_studio_environment, ivec2(cell.x * 2, cell.y), 0) * STUDIO_BYTE_MAX + 0.5);
    vec4 second = floor(texelFetch(u_studio_environment, ivec2(cell.x * 2 + 1, cell.y), 0) * STUDIO_BYTE_MAX + 0.5);
    return vec3(studio_half(first.xy), studio_half(first.zw), studio_half(second.xy));
}

vec3 studio_bilinear(vec2 position) {
    vec2 point = position - 0.5;
    ivec2 cell = ivec2(floor(point));
    vec2 amount = fract(point);
    return mix(mix(studio_texel(cell), studio_texel(cell + ivec2(1, 0)), amount.x),
        mix(studio_texel(cell + ivec2(0, 1)), studio_texel(cell + ivec2(1, 1)), amount.x), amount.y);
}

vec3 studio_cube(vec3 direction, float mip) {
    vec3 size = abs(direction);
    float face;
    vec2 uv;
    if (size.x > size.z && size.x > size.y) {
        face = direction.x > 0.0 ? 0.0 : STUDIO_FACE_ROW;
        uv = vec2(direction.x > 0.0 ? direction.z : -direction.z, direction.y) / size.x;
    } else if (size.y >= size.x && size.y >= size.z) {
        face = direction.y > 0.0 ? 1.0 : STUDIO_FACE_ROW + 1.0;
        uv = vec2(-direction.x, direction.y > 0.0 ? -direction.z : direction.z) / size.y;
    } else {
        face = direction.z > 0.0 ? 2.0 : STUDIO_FACE_ROW + 2.0;
        uv = vec2(direction.z > 0.0 ? -direction.x : direction.x, direction.y) / size.z;
    }
    float filter_level = max(STUDIO_MIN_MIP - mip, 0.0);
    float face_size = exp2(max(mip, STUDIO_MIN_MIP));
    uv = (uv + 1.0) * 0.5 * (face_size - 2.0) + 1.0;
    if (face > 2.0) {
        uv.y += face_size;
        face -= STUDIO_FACE_ROW;
    }
    uv.x += face * face_size + filter_level * STUDIO_FACE_ROW * STUDIO_MIN_TILE;
    uv.y += STUDIO_TILE_STRIDE * (exp2(STUDIO_MAX_MIP) - face_size);
    return studio_bilinear(uv);
}

float studio_mip(float roughness) {
    for (int idx = 1; idx < STUDIO_ROUGHNESS_STOPS; ++idx) {
        vec2 upper = STUDIO_ROUGHNESS[idx - 1];
        vec2 lower = STUDIO_ROUGHNESS[idx];
        if (roughness >= lower.x) return mix(upper.y, lower.y, (upper.x - roughness) / (upper.x - lower.x));
    }
    return -2.0 * log2(STUDIO_ROUGHNESS_SCALE * roughness);
}

vec3 studio_radiance(vec3 direction, float roughness) {
    float mip = clamp(studio_mip(roughness), STUDIO_ROUGHNESS[0].y, STUDIO_MAX_MIP);
    float base = floor(mip);
    vec3 color = studio_cube(direction, base);
    return fract(mip) == 0.0 ? color : mix(color, studio_cube(direction, base + 1.0), fract(mip));
}

vec2 studio_dfg(float dot_view, float roughness) {
    vec4 response = roughness * STUDIO_DFG_LOW + STUDIO_DFG_HIGH;
    float grazing = min(response.x * response.x, exp2(STUDIO_DFG_EXP * dot_view)) * response.x + response.y;
    return STUDIO_DFG_SCALE * grazing + response.zw;
}

vec3 studio_reflection(vec3 normal, vec3 view, float roughness) {
    return studio_radiance(normalize(mix(reflect(-view, normal), normal, roughness * roughness)), roughness);
}

vec3 studio_indirect(vec3 normal, vec3 view, vec3 diffuse, vec3 specular, float roughness) {
    vec2 response = studio_dfg(clamp(dot(normal, view), 0.0, 1.0), roughness);
    vec3 single_scatter = specular * response.x + response.y;
    float missing = 1.0 - response.x - response.y;
    vec3 average = specular + (1.0 - specular) * STUDIO_FRESNEL_AVERAGE;
    vec3 multiple_scatter = single_scatter * average / (1.0 - missing * average) * missing;
    vec3 total = single_scatter + multiple_scatter;
    vec3 residual = diffuse * (1.0 - max(max(total.r, total.g), total.b));
    return studio_reflection(normal, view, roughness) * single_scatter + (multiple_scatter + residual) * studio_radiance(normal, 1.0);
}
