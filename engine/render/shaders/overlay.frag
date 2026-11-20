#version 450
// The interface overlay's texel, premultiplied, laid over the picture by the blend state.

layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 out_colour;
layout(set = 0, binding = 0) uniform sampler2D overlay;

void main() {
    out_colour = texture(overlay, v_uv);
}
