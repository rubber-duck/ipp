#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// GUI-only parameterized fill/border box. Colors are straight linear RGBA;
// the single display conversion happens downstream in the present pass.
precision highp float;

uniform vec4 u_clip; // min.xy, max.xy in Surface metres

in vec2 v_surface_position;
flat in vec4 v_placement; // position.xy, size.xy in Surface metres
flat in vec4 v_color; // straight linear fill RGBA
flat in vec4 v_border_color; // straight linear border RGBA
flat in vec4 v_shape; // corner_rx, corner_ry, border_width, reserved

out vec4 o_color;

float sd_box(vec2 offset, vec2 half_size) {
    vec2 q = abs(offset) - half_size;
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0);
}

// Gradient-normalized implicit ellipse distance. Its zero contour is the
// requested ellipse, while the normalization keeps antialiasing in Surface
// metres under unequal radii. The rounded box uses this only in its corner
// quadrant; straight edges retain the ordinary box contour.
float sd_ellipse(vec2 point, vec2 radius) {
    float k0 = length(point / radius);
    if (k0 <= 1.0 / 65536.0) return -min(radius.x, radius.y);
    float k1 = length(point / (radius * radius));
    return k0 * (k0 - 1.0) / max(k1, 1.0 / 65536.0);
}

float sd_round_box(vec2 offset, vec2 half_size, vec2 radius) {
    // A zero lane has no corner extent on that axis, so the limiting shape is
    // the sharp box rather than an invented circular radius.
    if (min(radius.x, radius.y) <= 1.0 / 65536.0) {
        return sd_box(offset, half_size);
    }
    vec2 corner_point = max(abs(offset) - (half_size - radius), vec2(0.0));
    return sd_ellipse(corner_point, radius);
}

void main() {
    vec2 clip_width = max(fwidth(v_surface_position), vec2(1.0 / 65536.0));
    vec2 clip_inside = min(v_surface_position - u_clip.xy, u_clip.zw - v_surface_position);
    float clip_coverage = clamp(min(clip_inside.x / clip_width.x + 0.5, clip_inside.y / clip_width.y + 0.5), 0.0, 1.0);
    if (clip_coverage <= 0.0) discard;
    vec2 half_size = v_placement.zw * 0.5;
    vec2 center = v_placement.xy + half_size;
    // Clamp each explicit radius to the corresponding placed half size.
    vec2 corner = min(max(v_shape.xy, vec2(0.0)), half_size);
    vec2 offset = v_surface_position - center;
    float outer = sd_round_box(offset, half_size, corner);
    float edge = max(fwidth(outer), 1.0 / 65536.0);
    float fill = 1.0 - smoothstep(-edge, edge, outer);
    if (fill <= 0.0) discard;
    float border_mix = 0.0;
    if (v_shape.z > 0.0) {
        vec2 inner_half = max(half_size - v_shape.z, vec2(0.0));
        vec2 inner_corner = max(corner - v_shape.z, vec2(0.0));
        float inner = sd_round_box(offset, inner_half, inner_corner);
        border_mix = smoothstep(-edge, edge, inner);
    }
    vec3 rgb = mix(v_color.rgb, v_border_color.rgb, border_mix);
    float alpha = mix(v_color.a, v_border_color.a, border_mix) * fill * clip_coverage;
    if (alpha <= 0.0) discard;
    o_color = vec4(rgb, alpha);
}
