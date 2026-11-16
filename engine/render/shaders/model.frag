#version 450
// The surface colour (the lights summed per pixel) and a material's layers combined with it
// (the original's texture combiner stages for the few shapes the game uses,
// `docs/assets.md`, "The renderer").

layout(location = 0) in vec4 v_colour;
layout(location = 1) in vec2 v_uv;
layout(location = 2) flat in int v_draw;
layout(location = 3) in vec2 v_sphere;
layout(location = 4) in vec3 v_position;
layout(location = 5) in vec3 v_normal;
layout(location = 6) in vec2 v_uv1;

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
    vec4 extra;
    vec4 layer1[2];
    vec4 proj[2];
    ivec4 skin;
};
layout(std430, set = 0, binding = 0) readonly buffer Draws { Draw draws[]; };

layout(set = 1, binding = 0) uniform sampler2D layer0;
layout(set = 1, binding = 1) uniform sampler2D layer1;

layout(location = 0) out vec4 out_colour;

// GX's compare functions: never, less, equal, less or equal, greater, not equal, greater or
// equal, always.
bool compare(int f, float a, float r) {
    switch (f) {
        case 0: return false;
        case 1: return a < r;
        case 2: return a == r;
        case 3: return a <= r;
        case 4: return a > r;
        case 5: return a != r;
        case 6: return a >= r;
        default: return true;
    }
}
layout(location = 1) out uvec2 out_id;

// combine codes (counts.z)
const int VERTEX = 0, MODULATE = 1, MODULATE_UNLIT = 2, CROSSFADE = 3, OVER_PREV = 4, OVER_TEX = 5, ADD = 6;
// flags (counts.w): 1 alpha test, 2 first layer unlit, 4 sphere map added (weight in bits 3-4)

// What colour the surface has before its textures: lit by the ambient and the draw's lights
// (the original's light model: the colour of each light by its distance and the cosine to the
// normal), the vertex colours, or black at half alpha (a shadow).
vec4 surface(Draw d) {
    int mode = int(d.ambient.w);
    if (mode == 1) return v_colour;
    if (mode == 2) return vec4(0.0, 0.0, 0.0, 127.0 / 255.0);
    vec3 n = normalize(v_normal);
    vec3 sum = d.ambient.rgb;
    for (int i = 0; i < d.counts.x; i++) {
        sum += d.dir_colour[i].rgb * max(0.0, dot(n, d.dir_to_light[i].xyz));
    }
    for (int i = 0; i < d.counts.y; i++) {
        vec3 to = d.point_position[i].xyz - v_position;
        float dist = length(to);
        vec3 dir = dist > 0.0 ? to / dist : to;
        float atten = 1.0 / (d.point_colour[i].w + d.point_position[i].w * dist);
        sum += d.point_colour[i].rgb * (atten * max(0.0, dot(n, dir)));
    }
    return vec4(clamp(sum, 0.0, 1.0), 1.0);
}

void main() {
    Draw d = draws[v_draw];
    int combine = d.counts.z;
    vec4 ras = surface(d);
    vec4 t0 = texture(layer0, v_uv);
    vec4 c;
    if (combine == VERTEX) {
        c = ras;
    } else if (combine == MODULATE) {
        c = clamp(2.0 * t0 * ras, 0.0, 1.0);
    } else if (combine == MODULATE_UNLIT) {
        c = clamp(2.0 * t0, 0.0, 1.0);
    } else {
        vec4 t1 = texture(layer1, v_uv1);
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
    if ((d.counts.w & 1) != 0) {
        // GX's alpha compare, in 8 bits
        float a = floor(c.a * 255.0 + 0.5);
        bool p0 = compare((d.counts.w >> 5) & 7, a, d.params.y);
        bool p1 = compare((d.counts.w >> 8) & 7, a, d.params.w);
        int op = (d.counts.w >> 11) & 3;
        bool pass = op == 0 ? p0 && p1 : (op == 1 ? p0 || p1 : (op == 2 ? p0 != p1 : p0 == p1));
        if (!pass) discard;
    }
    if (d.extra.z != 0.0) c.a = d.extra.w;
    out_colour = c;
    out_id = uvec2(uint(v_draw) + 1u);
}
