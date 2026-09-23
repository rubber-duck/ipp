#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// Coverage math is derived from the Slug reference shaders.
// Copyright 2017, Eric Lengyel.
precision highp float;

uniform mat4 u_mvp;
uniform vec4 u_bounds;
uniform vec4 u_placement; // position.xy, scale.xy
uniform vec4 u_color;
uniform int u_band_offset;
uniform int u_curve_start;
uniform vec4 u_viewport;
out vec2 v_path_position;
out vec2 v_surface_position;
out vec4 v_color;
flat out vec4 v_bounds;
flat out int v_band_offset;
flat out int v_curve_start;

void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float(1 - ((gl_VertexID >> 1) & 1)));
    vec2 path = mix(u_bounds.xy, u_bounds.zw, corner);
    vec2 surface0 = u_placement.xy + path * u_placement.zw;
    vec4 clip0 = u_mvp * vec4(surface0, 0.0, 1.0);
    vec4 clipx = u_mvp * vec4(u_placement.z, 0.0, 0.0, 0.0);
    vec4 clipy = u_mvp * vec4(0.0, u_placement.w, 0.0, 0.0);
    vec2 ndc0 = clip0.xy / clip0.w;
    float inverse_w = 1.0 / max(abs(clip0.w), 1.0 / 65536.0);
    vec2 derivative_x = (clipx.xy - ndc0 * clipx.w) * inverse_w;
    vec2 derivative_y = (clipy.xy - ndc0 * clipy.w) * inverse_w;
    float px = max(length(derivative_x * u_viewport.xy * 0.5), 1.0 / 65536.0);
    float py = max(length(derivative_y * u_viewport.xy * 0.5), 1.0 / 65536.0);
    path += (corner * 2.0 - 1.0) * vec2(0.75 / px, 0.75 / py);
    vec2 surface = u_placement.xy + path * u_placement.zw;
    v_path_position = path;
    v_surface_position = surface;
    v_color = u_color;
    v_bounds = u_bounds;
    v_band_offset = u_band_offset;
    v_curve_start = u_curve_start;
    gl_Position = u_mvp * vec4(surface, 0.0, 1.0);
}
