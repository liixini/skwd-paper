uniform sampler2DComparison SHADOW_TEXTURE;
const float SHADOW_DEPTH_BIAS = 0.000001;
const float SHADOW_DEPTH_EPSILON = 0.000001;
const float SHADOW_DIAGONAL = 0.81616;
const float SHADOW_AXIAL = 1.02323;
const float SHADOW_FILTER_SAMPLES = 9.0;
const float POINT_RECEIVER_SCALE = 0.94;
const vec2 POINT_VIEWPORT_SCALE = vec2(0.5, 0.3333);

vec4 ShadowCoordinates(vec3 delta, vec4 direction, vec3 right, vec3 up, float scale) {
    vec3 projected;
    float face = -1.0;
    if (direction.w > 0.0) {
        projected = vec3(dot(delta, right), dot(delta, up), dot(delta, direction.xyz));
    } else {
        vec3 magnitude = abs(delta);
        if (magnitude.x >= magnitude.y && magnitude.x >= magnitude.z) {
            projected = vec3(delta.x >= 0.0 ? delta.z : -delta.z, delta.y, magnitude.x);
            face = delta.x >= 0.0 ? 0.0 : 1.0;
        } else if (magnitude.y >= magnitude.x && magnitude.y >= magnitude.z) {
            projected = vec3(delta.x, delta.y >= 0.0 ? delta.z : -delta.z, magnitude.y);
            face = delta.y >= 0.0 ? 2.0 : 3.0;
        } else {
            projected = vec3(delta.z >= 0.0 ? -delta.x : delta.x, delta.y, magnitude.z);
            face = delta.z >= 0.0 ? 4.0 : 5.0;
        }
    }
    float receiverScale = direction.w > 0.0 ? scale : POINT_RECEIVER_SCALE;
    vec2 uv = projected.xy * receiverScale / max(projected.z, SHADOW_DEPTH_EPSILON);
    return vec4(uv * vec2(0.5, -0.5) + vec2(0.5, 0.5), projected.z, face);
}

float ShadowSample(vec4 atlasSize, vec4 coordinates, vec4 parameters, bool filtered) {
    if (parameters.x <= 0.0 || coordinates.z <= 0.0) return 1.0;
    if (coordinates.x < 0.0 || coordinates.y < 0.0 || coordinates.x > 1.0 || coordinates.y > 1.0) return 1.0;
    if (coordinates.w >= 0.0) {
        vec2 face = vec2(mod(coordinates.w, 2.0), floor(coordinates.w * 0.5));
        coordinates.xy = (coordinates.xy + face) * POINT_VIEWPORT_SCALE;
    }
    float tile = parameters.x - 1.0;
    vec2 cell = vec2(mod(tile, atlasSize.x), floor(tile / atlasSize.x));
    vec2 uv = (cell + coordinates.xy) / atlasSize.xy;
    vec2 texel = 1.0 / atlasSize.zw;
    vec2 minimum = cell / atlasSize.xy + texel * 0.5;
    vec2 maximum = (cell + vec2(1.0, 1.0)) / atlasSize.xy - texel * 0.5;
    float depth = -parameters.y + parameters.z / coordinates.z;
    float shadow = texSample2DCompare(SHADOW_TEXTURE, clamp(uv, minimum, maximum), depth + SHADOW_DEPTH_BIAS).r;
    if (!filtered) return shadow;
    for (int row = -1; row <= 1; ++row) {
        for (int column = -1; column <= 1; ++column) {
            if (row == 0 && column == 0) continue;
            float scale = row != 0 && column != 0 ? SHADOW_DIAGONAL : SHADOW_AXIAL;
            vec2 offset = vec2(float(column), float(row)) * texel * scale;
            shadow += texSample2DCompare(SHADOW_TEXTURE, clamp(uv + offset, minimum, maximum), depth + SHADOW_DEPTH_BIAS).r;
        }
    }
    return shadow / SHADOW_FILTER_SAMPLES;
}
