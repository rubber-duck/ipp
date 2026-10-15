#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// Root eligibility, polynomial solving and two-axis coverage are adapted from
// the Slug reference shader. Copyright 2017, Eric Lengyel.
precision highp float;
precision highp int;

// Fixed-point curve texels and single-channel bands; see assets/paths.rs for the layout.
uniform highp isampler2D u_curves;
uniform float u_curve_scale;
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
flat in int v_curve_start;
out vec4 o_color;

// CANVAS_PAINTS
#ifndef IPP_CANVAS_PAINTS
vec4 ipp_canvas_paint(int slot, int block, vec2 position, vec2 size, vec4 color, float edge, vec4 data, vec2 canvasPosition) { return color; }
#endif
uniform vec4 u_path_paint; // paint slot, parameter block, inherited opacity, multiple contours; zero slot selects ordinary coverage.

#ifdef IPP_CANVAS_PAINTS
// Bounded nearest-point refinement on curves already selected by the coverage bands.
// Lines are exact. Quadratics use chord projection followed by four Newton steps,
// retaining endpoints so cusp/endpoint candidates cannot be lost.
vec3 contour_point(vec4 p12, vec2 p3, vec2 a, vec2 b, bool line) {
    vec2 chord=p3-p12.xy;
    float t=clamp(-dot(p12.xy,chord)/max(dot(chord,chord),1e-12),0.0,1.0);
    if(line) return vec3(p12.xy+chord*t,t);
    for(int step=0;step<4;step++) {
        vec2 q=(a*t-2.0*b)*t+p12.xy;
        vec2 tangent=2.0*(a*t-b);
        float denominator=dot(tangent,tangent)+dot(q,2.0*a);
        if(abs(denominator)>1e-10) t=clamp(t-dot(q,tangent)/denominator,0.0,1.0);
    }
    vec2 q=(a*t-2.0*b)*t+p12.xy;
    if(dot(p12.xy,p12.xy)<dot(q,q)) { q=p12.xy; t=0.0; }
    if(dot(p3,p3)<dot(q,q)) { q=p3; t=1.0; }
    return vec3(q,t);
}

void retain_candidate(vec3 nearest,vec2 tangent,inout vec4 candidates[3]) {
    float distance=length(nearest.xy);
    if(distance>=2.0) return;
    for(int index=0;index<3;index++) {
        if(length(nearest.xy-candidates[index].xy)<0.01) return;
    }
    float extent=length(tangent);
    vec2 normal=extent>1e-8 ? vec2(-tangent.y,tangent.x)/extent : vec2(1,0);
    vec4 candidate=vec4(nearest.xy,normal);
    for(int index=0;index<3;index++) {
        if(distance<length(candidates[index].xy)) {
            vec4 previous=candidates[index];
            candidates[index]=candidate;
            candidate=previous;
            distance=length(previous.xy);
        }
    }
}

#endif

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

int band_value(int index) {
    return int(texelFetch(u_bands, ivec2(index % u_band_width, index / u_band_width), 0).x);
}

ivec4 curve_texel(int index) {
    return texelFetch(u_curves, ivec2(index % u_curve_width, index / u_curve_width), 0);
}

void curve_points(int curve, vec2 sample_position, out vec4 p12, out vec2 p3, out vec2 a, out vec2 b, out bool line) {
    // Contour segments share endpoints: the next texel starts with this segment's end.
    ivec4 fixed12 = curve_texel(curve);
    ivec2 fixed3 = curve_texel(curve + 1).xy;
    // Power-of-two scaling of exact integers reproduces the packed coordinates exactly.
    vec4 absolute12 = vec4(fixed12) * u_curve_scale;
    vec2 absolute3 = vec2(fixed3) * u_curve_scale;
    // Derive the polynomial before translating to the fragment. Large font-unit
    // coordinates otherwise give mathematically linear axes a spurious quadratic term.
    a = absolute12.xy - 2.0 * absolute12.zw + absolute3.xy;
    b = absolute12.xy - absolute12.zw;
    // Lines store their end as the control point; integer equality is exact.
    line = all(equal(fixed12.zw, fixed3));
    p12 = absolute12 - vec4(sample_position, sample_position);
    p3 = absolute3.xy - sample_position;
}

void accumulate(vec2 sample_position, vec2 pixels_per_unit, bool collect_distance, out float xcov, out float ycov, out float xwgt, out float ywgt, out float edge_distance, out vec4 candidates[3]) {
    edge_distance=1e6;
#ifdef IPP_CANVAS_PAINTS
    for(int index=0;index<3;index++) candidates[index]=vec4(1e6,1e6,1,0);
#endif
    xcov = 0.0; ycov = 0.0; xwgt = 0.0; ywgt = 0.0;
    vec2 extent = max(v_bounds.zw - v_bounds.xy, vec2(1.0 / 65536.0));
    ivec2 band = clamp(ivec2((sample_position - v_bounds.xy) / extent * 16.0), ivec2(0), ivec2(15));
    int horizontal = v_band_offset + band.y * 2;
    int vertical = v_band_offset + 32 + band.x * 2;
    int grid = band_value(v_band_offset + 64);
    // A tile margin covers the entire pixel footprint. Retain full band
    // winding for minification, including oblique/projected presentation.
    if (grid > 0 && all(lessThanEqual(1.0 / pixels_per_unit, extent / float(grid)))) {
        ivec2 tile = clamp(ivec2((sample_position - v_bounds.xy) / extent * float(grid)), ivec2(0), ivec2(grid - 1));
        horizontal = v_band_offset + band_value(v_band_offset + 65) + (tile.y * grid + tile.x) * 4;
        vertical = horizontal + 2;
    }
    int list = v_band_offset + band_value(horizontal);
    int count = band_value(horizontal + 1);
    for (int index = 0; index < count; ++index) {
        int curve = v_curve_start + band_value(list + index);
        vec4 p12; vec2 p3; vec2 a; vec2 b; bool line;
        curve_points(curve, sample_position, p12, p3, a, b, line);
#ifdef IPP_CANVAS_PAINTS
        if(collect_distance) {
            vec3 nearest=contour_point(p12,p3,a,b,line);
            edge_distance=min(edge_distance,length(nearest.xy));
            if(u_path_paint.w>0.0) retain_candidate(nearest,line ? p3-p12.xy : 2.0*(a*nearest.z-b),candidates);
        }
#endif
        uint code = root_code(p12.y, p12.w, p3.y);
        if (code != 0u) {
            vec2 r = solve_horizontal(p12, p3, a, b, line) * pixels_per_unit.x;
            if ((code & 1u) != 0u) { xcov += clamp(r.x + 0.5, 0.0, 1.0); xwgt = max(xwgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0)); }
            if (code > 1u) { xcov -= clamp(r.y + 0.5, 0.0, 1.0); xwgt = max(xwgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0)); }
        }
    }
    list = v_band_offset + band_value(vertical);
    count = band_value(vertical + 1);
    for (int index = 0; index < count; ++index) {
        int curve = v_curve_start + band_value(list + index);
        vec4 p12; vec2 p3; vec2 a; vec2 b; bool line;
        curve_points(curve, sample_position, p12, p3, a, b, line);
#ifdef IPP_CANVAS_PAINTS
        if(collect_distance) {
            vec3 nearest=contour_point(p12,p3,a,b,line);
            edge_distance=min(edge_distance,length(nearest.xy));
            if(u_path_paint.w>0.0) retain_candidate(nearest,line ? p3-p12.xy : 2.0*(a*nearest.z-b),candidates);
        }
#endif
        uint code = root_code(p12.x, p12.z, p3.x);
        if (code != 0u) {
            vec2 r = solve_vertical(p12, p3, a, b, line) * pixels_per_unit.y;
            if ((code & 1u) != 0u) { ycov -= clamp(r.x + 0.5, 0.0, 1.0); ywgt = max(ywgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0)); }
            if (code > 1u) { ycov += clamp(r.y + 0.5, 0.0, 1.0); ywgt = max(ywgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0)); }
        }
    }
}

#ifdef IPP_CANVAS_PAINTS
// Closest-point winding probes reject internal overlapping stroke-strip caps.
// Only a near-contour painted fragment pays this cost; the probes reuse the
// ordinary coverage bands and never collect/refine contour distances themselves.
bool path_interior(vec2 position) {
    float xcov,ycov,xwgt,ywgt,ignored;
    vec4 candidates[3];
    accumulate(position,vec2(65536.0),false,xcov,ycov,xwgt,ywgt,ignored,candidates);
    float winding=min(abs(xcov),abs(ycov));
    return u_fill_rule==0 ? winding>0.5 : mod(floor(winding+0.5),2.0)>0.5;
}
#endif

void main() {
    // Derivatives precede every discard: GLSL ES 3.00 leaves them undefined once a
    // fragment of the quad has discarded.
    vec2 pixels_per_unit = 1.0 / max(fwidth(v_path_position), vec2(1.0 / 65536.0));
    vec2 clip_width = max(fwidth(v_surface_position), vec2(1.0 / 65536.0));
    vec2 clip_inside = min(v_surface_position - u_clip.xy, u_clip.zw - v_surface_position);
    float clip_coverage = clamp(min(clip_inside.x / clip_width.x + 0.5, clip_inside.y / clip_width.y + 0.5), 0.0, 1.0);
#ifndef IPP_CANVAS_PAINTS
    if (clip_coverage <= 0.0) discard;
#endif
    float xcov, ycov, xwgt, ywgt, edge_distance;
    vec4 candidates[3];
    accumulate(v_path_position, pixels_per_unit, u_path_paint.x>0.0, xcov, ycov, xwgt, ywgt, edge_distance,candidates);
    float coverage = max(abs(xcov * xwgt + ycov * ywgt) / max(xwgt + ywgt, 1.0 / 65536.0), min(abs(xcov), abs(ycov)));
    coverage = u_fill_rule == 0 ? clamp(coverage, 0.0, 1.0) : 1.0 - abs(1.0 - fract(coverage * 0.5) * 2.0);
#ifndef IPP_CANVAS_PAINTS
    if (coverage <= 0.0) discard;
#endif
    vec4 color=v_color;
#ifdef IPP_CANVAS_PAINTS
    if(u_path_paint.x>0.0 && u_path_paint.w>0.0 && edge_distance<2.0 && coverage>0.0) {
        edge_distance=2.0;
        for(int index=0;index<3;index++) {
            vec4 candidate=candidates[index];
            if(length(candidate.xy)>=2.0) break;
            vec2 point=v_path_position+candidate.xy;
            vec2 offset=candidate.zw*0.025;
            if(path_interior(point+offset)!=path_interior(point-offset)) {
                edge_distance=length(candidate.xy);
                break;
            }
        }
    }
#endif
    #ifdef IPP_CANVAS_PAINTS
    if(u_path_paint.x>0.0) {
        color=clamp(ipp_canvas_paint(int(u_path_paint.x),int(u_path_paint.y),v_path_position-v_bounds.xy,v_bounds.zw-v_bounds.xy,v_color,-edge_distance,vec4(0),v_surface_position),0.0,1.0);
        color.a*=u_path_paint.z;
    }
    #endif
    if(coverage<=0.0 || clip_coverage<=0.0) discard;
    o_color = vec4(color.rgb, color.a * coverage * clip_coverage);
}
