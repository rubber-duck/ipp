#version 300 es
precision highp float;
uniform mat4 u_mvp;
uniform vec4 u_placement; // position.xy, size.xy
out vec2 v_uv;
out vec2 v_surface_position;
void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float(1 - ((gl_VertexID >> 1) & 1)));
    vec2 local = corner * u_placement.zw;
    v_surface_position = u_placement.xy + local;
    v_uv = corner;
    gl_Position = u_mvp * vec4(v_surface_position, 0.0, 1.0);
}
