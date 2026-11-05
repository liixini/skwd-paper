const int SLAT_COUNT = 12;
const float SLAT_DEPTH = 0.045;
const float SLAT_FACE_GAP = 0.001;
const float SLAT_FACE_OFFSET = SLAT_DEPTH * 0.5 + SLAT_FACE_GAP;
const vec3 SLAT_SPLIT = vec3(0.075, 0.31, 0.87);
const vec2 SLAT_ASSEMBLY_TURN = vec2(0.18, 0.84);
const vec2 SLAT_DRIFT = vec2(0.15, 0.86);
const vec2 SLAT_SWAY = vec2(0.12, 0.84);
const vec2 SLAT_SPIN = vec2(0.12, 0.76);
const float SLAT_STAGGER = 0.008;
const float SLAT_SPREAD = 0.24;
const vec2 SLAT_SCATTER_PHASE = vec2(2.35, 1.89);
const vec2 SLAT_SCATTER = vec2(0.11, 0.2);
const vec2 SLAT_TILT = vec2(0.14, 0.085);
const vec2 SLAT_ASSEMBLY_TILT = vec2(-0.17, 0.12);
const vec3 SLAT_ASSEMBLY_LIFT = vec3(0.075, 0.13, 0.18);
const float SLAT_CAMERA_PULL = 0.37;
const vec3 SLAT_BACKGROUND = vec3(233.0, 229.0, 221.0) / 255.0;
const vec3 SLAT_EDGE = vec3(233.0, 228.0, 216.0) / 255.0;
const vec3 SLAT_FINISH = vec3(0.28, 0.05, 0.32);

void main() {
    float t = u_progress;
    if (t <= SLAT_SPLIT.x) {
        frag = texture(u_tex_old, v_uv);
        return;
    }
    if (t >= SLAT_SPLIT.z) {
        frag = texture(u_tex_new, v_uv);
        return;
    }
    float width = WORLD_HEIGHT * u_res.x / u_res.y;
    float slat_width = width / float(SLAT_COUNT);
    float split = world_bell(t, SLAT_SPLIT);
    float turn = world_ease(studio_span(t, SLAT_ASSEMBLY_TURN));
    float lift = sin(STUDIO_PI * studio_span(t, SLAT_SPLIT.xz));
    float drift = sin(STUDIO_PI * studio_span(t, SLAT_DRIFT));
    mat3 assembly = world_rotation(vec3(SLAT_ASSEMBLY_TILT.x * lift, STUDIO_PI * turn, SLAT_ASSEMBLY_TILT.y * drift));
    vec3 position = vec3(width * SLAT_ASSEMBLY_LIFT.x * sin(2.0 * STUDIO_PI * studio_span(t, SLAT_SWAY)) * split, SLAT_ASSEMBLY_LIFT.y * lift, SLAT_ASSEMBLY_LIFT.z * lift);
    vec3 camera = vec3(0.0, 0.0, WORLD_HEIGHT * 0.5 / tan(WORLD_HALF_FOV) + SLAT_FACE_OFFSET + SLAT_CAMERA_PULL * split);
    vec3 ray = world_ray(v_uv);
    vec3 origin = transpose(assembly) * (camera - position);
    vec3 direction = transpose(assembly) * ray;
    float nearest = WORLD_FAR;
    vec3 normal = vec3(0.0);
    vec3 color = SLAT_BACKGROUND;
    bool edge = false;
    for (int idx = 0; idx < SLAT_COUNT; ++idx) {
        float slat = float(idx);
        float home = (slat + 0.5) * slat_width - width * 0.5;
        float phase = (slat - float(SLAT_COUNT - 1) * 0.5) / float(SLAT_COUNT - 1);
        float spin = world_ease(studio_span(t, SLAT_SPIN + slat * SLAT_STAGGER));
        vec3 center = vec3(home * (1.0 + SLAT_SPREAD * split), sin(slat * SLAT_SCATTER_PHASE.x) * SLAT_SCATTER.x * split, sin(slat * SLAT_SCATTER_PHASE.y) * SLAT_SCATTER.y * split);
        mat3 rotation = world_rotation(vec3(phase * SLAT_TILT.x * split, 2.0 * STUDIO_PI * spin, phase * SLAT_TILT.y * split));
        vec3 local_origin = transpose(rotation) * (origin - center);
        vec3 local_ray = transpose(rotation) * direction;
        vec4 body = world_box(local_origin, local_ray, vec3(slat_width, WORLD_HEIGHT, SLAT_DEPTH) * 0.5);
        if (body.w < nearest) {
            nearest = body.w;
            normal = assembly * rotation * body.xyz;
            edge = true;
        }
        float side = local_ray.z < 0.0 ? 1.0 : -1.0;
        float face = world_plane(local_origin - vec3(0.0, 0.0, side * SLAT_FACE_OFFSET), local_ray, vec2(slat_width, WORLD_HEIGHT));
        if (face < nearest) {
            nearest = face;
            vec2 hit = local_origin.xy + local_ray.xy * face;
            vec2 uv = vec2((slat + hit.x / slat_width + 0.5) / float(SLAT_COUNT), 0.5 - hit.y / WORLD_HEIGHT);
            color = side > 0.0 ? texture(u_tex_old, uv).rgb : texture(u_tex_new, vec2(1.0 - uv.x, uv.y)).rgb;
            edge = false;
        }
    }
    if (edge) color = world_material(normal, -ray, SLAT_EDGE, SLAT_FINISH);
    frag = vec4(color, 1.0);
}
