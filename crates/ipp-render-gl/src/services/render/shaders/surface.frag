#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// Root eligibility, polynomial solving and two-axis coverage are adapted from
// the Slug reference shader. Copyright 2017, Eric Lengyel.
precision highp float;
precision highp int;

uniform sampler2D u_curves;
uniform int u_curve_count;
uniform int u_curve_start;
uniform int u_curve_width;
uniform highp usampler2D u_bands;
uniform int u_band_width;
uniform int u_fill_rule;
uniform vec4 u_clip;
in vec2 v_path_position;
in vec2 v_surface_position;
in vec4 v_color;
flat in vec4 v_bounds;
flat in int v_band_offset;
out vec4 o_color;

uint root_code(float p1, float p2, float p3) {
    uint i1 = floatBitsToUint(p1) >> 31u;
    uint i2 = floatBitsToUint(p2) >> 30u;
    uint i3 = floatBitsToUint(p3) >> 29u;
    uint shift = (i2 & 2u) | (i1 & ~2u);
    shift = (i3 & 4u) | (shift & ~4u);
    return (0x2E74u >> shift) & 0x0101u;
}

vec2 solve_horizontal(vec4 p12, vec2 p3, vec2 a, vec2 b, bool line) {
    if (line) {
        // Segment kind is authoritative; float cancellation must never turn a line into a parabola.
        float t = p12.y / (p12.y - p3.y);
        float crossing = mix(p12.x, p3.x, t);
        return vec2(crossing);
    }
    float d = sqrt(max(b.y * b.y - a.y * p12.y, 0.0));
    vec2 t = vec2(b.y - d, b.y + d) / a.y;
    if (abs(a.y) < 1.0 / 65536.0) t = vec2(p12.y / (2.0 * b.y));
    return (a.x * t - 2.0 * b.x) * t + p12.x;
}

vec2 solve_vertical(vec4 p12, vec2 p3, vec2 a, vec2 b, bool line) {
    vec4 q = p12.yxwz;
    return solve_horizontal(q, p3.yx, a.yx, b.yx, line);
}

uvec2 band_record(int index) {
    return texelFetch(u_bands, ivec2(index % u_band_width, index / u_band_width), 0).xy;
}

void curve_points(uint curve, vec2 sample_position, out vec4 p12, out vec2 p3, out vec2 a, out vec2 b, out bool line) {
    int first_index = int(curve) * 2;
    int second_index = first_index + 1;
    vec4 absolute12 = texelFetch(u_curves, ivec2(first_index % u_curve_width, first_index / u_curve_width), 0);
    vec4 absolute3 = texelFetch(u_curves, ivec2(second_index % u_curve_width, second_index / u_curve_width), 0);
    // Derive the polynomial before translating to the fragment. Large font-unit
    // coordinates otherwise give mathematically linear axes a spurious quadratic term.
    a = absolute12.xy - 2.0 * absolute12.zw + absolute3.xy;
    b = absolute12.xy - absolute12.zw;
    line = absolute3.z < 0.5;
    p12 = absolute12 - vec4(sample_position, sample_position);
    p3 = absolute3.xy - sample_position;
}

void accumulate(vec2 sample_position, out float xcov, out float ycov, out float xwgt, out float ywgt) {
    vec2 pixels_per_unit = 1.0 / max(fwidth(v_path_position), vec2(1.0 / 65536.0));
    xcov = 0.0; ycov = 0.0; xwgt = 0.0; ywgt = 0.0;
    vec2 extent = max(v_bounds.zw - v_bounds.xy, vec2(1.0 / 65536.0));
    ivec2 band = clamp(ivec2((sample_position - v_bounds.xy) / extent * 16.0), ivec2(0), ivec2(15));
    uvec2 header = band_record(v_band_offset + band.y);
    for (uint index = 0u; index < header.y; ++index) {
        uint curve = band_record(int(header.x + index)).x;
        vec4 p12; vec2 p3; vec2 a; vec2 b; bool line;
        curve_points(curve, sample_position, p12, p3, a, b, line);
        uint code = root_code(p12.y, p12.w, p3.y);
        if (code != 0u) {
            vec2 r = solve_horizontal(p12, p3, a, b, line) * pixels_per_unit.x;
            if ((code & 1u) != 0u) { xcov += clamp(r.x + 0.5, 0.0, 1.0); xwgt = max(xwgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0)); }
            if (code > 1u) { xcov -= clamp(r.y + 0.5, 0.0, 1.0); xwgt = max(xwgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0)); }
        }
    }
    header = band_record(v_band_offset + 16 + band.x);
    for (uint index = 0u; index < header.y; ++index) {
        uint curve = band_record(int(header.x + index)).x;
        vec4 p12; vec2 p3; vec2 a; vec2 b; bool line;
        curve_points(curve, sample_position, p12, p3, a, b, line);
        uint code = root_code(p12.x, p12.z, p3.x);
        if (code != 0u) {
            vec2 r = solve_vertical(p12, p3, a, b, line) * pixels_per_unit.y;
            if ((code & 1u) != 0u) { ycov -= clamp(r.x + 0.5, 0.0, 1.0); ywgt = max(ywgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0)); }
            if (code > 1u) { ycov += clamp(r.y + 0.5, 0.0, 1.0); ywgt = max(ywgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0)); }
        }
    }
}

void main() {
    vec2 clip_width = max(fwidth(v_surface_position), vec2(1.0 / 65536.0));
    vec2 clip_inside = min(v_surface_position - u_clip.xy, u_clip.zw - v_surface_position);
    float clip_coverage = clamp(min(clip_inside.x / clip_width.x + 0.5, clip_inside.y / clip_width.y + 0.5), 0.0, 1.0);
    if (clip_coverage <= 0.0) discard;
    float xcov, ycov, xwgt, ywgt;
    accumulate(v_path_position, xcov, ycov, xwgt, ywgt);
    float coverage = max(abs(xcov * xwgt + ycov * ywgt) / max(xwgt + ywgt, 1.0 / 65536.0), min(abs(xcov), abs(ycov)));
    coverage = u_fill_rule == 0 ? clamp(coverage, 0.0, 1.0) : 1.0 - abs(1.0 - fract(coverage * 0.5) * 2.0);
    if (coverage <= 0.0) discard;
    o_color = vec4(v_color.rgb, v_color.a * coverage * clip_coverage);
}
