#version 450
// The movie's texel (a white texel for a solid fill) tinted by the fill's colour, then through
// the clip's colour transform (multiplier, then offset), as Flash has it; premultiplied for the
// blend state (one, one minus source alpha). A premultiplied texture (the fonts' pages) is
// filtered as it is stored and divided by its alpha here.

layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_tint;
layout(location = 2) in vec4 v_mul;
layout(location = 3) in vec4 v_add;
layout(location = 4) in float v_premul;
layout(location = 0) out vec4 out_colour;
layout(set = 0, binding = 0) uniform sampler2D tex;

void main() {
    vec4 t = texture(tex, v_uv);
    if (v_premul > 0.5 && t.a > 0.0) {
        t.rgb /= t.a;
    }
    vec4 c = clamp(t * v_tint * v_mul + v_add, 0.0, 1.0);
    out_colour = vec4(c.rgb * c.a, c.a);
}
