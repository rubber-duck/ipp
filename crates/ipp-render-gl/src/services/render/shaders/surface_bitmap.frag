#version 300 es
precision highp float;
uniform sampler2D u_texture;
uniform vec4 u_clip;
uniform vec4 u_color;
in vec2 v_uv;
flat in vec4 v_bitmap_placement;
vec2 image_uv_coordinate;
in vec2 v_surface_position;
vec2 surface_position;
// SURFACE_INTERSECTION_FRAGMENT
out vec4 o_color;
void main() {
    surface_position = intersection_position(v_surface_position);
    image_uv_coordinate = u_intersection.x == 0.0 ? v_uv : (surface_position-v_bitmap_placement.xy)/v_bitmap_placement.zw;
    vec2 image_width=max(fwidth(image_uv_coordinate),vec2(1e-8));
    vec2 image_inside=min(image_uv_coordinate,1.0-image_uv_coordinate);
    float image_coverage=u_intersection.x == 0.0 ? 1.0 : clamp(min(image_inside.x/image_width.x,image_inside.y/image_width.y)+0.5,0.0,1.0);

    // Sampling and derivatives precede the discard: GLSL ES 3.00 leaves implicit and
    // explicit derivatives undefined once a fragment of the quad has discarded.
    vec2 clip_width = max(fwidth(surface_position), vec2(1.0 / 65536.0));
    vec4 sampled = texture(u_texture, image_uv_coordinate);
    vec2 clip_inside = min(surface_position - u_clip.xy, u_clip.zw - surface_position);
    float clip_coverage = clamp(min(clip_inside.x / clip_width.x + 0.5, clip_inside.y / clip_width.y + 0.5), 0.0, 1.0);
    clip_coverage *= intersection_coverage * image_coverage;
    if (clip_coverage <= 0.0) discard;
    o_color = vec4(sampled.rgb * u_color.rgb, sampled.a * u_color.a * clip_coverage);
}
