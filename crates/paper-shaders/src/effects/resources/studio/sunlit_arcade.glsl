uniform vec3 u_studio_surface;
uniform vec3 u_studio_surface_variant;
uniform vec3 u_studio_primary_container;
uniform vec3 u_studio_secondary_container;

const float ARCADE_GAP = 5.0;
const vec3 ARCADE_CAMERA_SWAY = vec3(0.25, 1.7, -0.035);
const vec3 ARCADE_FLOOR_CENTER = vec3(0.0, -3.3, -10.0);
const vec3 ARCADE_FLOOR_SIZE = vec3(50.0, 0.25, 60.0);
const vec3 ARCADE_FLOOR_FINISH = vec3(0.23, 0.35, 0.0);
const vec3 ARCADE_COLUMN_FINISH = vec3(0.9, 0.0, 0.0);
const vec3 ARCADE_STONE_FINISH = vec3(0.8, 0.0, 0.0);
const int ARCADE_COLUMN_COUNT = 3;
const float ARCADE_COLUMN_START = 1.0;
const float ARCADE_COLUMN_SPACING = 1.5;
const float ARCADE_FRONT = 3.0;
const float ARCADE_CAPITAL_HEIGHT = 2.65;
const vec3 ARCADE_COLUMN_SIZE = vec3(0.7, 12.0, 1.8);
const vec3 ARCADE_BASE_SIZE = vec3(1.1, 0.5, 2.1);
const vec3 ARCADE_CAPITAL_SIZE = vec3(1.1, 0.4, 2.1);
const vec3 ARCADE_CORNICE_CENTER = vec3(2.5, 5.0, 3.0);
const vec3 ARCADE_CORNICE_SIZE = vec3(5.0, 4.0, 2.2);
const int ARCADE_DENTIL_COUNT = 12;
const float ARCADE_DENTIL_SPACING = 0.3;
const vec2 ARCADE_DENTIL_CENTER = vec2(3.3, 4.2);
const vec3 ARCADE_DENTIL_SIZE = vec3(0.12, 0.6, 0.2);
const int ARCADE_SURFACE_FLOOR = 0;
const int ARCADE_SURFACE_COLUMN = 1;
const int ARCADE_SURFACE_STONE = 2;

struct ArcadeHit {
    float distance;
    vec3 normal;
    int surface;
};

void arcade_box(vec3 origin, vec3 ray, vec3 size, int surface, inout ArcadeHit nearest) {
    vec4 hit = world_box(origin, ray, size * 0.5);
    if (hit.w >= nearest.distance) return;
    nearest = ArcadeHit(hit.w, hit.xyz, surface);
}

void main() {
    float t = u_progress;
    if (t <= 0.0) {
        frag = texture(u_tex_old, v_uv);
        return;
    }
    if (t >= 1.0) {
        frag = texture(u_tex_new, v_uv);
        return;
    }
    float width = WORLD_HEIGHT * u_res.x / u_res.y;
    float distance = width + ARCADE_GAP;
    float progress = world_ease(t);
    float swoop = sin(progress * STUDIO_PI);
    vec3 origin = vec3(distance * progress, swoop * ARCADE_CAMERA_SWAY.x, WORLD_HEIGHT * 0.5 / tan(WORLD_HALF_FOV) - swoop * ARCADE_CAMERA_SWAY.y);
    vec3 ray = world_rotation(vec3(0.0, 0.0, sin(progress * STUDIO_PI * 2.0) * ARCADE_CAMERA_SWAY.z)) * world_ray(v_uv);
    vec2 photo_size = vec2(width, WORLD_HEIGHT);
    float photo_a = world_plane(origin, ray, photo_size);
    float photo_b = world_plane(origin - vec3(distance, 0.0, 0.0), ray, photo_size);
    float photo = min(photo_a, photo_b);
    ArcadeHit nearest = ArcadeHit(photo, vec3(0.0), -1);
    arcade_box(origin - ARCADE_FLOOR_CENTER, ray, ARCADE_FLOOR_SIZE, ARCADE_SURFACE_FLOOR, nearest);
    for (int idx = 0; idx < ARCADE_COLUMN_COUNT; ++idx) {
        float x = width * 0.5 + ARCADE_COLUMN_START + float(idx) * ARCADE_COLUMN_SPACING;
        arcade_box(origin - vec3(x, 0.0, ARCADE_FRONT), ray, ARCADE_COLUMN_SIZE, ARCADE_SURFACE_COLUMN, nearest);
        arcade_box(origin - vec3(x, -ARCADE_CAPITAL_HEIGHT, ARCADE_FRONT), ray, ARCADE_BASE_SIZE, ARCADE_SURFACE_STONE, nearest);
        arcade_box(origin - vec3(x, ARCADE_CAPITAL_HEIGHT, ARCADE_FRONT), ray, ARCADE_CAPITAL_SIZE, ARCADE_SURFACE_STONE, nearest);
    }
    vec3 cornice = ARCADE_CORNICE_CENTER + vec3(width * 0.5, 0.0, 0.0);
    arcade_box(origin - cornice, ray, ARCADE_CORNICE_SIZE, ARCADE_SURFACE_COLUMN, nearest);
    for (int idx = 0; idx < ARCADE_DENTIL_COUNT; ++idx) {
        float x = cornice.x + (float(idx) - float(ARCADE_DENTIL_COUNT - 1) * 0.5) * ARCADE_DENTIL_SPACING;
        arcade_box(origin - vec3(x, ARCADE_DENTIL_CENTER), ray, ARCADE_DENTIL_SIZE, ARCADE_SURFACE_STONE, nearest);
    }
    vec3 color = u_studio_surface;
    if (nearest.surface >= 0) {
        vec3 pigment = nearest.surface == ARCADE_SURFACE_FLOOR ? u_studio_surface_variant
            : nearest.surface == ARCADE_SURFACE_COLUMN ? u_studio_primary_container : u_studio_secondary_container;
        vec3 finish = nearest.surface == ARCADE_SURFACE_FLOOR ? ARCADE_FLOOR_FINISH
            : nearest.surface == ARCADE_SURFACE_COLUMN ? ARCADE_COLUMN_FINISH : ARCADE_STONE_FINISH;
        color = world_material(nearest.normal, -ray, pigment, finish);
    } else if (photo < WORLD_FAR) {
        vec2 hit = origin.xy + ray.xy * photo;
        bool incoming = photo_b < photo_a;
        if (incoming) hit.x -= distance;
        vec2 uv = vec2(hit.x / width + 0.5, 0.5 - hit.y / WORLD_HEIGHT);
        color = incoming ? texture(u_tex_new, uv).rgb : texture(u_tex_old, uv).rgb;
    }
    frag = vec4(color, 1.0);
}
