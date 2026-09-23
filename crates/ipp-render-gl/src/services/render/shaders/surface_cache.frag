#version 300 es
precision highp float;
// Premultiplied whole-Surface cache image composited over its root content rectangle.
uniform sampler2D u_surface_cache;
uniform vec4 u_clip;
in vec2 v_uv;
in vec2 v_surface_position;
out vec4 o_color;
void main() {
    // Sampling and derivatives precede the discard: GLSL ES 3.00 leaves implicit and
    // explicit derivatives undefined once a fragment of the quad has discarded.
    vec2 clip_width = max(fwidth(v_surface_position), vec2(1.0 / 65536.0));
    vec4 sampled = texture(u_surface_cache, v_uv);
    vec2 clip_inside = min(v_surface_position - u_clip.xy, u_clip.zw - v_surface_position);
    float clip_coverage = clamp(min(clip_inside.x / clip_width.x + 0.5, clip_inside.y / clip_width.y + 0.5), 0.0, 1.0);
    if (clip_coverage <= 0.0) discard;
    // Opacity was applied once when the image was painted; scale premultiplied
    // colour and alpha together by edge coverage only.
    o_color = sampled * clip_coverage;
}
