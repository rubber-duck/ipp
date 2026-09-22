#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// GUI-only parameterized box triangle list. Placement carries the scaled position and
// size in Surface metres; corner and border dimensions arrive per-vertex so
// resizing the box never stretches its corners.
precision highp float;

uniform mat4 u_mvp;
uniform vec4 u_viewport;

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
    // Expand only exterior vertices by a projected antialias footprint. Local
    // geometry remains retained across camera motion, including sparse strips.
    vec2 position = a_position;
    vec4 projected = u_mvp * vec4(position, 0.0, 1.0);
    if (projected.w > 0.0) {
        vec2 dx = (u_mvp[0].xy * projected.w - projected.xy * u_mvp[0].w)
            / (projected.w * projected.w) * u_viewport.xy * 0.5;
        vec2 dy = (u_mvp[1].xy * projected.w - projected.xy * u_mvp[1].w)
            / (projected.w * projected.w) * u_viewport.xy * 0.5;
        mat2 jacobian = mat2(dx, dy);
        if (abs(determinant(jacobian)) > 1e-8) {
            mat2 inv = inverse(jacobian);
            vec2 pad = max(1.5 * vec2(length(vec2(inv[0].x, inv[1].x)),
                length(vec2(inv[0].y, inv[1].y))) - vec2(0.002), vec2(0.0));
            vec2 lo = min(a_placement.xy, a_placement.xy + a_placement.zw);
            vec2 hi = max(a_placement.xy, a_placement.xy + a_placement.zw);
            // Inner strip edges also need coverage beyond the border contour.
            // Move shared seams together and collapse an undersized hole safely.
            vec2 center_delta = (lo + hi) * 0.5 - position;
            vec2 interior = vec2(greaterThanEqual(position, lo)) * vec2(lessThanEqual(position, hi));
            vec2 inward = sign(center_delta) * min(pad, abs(center_delta));
            vec2 exterior_low = vec2(lessThan(position, lo));
            vec2 exterior_high = vec2(greaterThan(position, hi));
            position += interior * inward;
            position -= exterior_low * pad;
            position += exterior_high * pad;
        }
    }
    v_surface_position = position;
    v_placement = a_placement;
    v_shape = a_shape;
    v_color0 = a_color0;
    v_color1 = a_color1;
    v_border_color = a_border_color;
    v_gradient_coords = a_gradient_coords;
    v_material_params = a_material_params;
    v_glow_color = a_glow_color;
    gl_Position = u_mvp * vec4(position, 0.0, 1.0);
}
