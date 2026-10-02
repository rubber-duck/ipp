#version 300 es
// SPDX-License-Identifier: MIT OR Apache-2.0
// GUI-only parameterized boxes, strokes and arcs with gradients, colour fields,
// two-sided edge glow and custom paints, each clipped by its own rectangle. A box
// may cut any corner at 45 degrees, thicken its border near corners and paint a
// checker beneath its fill; a stroke is one or two butt-capped segments; an arc is a
// butt-capped ring sector, optionally dashed. Every shape is a closed-form distance
// evaluation without loops or curve evaluation; atlas glyphs draw with
// surface_glyph.frag. Colors are straight linear RGBA; the single display conversion
// happens downstream in the present pass. Colour fields evaluate the HSV model on
// sRGB-encoded values and decode them to linear here, so that conversion returns the
// model's sRGB colour.
precision highp float;

// Paints. A paint is its shape's offset plus, for a box with a checker, the
// checker's, plus its fill type: solid (0), linear (1), radial (2), hue,
// saturation-value or a custom paint. Equal to the GUI_PAINT_* and GUI_FILL_*
// constants in gui_batch.rs; a Rust unit test compares them.
const float GUI_PAINT_STROKE = 16.0;
const float GUI_PAINT_ARC = 32.0;
const float GUI_PAINT_CHECKER = 8.0;
const float GUI_FILL_HUE = 3.0;
const float GUI_FILL_SATURATION_VALUE = 4.0;
const float GUI_FILL_PAINT = 5.0;
// A painted primitive packs its parameter block and slot as block * stride + slot.
const float GUI_PAINT_BLOCK_STRIDE = 16.0;
const float INV_SQRT2 = 0.70710678;
// A whole ring's half sweep, exactly as gui_batch.rs writes it; a Rust unit test
// compares them.
const float PI = 3.14159265;
const float TAU = 6.28318531;
// Distance of an absent feature, beyond any contour or glow reach.
const float FAR = 1.0e6;

in vec2 v_surface_position;
flat in vec4 v_placement; // position.xy, size.xy; a stroke's own bounds
flat in vec4 v_shape; // box: corner_rx, corner_ry, border_width (stroke thickness), accent_width; arc: radius, half sweep, thickness, duty
flat in vec4 v_corner_cut; // box: cut per corner [tl, tr, br, bl]; stroke: first segment [center.xy, half.xy]; arc: [center.xy, middle.xy]
flat in vec4 v_corner_accent; // box: accent span per corner [tl, tr, br, bl], or checker [cell, packed colours, packed alphas]; stroke: second segment; arc: [sin, cos of half sweep, signed dashes, 0]
flat in vec4 v_color0; // straight linear start/solid RGBA; colour fields: tint and opacity
flat in vec4 v_color1; // straight linear end RGBA; saturation-value: [hue, 0, 0, 0]
flat in vec4 v_border_color; // straight linear border RGBA
flat in vec4 v_gradient_coords; // linear and hue: [start.xy, end.xy], radial: [center.xy, radius, 0.0], saturation-value: part rectangle [min.xy, max.xy]
flat in vec4 v_material_params; // paint (fill type, +8 checker, +16 stroke, +32 arc), glow_inner_radius, glow_radius, glow_falloff
flat in vec4 v_glow_color; // straight linear glow RGB; alpha times intensity
flat in vec4 v_clip; // min.xy, max.xy

out vec4 o_color;

float max4(vec4 value) {
    return max(max(value.x, value.y), max(value.z, value.w));
}

float min4(vec4 value) {
    return min(min(value.x, value.y), min(value.z, value.w));
}

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
    vec2 q = abs(offset) - (half_size - radius);
    // Within the straight-edged core an edge is nearest, so the inner glow keeps
    // falling off where the corner ellipse would report a constant depth.
    if (max(q.x, q.y) <= 0.0) return max(q.x - radius.x, q.y - radius.y);
    return sd_ellipse(max(q, vec2(0.0)), radius);
}

// Pixel coverage of the band |x| <= half_extent for a footprint across it: the
// difference of its two edges' coverages, so a band narrower than a pixel keeps its
// area instead of the contrast loss of multiplied ramps.
vec2 band_coverage(vec2 x, vec2 half_extent, vec2 footprint) {
    return clamp((half_extent - x) / footprint + 0.5, 0.0, 1.0)
        - clamp((-half_extent - x) / footprint + 0.5, 0.0, 1.0);
}

float band_coverage(float x, float half_extent, float footprint) {
    return clamp((half_extent - x) / footprint + 0.5, 0.0, 1.0)
        - clamp((-half_extent - x) / footprint + 0.5, 0.0, 1.0);
}

// Linear-light gradient between straight RGBA stops, interpolated premultiplied so
// a transparent stop contributes no colour fringe, then returned straight again.
vec4 gradient_color(float t) {
    vec4 start = vec4(v_color0.rgb * v_color0.a, v_color0.a);
    vec4 end = vec4(v_color1.rgb * v_color1.a, v_color1.a);
    vec4 color = mix(start, end, t);
    return color.a > 0.0 ? vec4(color.rgb / color.a, color.a) : vec4(0.0);
}

// Fraction along the axis [start.xy, end.xy] of the gradient lanes, clamped to it.
float gradient_axis(vec2 local) {
    vec2 p0 = v_gradient_coords.xy;
    vec2 dir = v_gradient_coords.zw - p0;
    float len_sq = dot(dir, dir);
    return (len_sq > 1e-12) ? clamp(dot(local - p0, dir) / len_sq, 0.0, 1.0) : 0.0;
}

// sRGB-encoded colour of a hue in turns at full saturation and value. Each channel
// is a trapezoid of the hue, ramping linearly in encoded values between the primary
// and secondary colours a sixth of a turn apart, as the HSV model defines it.
vec3 hue_color(float hue) {
    return clamp(abs(fract(hue + vec3(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0, 0.0, 1.0);
}

// The sRGB transfer function's inverse. The target and the present pass apply the
// function itself, so an encoded colour decoded here is displayed as itself.
vec3 srgb_to_linear(vec3 encoded) {
    return mix(
        pow((encoded + 0.055) / 1.055, vec3(2.4)),
        encoded / 12.92,
        vec3(lessThanEqual(encoded, vec3(0.04045)))
    );
}

// Straight linear RGBA of a colour field at `local`: the hue along the gradient axis,
// or the saturation-value field of the hue in v_color1.x over the part rectangle,
// saturation rising along +x and value against +y. The model's sRGB colour is
// decoded, then takes the tint and opacity in v_color0.
vec4 colour_field(float fill_type, vec2 local) {
    vec3 encoded;
    if (fill_type < GUI_FILL_HUE + 0.5) {
        encoded = hue_color(gradient_axis(local));
    } else {
        vec2 size = v_gradient_coords.zw - v_gradient_coords.xy;
        // A zero extent divides by one instead; such a box paints nothing.
        vec2 sv = clamp((local - v_gradient_coords.xy) / (size + vec2(equal(size, vec2(0.0)))), 0.0, 1.0);
        encoded = mix(vec3(1.0), hue_color(v_color1.x), sv.x) * (1.0 - sv.y);
    }
    return vec4(srgb_to_linear(encoded) * v_color0.rgb, v_color0.a);
}

// Integral from zero of the square wave that is +1 over even cells and -1 over odd
// ones: a triangle wave of period two cells.
vec2 checker_wave_integral(vec2 x) {
    return 1.0 - abs(2.0 * fract(x * 0.5) - 1.0);
}

// Straight RGB of a checker colour packed as three 8-bit channels on a square-root
// curve. The lane holds an exact integer below 2^24, which converts exactly.
vec3 checker_rgb(float lane) {
    uint bits = uint(lane);
    vec3 root = vec3((uvec3(bits) >> uvec3(16u, 8u, 0u)) & 0xFFu) / 255.0;
    return root * root;
}

// Premultiplied checker colour over the pixel footprint of `local`, the fragment in
// the box's own orientation from its top-left corner, whose cell takes the first
// colour. The cells are a product of two square waves, so the box filter of the
// footprint's extent along each axis is exact and keeps thin cells' area. A footprint
// of a cell or more cannot resolve cells, and the box filter would leave a beat
// between neighbouring pixels: the waves fade out there, leaving the colours' mean.
vec4 checker_color(vec2 local, vec2 dx, vec2 dy) {
    float cell = v_corner_accent.x;
    vec2 cells = local / cell;
    vec2 width = max((abs(dx) + abs(dy)) / cell, vec2(1.0 / 65536.0));
    vec2 wave = (checker_wave_integral(cells + 0.5 * width) - checker_wave_integral(cells - 0.5 * width))
        / width;
    wave *= 1.0 - smoothstep(0.5, 1.0, width);
    float second = 0.5 - 0.5 * wave.x * wave.y;
    uint alphas = uint(v_corner_accent.w);
    vec2 alpha = vec2(uvec2(alphas >> 8u, alphas & 0xFFu)) / 255.0;
    return mix(
        vec4(checker_rgb(v_corner_accent.y) * alpha.x, alpha.x),
        vec4(checker_rgb(v_corner_accent.z) * alpha.y, alpha.y),
        second
    );
}

// Box contour distance, contour coverage and fill coverage inside the border ring.
//
// The contour is the rounded box intersected with one 45-degree half-plane per cut
// corner; an uncut corner keeps the radius. Inside, the larger of the box and cut
// distances is exact. Outside, the nearest contour point is the box point nearest the
// fragment unless a cut removed that point, and then it lies on that cut, so the
// distance stays exact and glow rounds the cut's ends instead of mitring. The
// renderer clamps cuts so they never overlap, which keeps that removed point unique.
// The border's inner contour is the same shape inset by the border width, and within
// an accent span by the accent width instead.
// A checker holds the accent lanes, so a box with a checker has no accents.
void box_coverage(bool checker, out float outer, out float shape_cov, out float fill_cov) {
    vec2 half_size = abs(v_placement.zw) * 0.5;
    vec2 center = v_placement.xy + v_placement.zw * 0.5;
    // Corners follow the box's own orientation: mirroring a box mirrors its corners.
    vec2 p = (v_surface_position - center)
        * vec2(v_placement.z < 0.0 ? -1.0 : 1.0, v_placement.w < 0.0 ? -1.0 : 1.0);
    vec4 cut = v_corner_cut;
    float border_width = max(v_shape.z, 0.0);
    // Clamp each explicit radius to the corresponding placed half size; a cut corner
    // has none.
    float quadrant_cut = p.y < 0.0 ? (p.x < 0.0 ? cut.x : cut.y) : (p.x < 0.0 ? cut.w : cut.z);
    vec2 corner = quadrant_cut > 0.0 ? vec2(0.0) : min(max(v_shape.xy, vec2(0.0)), half_size);

    outer = sd_round_box(p, half_size, corner);
    float cut_plane = -FAR;
    // Cuts are flat, so every invocation of a quad takes the same branch.
    if (max4(cut) > 0.0) {
        float diagonal = half_size.x + half_size.y;
        vec4 planes = (vec4(-p.x - p.y, p.x - p.y, p.x + p.y, p.y - p.x) - (diagonal - cut))
            * INV_SQRT2;
        cut_plane = max4(planes);
        float exact = max(outer, cut_plane);
        if (exact > 0.0) {
            vec2 nearest = clamp(p, -half_size, half_size);
            vec4 removed = vec4(
                -nearest.x - nearest.y,
                nearest.x - nearest.y,
                nearest.x + nearest.y,
                nearest.y - nearest.x
            ) - (diagonal - cut);
            // Beyond either end of a cut, along its direction.
            vec4 along = max(
                abs(vec4(p.y - p.x, p.x + p.y, p.x - p.y, -p.x - p.y) - (half_size.x - half_size.y)) - cut,
                vec4(0.0)
            ) * INV_SQRT2;
            vec4 to_cut = along * along + planes * planes;
            exact = max(outer, sqrt(max4(mix(vec4(0.0), to_cut, greaterThan(removed, vec4(0.0))))));
        }
        outer = exact;
    }

    float inner = max(
        sd_round_box(p, max(half_size - border_width, vec2(0.0)), max(corner - border_width, vec2(0.0))),
        cut_plane + border_width
    );
    // Each distance spans one pixel across its footprint. A two-footprint
    // smoothstep blurs subpixel rails and borders into the surrounding halo.
    float edge = max(length(vec2(dFdx(outer), dFdy(outer))), 1.0 / 65536.0);
    float inner_edge = max(length(vec2(dFdx(inner), dFdy(inner))), 1.0 / 65536.0);

    // Shape interior coverage [0.0, 1.0]; the fill ends at the inner border contour.
    shape_cov = clamp(0.5 - outer / edge, 0.0, 1.0);
    fill_cov = min(shape_cov, clamp(0.5 - inner / inner_edge, 0.0, 1.0));

    vec4 span = v_corner_accent;
    if (!checker && max4(span) > 0.0) {
        float accent_width = max(v_shape.w, 0.0);
        float accent_inner = max(
            sd_round_box(p, max(half_size - accent_width, vec2(0.0)), max(corner - accent_width, vec2(0.0))),
            cut_plane + accent_width
        );
        // Distance to the accented corner squares, measured from the rectangle's
        // corners; the square's sides are the accent's butt ends within the ring.
        vec4 reach = max(
            half_size.x + vec4(p.x, -p.x, -p.x, p.x),
            half_size.y + vec4(p.y, p.y, -p.y, -p.y)
        ) - span;
        float accent = min4(mix(vec4(FAR), reach, greaterThan(span, vec4(0.0))));
        float accent_inner_edge = max(length(vec2(dFdx(accent_inner), dFdy(accent_inner))), 1.0 / 65536.0);
        float accent_edge = max(length(vec2(dFdx(accent), dFdy(accent))), 1.0 / 65536.0);
        float accent_fill = min(shape_cov, clamp(0.5 - accent_inner / accent_inner_edge, 0.0, 1.0));
        fill_cov = mix(fill_cov, accent_fill, clamp(0.5 - accent / accent_edge, 0.0, 1.0));
    }
}

// Coverage and distance of one butt-capped segment [center.xy, half.xy], combined
// with earlier segments by union. A zero half vector paints nothing.
void segment_coverage(
    vec4 segment,
    vec2 local,
    vec2 dx,
    vec2 dy,
    float half_width,
    inout float coverage,
    inout float nearest
) {
    float half_length = length(segment.zw);
    if (half_length <= 0.0) return;
    vec2 axis = segment.zw / half_length;
    vec2 normal = vec2(-axis.y, axis.x);
    vec2 offset = local - segment.xy;
    vec2 q = vec2(dot(offset, axis), dot(offset, normal));
    // Pixel footprint along and across the segment.
    vec2 footprint = max(
        vec2(
            length(vec2(dot(dx, axis), dot(dy, axis))),
            length(vec2(dot(dx, normal), dot(dy, normal)))
        ),
        vec2(1.0 / 65536.0)
    );
    vec2 extent = vec2(half_length, half_width);
    vec2 band = band_coverage(q, extent, footprint);
    coverage = max(coverage, band.x * band.y);
    nearest = min(nearest, sd_box(q, extent));
}

// Stroke distance and coverage: the union of its segments, painted by the fill.
void stroke_coverage(out float outer, out float shape_cov) {
    vec2 local = v_surface_position - v_placement.xy;
    // Derivatives precede the segments' flat branches.
    vec2 dx = dFdx(local);
    vec2 dy = dFdy(local);
    float half_width = max(v_shape.z, 0.0) * 0.5;
    outer = FAR;
    shape_cov = 0.0;
    segment_coverage(v_corner_cut, local, dx, dy, half_width, shape_cov, outer);
    segment_coverage(v_corner_accent, local, dx, dy, half_width, shape_cov, outer);
}

// Signed distance to an arc end from `q`, the fragment in the end's frame: +y along
// its radial line from the centre and +x across it, positive beyond the arc. The
// line's side gives the sign and the end's segment across the ring the magnitude, so
// glow rounds the end's corners, and the line's continuation through the centre
// does not count as an edge.
float arc_end_distance(vec2 q, float radius, float half_width) {
    return sign(q.x) * length(vec2(q.x, max(abs(q.y - radius) - half_width, 0.0)));
}

// Integral from zero to x of a dash train with one dash of length `duty` at the
// start of every unit.
float dash_integral(float x, float duty) {
    return floor(x) * duty + min(fract(x), duty);
}

// Ring sector distance and coverage, painted by the fill.
//
// The arc's frame has +y through the middle of its sweep and +x along the sweep, so
// the sector spans the angles [-half_sweep, half_sweep] from +y and starts at
// -half_sweep. The distance is exact, so glow follows the contour round the butt
// ends. A plain arc folds its frame about the middle, making the nearer end the one
// at +half_sweep, and meets the ring's band distance with that end's distance
// without a transcendental function. A dashed arc measures the fragment's angle and
// takes the distance to the dash of its cell, the nearest one; behind a partial arc
// the cells split halfway between its first and last dash.
//
// Coverage is the band's across the ring, the difference of its two circles, so a
// ring thinner than a pixel keeps its area, times the coverage along the ring. A
// plain arc derives that from its ends' distances, combining both ends where the
// sweep or the gap is narrow enough for one pixel to span them. A dashed arc
// box-filters its dash train, clipped to the sweep, over the pixel's footprint
// along the ring: thin dashes keep their area and minified dashes fade to their
// duty instead of aliasing.
void arc_coverage(out float outer, out float shape_cov) {
    vec2 local = v_surface_position - v_placement.xy - v_corner_cut.xy;
    // Derivatives precede the flat branches below.
    vec2 dx = dFdx(local);
    vec2 dy = dFdy(local);
    float radius = v_shape.x;
    float half_sweep = v_shape.y;
    float half_width = max(v_shape.z, 0.0) * 0.5;
    vec2 middle = v_corner_cut.zw;
    vec2 cap = v_corner_accent.xy;
    float cells = v_corner_accent.z;
    vec2 along = vec2(-middle.y, middle.x) * (cells < 0.0 ? -1.0 : 1.0);
    vec2 p = vec2(dot(local, along), dot(local, middle));
    float r = length(p);
    vec2 radial = r > 0.0 ? local / r : middle;
    vec2 tangent = vec2(-radial.y, radial.x);
    // Pixel footprint across and along the ring.
    float across = max(length(vec2(dot(dx, radial), dot(dy, radial))), 1.0 / 65536.0);
    float around = max(length(vec2(dot(dx, tangent), dot(dy, tangent))), 1.0 / 65536.0);
    float band = abs(r - radius) - half_width;
    float ring = band_coverage(r - radius, half_width, across);
    // The batch writes exactly PI for a whole ring; a narrower gap than this
    // tolerance would be invisible anyway.
    bool full = half_sweep > PI - 1.0e-5;

    if (cells != 0.0) {
        // Positions along the arc in cells from its start. The arc spans `span`
        // cells, each centring a dash, so dashes run from `gap` to `last_end`.
        float count = abs(cells);
        float duty = v_shape.w;
        float gap = (1.0 - duty) * 0.5;
        float span = half_sweep * count / PI;
        float last = max(ceil(span - gap) - 1.0, 0.0);
        float last_end = min(last + gap + duty, span);
        float phi = r > 0.0 ? atan(p.x, p.y) : 0.0;
        float u = (phi + half_sweep) * count / TAU;
        float width = max(around * count / (TAU * max(r, 1.0 / 65536.0)), 1.0 / 65536.0);
        // A whole ring has no ends to clip the train at.
        float lo = (full ? u - 0.5 * width : clamp(u - 0.5 * width, 0.0, span)) - gap;
        float hi = (full ? u + 0.5 * width : clamp(u + 0.5 * width, 0.0, span)) - gap;
        // Whole cells add whole dashes to both ends of the window, so measuring from
        // its first cell keeps the difference precise.
        float base = floor(lo);
        float dashed = (dash_integral(hi - base, duty) - dash_integral(lo - base, duty)) / width;
        // A footprint of a cell or more cannot resolve dashes, and the box filter
        // would leave a beat between neighbouring pixels: fade to the dashes' mean
        // over the part of the footprint within the sweep.
        dashed = mix(dashed, duty * (hi - lo) / width, smoothstep(0.5, 1.0, width));
        shape_cov = ring * clamp(dashed, 0.0, 1.0);

        float seam = gap - 0.5 * (count - (last_end - gap));
        float cell = u - count * floor((u - seam) / count);
        float first = clamp(floor(cell), 0.0, last) + gap;
        float end = min(first + duty, span);
        float offset = phi - ((first + end) * PI / count - half_sweep);
        offset -= TAU * floor(offset / TAU + 0.5);
        float beyond = abs(offset) - (end - first) * PI / count;
        outer = max(band, arc_end_distance(r * vec2(sin(beyond), cos(beyond)), radius, half_width));
        return;
    }

    if (full) {
        outer = band;
        shape_cov = ring;
        return;
    }
    vec2 folded = vec2(abs(p.x), p.y);
    float end = arc_end_distance(
        vec2(folded.x * cap.y - folded.y * cap.x, folded.x * cap.x + folded.y * cap.y),
        radius,
        half_width
    );
    float angular = clamp(0.5 - end / around, 0.0, 1.0);
    // Where the sweep or the gap is under a quarter turn, both ends can share a
    // pixel: a narrow sweep intersects their coverages and a narrow gap unites them.
    if (half_sweep < 0.25 * PI || half_sweep > 0.75 * PI) {
        float other = arc_end_distance(
            vec2(-folded.x * cap.y - folded.y * cap.x, folded.y * cap.y - folded.x * cap.x),
            radius,
            half_width
        );
        angular += clamp(0.5 - other / around, 0.0, 1.0) - (half_sweep < 0.25 * PI ? 1.0 : 0.0);
    }
    outer = max(band, end);
    shape_cov = ring * clamp(angular, 0.0, 1.0);
}

// Custom paints. The canvas program replaces the placeholder below with the paint
// parameter array, one function per admitted paint and a dispatch on the slot that
// defines IPP_CANVAS_PAINTS; without paints this dispatch keeps the box's colour.
// The paint guide, CANVAS_PAINTS.md, owns the function interface.
// CANVAS_PAINTS
#ifndef IPP_CANVAS_PAINTS
vec4 ipp_canvas_paint(int slot, int block, vec2 position, vec2 size, vec4 color, float edge) {
    return color;
}
#endif

// Straight linear RGBA of a custom paint at `local`, the fragment from the placement
// origin, `outer` its distance from the contour. The colour lanes hold the paint's
// colour input; the slot, parameter block, opacity and signed visual scale; and the
// part rectangle's origin from the placement origin and its own size. The function
// sees the fragment in the shape's own units from its own top-left corner, so a
// mirrored box mirrors its paint, and the distance in those units; the opacity
// scales the alpha it returns.
vec4 paint_fill(vec2 local, float outer) {
    vec2 scale = v_color1.zw;
    float lane = floor(v_color1.x + 0.5);
    float slot = mod(lane, GUI_PAINT_BLOCK_STRIDE);
    float block = floor(lane / GUI_PAINT_BLOCK_STRIDE);
    vec4 color = ipp_canvas_paint(
        int(slot),
        int(block),
        (local - v_gradient_coords.xy) / scale,
        v_gradient_coords.zw,
        v_color0,
        outer / max(abs(scale.x), abs(scale.y))
    );
    color = clamp(color, 0.0, 1.0);
    return vec4(color.rgb, color.a * v_color1.y);
}

// Glow strength at a normalized distance from the contour.
float glow_alpha(float reach, float falloff) {
    float norm = clamp(1.0 - reach, 0.0, 1.0);
    float factor = (abs(falloff - 1.0) < 1e-5) ? norm : pow(norm, falloff);
    return clamp(v_glow_color.a * factor, 0.0, 1.0);
}

// Straight RGB and unclipped alpha of a box, stroke or arc fragment.
//
// The paint is flat, so every invocation of a 2x2 quad, which shades a single
// primitive, takes the same branch and the derivatives below stay defined. A zero border evaluates the inner contour as
// the outer one. Strokes and arcs have no border ring: the fill paints them.
vec4 shape_color(float paint) {
    // Shape offsets are multiples of the stroke's, each followed by fill types and
    // the checker's offset.
    float fill_type = mod(paint, GUI_PAINT_CHECKER);
    float variant = mod(paint, GUI_PAINT_STROKE);
    bool checker = variant > GUI_PAINT_CHECKER - 0.5;
    float shape = paint - variant;
    float outer;
    float shape_cov;
    float fill_cov;
    if (shape > GUI_PAINT_ARC - 0.5) {
        arc_coverage(outer, shape_cov);
        fill_cov = shape_cov;
    } else if (shape > GUI_PAINT_STROKE - 0.5) {
        stroke_coverage(outer, shape_cov);
        fill_cov = shape_cov;
    } else {
        box_coverage(checker, outer, shape_cov, fill_cov);
    }

    // Fill color evaluation: solid, linear gradient, radial gradient, colour field or
    // custom paint.
    vec4 fill_color = v_color0;
    vec2 local_pos = v_surface_position - v_placement.xy;
    if (fill_type > GUI_FILL_PAINT - 0.5) {
        fill_color = paint_fill(local_pos, outer);
    } else if (fill_type > 0.5 && fill_type < 1.5) {
        fill_color = gradient_color(gradient_axis(local_pos));
    } else if (fill_type > 1.5 && fill_type < 2.5) {
        vec2 center_pt = v_gradient_coords.xy;
        float radius = v_gradient_coords.z;
        float dist = length(local_pos - center_pt);
        float t = (radius > 1e-6) ? clamp(dist / radius, 0.0, 1.0) : 0.0;
        fill_color = gradient_color(t);
    } else if (fill_type > 2.5) {
        fill_color = colour_field(fill_type, local_pos);
    }

    // Edge glow falls off from the outer contour. Outward it occupies only the pixel
    // area outside the shape, so its single attenuation is that uncovered fraction and
    // the halo never fills a transparent shape or focus ring. Inward it lies over the
    // fill, beneath the border.
    float outer_glow = 0.0;
    float inner_glow = 0.0;
    if (v_glow_color.a > 0.0) {
        float outer_radius = v_material_params.z;
        if (outer_radius > 0.0 && outer < outer_radius) {
            outer_glow = glow_alpha(max(outer, 0.0) / outer_radius, v_material_params.w)
                * (1.0 - shape_cov);
        }
        float inner_radius = v_material_params.y;
        if (inner_radius > 0.0 && -outer < inner_radius) {
            inner_glow = glow_alpha(max(-outer, 0.0) / inner_radius, v_material_params.w);
        }
    }

    // Coverage of the ring is the difference of its two contours. Multiplying
    // two antialias ramps loses contrast when a border is narrower than a pixel.
    // Fill, ring and outer glow cover disjoint parts of the pixel: composite
    // premultiplied linear RGBA.
    vec4 fill = vec4(fill_color.rgb * fill_color.a, fill_color.a);
    // A checker lies beneath the fill, in the box's own orientation; its paint is
    // flat, so the derivatives stay defined.
    if (checker) {
        vec2 own = local_pos * sign(v_placement.zw);
        fill += checker_color(own, dFdx(own), dFdy(own)) * (1.0 - fill.a);
    }
    fill = vec4(v_glow_color.rgb, 1.0) * inner_glow + fill * (1.0 - inner_glow);
    vec4 color = fill * fill_cov
        + vec4(v_border_color.rgb * v_border_color.a, v_border_color.a) * (shape_cov - fill_cov)
        + vec4(v_glow_color.rgb, 1.0) * outer_glow;
    if (color.a <= 0.0) return vec4(0.0);

    return vec4(color.rgb / color.a, color.a);
}

void main() {
    // Every screen-space derivative precedes the discard: GLSL ES 3.00 leaves
    // derivatives undefined once a fragment of the quad has discarded.
    vec2 clip_width = max(fwidth(v_surface_position), vec2(1.0 / 65536.0));
    vec2 clip_inside = min(v_surface_position - v_clip.xy, v_clip.zw - v_surface_position);
    float clip_coverage = clamp(min(clip_inside.x / clip_width.x + 0.5, clip_inside.y / clip_width.y + 0.5), 0.0, 1.0);

    vec4 color = shape_color(v_material_params.x);
    float out_alpha = color.a * clip_coverage;
    if (out_alpha <= 0.0) discard;

    o_color = vec4(color.rgb, out_alpha);
}
