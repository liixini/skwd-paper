const vec2 RELAY_ENTER = vec2(0.04, 0.27);
const vec2 RELAY_REFRAME = vec2(0.36, 0.5);
const vec2 RELAY_BREATH = vec2(0.55, 0.68);
const vec2 RELAY_RELEASE = vec2(0.73, 0.97);
const vec2 RELAY_PRESENCE = vec2(0.04, 0.23);
const vec2 RELAY_ALIGN = vec2(0.19, 0.29);
const vec2 RELAY_CARRY = vec2(0.29, 0.49);
const vec2 RELAY_HANDOFF = vec2(0.49, 0.57);
const vec2 RELAY_EXIT = vec2(0.59, 0.73);
const vec2 RELAY_BAND = vec2(0.64, 0.48);
const float RELAY_BAND_TOP = 0.26;
const float RELAY_CAMERA_SCALE = 1.22;
const vec3 RELAY_SCALE = vec3(0.075, 0.09, -0.025);
const vec3 RELAY_OFFSET = vec3(0.045, -0.025, 0.008);
const float RELAY_VERTICAL = -0.018;
const float RELAY_SEAM_ALPHA = 0.55;
const float RELAY_LANDING = 0.18;
const float RELAY_EXIT_DISTANCE = 1.15;
const float RELAY_CAMERA_Y = 0.035;

void main() {
    float t = u_progress;
    if (t <= STUDIO_FIRST) {
        frag = texture(u_tex_old, v_uv);
        return;
    }
    if (t >= STUDIO_LAST) {
        frag = texture(u_tex_new, v_uv);
        return;
    }

    float enter = studio_settle(studio_span(t, RELAY_ENTER));
    float reframe = studio_settle(studio_span(t, RELAY_REFRAME));
    float breath = studio_ease(studio_span(t, RELAY_BREATH));
    float release = studio_ease(studio_span(t, RELAY_RELEASE));
    float presence = studio_ease(studio_span(t, RELAY_PRESENCE)) * (1.0 - release);
    float seam = u_res.x * mix(1.0, 0.5, enter) * (1.0 - release);
    float scale = 1.0 + presence * (RELAY_SCALE.x + RELAY_SCALE.y * reframe + RELAY_SCALE.z * breath);
    float align = 1.0 - studio_ease(studio_span(t, RELAY_ALIGN));
    float offset = presence * (RELAY_OFFSET.x * align + RELAY_OFFSET.y * reframe + RELAY_OFFSET.z * breath);
    vec2 camera = vec2(offset, RELAY_VERTICAL * presence * reframe);
    vec2 p = v_uv * u_res;

    vec4 result = studio_old(v_uv, scale, camera);
    if (p.x >= seam) {
        result = studio_new(v_uv, scale, vec2(-camera.x, camera.y));
    }
    if (abs(p.x - seam) <= 0.5) {
        result = studio_over(result, vec4(STUDIO_IVORY, 1.0), presence * RELAY_SEAM_ALPHA);
    }

    if (t > RELAY_CARRY.x && t < RELAY_EXIT.y) {
        float carry = studio_settle(studio_span(t, RELAY_CARRY));
        float handoff = studio_ease(studio_span(t, RELAY_HANDOFF));
        float depart = studio_ease(studio_span(t, RELAY_EXIT));
        vec2 size = u_res * RELAY_BAND;
        vec2 origin = vec2(mix(-size.x, u_res.x * RELAY_LANDING, carry) + depart * u_res.x * RELAY_EXIT_DISTANCE,
                           u_res.y * RELAY_BAND_TOP);
        if (all(greaterThanEqual(p, origin)) && all(lessThan(p, origin + size))) {
            vec2 crop = vec2((origin.x + size.x * 0.5 - u_res.x * 0.5) / u_res.x, RELAY_CAMERA_Y);
            result = studio_old(v_uv, RELAY_CAMERA_SCALE, crop);
            if (p.x < origin.x + size.x * handoff) {
                result = studio_new(v_uv, RELAY_CAMERA_SCALE, crop);
            }
        }
    }
    frag = result;
}
