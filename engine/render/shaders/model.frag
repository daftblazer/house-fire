#version 450
// A material's layers combined with the surface colour (the original's texture combiner
// stages for the few shapes the game uses, `docs/assets.md`, "The renderer").

layout(location = 0) in vec4 v_colour;
layout(location = 1) in vec2 v_uv;
layout(location = 2) flat in int v_draw;
layout(location = 3) in vec2 v_sphere;

struct Draw {
    vec4 mv[3];
    vec4 nm[3];
    vec4 ambient;
    vec4 dir_colour[2];
    vec4 dir_to_light[2];
    vec4 point_colour[4];
    vec4 point_position[4];
    ivec4 counts;
    vec4 params;
};
layout(std430, set = 0, binding = 0) readonly buffer Draws { Draw draws[]; };

layout(set = 1, binding = 0) uniform sampler2D layer0;
layout(set = 1, binding = 1) uniform sampler2D layer1;

layout(location = 0) out vec4 out_colour;
layout(location = 1) out uint out_id;

// combine codes (counts.z)
const int VERTEX = 0, MODULATE = 1, MODULATE_UNLIT = 2, CROSSFADE = 3, OVER_PREV = 4, OVER_TEX = 5, ADD = 6;

void main() {
    Draw d = draws[v_draw];
    int combine = d.counts.z;
    vec4 ras = v_colour;
    vec4 t0 = texture(layer0, v_uv);
    vec4 c;
    if (combine == VERTEX) {
        c = ras;
    } else if (combine == MODULATE) {
        c = clamp(2.0 * t0 * ras, 0.0, 1.0);
    } else if (combine == MODULATE_UNLIT) {
        c = clamp(2.0 * t0, 0.0, 1.0);
    } else {
        vec4 t1 = texture(layer1, v_uv);
        if (combine == CROSSFADE) {
            vec4 a = clamp(t0 * ras, 0.0, 1.0);
            vec4 b = clamp(t1 * ras, 0.0, 1.0);
            vec4 m = mix(a, b, d.params.x);
            c = vec4(clamp(2.0 * m.rgb, 0.0, 1.0), m.a);
        } else {
            vec4 first = (d.counts.w & 2) != 0 ? t0 : t0 * ras;
            if (combine == ADD) {
                c = vec4(clamp(2.0 * t1.rgb * first.a, 0.0, 1.0), first.a);
            } else {
                float by = combine == OVER_TEX ? t1.a : first.a;
                c = vec4(clamp(2.0 * mix(first.rgb, t1.rgb, by), 0.0, 1.0), first.a);
            }
        }
    }
    if ((d.counts.w & 4) != 0) {
        int by = (d.counts.w >> 3) & 3;
        float weight = by == 0 ? 1.0 : (by == 1 ? c.a : d.params.z);
        c.rgb = clamp(c.rgb + texture(layer1, v_sphere).rgb * weight, 0.0, 1.0);
    }
    if ((d.counts.w & 1) != 0 && c.a < d.params.y) discard;
    out_colour = c;
    out_id = uint(v_draw) + 1u;
}
