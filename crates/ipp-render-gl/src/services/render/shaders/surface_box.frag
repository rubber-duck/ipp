#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// GUI-only parameterized fill/border box with gradients and glow. Colors are straight linear RGBA;
// the single display conversion happens downstream in the present pass.
precision highp float;

uniform vec4 u_clip; // min.xy, max.xy in Surface metres

in vec2 v_surface_position;
flat in vec4 v_placement; // position.xy, size.xy in Surface metres
flat in vec4 v_shape; // corner_rx, corner_ry, border_width, reserved
flat in vec4 v_color0; // straight linear start/solid RGBA
flat in vec4 v_color1; // straight linear end RGBA
flat in vec4 v_border_color; // straight linear border RGBA
flat in vec4 v_gradient_coords; // linear: [start.xy, end.xy], radial: [center.xy, radius, 0.0]
flat in vec4 v_material_params; // fill_type (0=solid, 1=linear, 2=radial), glow_intensity, glow_radius, glow_falloff
flat in vec4 v_glow_color; // straight linear glow RGBA

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

    vec2 half_size = abs(v_placement.zw) * 0.5;
    vec2 center = v_placement.xy + v_placement.zw * 0.5;
    // Clamp each explicit radius to the corresponding placed half size.
    vec2 corner = min(max(v_shape.xy, vec2(0.0)), half_size);
    vec2 offset = v_surface_position - center;
    float outer = sd_round_box(offset, half_size, corner);
    // The distance spans one pixel across this footprint. A two-footprint
    // smoothstep blurs subpixel rails and borders into the surrounding halo.
    float edge = max(length(vec2(dFdx(outer), dFdy(outer))), 1.0 / 65536.0);

    // Shape interior coverage [0.0, 1.0]
    float shape_cov = clamp(0.5 - outer / edge, 0.0, 1.0);

    // Outer glow evaluation
    float glow_alpha = 0.0;
    vec3 glow_rgb = v_glow_color.rgb;
    float glow_radius = v_material_params.z;
    float glow_intensity = v_material_params.y;
    if (glow_radius > 0.0 && glow_intensity > 0.0) {
        float dist_outside = max(outer, 0.0);
        if (dist_outside < glow_radius) {
            float norm = clamp(1.0 - dist_outside / glow_radius, 0.0, 1.0);
            float factor = (abs(v_material_params.w - 1.0) < 1e-5) ? norm : pow(norm, v_material_params.w);
            // An outer halo must not fill a transparent shape or focus ring.
            glow_alpha = clamp(v_glow_color.a * glow_intensity * factor, 0.0, 1.0) * (1.0 - shape_cov);
        }
    }

    // Fill color evaluation (solid, linear gradient, or radial gradient)
    vec4 fill_color = v_color0;
    vec2 local_pos = v_surface_position - v_placement.xy;
    if (v_material_params.x > 0.5 && v_material_params.x < 1.5) {
        vec2 p0 = v_gradient_coords.xy;
        vec2 p1 = v_gradient_coords.zw;
        vec2 dir = p1 - p0;
        float len_sq = dot(dir, dir);
        float t = (len_sq > 1e-12) ? clamp(dot(local_pos - p0, dir) / len_sq, 0.0, 1.0) : 0.0;
        fill_color = mix(v_color0, v_color1, t);
    } else if (v_material_params.x >= 1.5) {
        vec2 center_pt = v_gradient_coords.xy;
        float radius = v_gradient_coords.z;
        float dist = length(local_pos - center_pt);
        float t = (radius > 1e-6) ? clamp(dist / radius, 0.0, 1.0) : 0.0;
        fill_color = mix(v_color0, v_color1, t);
    }

    // Border evaluation
    float fill_cov = shape_cov;
    if (v_shape.z > 0.0) {
        vec2 inner_half = max(half_size - v_shape.z, vec2(0.0));
        vec2 inner_corner = max(corner - v_shape.z, vec2(0.0));
        float inner = sd_round_box(offset, inner_half, inner_corner);
        float inner_edge = max(length(vec2(dFdx(inner), dFdy(inner))), 1.0 / 65536.0);
        fill_cov = min(shape_cov, clamp(0.5 - inner / inner_edge, 0.0, 1.0));
    }

    // Composite straight fill and border colors
    // Coverage of the ring is the difference of its two contours. Multiplying
    // two antialias ramps loses contrast when a border is narrower than a pixel.
    float fill_a = fill_color.a * fill_cov;
    float border_a = v_border_color.a * (shape_cov - fill_cov);
    float shape_base_alpha = fill_a + border_a;
    vec3 shape_rgb = vec3(0.0);
    if (shape_base_alpha > 1e-5) {
        shape_rgb = (fill_color.rgb * fill_a + v_border_color.rgb * border_a) / shape_base_alpha;
    }
    float shape_alpha = shape_base_alpha;

    // Composite shape and outer glow in straight linear RGBA
    float combined_alpha = shape_alpha + glow_alpha * (1.0 - shape_alpha);
    if (combined_alpha <= 0.0) discard;

    vec3 out_rgb = (shape_rgb * shape_alpha + glow_rgb * glow_alpha * (1.0 - shape_alpha)) / combined_alpha;
    float out_alpha = combined_alpha * clip_coverage;
    if (out_alpha <= 0.0) discard;

    o_color = vec4(out_rgb, out_alpha);
}
