#version 450
// Premultiplied linear output into the sRGB color attachment.
layout(location=0) in vec2 vertex_local;
layout(location=1) flat in uint slot;
layout(location=2) in vec4 vertex_world;
layout(location=3) in vec3 vertex_right;
layout(location=4) in vec3 vertex_down;
layout(location=5) in vec3 vertex_normal;
layout(location=0) out vec4 out_color;
layout(set=0,binding=0) readonly buffer DataRecords { vec4 paints[]; };
layout(set=0,binding=1) uniform sampler nearest_sampler;
layout(set=1,binding=2) uniform texture2D layer_pixels;
layout(set=1,binding=0) uniform texture2D pixels;
layout(set=1,binding=1) uniform sampler pixel_sampler;
layout(set=2,binding=0) readonly buffer PathRecords { vec4 paths[]; };
vec4 record_at(uint i) {
    return paths[i];
}
vec4 paint_at(uint i) {
    return paints[i];
}
struct CanvasPaint {
    vec4 tint;
    vec4 bounds;
    vec4 fill[2];
    vec4 gradient;
    vec4 border;
    vec4 options;
    vec4 shape;
    vec4 segment[2];
    vec4 cut;
    vec4 accent;
    vec4 glow_color;
    vec4 glow;
    vec4 checker[2];
    vec4 section;
    vec4 clip;
    vec4 paint_origin_scale;
    vec4 paint_size_opacity;
    vec4 paint_data;
    vec4 path_parameters;
};

CanvasPaint fetch_paint(uint base) {
    CanvasPaint p;
    p.tint = paint_at(base + 0u);
    p.bounds = paint_at(base + 1u);
    p.fill[0] = paint_at(base + 2u);
    p.fill[1] = paint_at(base + 3u);
    p.gradient = paint_at(base + 4u);
    p.border = paint_at(base + 5u);
    p.options = paint_at(base + 6u);
    p.shape = paint_at(base + 7u);
    p.segment[0] = paint_at(base + 8u);
    p.segment[1] = paint_at(base + 9u);
    p.cut = paint_at(base + 10u);
    p.accent = paint_at(base + 11u);
    p.glow_color = paint_at(base + 12u);
    p.glow = paint_at(base + 13u);
    p.checker[0] = paint_at(base + 14u);
    p.checker[1] = paint_at(base + 15u);
    p.section = paint_at(base + 16u);
    p.clip = paint_at(base + 17u);
    p.paint_origin_scale = paint_at(base + 18u);
    p.paint_size_opacity = paint_at(base + 19u);
    p.paint_data = paint_at(base + 20u);
    p.path_parameters = paint_at(base + 21u);
    return p;
}

vec4 world_point;
vec3 right_axis,down_axis,normal_axis;
const uint IPP_PARAMETER_OFFSET = 23u;
uint light_base() { return uint(paint_at(slot+21u).y); }
vec3 unit_direction(vec3 v) { return v * inversesqrt(max(dot(v,v),1e-20)); }
vec3 surface_direction(vec3 v) {
    vec3 x=unit_direction(right_axis);
    vec3 y=unit_direction(down_axis-x*dot(down_axis,x));
    return vec3(dot(v,x),dot(v,y),dot(v,unit_direction(normal_axis)));
}
vec3 canvas_view() {
    if (world_point.w == 0.0) return vec3(0,0,1);
    vec4 eye=paint_at(light_base());
    return surface_direction(eye.w>0.5 ? eye.xyz : unit_direction(eye.xyz-world_point.xyz));
}
int canvas_light_count() { return world_point.w==0.0 ? 0 : int(paint_at(light_base()+1u).w); }
vec3 sceneLightDirection(int index) {
    if (index<0 || index>=canvas_light_count()) return vec3(0,0,1);
    uint base=light_base()+2u+uint(index)*4u;
    vec4 light=paint_at(base);
    return surface_direction(unit_direction(light.w<0.5 ? -paint_at(base+1u).xyz : light.xyz-world_point.xyz));
}
vec3 sceneLightRadiance(int index) {
    if (index<0 || index>=canvas_light_count()) return vec3(0);
    uint base=light_base()+2u+uint(index)*4u;
    vec4 light=paint_at(base), tint=paint_at(base+2u);
    vec3 delta=light.xyz-world_point.xyz;
    float attenuation=1.0;
    if (light.w>0.5) {
        float d2=max(dot(delta,delta),0.0001);
        attenuation=clamp(1.0-pow(sqrt(d2)/tint.w,4.0),0.0,1.0)/d2;
        if (light.w>1.5) {
            vec4 direction=paint_at(base+1u);
            float outer=paint_at(base+3u).x;
            float cone=clamp((dot(-unit_direction(delta),direction.xyz)-outer)/max(direction.w-outer,1e-6),0,1);
            attenuation*=cone*cone;
        }
    }
    return tint.rgb*attenuation;
}
// IPP_INTERSECTION
// IPP_PAINT_FUNCTION
vec4 authored_color(CanvasPaint paint, vec2 point, float edge, vec4 color) {
    vec2 scale = paint.paint_origin_scale.zw;
    vec4 value = ipp_paint((point - paint.paint_origin_scale.xy) / scale, paint.paint_size_opacity.xy, color, edge / max(scale.x, scale.y), paint.paint_data);
    value = clamp(value, 0.0, 1.0);
    value.a *= paint.paint_size_opacity.z;
    return value;
}
const float GUI_PAINT_STROKE = 16.0;
const float GUI_PAINT_ARC = 32.0;
const float GUI_PAINT_CHECKER = 8.0;
const float GUI_FILL_HUE = 3.0;
const float INV_SQRT2 = 0.70710678;
const float PI = 3.14159265;
const float TAU = 6.28318531;
const float FAR = 1.0e6;

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

// Separable pixel area for a sharp rectangle, including rectangles narrower than
// one footprint. A zero inset extent leaves no fill, so a full-width border stays
// a border instead of admitting a half-covered residual interior.
float rectangle_coverage(vec2 p, vec2 half_size, vec2 footprint) {
    vec2 band = band_coverage(p, half_size, footprint);
    return band.x * band.y;
}

float band_coverage(float x, float half_extent, float footprint) {
    return clamp((half_extent - x) / footprint + 0.5, 0.0, 1.0)
        - clamp((-half_extent - x) / footprint + 0.5, 0.0, 1.0);
}

// Linear-light gradient between straight RGBA stops, interpolated premultiplied so
// a transparent stop contributes no colour fringe, then returned straight again.
vec4 gradient_color(CanvasPaint paint, float t) {
    vec4 start = vec4(paint.fill[0].rgb * paint.fill[0].a, paint.fill[0].a);
    vec4 end = vec4(paint.fill[1].rgb * paint.fill[1].a, paint.fill[1].a);
    vec4 color = mix(start, end, t);
    return color.a > 0.0 ? vec4(color.rgb / color.a, color.a) : vec4(0.0);
}

// Fraction along the axis [start.xy, end.xy] of the gradient lanes, clamped to it.
float gradient_axis(CanvasPaint paint, vec2 local) {
    vec2 p0 = paint.gradient.xy;
    vec2 dir = paint.gradient.zw - p0;
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
    return mix(pow((encoded + 0.055) / 1.055, vec3(2.4)), encoded / 12.92,
                  lessThanEqual(encoded,vec3(0.04045)));
}

// Straight linear RGBA of a colour field at `local`: the hue along the gradient axis,
// or the saturation-value field of the hue in paint.fill[1].x over the part rectangle,
// saturation rising along +x and value against +y. The model's sRGB colour is
// decoded, then takes the tint and opacity in paint.fill[0].
vec4 colour_field(CanvasPaint paint, float fill_type, vec2 local) {
    vec3 encoded;
    if (fill_type < GUI_FILL_HUE + 0.5) {
        encoded = hue_color(gradient_axis(paint, local));
    } else {
        vec2 size = paint.gradient.zw - paint.gradient.xy;
        // A zero extent divides by one instead; such a box paints nothing.
        vec2 sv = clamp((local - paint.gradient.xy) / (size + mix(vec2(0.0), vec2(1.0), equal(size,vec2(0.0)))), 0.0, 1.0);
        encoded = mix(vec3(1.0), hue_color(paint.fill[1].x), sv.x) * (1.0 - sv.y);
    }
    return vec4(srgb_to_linear(encoded) * paint.fill[0].rgb, paint.fill[0].a);
}

// Integral from zero of the square wave that is +1 over even cells and -1 over odd
// ones: a triangle wave of period two cells.
vec2 checker_wave_integral(vec2 x) {
    return 1.0 - abs(2.0 * fract(x * 0.5) - 1.0);
}

// Premultiplied checker colour over the pixel footprint of `local`, the fragment in
// the box's own orientation from its top-left corner, whose cell takes the first
// colour. The cells are a product of two square waves, so the box filter of the
// footprint's extent along each axis is exact and keeps thin cells' area. A footprint
// of a cell or more cannot resolve cells, and the box filter would leave a beat
// between neighbouring pixels: the waves fade out there, leaving the colours' mean.
vec4 checker_color(CanvasPaint paint, vec2 local, vec2 dx, vec2 dy) {
    float cell = paint.options.z;
    vec2 cells = local / cell;
    vec2 width = max((abs(dx) + abs(dy)) / cell, vec2(1.0 / 65536.0));
    vec2 wave = (checker_wave_integral(cells + 0.5 * width) - checker_wave_integral(cells - 0.5 * width))
        / width;
    wave *= 1.0 - smoothstep(0.5, 1.0, width);
    float second = 0.5 - 0.5 * wave.x * wave.y;
    vec4 a = paint.checker[0];
    vec4 b = paint.checker[1];
    return mix(vec4(a.rgb * a.a, a.a), vec4(b.rgb * b.a, b.a), second);
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
void box_coverage(CanvasPaint paint, vec2 surface_position, bool checker, inout float outer, inout float shape_cov, inout float fill_cov) {
    vec2 half_size = abs(paint.bounds.zw) * 0.5;
    vec2 center = paint.bounds.xy + paint.bounds.zw * 0.5;
    // Corners follow the box's own orientation: mirroring a box mirrors its corners.
    vec2 p = (surface_position - center)
        * vec2(paint.bounds.z < 0.0 ? -1.0 : 1.0, paint.bounds.w < 0.0 ? -1.0 : 1.0);
    vec4 cut = paint.cut;
    float border_width = max(paint.shape.z, 0.0);
    // Clamp each explicit radius to the corresponding placed half size; a cut corner
    // has none.
    float quadrant_cut = p.y < 0.0 ? (p.x < 0.0 ? cut.x : cut.y) : (p.x < 0.0 ? cut.w : cut.z);
    vec2 corner = quadrant_cut > 0.0 ? vec2(0.0) : min(max(paint.shape.xy, vec2(0.0)), half_size);

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
            exact = max(outer, sqrt(max4(mix(vec4(0.0), to_cut, greaterThan(removed,vec4(0.0))))));
        }
        outer = exact;
    }

    // Differentiating abs(p) can cancel across a quad straddling a thin box's
    // centre: both neighboring fragments report the same distance, so the box
    // disappears or overestimates its area. Differentiate unfolded coordinates
    // and integrate the two opposing edges instead. Rounded/cut contours keep
    // their distance coverage; only the limiting sharp rectangle is separable.
    bool sharp = max4(cut) <= 0.0 && min(corner.x, corner.y) <= 1.0 / 65536.0;
    vec2 footprint = vec2(0.0);
    if (sharp) {
        footprint = max(vec2(
            length(vec2(dFdx(p.x), dFdy(p.x))),
            length(vec2(dFdx(p.y), dFdy(p.y)))
        ), vec2(1.0 / 65536.0));
        shape_cov = rectangle_coverage(p, half_size, footprint);
        fill_cov = shape_cov;
        if (border_width > 0.0) {
            fill_cov = min(shape_cov, rectangle_coverage(p, max(half_size - border_width, vec2(0.0)), footprint));
        }
    } else {
        // An uncut, borderless box has one contour for its fill and its shape.
        // Cut endpoints retain their separate distance calculation above.
        float edge = max(length(vec2(dFdx(outer), dFdy(outer))), 1.0 / 65536.0);
        shape_cov = clamp(0.5 - outer / edge, 0.0, 1.0);
        fill_cov = shape_cov;
        if (border_width > 0.0 || max4(cut) > 0.0) {
            float inner = max(
                sd_round_box(p, max(half_size - border_width, vec2(0.0)), max(corner - border_width, vec2(0.0))),
                cut_plane + border_width
            );
            float inner_edge = max(length(vec2(dFdx(inner), dFdy(inner))), 1.0 / 65536.0);
            fill_cov = min(shape_cov, clamp(0.5 - inner / inner_edge, 0.0, 1.0));
        }
    }

    vec4 span = paint.accent;
    if (!checker && max4(span) > 0.0) {
        float accent_width = max(paint.shape.w, 0.0);
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
        float accent = min4(mix(vec4(FAR), reach, greaterThan(span,vec4(0.0))));
        float accent_inner_edge = max(length(vec2(dFdx(accent_inner), dFdy(accent_inner))), 1.0 / 65536.0);
        float accent_edge = max(length(vec2(dFdx(accent), dFdy(accent))), 1.0 / 65536.0);
        float accent_fill = min(shape_cov, sharp
            ? rectangle_coverage(p, max(half_size - accent_width, vec2(0.0)), footprint)
            : clamp(0.5 - accent_inner / accent_inner_edge, 0.0, 1.0));
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
void stroke_coverage(CanvasPaint paint, vec2 surface_position, inout float outer, inout float shape_cov) {
    vec2 local = surface_position - paint.bounds.xy;
    // Derivatives precede the segments' flat branches.
    vec2 dx = dFdx(local);
    vec2 dy = dFdy(local);
    float half_width = max(paint.shape.z, 0.0) * 0.5;
    outer = FAR;
    shape_cov = 0.0;
    segment_coverage(paint.segment[0], local, dx, dy, half_width, shape_cov, outer);
    segment_coverage(paint.segment[1], local, dx, dy, half_width, shape_cov, outer);
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
void arc_coverage(CanvasPaint paint, vec2 surface_position, inout float outer, inout float shape_cov) {
    vec2 local = surface_position - paint.bounds.xy - paint.segment[0].xy;
    // Derivatives precede the flat branches below.
    vec2 dx = dFdx(local);
    vec2 dy = dFdy(local);
    float radius = paint.shape.x;
    float half_sweep = paint.shape.y;
    float half_width = max(paint.shape.z, 0.0) * 0.5;
    vec2 middle = paint.segment[0].zw;
    vec2 cap = paint.segment[1].xy;
    float cells = paint.segment[1].z;
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
        float duty = paint.shape.w;
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

// Glow strength at a normalized distance from the contour.
float glow_alpha(CanvasPaint paint, float reach, float falloff) {
    float norm = clamp(1.0 - reach, 0.0, 1.0);
    float factor = (abs(falloff - 1.0) < 1e-5) ? norm : pow(norm, falloff);
    return clamp(paint.glow_color.a * factor, 0.0, 1.0);
}

// Straight RGB and unclipped alpha of a box, stroke or arc fragment.
//
// The paint is flat, so every invocation of a 2x2 quad, which shades a single
// primitive, takes the same branch and the derivatives below stay defined. A zero border evaluates the inner contour as
// the outer one. Strokes and arcs have no border ring: the fill paints them.
vec4 shape_color(CanvasPaint paint, vec2 surface_position, float variant_id) {
    // Shape offsets are multiples of the stroke's, each followed by fill types and
    // the checker's offset.
    float fill_type = mod(variant_id, GUI_PAINT_CHECKER);
    float variant = mod(variant_id, GUI_PAINT_STROKE);
    bool checker = variant > GUI_PAINT_CHECKER - 0.5;
    float shape = variant_id - variant;
    float outer;
    float shape_cov;
    float fill_cov;
    if (shape > GUI_PAINT_ARC - 0.5) {
        arc_coverage(paint, surface_position, outer, shape_cov);
        fill_cov = shape_cov;
    } else if (shape > GUI_PAINT_STROKE - 0.5) {
        stroke_coverage(paint, surface_position, outer, shape_cov);
        fill_cov = shape_cov;
    } else {
        box_coverage(paint, surface_position, checker, outer, shape_cov, fill_cov);
    }

    // Fill color evaluation: solid, linear gradient, radial gradient, colour field or
    // custom paint.
    vec4 fill_color = paint.fill[0];
    vec2 local_pos = surface_position - paint.bounds.xy;
    if (fill_type > 4.5) {
        fill_color = authored_color(paint, local_pos, outer, fill_color);
    } else if (fill_type > 0.5 && fill_type < 1.5) {
        fill_color = gradient_color(paint, gradient_axis(paint, local_pos));
    } else if (fill_type > 1.5 && fill_type < 2.5) {
        vec2 center_pt = paint.gradient.xy;
        float radius = paint.gradient.z;
        float dist = length(local_pos - center_pt);
        float t = (radius > 1e-6) ? clamp(dist / radius, 0.0, 1.0) : 0.0;
        fill_color = gradient_color(paint, t);
    } else if (fill_type > 2.5) {
        fill_color = colour_field(paint, fill_type, local_pos);
    }

    // Edge glow falls off from the outer contour. Outward it occupies only the pixel
    // area outside the shape, so its single attenuation is that uncovered fraction and
    // the halo never fills a transparent shape or focus ring. Inward it lies over the
    // fill, beneath the border.
    float outer_glow = 0.0;
    float inner_glow = 0.0;
    if (paint.glow_color.a > 0.0) {
        float outer_radius = paint.glow.z;
        if (outer_radius > 0.0 && outer < outer_radius) {
            outer_glow = glow_alpha(paint, max(outer, 0.0) / outer_radius, paint.glow.w)
                * (1.0 - shape_cov);
        }
        float inner_radius = paint.glow.y;
        if (inner_radius > 0.0 && -outer < inner_radius) {
            inner_glow = glow_alpha(paint, max(-outer, 0.0) / inner_radius, paint.glow.w);
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
        vec2 own = local_pos * sign(paint.bounds.zw);
        fill += checker_color(paint, own, dFdx(own), dFdy(own)) * (1.0 - fill.a);
    }
    fill = vec4(paint.glow_color.rgb, 1.0) * inner_glow + fill * (1.0 - inner_glow);
    vec4 color = fill * fill_cov
        + vec4(paint.border.rgb * paint.border.a, paint.border.a) * (shape_cov - fill_cov)
        + vec4(paint.glow_color.rgb, 1.0) * outer_glow;
    return color;
}


// IPP_PATH_COVERAGE

#ifdef IPP_SCREEN
// Exact screen samples were painted with this projection. Re-evaluate their scene depth
// against the current opaque pass so moving geometry can still occlude this Canvas.
void main() {
    vec4 address=paint_at(slot+21u);
    float coverage,depth;
    intersect_surface(uint(address.w),uint(address.z),coverage,depth);
    gl_FragDepth=depth;
    out_color=texture(sampler2D(pixels,nearest_sampler),vertex_local);
}
#elif defined(IPP_PANEL)
// Canvas placements have already clipped the panel polygon. The child color is premultiplied.
void main() {
    vec2 uv=vertex_local/paint_at(slot+1u).zw;
    out_color=texture(sampler2D(pixels,pixel_sampler),uv)*paint_at(slot).a;
}
#else
void main() {
    CanvasPaint paint = fetch_paint(slot);
    float surface_coverage=1.0;
    vec2 local=vertex_local;
#ifdef IPP_ANALYTIC
    float depth;
    local=intersect_surface(uint(paint.path_parameters.w),uint(paint.path_parameters.z),surface_coverage,depth);
    gl_FragDepth=depth;
#else
    world_point=vertex_world;
    right_axis=vertex_right;
    down_axis=vertex_down;
    normal_axis=vertex_normal;
#endif
    vec2 dx = dFdx(local);
    vec2 dy = dFdy(local);
    vec2 footprint = max(abs(dx) + abs(dy), vec2(1.0 / 65536.0));
    vec2 inside = min(local - paint.clip.xy, paint.clip.zw - local);
    float clip = clamp(min(inside.x / footprint.x, inside.y / footprint.y) + 0.5, 0.0, 1.0);
    vec4 color;
    if (paint.options.y > 2.5) {
        float coverage = path_coverage(uint(paint.path_parameters.x), local, paint.options.y, paint.options.w, dx, dy);
        vec4 fill = paint.tint;
        if (paint.paint_size_opacity.w > 0.5) fill = authored_color(paint, local, -coverage, fill);
        float alpha = fill.a * coverage;
        color = vec4(fill.rgb * alpha, alpha);
    } else if (paint.options.y > 0.5) {
        vec2 uv=local / paint.bounds.zw;
        vec2 layer=paint_at(slot+22u).zw;
        if (layer.x >= 0.0) {
            ivec2 size=textureSize(sampler2D(layer_pixels, nearest_sampler),0);
            ivec2 pixel=clamp(ivec2(uv*vec2(size)),ivec2(0),size-ivec2(1));
            vec4 bytes=round(255.0*texelFetch(sampler2D(layer_pixels,nearest_sampler),pixel,0));
            if (any(notEqual(layer,bytes.xz + 256.0*bytes.yw))) discard;
        }
        color = texture(sampler2D(pixels, pixel_sampler), uv);
        if (paint.options.y < 1.5) color.rgb *= color.a;
        color.rgb *= paint.tint.rgb;
        color *= paint.tint.a;
    } else {
        color = shape_color(paint, local, paint.options.x);
        vec4 section = paint.section;
        if (section.x > 0.5 && section.x < 1.5 && local.x < section.y) discard;
        if (section.x > 1.5 && section.x < 2.5 && local.y < section.y) discard;
        if (section.x > 2.5 && section.x < 3.5 && local.x >= section.y) discard;
        if (section.x > 3.5 && local.y >= section.y) discard;
    }
    clip *= surface_coverage;
    if (color.a * clip <= 0.0) discard;
#ifdef IPP_OWNER
    vec2 owner=paint_at(slot+22u).xy;
    out_color=vec4(mod(owner.x,256.0),floor(owner.x/256.0),mod(owner.y,256.0),floor(owner.y/256.0))/255.0;
#else
    out_color = color * clip;
#endif
}

#endif
