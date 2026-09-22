#version 300 es
precision highp float;

uniform sampler2D u_atlas;
uniform vec4 u_clip;

in vec2 v_surface_position;
in vec2 v_uv;
in vec4 v_color;

out vec4 o_color;

void main() {
    vec2 clip_width = max(fwidth(v_surface_position), vec2(1.0 / 65536.0));
    vec2 clip_inside = min(v_surface_position - u_clip.xy, u_clip.zw - v_surface_position);
    float clip_coverage = clamp(min(clip_inside.x / clip_width.x + 0.5, clip_inside.y / clip_width.y + 0.5), 0.0, 1.0);
    if (clip_coverage <= 0.0) discard;

    vec4 tex = texture(u_atlas, v_uv);
    float coverage = max(tex.a, tex.r);
    if (coverage <= 0.0) discard;

    float alpha = v_color.a * coverage * clip_coverage;
    o_color = vec4(v_color.rgb, alpha);
}
