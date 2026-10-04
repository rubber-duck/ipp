#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 2) in vec2 a_content;
uniform mat4 u_mvp;
uniform vec4 u_placement;
uniform int u_image_flip;
out vec2 v_uv;
out vec2 v_surface_position;
void main() {
    v_surface_position = a_content * u_placement.zw;
    v_uv = vec2(a_content.x, u_image_flip != 0 ? 1.0 - a_content.y : a_content.y);
    gl_Position = u_mvp * vec4(a_position, 1.0);
}
