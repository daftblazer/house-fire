#version 450
// A movie's triangle: the position is already in clip space (the drawing scales the movie's
// stage to the picture), the rest goes to the fragment shader.

layout(location = 0) in vec2 in_pos;
layout(location = 1) in vec2 in_uv;
layout(location = 2) in vec4 in_tint;
layout(location = 3) in vec4 in_mul;
layout(location = 4) in vec4 in_add;
layout(location = 5) in float in_premul;

layout(location = 0) out vec2 v_uv;
layout(location = 1) out vec4 v_tint;
layout(location = 2) out vec4 v_mul;
layout(location = 3) out vec4 v_add;
layout(location = 4) out float v_premul;

void main() {
    v_uv = in_uv;
    v_tint = in_tint;
    v_mul = in_mul;
    v_add = in_add;
    v_premul = in_premul;
    gl_Position = vec4(in_pos, 0.0, 1.0);
}
