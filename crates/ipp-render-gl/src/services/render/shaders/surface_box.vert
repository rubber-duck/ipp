#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// GUI-only parameterized box quad. Placement carries the scaled position and
// size in Surface metres; corner and border dimensions arrive separately so
// resizing the box never stretches its corners.
precision highp float;
uniform mat4 u_mvp;
uniform vec4 u_placement; // position.xy, size.xy
out vec2 v_surface_position;
void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float(1 - ((gl_VertexID >> 1) & 1)));
    vec2 local = corner * u_placement.zw;
    v_surface_position = u_placement.xy + local;
    gl_Position = u_mvp * vec4(v_surface_position, 0.0, 1.0);
}
