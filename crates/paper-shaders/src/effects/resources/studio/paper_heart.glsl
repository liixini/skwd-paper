const vec2 HEART_RISE = vec2(0.02, 0.22);
const vec2 HEART_EXPAND = vec2(0.47, 0.94);
const vec2 HEART_GROW = vec2(0.1, 0.48);
const vec2 HEART_DRIFT = vec2(0.12, 0.48);
const vec2 HEART_PRINT = vec2(0.265, 0.345);
const vec2 HEART_COLOUR = vec2(0.435, 0.515);
const vec2 HEART_VISIBLE = vec2(0.03, 0.16);
const vec2 HEART_DRAW = vec2(0.28, 0.43);
const vec2 HEART_END = vec2(0.94, 0.985);
const vec3 HEART_WASH = vec3(0.10588235, 0.09411765, 0.14117647);
const vec3 HEART_RED = vec3(0.91372549, 0.29411765, 0.25882353);
const vec3 HEART_PINK = vec3(1.0, 0.74509804, 0.70196078);
const int HEART_SEGMENTS = 48;
const vec2 HEART_TIP = vec2(0.0, 0.83);
const vec2 HEART_CLEFT = vec2(0.0, -0.48);
const vec2 HEART_LEFT_A = vec2(-1.55, -0.05);
const vec2 HEART_LEFT_B = vec2(-0.87, -1.22);
const vec2 HEART_RIGHT_A = vec2(0.87, -1.22);
const vec2 HEART_RIGHT_B = vec2(1.55, -0.05);
const vec2 HEART_STAGE = vec2(0.08, 0.78);
const vec2 HEART_CENTER = vec2(0.4, 0.54);
const vec2 HEART_CENTER_MOVE = vec2(0.1, -0.045);
const vec2 HEART_ENTRY = vec2(0.35, 0.9);
const vec2 HEART_RADIUS = vec2(0.25, 0.43);
const float HEART_FINAL_RADIUS = 1.5;
const vec2 HEART_ROTATION = vec2(-0.14, 0.11);
const float HEART_ROTATION_WAVE = 7.0;
const float HEART_CAMERA_ZOOM = 0.09;
const float HEART_WASH_ALPHA = 0.44;
const vec3 HEART_WASH_PHASE = vec3(0.15, 0.43, 0.94);
const vec2 HEART_SHADOW_OFFSET = vec2(0.015, 0.025);
const float HEART_SHADOW_ALPHA = 0.33333333;
const float HEART_PRINT_CONTRAST = 1.15;
const float HEART_PAPER_LINE = 0.006;
const float HEART_INK_LINE = 0.012;
const float HEART_INNER_OFFSET = 0.04;
const float HEART_INNER_SCALE = 0.87;
const float HEART_INNER_ALPHA = 0.22;
const float HEART_OUTER_SCALE = 1.045;
const float HEART_PATH_LENGTH = 6.2;

vec2 heart_curve(float t, vec2 p0, vec2 p1, vec2 p2, vec2 p3) {
    float q = 1.0 - t;
    return q * q * q * p0 + 3.0 * q * q * t * p1 + 3.0 * q * t * t * p2 + t * t * t * p3;
}

vec2 heart_point(float t) {
    if (t < 0.5) {
        float p = t * 2.0;
        return heart_curve(p, HEART_TIP, HEART_LEFT_A, HEART_LEFT_B, HEART_CLEFT);
    }
    float p = (t - 0.5) * 2.0;
    return heart_curve(p, HEART_CLEFT, HEART_RIGHT_A, HEART_RIGHT_B, HEART_TIP);
}

vec2 heart_edge(vec2 p, float radius) {
    vec2 q = p / radius;
    float closest = 100.0;
    float phase = 0.0;
    float travelled = 0.0;
    bool inside = false;
    vec2 previous = heart_point(0.0);
    for (int i = 1; i <= HEART_SEGMENTS; i++) {
        float t = float(i) / float(HEART_SEGMENTS);
        vec2 next = heart_point(t);
        vec2 edge = next - previous;
        float side = clamp(dot(q - previous, edge) / dot(edge, edge), 0.0, 1.0);
        float distance = length(q - previous - edge * side);
        if (distance < closest) {
            closest = distance;
            phase = (travelled + length(edge) * side) / HEART_PATH_LENGTH;
        }
        if ((previous.y > q.y) != (next.y > q.y)) {
            float cross_x = previous.x + (q.y - previous.y) * edge.x / edge.y;
            if (q.x < cross_x) inside = !inside;
        }
        travelled += length(edge);
        previous = next;
    }
    return vec2((inside ? -closest : closest) * radius, phase);
}

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

    float unit = min(u_res.x, u_res.y);
    float p = studio_span(t, HEART_STAGE);
    float rise = studio_out(studio_span(t, HEART_RISE));
    float expand = studio_ease(studio_span(t, HEART_EXPAND));
    float radius = mix(unit * mix(HEART_RADIUS.x, HEART_RADIUS.y, studio_out(studio_span(t, HEART_GROW))),
                       length(u_res) * HEART_FINAL_RADIUS, expand);
    vec2 center = vec2(
        u_res.x * (HEART_CENTER.x + HEART_CENTER_MOVE.x * studio_out(studio_span(t, HEART_DRIFT))) + (1.0 - rise) * u_res.x * HEART_ENTRY.x,
        u_res.y * (HEART_CENTER.y + HEART_CENTER_MOVE.y * sin(p * STUDIO_PI)) + (1.0 - rise) * u_res.y * HEART_ENTRY.y
    );
    float rotation = (HEART_ROTATION.x + sin(p * HEART_ROTATION_WAVE) * HEART_ROTATION.y) * (1.0 - expand);
    float print = studio_ease(studio_span(t, HEART_PRINT));
    float colour = studio_ease(studio_span(t, HEART_COLOUR));
    float visibility = studio_ease(studio_span(t, HEART_VISIBLE));
    vec2 local = studio_turn(v_uv * u_res - center, -rotation);

    vec4 result = studio_old(v_uv, 1.0 + HEART_CAMERA_ZOOM * p * (1.0 - expand), vec2(0.0));
    float wash = studio_pulse(t, HEART_WASH_PHASE) * HEART_WASH_ALPHA;
    result = studio_over(result, vec4(HEART_WASH, 1.0), wash);

    float paper = 1.0 - print;
    if (visibility == 0.0) {
        frag = studio_over(result, texture(u_tex_new, v_uv), studio_ease(studio_span(t, HEART_END)));
        return;
    }
    if (paper > 0.0) {
        vec2 shadow = heart_edge(local - unit * HEART_SHADOW_OFFSET, radius);
        float shadow_aa = fwidth(shadow.x);
        float shadow_fill = 1.0 - smoothstep(-shadow_aa, shadow_aa, shadow.x);
        result = studio_over(result, vec4(0.0, 0.0, 0.0, 1.0), visibility * paper * HEART_SHADOW_ALPHA * shadow_fill);
    }
    vec2 boundary = heart_edge(local, radius);
    float edge_aa = fwidth(boundary.x);
    float fill = 1.0 - smoothstep(-edge_aa, edge_aa, boundary.x);
    if (fill > 0.0) {
        vec4 body = vec4(HEART_RED, 1.0);
        vec4 next_image = texture(u_tex_new, v_uv);
        body = studio_over(body, vec4(studio_mono(next_image.rgb, HEART_PRINT_CONTRAST), next_image.a), print);
        body = studio_over(body, next_image, colour);
        result = studio_over(result, body, visibility * fill);
    }

    float paper_line = unit * HEART_PAPER_LINE;
    float paper_stroke = 1.0 - smoothstep(paper_line * 0.5 - edge_aa, paper_line * 0.5 + edge_aa, abs(boundary.x));
    result = studio_over(result, vec4(HEART_PINK, 1.0), visibility * paper * paper_stroke);
    if (paper > 0.0) {
        vec2 inner = heart_edge(local + radius * vec2(HEART_INNER_OFFSET), radius * HEART_INNER_SCALE);
        float inner_aa = fwidth(inner.x);
        float inner_stroke = 1.0 - smoothstep(paper_line * 0.5 - inner_aa, paper_line * 0.5 + inner_aa, abs(inner.x));
        result = studio_over(result, vec4(1.0), visibility * paper * HEART_INNER_ALPHA * inner_stroke);
    }
    float ink_line = unit * HEART_INK_LINE;
    float ink_stroke = 1.0 - smoothstep(ink_line * 0.5 - edge_aa, ink_line * 0.5 + edge_aa, abs(boundary.x));
    result = studio_over(result, vec4(STUDIO_IVORY, 1.0), visibility * print * ink_stroke);
    if (print > 0.0) {
        vec2 outer = heart_edge(local, radius * HEART_OUTER_SCALE);
        if (outer.y <= studio_out(studio_span(t, HEART_DRAW))) {
            float outer_aa = fwidth(outer.x);
            float outer_stroke = 1.0 - smoothstep(1.0 - outer_aa, 1.0 + outer_aa, abs(outer.x));
            result = studio_over(result, vec4(STUDIO_INK, 1.0), visibility * print * outer_stroke);
        }
    }
    frag = studio_over(result, texture(u_tex_new, v_uv), studio_ease(studio_span(t, HEART_END)));
}
