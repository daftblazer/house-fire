#version 450
// A model's vertices: compact attributes (16-bit positions and coordinates, 8-bit normals)
// scaled here and carried to the fragment shader in view space, where the lights are summed.

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
    vec4 params;       // crossfade share, alpha reference, sphere weight
    vec4 extra;        // x: the mesh's texture coordinate unit; y: layer 1 from the position; z, w: constant alpha on, its value
    vec4 layer1[2];    // layer 1 from the position: u then v as (x, y, offset)
    vec4 proj[2];      // the projection: clip x = p.x * x + p.y * z; y = p.z * y + p.w * z; z = p.x * z + p.y
    ivec4 skin;        // x: added to the vertex index to find a skinned vertex in `skinned`
};

// a skinned vertex as the compute pass (skin.comp) leaves it
struct Skinned {
    vec4 position;
    vec4 normal;
};

layout(std430, set = 0, binding = 0) readonly buffer Draws { Draw draws[]; };
layout(std430, set = 0, binding = 1) readonly buffer SkinnedVertices { Skinned skinned[]; };

layout(location = 0) out vec4 v_colour;
layout(location = 1) out vec2 v_uv;
layout(location = 2) flat out int v_draw;
layout(location = 3) out vec2 v_sphere;
layout(location = 4) out vec3 v_position;
layout(location = 5) out vec3 v_normal;
layout(location = 6) out vec2 v_uv1;

void main() {
    Draw d = draws[gl_InstanceIndex];
#ifdef SKINNED
    Skinned sk = skinned[gl_VertexIndex + d.skin.x];
    vec4 p = vec4(sk.position.xyz, 1.0);
    vec3 in_n = sk.normal.xyz;
#else
    vec4 p = vec4(vec3(in_position.xyz), 1.0);
    vec3 in_n = in_normal.xyz;
#endif
    vec3 v = vec3(dot(d.mv[0], p), dot(d.mv[1], p), dot(d.mv[2], p));
    float w = -v.z;
    gl_Position = vec4(d.proj[0].x * v.x + d.proj[0].y * v.z, -(d.proj[0].z * v.y + d.proj[0].w * v.z),
                       d.proj[1].x * v.z + d.proj[1].y + w, w);
    v_uv = vec2(in_uv) * (d.extra.x / 4096.0);
    v_uv1 = d.extra.y != 0.0 ? vec2(dot(d.layer1[0].xy, vec2(in_position.xy)) + d.layer1[0].z, dot(d.layer1[1].xy, vec2(in_position.xy)) + d.layer1[1].z) : v_uv;
    v_draw = gl_InstanceIndex;
    v_colour = in_colour;

    vec3 n = vec3(dot(d.nm[0].xyz, in_n), dot(d.nm[1].xyz, in_n), dot(d.nm[2].xyz, in_n));
    float nl = length(n);
    n = nl > 0.0 ? n / nl : n;
    // the sphere map: the view-space position reflected about the normal, turned into a pair
    // of coordinates
    vec3 o = vec3(0.0, 0.0, 1.0);
    if (nl > 0.0) {
        o = v - n * (2.0 * dot(v, n));
        o.z += length(o) + 1.0;
        o = normalize(o);
    }
    v_sphere = o.xy * 0.5 + 0.5;
    v_position = v;
    v_normal = n;
}
