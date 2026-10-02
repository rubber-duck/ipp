#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// Instanced atlas glyph quads: one record per glyph, six vertices from gl_VertexID.
// Atlas entries hold a blank texel around their coverage, so quads need no
// antialias growth. Each record carries its run's clip rectangle.
precision highp float;

uniform mat4 u_mvp;

layout(location = 0) in vec4 a_rect; // corners [x0, y0, x1, y1]
layout(location = 1) in vec4 a_uv; // atlas coordinates at those corners
layout(location = 2) in vec4 a_color; // straight linear RGBA tint
layout(location = 3) in vec4 a_clip; // min.xy, max.xy

out vec2 v_surface_position;
out vec2 v_uv;
flat out vec4 v_color;
flat out vec4 v_clip;

// Whether each of the six vertices takes the second corner on each axis: the
// triangles [TL, BL, BR] and [TL, BR, TR].
const bvec2 CORNERS[6] = bvec2[6](
    bvec2(false, false),
    bvec2(false, true),
    bvec2(true, true),
    bvec2(false, false),
    bvec2(true, true),
    bvec2(true, false)
);

void main() {
    bvec2 far_corner = CORNERS[gl_VertexID];
    vec2 position = vec2(far_corner.x ? a_rect.z : a_rect.x, far_corner.y ? a_rect.w : a_rect.y);
    v_uv = vec2(far_corner.x ? a_uv.z : a_uv.x, far_corner.y ? a_uv.w : a_uv.y);
    v_surface_position = position;
    v_color = a_color;
    v_clip = a_clip;
    gl_Position = u_mvp * vec4(position, 0.0, 1.0);
}
