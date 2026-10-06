#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// Instanced quads of parameterized boxes, strokes and arcs: one record per quad, six
// vertices from gl_VertexID, so painter order within a draw is record order. Box
// placement carries the scaled position and size in logical units; corner, cut,
// accent and border dimensions arrive per record so resizing the box never stretches
// its corners. A stroke's or arc's placement is its own tight bounds. Every record
// carries its primitive's clip rectangle. Glyphs draw with surface_glyph.vert.
precision highp float;

uniform mat4 u_mvp;
uniform vec4 u_viewport;

// Exterior margin already present in generated geometry. Equal to
// GUI_BOX_ANTIALIAS_PAD in retained/box_records.rs; a Rust unit test compares them.
const float GUI_BOX_ANTIALIAS_PAD = 0.002;
// Projected pixels of coverage geometry every edge keeps beyond its contour.
const float ANTIALIAS_PIXELS = 1.5;
// Grazing-angle bounds on each padded axis: at most this many projected pixels
// along the axis, this fraction of the vertex's clip w, and this multiple of the
// larger placed box extent.
const float MAX_PAD_PIXELS = 4.0;
const float DEPTH_PAD_FRACTION = 0.25;
const float SIZE_PAD_FACTOR = 2.0;

layout(location = 0) in vec4 a_rect; // covered rectangle: top-left and bottom-right corners
layout(location = 1) in vec4 a_placement; // position.xy, size.xy
layout(location = 2) in vec4 a_shape; // box: corner_rx, corner_ry, border_width, accent_width; arc: radius, half sweep, thickness, duty
layout(location = 3) in vec4 a_corner_cut; // box: cut per corner; stroke: first segment; arc: centre, middle direction
layout(location = 4) in vec4 a_corner_accent; // box: accent span per corner or checker; stroke: second segment; arc: half-sweep sine and cosine, dashes
layout(location = 5) in vec4 a_color0; // fill linear RGBA (solid or gradient start; colour fields: tint)
layout(location = 6) in vec4 a_color1; // fill linear RGBA (gradient end; saturation-value: hue)
layout(location = 7) in vec4 a_border_color; // border linear RGBA
layout(location = 8) in vec4 a_gradient_coords; // linear and hue: [start.xy, end.xy], radial: [center.xy, radius, 0.0], saturation-value: [min.xy, max.xy]
layout(location = 9) in vec4 a_material_params; // paint, glow_inner_radius, glow_radius, glow_falloff
layout(location = 10) in vec4 a_glow_color; // glow linear RGB, alpha times intensity
layout(location = 11) in vec4 a_clip; // min.xy, max.xy

out vec2 v_surface_position;
flat out vec4 v_placement;
flat out vec4 v_shape;
flat out vec4 v_corner_cut;
flat out vec4 v_corner_accent;
flat out vec4 v_color0;
flat out vec4 v_color1;
flat out vec4 v_border_color;
flat out vec4 v_gradient_coords;
flat out vec4 v_material_params;
flat out vec4 v_glow_color;
flat out vec4 v_clip;

// Whether each of the six vertices takes the rectangle's right and bottom edge: the
// triangles [TL, BL, BR] and [TL, BR, TR].
const bvec2 CORNERS[6] = bvec2[6](
    bvec2(false, false),
    bvec2(false, true),
    bvec2(true, true),
    bvec2(false, false),
    bvec2(true, true),
    bvec2(true, false)
);

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
    // Corners are selected, not interpolated, so they equal the record's lanes
    // exactly and quads sharing an edge keep sharing it.
    bvec2 far_corner = CORNERS[gl_VertexID];
    vec2 corner = vec2(far_corner.x ? a_rect.z : a_rect.x, far_corner.y ? a_rect.w : a_rect.y);
    vec2 position = corner;
    vec4 projected = u_mvp * vec4(position, 0.0, 1.0);
    // Local geometry remains retained across camera motion, including sparse strips,
    // so the projected antialias footprint is applied here.
    if (projected.w > 0.0) {
        vec2 pad = antialias_pad(projected);
        vec2 lo = min(a_placement.xy, a_placement.xy + a_placement.zw);
        vec2 hi = max(a_placement.xy, a_placement.xy + a_placement.zw);
        vec2 exterior_low = vec2(lessThan(corner, lo));
        vec2 exterior_high = vec2(greaterThan(corner, hi));
        vec2 interior = vec2(greaterThanEqual(corner, lo)) * vec2(lessThanEqual(corner, hi));

        // Exterior corners already carry the generated margin. Interior corners of
        // sparse strips lie on the inner contour without one, so they move inward by
        // the whole footprint: shared seams move together and an undersized hole
        // collapses to the centre.
        vec2 exterior_pad = max(pad - vec2(GUI_BOX_ANTIALIAS_PAD), vec2(0.0));
        vec2 center_delta = (lo + hi) * 0.5 - corner;
        position += interior * sign(center_delta) * min(pad, abs(center_delta));
        position -= exterior_low * exterior_pad;
        position += exterior_high * exterior_pad;
    }
    v_surface_position = position;
    v_placement = a_placement;
    v_shape = a_shape;
    v_corner_cut = a_corner_cut;
    v_corner_accent = a_corner_accent;
    v_color0 = a_color0;
    v_color1 = a_color1;
    v_border_color = a_border_color;
    v_gradient_coords = a_gradient_coords;
    v_material_params = a_material_params;
    v_glow_color = a_glow_color;
    v_clip = a_clip;
    gl_Position = u_mvp * vec4(position, 0.0, 1.0);
}
