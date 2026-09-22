#version 300 es
precision highp float;

layout(location = 0) in vec2 a_position;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in vec4 a_color;

uniform mat4 u_mvp;

out vec2 v_surface_position;
out vec2 v_uv;
out vec4 v_color;

void main() {
    v_surface_position = a_position;
    v_uv = a_uv;
    v_color = a_color;
    gl_Position = u_mvp * vec4(a_position, 0.0, 1.0);
}
