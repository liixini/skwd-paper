const vec3 CUTOUT_BLUE = vec3(0.72549020, 0.78431373, 1.0);
const vec3 CUTOUT_ORANGE = vec3(1.0, 0.32941176, 0.18039216);
const vec2 CUTOUT_ENTER = vec2(0.025, 0.17);
const vec2 CUTOUT_SHIFT = vec2(0.32, 0.44);
const vec2 CUTOUT_RELEASE = vec2(0.68, 0.965);
const vec2 CUTOUT_MEDIA = vec2(0.31, 0.37);
const vec2 CUTOUT_COLOUR = vec2(0.51, 0.57);
const vec2 CUTOUT_END = vec2(0.91, 0.985);
const float CUTOUT_LOBES = 12.0;
const float CUTOUT_SCALLOP = 0.075;
const float CUTOUT_RADIUS = 0.455;
const float CUTOUT_CROP = 0.11;
const vec2 CUTOUT_CENTER = vec2(0.44, 0.52);
const vec2 CUTOUT_CENTER_SHIFT = vec2(0.12, -0.04);
const vec2 CUTOUT_ROTATION = vec2(-0.095, 0.155);
const vec2 CUTOUT_SHAPE_SCALE = vec2(0.79, 0.97);
const float CUTOUT_SHIFT_SCALE = 0.035;
const vec2 CUTOUT_CROP_X = vec2(-0.025, 0.045);
const float CUTOUT_CROP_Y = 0.015;
const float CUTOUT_MONO_OLD = 1.3;
const float CUTOUT_MONO_NEW = 1.25;
const float CUTOUT_NEW_SCALE = 1.11;

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

    float enter = studio_ease(studio_span(t, CUTOUT_ENTER));
    float shift = studio_settle(studio_span(t, CUTOUT_SHIFT));
    float release = studio_ease(studio_span(t, CUTOUT_RELEASE));
    float media = studio_ease(studio_span(t, CUTOUT_MEDIA));
    float colour = studio_ease(studio_span(t, CUTOUT_COLOUR));
    float unit = min(u_res.x, u_res.y);
    vec2 p = v_uv * u_res;

    vec4 result = texture(u_tex_old, v_uv);
    result = studio_over(result, vec4(STUDIO_IVORY, 1.0), enter);
    if (p.x >= u_res.x * (1.0 - media)) {
        result = studio_over(result, vec4(CUTOUT_BLUE, 1.0), enter);
    }
    if (p.y >= u_res.y * (1.0 - colour)) {
        result = studio_over(result, vec4(CUTOUT_ORANGE, 1.0), enter);
    }

    float radius = mix(unit * CUTOUT_RADIUS, length(u_res), release);
    vec2 center = vec2(
        u_res.x * (CUTOUT_CENTER.x + CUTOUT_CENTER_SHIFT.x * shift) * (1.0 - release) + u_res.x * 0.5 * release,
        u_res.y * (CUTOUT_CENTER.y + CUTOUT_CENTER_SHIFT.y * shift) * (1.0 - release) + u_res.y * 0.5 * release
    );
    float rotation = (CUTOUT_ROTATION.x + CUTOUT_ROTATION.y * shift) * (1.0 - release) * enter;
    vec2 mask_scale = vec2(
        (mix(1.0, u_res.x / unit * CUTOUT_SHAPE_SCALE.x, enter) * (1.0 - CUTOUT_SHIFT_SCALE * shift) * (1.0 - release) + release),
        mix(1.0, CUTOUT_SHAPE_SCALE.y, enter)
    );
    vec2 shape = studio_turn(p - center, -rotation) / mask_scale;
    float angle = atan(shape.y, shape.x);
    float edge = radius * (1.0 + CUTOUT_SCALLOP * cos(angle * CUTOUT_LOBES));
    float signed_edge = length(shape) - edge;
    float coverage = 1.0 - smoothstep(-fwidth(signed_edge), fwidth(signed_edge), signed_edge);
    if (coverage > 0.0) {
        vec2 crop = vec2(
            (CUTOUT_CROP_X.x + CUTOUT_CROP_X.y * shift) * enter * (1.0 - release),
            CUTOUT_CROP_Y * enter * (1.0 - release)
        );
        vec4 outside = result;
        vec4 old_image = studio_old(v_uv, 1.0 + CUTOUT_CROP * enter, crop);
        result = old_image;
        result = studio_over(result, vec4(studio_mono(old_image.rgb, CUTOUT_MONO_OLD), old_image.a), enter);
        if (p.x >= u_res.x * (1.0 - media)) {
            vec4 new_image = studio_new(v_uv, mix(CUTOUT_NEW_SCALE, 1.0, release), crop);
            result = vec4(studio_mono(new_image.rgb, CUTOUT_MONO_NEW), new_image.a);
        }
        if (p.y >= u_res.y * (1.0 - colour)) {
            result = studio_new(v_uv, mix(CUTOUT_NEW_SCALE, 1.0, release), crop);
        }
        result = studio_over(outside, result, coverage);
    }
    frag = studio_over(result, texture(u_tex_new, v_uv), studio_ease(studio_span(t, CUTOUT_END)));
}
