#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// GUI-only parameterized box triangle list. Placement carries the scaled position and
// size in Surface metres; corner and border dimensions arrive per-vertex so
// resizing the box never stretches its corners.
precision highp float;

uniform mat4 u_mvp;
uniform vec4 u_viewport;

// Exterior margin already present in generated geometry. Equal to
// GUI_BOX_ANTIALIAS_PAD in gui_batch.rs; a Rust unit test compares them.
const float GUI_BOX_ANTIALIAS_PAD = 0.002;
// Projected pixels of coverage geometry every edge keeps beyond its contour.
const float ANTIALIAS_PIXELS = 1.5;
// Grazing-angle bounds on each padded axis: at most this many projected pixels
// along the axis, this fraction of the vertex's clip w, and this multiple of the
// larger placed box extent.
const float MAX_PAD_PIXELS = 4.0;
const float DEPTH_PAD_FRACTION = 0.25;
const float SIZE_PAD_FACTOR = 2.0;

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

// Surface-metre padding placing ANTIALIAS_PIXELS of geometry beyond each contour.
//
// That footprint grows without bound as a Surface turns edge-on: reaching the
// perpendicular margin needs ever longer moves nearly along the projected edge,
// stretching triangles toward and past the camera. The bounds cap each axis move
// at MAX_PAD_PIXELS of projected length, keep at least half of the vertex's clip w
// across both axes, and limit Surface-metre growth where projection is affine.
// Near edge-on, the margin therefore narrows instead of inflating triangles.
vec2 antialias_pad(vec4 projected) {
    vec2 dx = (u_mvp[0].xy * projected.w - projected.xy * u_mvp[0].w)
        / (projected.w * projected.w) * u_viewport.xy * 0.5;
    vec2 dy = (u_mvp[1].xy * projected.w - projected.xy * u_mvp[1].w)
        / (projected.w * projected.w) * u_viewport.xy * 0.5;
    mat2 jacobian = mat2(dx, dy);
    if (abs(determinant(jacobian)) <= 1e-8) {
        return vec2(0.0);
    }

    // Rows of the inverse Jacobian: Surface metres per pixel perpendicular to the
    // projected edges of each axis. Columns: pixels per Surface metre along it.
    mat2 inv = inverse(jacobian);
    vec2 footprint = vec2(length(vec2(inv[0].x, inv[1].x)), length(vec2(inv[0].y, inv[1].y)));
    vec2 pixels_per_metre = vec2(length(dx), length(dy));
    vec2 pixel_bound = MAX_PAD_PIXELS / max(pixels_per_metre, vec2(1e-12));
    vec2 w_rate = abs(vec2(u_mvp[0].w, u_mvp[1].w));
    vec2 depth_bound = DEPTH_PAD_FRACTION * projected.w / max(w_rate, vec2(1e-12));
    float extent = max(max(abs(a_placement.z), abs(a_placement.w)), GUI_BOX_ANTIALIAS_PAD);
    vec2 bound = min(pixel_bound, min(depth_bound, vec2(SIZE_PAD_FACTOR * extent)));
    return min(ANTIALIAS_PIXELS * footprint, bound);
}

void main() {
    // Local geometry remains retained across camera motion, including sparse strips,
    // so the projected antialias footprint is applied here.
    vec2 position = a_position;
    vec4 projected = u_mvp * vec4(position, 0.0, 1.0);
    if (projected.w > 0.0) {
        vec2 pad = antialias_pad(projected);
        vec2 lo = min(a_placement.xy, a_placement.xy + a_placement.zw);
        vec2 hi = max(a_placement.xy, a_placement.xy + a_placement.zw);
        vec2 exterior_low = vec2(lessThan(a_position, lo));
        vec2 exterior_high = vec2(greaterThan(a_position, hi));
        vec2 interior = vec2(greaterThanEqual(a_position, lo)) * vec2(lessThanEqual(a_position, hi));

        // Exterior vertices already carry the generated margin. Interior vertices of
        // sparse strips lie on the inner contour without one, so they move inward by
        // the whole footprint: shared seams move together and an undersized hole
        // collapses to the centre.
        vec2 exterior_pad = max(pad - vec2(GUI_BOX_ANTIALIAS_PAD), vec2(0.0));
        vec2 center_delta = (lo + hi) * 0.5 - a_position;
        position += interior * sign(center_delta) * min(pad, abs(center_delta));
        position -= exterior_low * exterior_pad;
        position += exterior_high * exterior_pad;
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
