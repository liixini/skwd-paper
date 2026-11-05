const float WORLD_HEIGHT = 6.0;
const float WORLD_HALF_FOV = STUDIO_PI / 9.0;
const float WORLD_NEAR = 0.05;
const float WORLD_FAR = 100.0;
const float WORLD_EPSILON = 0.000001;
const float WORLD_EASE_POWER = 4.0;
const float WORLD_EASE_SCALE = 8.0;
const vec3 WORLD_LIGHT_POS[3] = vec3[3](vec3(-3.0, 5.0, 6.0), vec3(5.0, 0.0, 3.0), vec3(0.0, -4.0, 2.0));
const vec3 WORLD_LIGHT_COLOR[3] = vec3[3](vec3(1.0), vec3(0.462077, 0.597202, 1.0), vec3(1.0, 0.871367, 0.610496));
const float WORLD_LIGHT_POWER[3] = float[3](4.0, 3.0, 2.0);
const vec3 WORLD_SKY = vec3(0.863157, 0.887923, 1.0);
const vec3 WORLD_GROUND = vec3(0.015996, 0.014444, 0.023153);
const float WORLD_HEMISPHERE_POWER = 2.0;
const float WORLD_DIELECTRIC = 0.04;
const float WORLD_MIN_ROUGHNESS = 0.0525;
const float WORLD_EXPOSURE = 1.12 / 0.6;
const float WORLD_SRGB_CUTOFF = 0.04045;
const float WORLD_LINEAR_CUTOFF = 0.0031308;
const float WORLD_SRGB_SLOPE = 12.92;
const float WORLD_SRGB_SCALE = 1.055;
const float WORLD_SRGB_OFFSET = 0.055;
const float WORLD_SRGB_POWER = 2.4;
const vec2 WORLD_FRESNEL_EXP = vec2(-5.55473, -6.98316);
const mat3 WORLD_ACES_INPUT = mat3(0.59719, 0.07600, 0.02840, 0.35458, 0.90834, 0.13383, 0.04823, 0.01566, 0.83777);
const mat3 WORLD_ACES_OUTPUT = mat3(1.60475, -0.10208, -0.00327, -0.53108, 1.10813, -0.07276, -0.07367, -0.00605, 1.07602);
const vec2 WORLD_ACES_NUMERATOR = vec2(0.0245786, 0.000090537);
const vec3 WORLD_ACES_DENOMINATOR = vec3(0.983729, 0.4329510, 0.238081);

float world_ease(float t) {
    float p = clamp(t, 0.0, 1.0);
    return p < 0.5 ? WORLD_EASE_SCALE * pow(p, WORLD_EASE_POWER)
        : 1.0 - pow(-2.0 * p + 2.0, WORLD_EASE_POWER) * 0.5;
}

float world_bell(float t, vec3 interval) {
    return t < interval.y
        ? 1.0 - pow(1.0 - studio_span(t, interval.xy), WORLD_EASE_POWER)
        : 1.0 - world_ease(studio_span(t, interval.yz));
}

mat3 world_rotation(vec3 angle) {
    vec3 co = cos(angle);
    vec3 si = sin(angle);
    mat3 around_x = mat3(1.0, 0.0, 0.0, 0.0, co.x, si.x, 0.0, -si.x, co.x);
    mat3 around_y = mat3(co.y, 0.0, -si.y, 0.0, 1.0, 0.0, si.y, 0.0, co.y);
    mat3 around_z = mat3(co.z, si.z, 0.0, -si.z, co.z, 0.0, 0.0, 0.0, 1.0);
    return around_x * around_y * around_z;
}

vec3 world_ray(vec2 uv) {
    float distance = WORLD_HEIGHT * 0.5 / tan(WORLD_HALF_FOV);
    return normalize(vec3((uv.x - 0.5) * WORLD_HEIGHT * u_res.x / u_res.y, (0.5 - uv.y) * WORLD_HEIGHT, -distance));
}

vec4 world_box(vec3 origin, vec3 ray, vec3 half_size) {
    vec3 direction = mix(vec3(-1.0), vec3(1.0), greaterThanEqual(ray, vec3(0.0)));
    vec3 inverse = direction / max(abs(ray), vec3(WORLD_EPSILON));
    vec3 first = (-half_size - origin) * inverse;
    vec3 last = (half_size - origin) * inverse;
    vec3 near_face = min(first, last);
    vec3 far_face = max(first, last);
    float enter = max(max(near_face.x, near_face.y), near_face.z);
    float leave = min(min(far_face.x, far_face.y), far_face.z);
    if (leave < max(enter, WORLD_NEAR)) return vec4(0.0, 0.0, 0.0, WORLD_FAR);
    bool inside = enter < WORLD_NEAR;
    float distance = inside ? leave : enter;
    vec3 face = inside ? far_face : near_face;
    vec3 normal = vec3(0.0);
    if (face.x == distance) normal.x = direction.x;
    else if (face.y == distance) normal.y = direction.y;
    else normal.z = direction.z;
    return vec4(inside ? normal : -normal, distance);
}

float world_plane(vec3 origin, vec3 ray, vec2 size) {
    if (abs(ray.z) < WORLD_EPSILON) return WORLD_FAR;
    float distance = -origin.z / ray.z;
    vec2 p = origin.xy + ray.xy * distance;
    return distance >= WORLD_NEAR && all(lessThanEqual(abs(p), size * 0.5)) ? distance : WORLD_FAR;
}

vec3 world_linear(vec3 color) {
    return mix(pow((color + WORLD_SRGB_OFFSET) / WORLD_SRGB_SCALE, vec3(WORLD_SRGB_POWER)), color / WORLD_SRGB_SLOPE, lessThanEqual(color, vec3(WORLD_SRGB_CUTOFF)));
}

vec3 world_output(vec3 color) {
    vec3 mapped = WORLD_ACES_INPUT * (color * WORLD_EXPOSURE);
    vec3 numerator = mapped * (mapped + WORLD_ACES_NUMERATOR.x) - WORLD_ACES_NUMERATOR.y;
    vec3 denominator = mapped * (WORLD_ACES_DENOMINATOR.x * mapped + WORLD_ACES_DENOMINATOR.y) + WORLD_ACES_DENOMINATOR.z;
    mapped = clamp(WORLD_ACES_OUTPUT * (numerator / denominator), 0.0, 1.0);
    return mix(WORLD_SRGB_SCALE * pow(mapped, vec3(1.0 / WORLD_SRGB_POWER)) - WORLD_SRGB_OFFSET, mapped * WORLD_SRGB_SLOPE, lessThanEqual(mapped, vec3(WORLD_LINEAR_CUTOFF)));
}

vec3 world_light(vec3 normal, vec3 view, vec3 light, vec3 specular, float roughness) {
    vec3 half_direction = normalize(light + view);
    float dot_light = max(dot(normal, light), 0.0);
    float dot_view = max(dot(normal, view), 0.0);
    float dot_half = max(dot(normal, half_direction), 0.0);
    float dot_view_half = max(dot(view, half_direction), 0.0);
    float alpha = roughness * roughness;
    float alpha_squared = alpha * alpha;
    float fresnel = exp2((WORLD_FRESNEL_EXP.x * dot_view_half + WORLD_FRESNEL_EXP.y) * dot_view_half);
    float geometry_light = dot_view * sqrt(alpha_squared + (1.0 - alpha_squared) * dot_light * dot_light);
    float geometry_view = dot_light * sqrt(alpha_squared + (1.0 - alpha_squared) * dot_view * dot_view);
    float visibility = 0.5 / max(geometry_light + geometry_view, WORLD_EPSILON);
    float denominator = dot_half * dot_half * (alpha_squared - 1.0) + 1.0;
    float distribution = alpha_squared / (STUDIO_PI * denominator * denominator);
    return mix(specular, vec3(1.0), fresnel) * visibility * distribution;
}

vec3 world_material(vec3 normal, vec3 view, vec3 color, vec3 finish) {
    vec3 linear = world_linear(color);
    vec3 diffuse = linear * (1.0 - finish.y);
    vec3 specular = mix(vec3(WORLD_DIELECTRIC), linear, finish.y);
    float roughness = max(finish.x, WORLD_MIN_ROUGHNESS);
    vec3 irradiance = mix(WORLD_GROUND, WORLD_SKY, dot(normal, vec3(0.0, 1.0, 0.0)) * 0.5 + 0.5) * WORLD_HEMISPHERE_POWER;
    vec3 shaded = irradiance * diffuse / STUDIO_PI;
    vec3 coat = vec3(0.0);
    for (int idx = 0; idx < 3; ++idx) {
        vec3 light = normalize(WORLD_LIGHT_POS[idx]);
        float dot_light = max(dot(normal, light), 0.0);
        vec3 incident = WORLD_LIGHT_COLOR[idx] * WORLD_LIGHT_POWER[idx] * dot_light;
        shaded += incident * (diffuse / STUDIO_PI + world_light(normal, view, light, specular, roughness));
        coat += incident * world_light(normal, view, light, vec3(WORLD_DIELECTRIC), WORLD_MIN_ROUGHNESS);
    }
    shaded += studio_indirect(normal, view, diffuse, specular, roughness);
    float dot_view = max(dot(normal, view), 0.0);
    vec2 coat_response = studio_dfg(dot_view, WORLD_MIN_ROUGHNESS);
    coat += studio_reflection(normal, view, WORLD_MIN_ROUGHNESS) * (WORLD_DIELECTRIC * coat_response.x + coat_response.y);
    float coat_fresnel = mix(WORLD_DIELECTRIC, 1.0, exp2((WORLD_FRESNEL_EXP.x * dot_view + WORLD_FRESNEL_EXP.y) * dot_view));
    return world_output(shaded * (1.0 - finish.z * coat_fresnel) + coat * finish.z);
}
