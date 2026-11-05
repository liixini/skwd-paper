#if LIGHTING
#if SKWD_LIGHT_SHADOWS
uniform vec4 g_LShadow_Size;
#endif
#if LIGHTS_POINT > 0
uniform vec4 g_LPoint_Origin[LIGHTS_POINT];
uniform vec4 g_LPoint_Color[LIGHTS_POINT];
#if SKWD_LIGHT_SHADOWS
uniform vec4 g_LPoint_Shadow[LIGHTS_POINT];
#endif
#endif
#if LIGHTS_SPOT > 0
#if LIGHTS_COOKIE
#ifndef COOKIE_SAMPLER
#define COOKIE_SAMPLER g_Texture7
uniform sampler2D g_Texture7;
#endif
uniform vec4 g_LSpot_Cookie[LIGHTS_SPOT];
uniform vec4 g_LSpot_CookieRight[LIGHTS_SPOT];
uniform vec4 g_LSpot_CookieUp[LIGHTS_SPOT];
#endif
uniform vec4 g_LSpot_Origin[LIGHTS_SPOT];
uniform vec4 g_LSpot_Color[LIGHTS_SPOT];
uniform vec4 g_LSpot_Direction[LIGHTS_SPOT];
uniform vec4 g_LSpot_Exponent[LIGHTS_SPOT];
#if SKWD_LIGHT_SHADOWS
uniform vec4 g_LSpot_Shadow[LIGHTS_SPOT];
uniform vec4 g_LSpot_ShadowRight[LIGHTS_SPOT];
uniform vec4 g_LSpot_ShadowUp[LIGHTS_SPOT];
#endif
#endif
#if LIGHTS_DIRECTIONAL > 0
uniform vec4 g_LDirectional_Color[LIGHTS_DIRECTIONAL];
uniform vec4 g_LDirectional_Direction[LIGHTS_DIRECTIONAL];
#if SKWD_LIGHT_SHADOWS
uniform vec4 g_LDirectional_Shadow[LIGHTS_DIRECTIONAL];
uniform mat4 g_LFeature_ShadowProjection[SKWD_LIGHT_PROJECTIONS];
uniform vec4 g_LFeature_ShadowProjectionTransform[SKWD_LIGHT_PROJECTIONS];
const float CASCADE_EDGE = 0.99;
const float CASCADE_FAR = 0.01;
vec4 CascadeCoordinates(vec3 worldPos, mat4 shadowViewProjection) {
    vec4 proj = mul(vec4(worldPos, 1.0), shadowViewProjection);
    proj.xyz /= proj.w;
    proj.w = step(1.0, dot(CAST3(1.0), vec3(step(CASCADE_EDGE, abs(proj.xy)), step(proj.z, CASCADE_FAR))));
    proj.xy = proj.xy * CAST2(0.5) + CAST2(0.5);
    proj.y = 1.0 - proj.y;
    return proj;
}
float CascadeShadow(vec3 projectedCoords, vec4 atlasTransform) {
    vec2 uv = projectedCoords.xy * atlasTransform.zw + atlasTransform.xy;
    vec2 texel = CAST2(1.0) / g_LShadow_Size.zw;
    vec2 roundOffset = texel * SHADOW_DIAGONAL;
    vec2 offsets = texel * SHADOW_AXIAL;
    return (texSample2DCompare(g_Texture6, uv - roundOffset, projectedCoords.z).r
        + texSample2DCompare(g_Texture6, uv + vec2(0.0, -offsets.y), projectedCoords.z).r
        + texSample2DCompare(g_Texture6, uv + vec2(roundOffset.x, -roundOffset.y), projectedCoords.z).r
        + texSample2DCompare(g_Texture6, uv + vec2(-offsets.x, 0.0), projectedCoords.z).r
        + texSample2DCompare(g_Texture6, uv, projectedCoords.z).r
        + texSample2DCompare(g_Texture6, uv + vec2(offsets.x, 0.0), projectedCoords.z).r
        + texSample2DCompare(g_Texture6, uv + vec2(-roundOffset.x, roundOffset.y), projectedCoords.z).r
        + texSample2DCompare(g_Texture6, uv + vec2(0.0, offsets.y), projectedCoords.z).r
        + texSample2DCompare(g_Texture6, uv + roundOffset, projectedCoords.z).r) / SHADOW_FILTER_SAMPLES;
}
#endif
#endif
vec3 PerformLighting_V1(vec3 worldPos, vec3 albedo, vec3 normal, vec3 viewDir, vec3 specularTint, vec3 baseReflectance, float roughness, float metallic) {
    vec3 light = CAST3(0.0);
#if LIGHTS_POINT > 0
    for (int i = 0; i < LIGHTS_POINT; ++i) {
        vec3 delta = g_LPoint_Origin[i].xyz - worldPos;
        float shadow = 1.0;
#if SKWD_LIGHT_SHADOWS
        vec4 coordinates = ShadowCoordinates(-delta, vec4(0.0, 0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0), g_LPoint_Shadow[i].w);
        shadow = ShadowSample(g_LShadow_Size, coordinates, g_LPoint_Shadow[i], true);
#endif
        light += ComputePBRLightShadow(normal, delta, viewDir, albedo, g_LPoint_Color[i].rgb, g_LPoint_Color[i].w, g_LPoint_Origin[i].w, specularTint, baseReflectance, roughness, metallic, shadow);
    }
#endif
#if LIGHTS_SPOT > 0
    for (int i = 0; i < LIGHTS_SPOT; ++i) {
        vec3 delta = g_LSpot_Origin[i].xyz - worldPos;
        float cone = -dot(normalize(delta), g_LSpot_Direction[i].xyz);
        cone = smoothstep(g_LSpot_Direction[i].w, g_LSpot_Origin[i].w, cone);
        vec3 cookie = CAST3(cone);
#if LIGHTS_COOKIE
        if (g_LSpot_Cookie[i].z > 0.0) {
            vec3 projected = vec3(dot(-delta, g_LSpot_CookieRight[i].xyz), dot(-delta, g_LSpot_CookieUp[i].xyz), dot(-delta, g_LSpot_Direction[i].xyz));
            vec2 uv = projected.xy * g_LSpot_CookieRight[i].w / max(projected.z, 0.000001);
            uv = uv * vec2(0.5, -0.5) + vec2(0.5, 0.5);
            cookie = CAST3(0.0);
            if (projected.z > 0.0 && uv.x >= 0.0 && uv.y >= 0.0 && uv.x <= 1.0 && uv.y <= 1.0)
                cookie = texSample2D(g_Texture7, g_LSpot_Cookie[i].xy + uv * g_LSpot_Cookie[i].zw).rgb;
        }
#endif
        float shadow = 1.0;
#if SKWD_LIGHT_SHADOWS
        vec4 coordinates = ShadowCoordinates(-delta, vec4(g_LSpot_Direction[i].xyz, 1.0), g_LSpot_ShadowRight[i].xyz, g_LSpot_ShadowUp[i].xyz, g_LSpot_Shadow[i].w);
        shadow = ShadowSample(g_LShadow_Size, coordinates, g_LSpot_Shadow[i], true);
#endif
        light += ComputePBRLightShadow(normal, delta, viewDir, albedo, g_LSpot_Color[i].rgb * cookie, g_LSpot_Color[i].w, g_LSpot_Exponent[i].x, specularTint, baseReflectance, roughness, metallic, shadow);
    }
#endif
#if LIGHTS_DIRECTIONAL > 0
    for (int i = 0; i < LIGHTS_DIRECTIONAL; ++i) {
        float shadowFactor = 1.0;
#if SKWD_LIGHT_SHADOWS
        if (g_LDirectional_Shadow[i].x > 0.0) {
            int p1 = int(g_LDirectional_Shadow[i].x) - 1;
            int p2 = p1 + 1;
            int p3 = p1 + 2;
            vec4 projectedCoords1 = CascadeCoordinates(worldPos, g_LFeature_ShadowProjection[p1]);
            vec4 projectedCoords2 = CascadeCoordinates(worldPos, g_LFeature_ShadowProjection[p2]);
            vec4 projectedCoords3 = CascadeCoordinates(worldPos, g_LFeature_ShadowProjection[p3]);
            projectedCoords1.xyz = mix(projectedCoords1.xyz, projectedCoords2.xyz, projectedCoords1.w);
            projectedCoords1.xyz = mix(projectedCoords1.xyz, projectedCoords3.xyz, projectedCoords2.w);
            vec4 uvTransforms = mix(g_LFeature_ShadowProjectionTransform[p1], g_LFeature_ShadowProjectionTransform[p2], projectedCoords1.w);
            uvTransforms = mix(uvTransforms, g_LFeature_ShadowProjectionTransform[p3], projectedCoords2.w);
            shadowFactor = max(projectedCoords3.w, CascadeShadow(projectedCoords1.xyz, uvTransforms));
        }
#endif
        light += ComputePBRLightShadowInfinite(normal, g_LDirectional_Direction[i].xyz, viewDir, albedo, g_LDirectional_Color[i].rgb, specularTint, baseReflectance, roughness, metallic, shadowFactor);
    }
#endif
    return light;
}
#else
vec3 PerformLighting_V1(vec3 worldPos, vec3 albedo, vec3 normal, vec3 viewDir, vec3 specularTint, vec3 baseReflectance, float roughness, float metallic) { return CAST3(0.0); }
#endif
