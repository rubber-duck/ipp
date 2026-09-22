#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// GUI-only parameterized box triangle list. Placement carries the scaled position and
// size in Surface metres; corner and border dimensions arrive per-vertex so
// resizing the box never stretches its corners.
precision highp float;

uniform mat4 u_mvp;

layout(location = 0) in vec2 a_position;
layout(location = 1) in vec4 a_placement; // position.xy, size.xy
layout(location = 2) in vec4 a_shape; // corner_rx, corner_ry, border_width, reserved
layout(location = 3) in vec4 a_color0; // fill linear RGBA (solid or gradient start)
layout(location = 4) in vec4 a_color1; // fill linear RGBA (gradient end)
layout(location = 5) in vec4 a_border_color; // border linear RGBA
layout(location = 6) in vec4 a_gradient_coords; // linear: [start.xy, end.xy], radial: [center.xy, radius, 0.0]
layout(location = 7) in vec4 a_material_params; // fill_type, glow_intensity, glow_radius, glow_falloff
layout(location = 8) in vec4 a_glow_color; // glow linear RGBA

out vec2 v_surface_position;
flat out vec4 v_placement;
flat out vec4 v_shape;
flat out vec4 v_color0;
flat out vec4 v_color1;
flat out vec4 v_border_color;
flat out vec4 v_gradient_coords;
flat out vec4 v_material_params;
flat out vec4 v_glow_color;

void main() {
    v_surface_position = a_position;
    v_placement = a_placement;
    v_shape = a_shape;
    v_color0 = a_color0;
    v_color1 = a_color1;
    v_border_color = a_border_color;
    v_gradient_coords = a_gradient_coords;
    v_material_params = a_material_params;
    v_glow_color = a_glow_color;
    gl_Position = u_mvp * vec4(a_position, 0.0, 1.0);
}
