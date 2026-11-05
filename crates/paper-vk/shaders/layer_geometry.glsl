const uint UV_TRANSFORM = 4u;
const uint UV_PADDING = 8u;
const float IMAGE_UV_INSET = 0.15;
vec4 layer_position(vec2 position) {
    vec4 clip;
    if (pc.angle == 0.0) {
        vec2 clip_scale = 2.0 / pc.canvas;
        precise vec2 xy = position * (pc.rect.zw * clip_scale) + (pc.rect.xy * clip_scale - 1.0);
        clip = vec4(xy, 0.0, 1.0);
    } else {
        vec2 scaled = position * pc.rect.zw;
        float s = sin(pc.angle);
        float c = cos(pc.angle);
        vec2 rotated = vec2(scaled.x * c - scaled.y * s, scaled.x * s + scaled.y * c);
        vec2 pos = (pc.rect.xy + rotated) / pc.canvas;
        clip = vec4(pos * 2.0 - 1.0, 0.0, 1.0);
    }
    if (any(notEqual(pc.projection_w, vec4(0.0)))) {
        vec4 local = vec4(position, 0.0, 1.0);
        vec3 depth = vec3(pc.projection_x.z, pc.projection_y.z, pc.projection_w.z);
        clip = vec4(dot(pc.projection_x, local), dot(pc.projection_y, local), dot(depth, vec3(position, 1.0)), dot(pc.projection_w, local));
    }
    return clip;
}
vec2 layer_uv_size(vec2 corner, vec2 size) {
    if ((uint(pc.pad) & UV_TRANSFORM) != 0u) {
        precise vec2 atlas_uv = corner;
        if ((uint(pc.pad) & UV_PADDING) != 0u) {
            vec2 inset = vec2(IMAGE_UV_INSET) / size;
            atlas_uv = inset + corner * (1.0 - inset * 2.0);
        }
        precise vec2 transformed = pc.uv.xy + atlas_uv.x * pc.uv_axes.xy + atlas_uv.y * pc.uv_axes.zw;
        return transformed;
    }
    return pc.uv.xy + corner * pc.uv.zw;
}

vec2 layer_uv(vec2 corner) {
    return layer_uv_size(corner, vec2(textureSize(tex, 0)));
}
