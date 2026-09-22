#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// GUI-only parameterized box triangle list. Placement carries the scaled position and
// size in Surface metres; corner and border dimensions arrive per-vertex so
// resizing the box never stretches its corners.
precision highp float;

uniform mat4 u_mvp;

layout(location = 0) in vec2 a_position;
layout(location = 1) in vec4 a_placement; // position.xy, size.xy
layout(location = 2) in vec4 a_color; // fill linear RGBA
layout(location = 3) in vec4 a_border_color; // border linear RGBA
layout(location = 4) in vec4 a_shape; // corner_rx, corner_ry, border_width, reserved

out vec2 v_surface_position;
flat out vec4 v_placement;
flat out vec4 v_color;
flat out vec4 v_border_color;
flat out vec4 v_shape;

void main() {
    v_surface_position = a_position;
    v_placement = a_placement;
    v_color = a_color;
    v_border_color = a_border_color;
    v_shape = a_shape;
    gl_Position = u_mvp * vec4(a_position, 0.0, 1.0);
}
