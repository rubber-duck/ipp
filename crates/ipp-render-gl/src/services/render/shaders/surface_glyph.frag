#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// Atlas glyph coverage tinted by its record's straight linear colour and clipped by
// its record's rectangle.
precision highp float;

// Single-channel glyph coverage page.
uniform sampler2D u_atlas;

in vec2 v_surface_position;
in vec2 v_uv;
flat in vec4 v_color;
flat in vec4 v_clip;

out vec4 o_color;

void main() {
    // The derivative precedes the discard: GLSL ES 3.00 leaves derivatives undefined
    // once a fragment of the quad has discarded.
    vec2 clip_width = max(fwidth(v_surface_position), vec2(1.0 / 65536.0));
    vec2 clip_inside = min(v_surface_position - v_clip.xy, v_clip.zw - v_surface_position);
    float clip_coverage = clamp(min(clip_inside.x / clip_width.x + 0.5, clip_inside.y / clip_width.y + 0.5), 0.0, 1.0);

    // Atlas pages have no mipmaps, so level zero equals implicit-derivative sampling.
    // R8 pages store coverage in red; their alpha always samples as one.
    float coverage = textureLod(u_atlas, v_uv, 0.0).r;
    float out_alpha = v_color.a * coverage * clip_coverage;
    if (out_alpha <= 0.0) discard;

    o_color = vec4(v_color.rgb, out_alpha);
}
