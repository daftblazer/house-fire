#version 450
// A model's vertices: compact attributes (16-bit positions and coordinates, 8-bit normals)
// scaled here, the world and view matrices applied, and the lights summed per vertex.

layout(location = 0) in ivec4 in_position;
layout(location = 1) in ivec2 in_uv;
layout(location = 2) in vec4 in_colour;
layout(location = 3) in vec4 in_normal;

struct Draw {
    vec4 mv[3];        // world then view: model space to view space, 3x4 rows
    vec4 nm[3];        // the normal matrix (inverse transpose of the 3x3), rows
    vec4 ambient;      // rgb; w: 0 lit, 1 vertex colours, 2 black
    vec4 dir_colour[2];
    vec4 dir_to_light[2];
    vec4 point_colour[4];    // w: k0
    vec4 point_position[4];  // w: k1
    ivec4 counts;      // directionals, points, combine, flags
    vec4 params;       // crossfade share, alpha reference
};

layout(std430, set = 0, binding = 0) readonly buffer Draws { Draw draws[]; };

layout(push_constant) uniform Camera {
    vec4 p0;  // clip x = p.x * x + p.y * z; y = p.z * y + p.w * z
    vec4 p1;  // z = p.x * z + p.y
} cam;

layout(location = 0) out vec4 v_colour;
layout(location = 1) out vec2 v_uv;
layout(location = 2) flat out int v_draw;

float byte(float v) { return floor(clamp(v, 0.0, 1.0) * 255.0 + 0.5) / 255.0; }

void main() {
    Draw d = draws[gl_InstanceIndex];
    vec4 p = vec4(vec3(in_position.xyz), 1.0);
    vec3 v = vec3(dot(d.mv[0], p), dot(d.mv[1], p), dot(d.mv[2], p));
    float w = -v.z;
    gl_Position = vec4(cam.p0.x * v.x + cam.p0.y * v.z, -(cam.p0.z * v.y + cam.p0.w * v.z),
                       cam.p1.x * v.z + cam.p1.y + w, w);
    v_uv = vec2(in_uv) * (1.0 / 4096.0);
    v_draw = gl_InstanceIndex;

    int mode = int(d.ambient.w);
    vec4 c;
    if (mode == 0) {
        vec3 n = vec3(dot(d.nm[0].xyz, in_normal.xyz), dot(d.nm[1].xyz, in_normal.xyz), dot(d.nm[2].xyz, in_normal.xyz));
        float l = length(n);
        n = l > 0.0 ? n / l : n;
        vec3 sum = d.ambient.rgb;
        for (int i = 0; i < d.counts.x; i++) {
            sum += d.dir_colour[i].rgb * max(0.0, dot(n, d.dir_to_light[i].xyz));
        }
        for (int i = 0; i < d.counts.y; i++) {
            vec3 to = d.point_position[i].xyz - v;
            float dist = length(to);
            vec3 dir = dist > 0.0 ? to / dist : to;
            float atten = 1.0 / (d.point_colour[i].w + d.point_position[i].w * dist);
            sum += d.point_colour[i].rgb * (atten * max(0.0, dot(n, dir)));
        }
        c = vec4(sum, 1.0);
    } else if (mode == 1) {
        c = in_colour;
    } else {
        c = vec4(0.0, 0.0, 0.0, 127.0 / 255.0);
    }
    v_colour = vec4(byte(c.r), byte(c.g), byte(c.b), byte(c.a));
}
