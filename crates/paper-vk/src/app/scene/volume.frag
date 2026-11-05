uniform vec2 g_VolumeFog;
uniform vec4 g_VolumeFogDistance;
uniform vec4 g_VolumeFogHeight;
uniform vec3 g_VolumeEye;

float VolumeFog(vec3 point) {
    float distanceFog = 0.0;
    float heightFog = 0.0;
    if (g_VolumeFog.x > 0.0) {
        float state = clamp((length(g_VolumeEye - point) - g_VolumeFogDistance.x) / g_VolumeFogDistance.y, 0.0, 1.0);
        distanceFog = g_VolumeFogDistance.z + g_VolumeFogDistance.w * state * state;
    }
    if (g_VolumeFog.y > 0.0) {
        float state = clamp((point.y - g_VolumeFogHeight.x) / g_VolumeFogHeight.y, 0.0, 1.0);
        heightFog = g_VolumeFogHeight.z + g_VolumeFogHeight.w * state * state;
    }
    float fog = clamp(max(distanceFog, heightFog), 0.0, 1.0);
    return 1.0 - fog * fog;
}

uniform mat4 g_InverseProjection;
uniform vec2 g_Resolution;
uniform vec4 g_VolumeOrigin[VOLUME_LIGHTS];
uniform vec4 g_VolumeColor[VOLUME_LIGHTS];
uniform vec4 g_VolumeDirection[VOLUME_LIGHTS];
uniform vec4 g_VolumeParameters[VOLUME_LIGHTS];
uniform vec4 g_VolumeRight[VOLUME_LIGHTS];
uniform vec4 g_VolumeUp[VOLUME_LIGHTS];
#if VOLUME_COOKIE
uniform sampler2D g_Texture2;
uniform vec4 g_VolumeCookie[VOLUME_LIGHTS];
#endif
#if VOLUME_DEPTH
uniform sampler2D g_Texture0;
#endif
#if VOLUME_SHADOW
uniform vec4 g_VolumeShadow[VOLUME_LIGHTS];
uniform vec4 g_ShadowSize;
#endif

const int SAMPLE_COUNT = 3;
const int SHADOW_SAMPLE_COUNT = 24;
const float COOKIE_PROJECTION_SCALE = 0.525;
const float LIGHT_SCALE = 0.1;
const float PARALLEL_EPSILON = 0.000001;
const float VOLUME_HASH_SCALE = 43758.5453;
const float VOLUME_HASH_OFFSET = 19.19;

vec3 worldPosition(vec2 ndc, float depth) {
    vec4 world = g_InverseProjection * vec4(ndc, depth, 1.0);
    return world.xyz / world.w;
}

bool clipPlane(vec3 relative, vec3 ray, vec3 normal, float offset, inout float entry, inout float exit) {
    float position = dot(relative, normal) + offset;
    float direction = dot(ray, normal);
    if (abs(direction) < PARALLEL_EPSILON) return position >= 0.0;
    float crossing = -position / direction;
    if (direction > 0.0) entry = max(entry, crossing);
    else exit = min(exit, crossing);
    return exit > entry;
}

void main() {
    vec2 uv = gl_FragCoord.xy / g_Resolution;
    vec2 ndc = uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    float depth = 0.0;
#if VOLUME_DEPTH
    depth = texelFetch(g_Texture0, ivec2(uv * vec2(textureSize(g_Texture0, 0))), 0).r;
#endif
    vec3 start = worldPosition(ndc, 1.0);
    vec3 end = worldPosition(ndc, depth);
    float limit = length(end - start);
    vec3 ray = normalize(end - start);
    vec3 light = vec3(0.0);
    for (int slot = 0; slot < VOLUME_LIGHTS; ++slot) {
        float radius = g_VolumeOrigin[slot].w;
        if (radius <= 0.0) continue;
        vec3 relative = start - g_VolumeOrigin[slot].xyz;
        float entry = 0.0;
        float exit = limit;
        if (g_VolumeParameters[slot].z > 0.0) {
            vec3 forward = g_VolumeDirection[slot].xyz;
            float tangent = tan(acos(g_VolumeParameters[slot].y));
            vec3 side = forward * tangent;
            vec3 right = g_VolumeRight[slot].xyz;
            vec3 up = g_VolumeUp[slot].xyz;
            if (!clipPlane(relative, ray, forward, 0.0, entry, exit)
                || !clipPlane(relative, ray, -forward, radius, entry, exit)
                || !clipPlane(relative, ray, side + right, 0.0, entry, exit)
                || !clipPlane(relative, ray, side - right, 0.0, entry, exit)
                || !clipPlane(relative, ray, side + up, 0.0, entry, exit)
                || !clipPlane(relative, ray, side - up, 0.0, entry, exit)) continue;
        } else {
            float projected = dot(relative, ray);
            float discriminant = projected * projected - dot(relative, relative) + radius * radius;
            if (discriminant <= 0.0) continue;
            float root = sqrt(discriminant);
            entry = max(0.0, -projected - root);
            exit = min(limit, -projected + root);
        }
        if (exit <= entry) continue;
        int sampleCount = SAMPLE_COUNT;
        bool hasCookie = false;
#if VOLUME_COOKIE
        hasCookie = g_VolumeCookie[slot].z > 0.0;
        if (hasCookie) sampleCount = SHADOW_SAMPLE_COUNT;
#endif
#if VOLUME_SHADOW
        if (g_VolumeShadow[slot].x > 0.0) sampleCount = SHADOW_SAMPLE_COUNT;
#endif
        float stepLength = (exit - entry) / float(sampleCount + 1);
        float marchOffset = 0.0;
#if VOLUME_SHADOW
        if (g_VolumeShadow[slot].x > 0.0) {
            vec3 hash = fract(vec3(uv.x, uv.y, uv.x) * VOLUME_HASH_SCALE);
            hash += vec3(dot(hash, hash.yzx + vec3(VOLUME_HASH_OFFSET)));
            marchOffset = fract((hash.x + hash.y) * hash.z);
        }
#endif
        vec3 scattering = vec3(0.0, 0.0, 0.0);
        for (int sampleIndex = 0; sampleIndex < sampleCount; ++sampleIndex) {
            vec3 point = start + ray * (entry + (float(sampleIndex + 1) + marchOffset) * stepLength);
            vec3 delta = point - g_VolumeOrigin[slot].xyz;
            float falloff = pow(max(0.0, 1.0 - length(delta) / radius), g_VolumeParameters[slot].x);
            if (g_VolumeParameters[slot].z > 0.0 && !hasCookie) {
                float cone = dot(normalize(delta), g_VolumeDirection[slot].xyz);
                falloff *= smoothstep(g_VolumeParameters[slot].y, g_VolumeDirection[slot].w, cone);
            }
#if VOLUME_SHADOW
            vec4 direction = vec4(g_VolumeDirection[slot].xyz, g_VolumeParameters[slot].z);
            vec4 coordinates = ShadowCoordinates(delta, direction, g_VolumeUp[slot].xyz, g_VolumeRight[slot].xyz, g_VolumeShadow[slot].w);
            if (direction.w > 0.0) {
                coordinates.xy = (coordinates.xy - vec2(0.5)) * (COOKIE_PROJECTION_SCALE * 2.0) + vec2(0.5);
            }
            falloff *= ShadowSample(g_ShadowSize, coordinates, g_VolumeShadow[slot], false);
#endif
            if (g_VolumeFog.x + g_VolumeFog.y > 0.0) {
                falloff *= falloff * VolumeFog(point);
            }
            vec3 tint = vec3(1.0, 1.0, 1.0);
#if VOLUME_COOKIE
            if (hasCookie) {
                vec3 projected = vec3(dot(delta, g_VolumeUp[slot].xyz), dot(delta, g_VolumeRight[slot].xyz), dot(delta, g_VolumeDirection[slot].xyz));
                float scale = 1.0 / tan(acos(g_VolumeParameters[slot].y));
                vec2 uv = projected.xy * scale / max(projected.z, PARALLEL_EPSILON);
                uv = clamp(uv * vec2(COOKIE_PROJECTION_SCALE, -COOKIE_PROJECTION_SCALE) + vec2(0.5, 0.5), vec2(0.0, 0.0), vec2(1.0, 1.0));
                tint = texSample2D(g_Texture2, g_VolumeCookie[slot].xy + uv * g_VolumeCookie[slot].zw).rgb;
            }
#endif
            scattering += falloff * tint;
        }
        float normalization = mix(0.5, 1.0, g_VolumeParameters[slot].z);
        light += g_VolumeColor[slot].rgb * g_VolumeColor[slot].w
            * ((exit - entry) / radius) * normalization * LIGHT_SCALE
            * scattering / float(sampleCount);
    }
    gl_FragColor = vec4(light, 1.0);
}
