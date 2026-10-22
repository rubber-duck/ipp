#version 300 es
precision highp float;
// Premultiplied whole-Surface cache image composited over its root content rectangle.
uniform sampler2D u_surface_cache;
uniform sampler2D u_surface_ownership;
uniform float u_cutout_owner;
uniform vec4 u_clip;
uniform float u_opacity;
// xy: active texels, zw: allocated capacity.
uniform vec4 u_image_area;
in vec2 v_uv;
in vec2 v_surface_position;
out vec4 o_color;
// Filter color and ownership together: every source texel belongs to exactly
// one cutout, including the composite under translucent edges and glows.
vec4 cutout_texel(ivec2 point) {
    point = clamp(point, ivec2(0), ivec2(u_image_area.xy) - ivec2(1));
    vec3 bytes = floor(texelFetch(u_surface_ownership, point, 0).rgb * 255.0 + 0.5);
    float owner = dot(bytes, vec3(1.0, 256.0, 65536.0));
    return owner == u_cutout_owner ? texelFetch(u_surface_cache, point, 0) : vec4(0.0);
}
void main() {
    // Sampling and derivatives precede the discard: GLSL ES 3.00 leaves implicit and
    // explicit derivatives undefined once a fragment of the quad has discarded.
    vec2 clip_width = max(fwidth(v_surface_position), vec2(1.0 / 65536.0));
    vec2 image_uv = clamp(v_uv * u_image_area.xy, vec2(0.5), u_image_area.xy - vec2(0.5)) / u_image_area.zw;
    vec4 sampled = texture(u_surface_cache, image_uv);
    if (u_cutout_owner > 0.0) {
        vec2 point = image_uv * u_image_area.zw - 0.5;
        ivec2 base = ivec2(floor(point));
        vec2 fraction = fract(point);
        sampled = mix(mix(cutout_texel(base), cutout_texel(base + ivec2(1, 0)), fraction.x),
                      mix(cutout_texel(base + ivec2(0, 1)), cutout_texel(base + ivec2(1, 1)), fraction.x), fraction.y);
    }
    vec2 clip_inside = min(v_surface_position - u_clip.xy, u_clip.zw - v_surface_position);
    float clip_coverage = clamp(min(clip_inside.x / clip_width.x + 0.5, clip_inside.y / clip_width.y + 0.5), 0.0, 1.0);
    if (clip_coverage <= 0.0) discard;
    o_color = sampled * (clip_coverage * u_opacity);
}
