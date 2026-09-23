#version 300 es
precision highp float;
layout(location = 0) in vec4 i_bounds;
layout(location = 1) in vec4 i_placement;
layout(location = 2) in vec4 i_color;
layout(location = 3) in vec3 i_descriptor;
uniform mat4 u_mvp;
uniform vec4 u_viewport;
out vec2 v_path_position;
out vec2 v_surface_position;
out vec4 v_color;
flat out vec4 v_bounds;
flat out int v_band_offset;
flat out int v_curve_start;
void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float(1 - ((gl_VertexID >> 1) & 1)));
    vec2 path = mix(i_bounds.xy, i_bounds.zw, corner);
    vec2 surface0 = i_placement.xy + path * i_placement.zw;
    vec4 clip0 = u_mvp * vec4(surface0, 0.0, 1.0);
    vec4 clipx = u_mvp * vec4(i_placement.z, 0.0, 0.0, 0.0);
    vec4 clipy = u_mvp * vec4(0.0, i_placement.w, 0.0, 0.0);
    vec2 ndc0 = clip0.xy / clip0.w;
    float inverse_w = 1.0 / max(abs(clip0.w), 1.0 / 65536.0);
    vec2 derivative_x = (clipx.xy - ndc0 * clipx.w) * inverse_w;
    vec2 derivative_y = (clipy.xy - ndc0 * clipy.w) * inverse_w;
    float px = max(length(derivative_x * u_viewport.xy * 0.5), 1.0 / 65536.0);
    float py = max(length(derivative_y * u_viewport.xy * 0.5), 1.0 / 65536.0);
    path += (corner * 2.0 - 1.0) * vec2(0.75 / px, 0.75 / py);
    vec2 surface = i_placement.xy + path * i_placement.zw;
    v_path_position = path;
    v_surface_position = surface;
    v_color = i_color;
    v_bounds = i_bounds;
    v_band_offset = int(i_descriptor.z);
    v_curve_start = int(i_descriptor.x);
    gl_Position = u_mvp * vec4(surface, 0.0, 1.0);
}
