// SPDX-License-Identifier: MIT OR Apache-2.0
// Built-in Canvas coverage in local logical units, shared with the GLES equations.
struct CanvasVertex {
    packed_float4 clip;
    packed_float2 local;
    packed_float4 world;
    packed_float3 right;
    packed_float3 down;
    packed_float3 normal;
    uint slot;
};

struct CanvasRaster {
    float4 position [[position]];
    float2 local;
    float4 world;
    float3 right;
    float3 down;
    float3 normal;
    uint slot [[flat]];
    float4 ray_origin;
    uint orthonormal [[flat]];
};

struct CanvasMapping {
    float4 parameters;
    float4x4 projection;
    float4x4 inverse;
    float4x4 world;
    float4 chart_to_root;
    uint4 options;
};

struct CanvasColumn {
    float4 point;  // Hit X/Z, hit Y intercept and slope in physical pixels.
    float4 chart;  // Arc length, tangent X/Z, and projected depth.
    float4 ray;    // Ray parameter, direction XZ squared, Y intercept and slope.
    float4 origin; // Near point XZ squared, Y intercept and slope, discriminant.
};

struct CanvasAnalytic {
    float4 local_x;
    float4 local_y;
    float4 clip_rectangle;
    float2 cover[4];
    float4 clip_edges;
};

constant bool ipp_column_lookup [[function_constant(0)]];
constant bool ipp_canvas_shapes_only [[function_constant(1)]];
constant bool ipp_canvas_cylinder [[function_constant(2)]];

struct CanvasPaint {
    float4 tint;
    float4 bounds;
    float4 fill[2];
    float4 gradient;
    float4 border;
    float4 options;
    float4 shape;
    float4 segment[2];
    float4 cut;
    float4 accent;
    float4 glow_color;
    float4 glow;
    float4 checker[2];
    float4 section;
    float4 clip;
    float4 paint_origin_scale;
    float4 paint_size_opacity;
    float4 paint_data;
};

static CanvasRaster canvas_default_vertex(CanvasVertex v) {
    float4 position = v.clip;
    position.z = (position.z + position.w) * 0.5;
    return {position, v.local, v.world, v.right, v.down, v.normal, v.slot, float4(0.0), 0};
}

vertex CanvasRaster canvas_vertex(const device CanvasVertex *vertices [[buffer(0)]],
                                  uint index [[vertex_id]]) {
    return canvas_default_vertex(vertices[index]);
}

vertex CanvasRaster canvas_intersection_vertex(
    constant CanvasAnalytic *intersection [[buffer(1)]],
    constant CanvasMapping &mapping [[buffer(2)]], uint index [[vertex_id]]) {
    constexpr uint corners[6] = {0, 1, 2, 2, 1, 3};
    uint slot = index / 6;
    constant CanvasAnalytic &surface = intersection[slot];
    float2 point = surface.cover[corners[index % 6]];
    // Analytic covering quads have unit clip W, so inverse projection is
    // affine across the raster interpolation domain.
    float4 origin = mapping.inverse * float4(point, -1.0, 1.0);
    return {float4(point, 0.5, 1.0), float2(0.0), float4(0.0), float3(0.0),
            float3(0.0), float3(0.0), slot, origin, 0};
}

template<typename T, typename U>
T ipp_mod(T x, U y) { return x - y * floor(x / y); }

template<typename T>
T ipp_atan(T x) { return atan(x); }

template<typename T>
T ipp_atan(T y, T x) { return atan2(y, x); }

template<typename T>
auto greaterThanEqual(T a, T b) -> decltype(a >= b) { return a >= b; }

template<typename T>
auto lessThanEqual(T a, T b) -> decltype(a <= b) { return a <= b; }

template<typename T>
auto greaterThan(T a, T b) -> decltype(a > b) { return a > b; }

template<typename T>
auto lessThan(T a, T b) -> decltype(a < b) { return a < b; }

constexpr sampler ippSamplerLinear(coord::normalized, address::clamp_to_edge, filter::linear);

float3 surface_coordinates(CanvasRaster raster, float3 world_direction) {
    if (raster.orthonormal) {
        return float3(dot(world_direction, raster.right), dot(world_direction, raster.down),
                      dot(world_direction, raster.normal));
    }
    float3 right = unit(raster.right);
    float3 normal = unit(raster.normal);
    float3 down = unit(raster.down - right * dot(raster.down, right));
    return float3(dot(world_direction, right), dot(world_direction, down), dot(world_direction, normal));
}

float3 canvas_view(CanvasRaster raster, constant Lighting &lighting) {
    if (raster.world.w == 0.0) return float3(0.0, 0.0, 1.0);
    float3 eye = lighting.camera.w > 0.5
        ? lighting.camera.xyz : unit(lighting.camera.xyz - raster.world.xyz);
    return surface_coordinates(raster, eye);
}

float3 canvas_light_direction(CanvasRaster raster, constant Lighting &lighting, int index) {
    if (raster.world.w == 0.0 || index < 0 || index >= int(lighting.ambient_count.w))
        return float3(0.0, 0.0, 1.0);
    constant PunctualLight &light = lighting.lights[index];
    float3 delta = light.position_kind.w < 0.5
        ? -light.direction_inner.xyz : light.position_kind.xyz - raster.world.xyz;
    return surface_coordinates(raster, unit(delta));
}

float3 canvas_light_radiance(CanvasRaster raster, constant Lighting &lighting, int index) {
    if (raster.world.w == 0.0 || index < 0 || index >= int(lighting.ambient_count.w))
        return float3(0.0);
    constant PunctualLight &light = lighting.lights[index];
    float attenuation = 1.0;
    if (light.position_kind.w > 0.5) {
        float3 delta = light.position_kind.xyz - raster.world.xyz;
        float distance_squared = max(dot(delta, delta), 0.0001);
        float ratio = sqrt(distance_squared) / light.color_range.w;
        attenuation = clamp(1.0 - pow(ratio, 4.0), 0.0, 1.0) / distance_squared;
        if (light.position_kind.w > 1.5) {
            float outer = light.cone.x;
            float cone = clamp((dot(-unit(delta), light.direction_inner.xyz) - outer)
                / max(light.direction_inner.w - outer, 1e-6), 0.0, 1.0);
            attenuation *= cone * cone;
        }
    }
    return light.color_range.rgb * attenuation;
}

/* IPP_AUTHORED_PAINT */
#ifndef IPP_AUTHORED_PAINT
float4 ipp_paint(float2 position, float2 size, float4 color, float edge, float4 data,
                 constant float4 *parameters, CanvasRaster raster, constant Lighting &scene_lighting) {
    return color;
}
#endif

float4 authored_color(constant CanvasPaint &paint, float2 local, float edge, float4 color,
                      constant float4 *parameters, CanvasRaster raster, constant Lighting &scene_lighting) {
    float2 scale = paint.paint_origin_scale.zw;
    float4 painted = ipp_paint((local - paint.paint_origin_scale.xy) / scale,
                               paint.paint_size_opacity.xy, color,
                               edge / max(scale.x, scale.y), paint.paint_data,
                               parameters, raster, scene_lighting);
    float4 clipped = clamp(painted, 0.0, 1.0);
    clipped.a *= paint.paint_size_opacity.z;
    return clipped;
}

constant float GUI_PAINT_STROKE = 16.0;
constant float GUI_PAINT_ARC = 32.0;
constant float GUI_PAINT_CHECKER = 8.0;
constant float GUI_FILL_HUE = 3.0;
constant float INV_SQRT2 = 0.70710678;
constant float PI = 3.14159265;
constant float TAU = 6.28318531;
constant float FAR = 1.0e6;

float max4(float4 value) {
    return max(max(value.x, value.y), max(value.z, value.w));
}

float min4(float4 value) {
    return min(min(value.x, value.y), min(value.z, value.w));
}

float sd_box(float2 offset, float2 half_size) {
    float2 q = abs(offset) - half_size;
    return length(max(q, float2(0.0))) + min(max(q.x, q.y), 0.0);
}

// Gradient-normalized implicit ellipse distance. Its zero contour is the
// requested ellipse, while the normalization keeps antialiasing in Surface
// metres under unequal radii. The rounded box uses this only in its corner
// quadrant; straight edges retain the ordinary box contour.
float sd_ellipse(float2 point, float2 radius) {
    float k0 = length(point / radius);
    if (k0 <= 1.0 / 65536.0) return -min(radius.x, radius.y);
    float k1 = length(point / (radius * radius));
    return k0 * (k0 - 1.0) / max(k1, 1.0 / 65536.0);
}

float sd_round_box(float2 offset, float2 half_size, float2 radius) {
    // A zero lane has no corner extent on that axis, so the limiting shape is
    // the sharp box rather than an invented circular radius.
    if (min(radius.x, radius.y) <= 1.0 / 65536.0) {
        return sd_box(offset, half_size);
    }
    float2 q = abs(offset) - (half_size - radius);
    // Within the straight-edged core an edge is nearest, so the inner glow keeps
    // falling off where the corner ellipse would report a constant depth.
    if (max(q.x, q.y) <= 0.0) return max(q.x - radius.x, q.y - radius.y);
    return sd_ellipse(max(q, float2(0.0)), radius);
}

// Pixel coverage of the band |x| <= half_extent for a footprint across it: the
// difference of its two edges' coverages, so a band narrower than a pixel keeps its
// area instead of the contrast loss of multiplied ramps.
float2 band_coverage(float2 x, float2 half_extent, float2 footprint) {
    return clamp((half_extent - x) / footprint + 0.5, 0.0, 1.0)
        - clamp((-half_extent - x) / footprint + 0.5, 0.0, 1.0);
}

// Separable pixel area for a sharp rectangle, including rectangles narrower than
// one footprint. A zero inset extent leaves no fill, so a full-width border stays
// a border instead of admitting a half-covered residual interior.
float rectangle_coverage(float2 p, float2 half_size, float2 footprint) {
    float2 band = band_coverage(p, half_size, footprint);
    return band.x * band.y;
}

float band_coverage(float x, float half_extent, float footprint) {
    return clamp((half_extent - x) / footprint + 0.5, 0.0, 1.0)
        - clamp((-half_extent - x) / footprint + 0.5, 0.0, 1.0);
}

// Linear-light gradient between straight RGBA stops, interpolated premultiplied so
// a transparent stop contributes no colour fringe, then returned straight again.
float4 gradient_color(constant CanvasPaint &paint, float t) {
    float4 start = float4(paint.fill[0].rgb * paint.fill[0].a, paint.fill[0].a);
    float4 end = float4(paint.fill[1].rgb * paint.fill[1].a, paint.fill[1].a);
    float4 color = mix(start, end, t);
    return color.a > 0.0 ? float4(color.rgb / color.a, color.a) : float4(0.0);
}

// Fraction along the axis [start.xy, end.xy] of the gradient lanes, clamped to it.
float gradient_axis(constant CanvasPaint &paint, float2 local) {
    float2 p0 = paint.gradient.xy;
    float2 dir = paint.gradient.zw - p0;
    float len_sq = dot(dir, dir);
    return (len_sq > 1e-12) ? clamp(dot(local - p0, dir) / len_sq, 0.0, 1.0) : 0.0;
}

// sRGB-encoded colour of a hue in turns at full saturation and value. Each channel
// is a trapezoid of the hue, ramping linearly in encoded values between the primary
// and secondary colours a sixth of a turn apart, as the HSV model defines it.
float3 hue_color(float hue) {
    return clamp(abs(fract(hue + float3(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0, 0.0, 1.0);
}

// The sRGB transfer function's inverse. The target and the present pass apply the
// function itself, so an encoded colour decoded here is displayed as itself.
float3 srgb_to_linear(float3 encoded) {
    return select(pow((encoded + 0.055) / 1.055, float3(2.4)), encoded / 12.92,
                  encoded <= float3(0.04045));
}

// Straight linear RGBA of a colour field at `local`: the hue along the gradient axis,
// or the saturation-value field of the hue in paint.fill[1].x over the part rectangle,
// saturation rising along +x and value against +y. The model's sRGB colour is
// decoded, then takes the tint and opacity in paint.fill[0].
float4 colour_field(constant CanvasPaint &paint, float fill_type, float2 local) {
    float3 encoded;
    if (fill_type < GUI_FILL_HUE + 0.5) {
        encoded = hue_color(gradient_axis(paint, local));
    } else {
        float2 size = paint.gradient.zw - paint.gradient.xy;
        // A zero extent divides by one instead; such a box paints nothing.
        float2 sv = clamp((local - paint.gradient.xy) / (size + select(float2(0.0), float2(1.0), size == float2(0.0))), 0.0, 1.0);
        encoded = mix(float3(1.0), hue_color(paint.fill[1].x), sv.x) * (1.0 - sv.y);
    }
    return float4(srgb_to_linear(encoded) * paint.fill[0].rgb, paint.fill[0].a);
}

// Integral from zero of the square wave that is +1 over even cells and -1 over odd
// ones: a triangle wave of period two cells.
float2 checker_wave_integral(float2 x) {
    return 1.0 - abs(2.0 * fract(x * 0.5) - 1.0);
}

// Premultiplied checker colour over the pixel footprint of `local`, the fragment in
// the box's own orientation from its top-left corner, whose cell takes the first
// colour. The cells are a product of two square waves, so the box filter of the
// footprint's extent along each axis is exact and keeps thin cells' area. A footprint
// of a cell or more cannot resolve cells, and the box filter would leave a beat
// between neighbouring pixels: the waves fade out there, leaving the colours' mean.
float4 checker_color(constant CanvasPaint &paint, float2 local, float2 dx, float2 dy) {
    float cell = paint.options.z;
    float2 cells = local / cell;
    float2 width = max((abs(dx) + abs(dy)) / cell, float2(1.0 / 65536.0));
    float2 wave = (checker_wave_integral(cells + 0.5 * width) - checker_wave_integral(cells - 0.5 * width))
        / width;
    wave *= 1.0 - smoothstep(0.5, 1.0, width);
    float second = 0.5 - 0.5 * wave.x * wave.y;
    float4 a = paint.checker[0];
    float4 b = paint.checker[1];
    return mix(float4(a.rgb * a.a, a.a), float4(b.rgb * b.a, b.a), second);
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
void box_coverage(constant CanvasPaint &paint, float2 surface_position, bool checker, thread float &outer, thread float &shape_cov, thread float &fill_cov) {
    float2 half_size = abs(paint.bounds.zw) * 0.5;
    float2 center = paint.bounds.xy + paint.bounds.zw * 0.5;
    // Corners follow the box's own orientation: mirroring a box mirrors its corners.
    float2 p = (surface_position - center)
        * float2(paint.bounds.z < 0.0 ? -1.0 : 1.0, paint.bounds.w < 0.0 ? -1.0 : 1.0);
    float4 cut = paint.cut;
    float border_width = max(paint.shape.z, 0.0);
    // Clamp each explicit radius to the corresponding placed half size; a cut corner
    // has none.
    float quadrant_cut = p.y < 0.0 ? (p.x < 0.0 ? cut.x : cut.y) : (p.x < 0.0 ? cut.w : cut.z);
    float2 corner = quadrant_cut > 0.0 ? float2(0.0) : min(max(paint.shape.xy, float2(0.0)), half_size);

    outer = sd_round_box(p, half_size, corner);
    float cut_plane = -FAR;
    // Cuts are flat, so every invocation of a quad takes the same branch.
    if (max4(cut) > 0.0) {
        float diagonal = half_size.x + half_size.y;
        float4 planes = (float4(-p.x - p.y, p.x - p.y, p.x + p.y, p.y - p.x) - (diagonal - cut))
            * INV_SQRT2;
        cut_plane = max4(planes);
        float exact = max(outer, cut_plane);
        if (exact > 0.0) {
            float2 nearest = clamp(p, -half_size, half_size);
            float4 removed = float4(
                -nearest.x - nearest.y,
                nearest.x - nearest.y,
                nearest.x + nearest.y,
                nearest.y - nearest.x
            ) - (diagonal - cut);
            // Beyond either end of a cut, along its direction.
            float4 along = max(
                abs(float4(p.y - p.x, p.x + p.y, p.x - p.y, -p.x - p.y) - (half_size.x - half_size.y)) - cut,
                float4(0.0)
            ) * INV_SQRT2;
            float4 to_cut = along * along + planes * planes;
            exact = max(outer, sqrt(max4(select(float4(0.0), to_cut, removed > float4(0.0)))));
        }
        outer = exact;
    }

    // Differentiating abs(p) can cancel across a quad straddling a thin box's
    // centre: both neighboring fragments report the same distance, so the box
    // disappears or overestimates its area. Differentiate unfolded coordinates
    // and integrate the two opposing edges instead. Rounded/cut contours keep
    // their distance coverage; only the limiting sharp rectangle is separable.
    bool sharp = max4(cut) <= 0.0 && min(corner.x, corner.y) <= 1.0 / 65536.0;
    float2 footprint = float2(0.0);
    if (sharp) {
        footprint = max(float2(
            length(float2(dfdx(p.x), dfdy(p.x))),
            length(float2(dfdx(p.y), dfdy(p.y)))
        ), float2(1.0 / 65536.0));
        shape_cov = rectangle_coverage(p, half_size, footprint);
        fill_cov = shape_cov;
        if (border_width > 0.0) {
            fill_cov = min(shape_cov, rectangle_coverage(p, max(half_size - border_width, float2(0.0)), footprint));
        }
    } else {
        // An uncut, borderless box has one contour for its fill and its shape.
        // Cut endpoints retain their separate distance calculation above.
        float edge = max(length(float2(dfdx(outer), dfdy(outer))), 1.0 / 65536.0);
        shape_cov = clamp(0.5 - outer / edge, 0.0, 1.0);
        fill_cov = shape_cov;
        if (border_width > 0.0 || max4(cut) > 0.0) {
            float inner = max(
                sd_round_box(p, max(half_size - border_width, float2(0.0)), max(corner - border_width, float2(0.0))),
                cut_plane + border_width
            );
            float inner_edge = max(length(float2(dfdx(inner), dfdy(inner))), 1.0 / 65536.0);
            fill_cov = min(shape_cov, clamp(0.5 - inner / inner_edge, 0.0, 1.0));
        }
    }

    float4 span = paint.accent;
    if (!checker && max4(span) > 0.0) {
        float accent_width = max(paint.shape.w, 0.0);
        float accent_inner = max(
            sd_round_box(p, max(half_size - accent_width, float2(0.0)), max(corner - accent_width, float2(0.0))),
            cut_plane + accent_width
        );
        // Distance to the accented corner squares, measured from the rectangle's
        // corners; the square's sides are the accent's butt ends within the ring.
        float4 reach = max(
            half_size.x + float4(p.x, -p.x, -p.x, p.x),
            half_size.y + float4(p.y, p.y, -p.y, -p.y)
        ) - span;
        float accent = min4(select(float4(FAR), reach, span > float4(0.0)));
        float accent_inner_edge = max(length(float2(dfdx(accent_inner), dfdy(accent_inner))), 1.0 / 65536.0);
        float accent_edge = max(length(float2(dfdx(accent), dfdy(accent))), 1.0 / 65536.0);
        float accent_fill = min(shape_cov, sharp
            ? rectangle_coverage(p, max(half_size - accent_width, float2(0.0)), footprint)
            : clamp(0.5 - accent_inner / accent_inner_edge, 0.0, 1.0));
        fill_cov = mix(fill_cov, accent_fill, clamp(0.5 - accent / accent_edge, 0.0, 1.0));
    }
}

// Coverage and distance of one butt-capped segment [center.xy, half.xy], combined
// with earlier segments by union. A zero half vector paints nothing.
void segment_coverage(
    float4 segment,
    float2 local,
    float2 dx,
    float2 dy,
    float half_width,
    thread float &coverage,
    thread float &nearest
) {
    float half_length = length(segment.zw);
    if (half_length <= 0.0) return;
    float2 axis = segment.zw / half_length;
    float2 normal = float2(-axis.y, axis.x);
    float2 offset = local - segment.xy;
    float2 q = float2(dot(offset, axis), dot(offset, normal));
    // Pixel footprint along and across the segment.
    float2 footprint = max(
        float2(
            length(float2(dot(dx, axis), dot(dy, axis))),
            length(float2(dot(dx, normal), dot(dy, normal)))
        ),
        float2(1.0 / 65536.0)
    );
    float2 extent = float2(half_length, half_width);
    float2 band = band_coverage(q, extent, footprint);
    coverage = max(coverage, band.x * band.y);
    nearest = min(nearest, sd_box(q, extent));
}

// Stroke distance and coverage: the union of its segments, painted by the fill.
void stroke_coverage(constant CanvasPaint &paint, float2 surface_position, thread float &outer, thread float &shape_cov) {
    float2 local = surface_position - paint.bounds.xy;
    // Derivatives precede the segments' flat branches.
    float2 dx = dfdx(local);
    float2 dy = dfdy(local);
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
float arc_end_distance(float2 q, float radius, float half_width) {
    return sign(q.x) * length(float2(q.x, max(abs(q.y - radius) - half_width, 0.0)));
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
void arc_coverage(constant CanvasPaint &paint, float2 surface_position, thread float &outer, thread float &shape_cov) {
    float2 local = surface_position - paint.bounds.xy - paint.segment[0].xy;
    // Derivatives precede the flat branches below.
    float2 dx = dfdx(local);
    float2 dy = dfdy(local);
    float radius = paint.shape.x;
    float half_sweep = paint.shape.y;
    float half_width = max(paint.shape.z, 0.0) * 0.5;
    float2 middle = paint.segment[0].zw;
    float2 cap = paint.segment[1].xy;
    float cells = paint.segment[1].z;
    float2 along = float2(-middle.y, middle.x) * (cells < 0.0 ? -1.0 : 1.0);
    float2 p = float2(dot(local, along), dot(local, middle));
    float r = length(p);
    float2 radial = r > 0.0 ? local / r : middle;
    float2 tangent = float2(-radial.y, radial.x);
    // Pixel footprint across and along the ring.
    float across = max(length(float2(dot(dx, radial), dot(dy, radial))), 1.0 / 65536.0);
    float around = max(length(float2(dot(dx, tangent), dot(dy, tangent))), 1.0 / 65536.0);
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
        float phi = r > 0.0 ? atan2(p.x, p.y) : 0.0;
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
        outer = max(band, arc_end_distance(r * float2(sin(beyond), cos(beyond)), radius, half_width));
        return;
    }

    if (full) {
        outer = band;
        shape_cov = ring;
        return;
    }
    float2 folded = float2(abs(p.x), p.y);
    float end = arc_end_distance(
        float2(folded.x * cap.y - folded.y * cap.x, folded.x * cap.x + folded.y * cap.y),
        radius,
        half_width
    );
    float angular = clamp(0.5 - end / around, 0.0, 1.0);
    // Where the sweep or the gap is under a quarter turn, both ends can share a
    // pixel: a narrow sweep intersects their coverages and a narrow gap unites them.
    if (half_sweep < 0.25 * PI || half_sweep > 0.75 * PI) {
        float other = arc_end_distance(
            float2(-folded.x * cap.y - folded.y * cap.x, folded.y * cap.y - folded.x * cap.x),
            radius,
            half_width
        );
        angular += clamp(0.5 - other / around, 0.0, 1.0) - (half_sweep < 0.25 * PI ? 1.0 : 0.0);
    }
    outer = max(band, end);
    shape_cov = ring * clamp(angular, 0.0, 1.0);
}

// Glow strength at a normalized distance from the contour.
float glow_alpha(constant CanvasPaint &paint, float reach, float falloff) {
    float norm = clamp(1.0 - reach, 0.0, 1.0);
    float factor = (abs(falloff - 1.0) < 1e-5) ? norm : pow(norm, falloff);
    return clamp(paint.glow_color.a * factor, 0.0, 1.0);
}

// Straight RGB and unclipped alpha of a box, stroke or arc fragment.
//
// The paint is flat, so every invocation of a 2x2 quad, which shades a single
// primitive, takes the same branch and the derivatives below stay defined. A zero border evaluates the inner contour as
// the outer one. Strokes and arcs have no border ring: the fill paints them.
float4 shape_color(constant CanvasPaint &paint, float2 surface_position, float variant_id,
                   constant float4 *parameters, CanvasRaster raster, constant Lighting &scene_lighting) {
    // Shape offsets are multiples of the stroke's, each followed by fill types and
    // the checker's offset.
    float fill_type = fmod(variant_id, GUI_PAINT_CHECKER);
    float variant = fmod(variant_id, GUI_PAINT_STROKE);
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
    float4 fill_color = paint.fill[0];
    float2 local_pos = surface_position - paint.bounds.xy;
    if (fill_type > 4.5) {
        fill_color = authored_color(paint, local_pos, outer, fill_color, parameters, raster, scene_lighting);
    } else if (fill_type > 0.5 && fill_type < 1.5) {
        fill_color = gradient_color(paint, gradient_axis(paint, local_pos));
    } else if (fill_type > 1.5 && fill_type < 2.5) {
        float2 center_pt = paint.gradient.xy;
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
    float4 fill = float4(fill_color.rgb * fill_color.a, fill_color.a);
    // A checker lies beneath the fill, in the box's own orientation; its paint is
    // flat, so the derivatives stay defined.
    if (checker) {
        float2 own = local_pos * sign(paint.bounds.zw);
        fill += checker_color(paint, own, dfdx(own), dfdy(own)) * (1.0 - fill.a);
    }
    fill = float4(paint.glow_color.rgb, 1.0) * inner_glow + fill * (1.0 - inner_glow);
    float4 color = fill * fill_cov
        + float4(paint.border.rgb * paint.border.a, paint.border.a) * (shape_cov - fill_cov)
        + float4(paint.glow_color.rgb, 1.0) * outer_glow;
    return color;
}


// Contours may have a signed base winding per cell. Band and cell indices
// both address the original edge records in path order.
float cross2(float2 a, float2 b) {
    return a.x * b.y - a.y * b.x;
}

bool path_contains(const device float4 *records, float2 point, float mode, float radius) {
    float4 header = records[0];
    float4 range;
    uint indices;
    if (header.x < 0.0) {
        float4 domain = records[1];
        uint side = uint(-header.x);
        if (point.x < domain.x || point.x > domain.z || point.y < header.y || point.y >= domain.w) return false;
        uint2 cell = min(uint2((point - float2(domain.x, header.y)) * float2(domain.y, header.z)), side - 1);
        range = records[2 + cell.y * side + cell.x];
        indices = 2 + side * side;
    } else {
        int row = int(floor((point.y - header.y) * header.z));
        if (row < 0 || row >= int(header.x)) return false;
        range = records[row + 1];
        indices = 1 + uint(header.x);
    }
    uint begin = uint(range.x);
    uint end = begin + uint(range.y);
    int winding = int(range.z);
    for (uint i = begin; i < end; ++i) {
        uint element = uint(records[indices + i / 4][i % 4]);
        uint record = uint(header.w) + element * 2;
        float4 pair = records[record];
        float2 a = pair.xy;
        float2 b = pair.zw;
        float4 endpoint = records[record + 1];
        float2 c = endpoint.xy;
        if (mode > 5.5) {
            float2 axis = b - a;
            float length_squared = dot(axis, axis);
            float t = length_squared > 0.0
                ? clamp(dot(point - a, axis) / length_squared, 0.0, 1.0)
                : 0.0;
            float2 delta = point - (a + t * axis);
            if (dot(delta, delta) < radius * radius) return true;
        } else if (mode > 4.5) {
            float u = cross2(b - a, point - a);
            float v = cross2(c - b, point - b);
            float w = cross2(a - c, point - c);
            if ((u > 0.0 || v > 0.0 || w > 0.0) && min(u, min(v, w)) >= 0.0) return true;
            if ((u < 0.0 || v < 0.0 || w < 0.0) && max(u, max(v, w)) <= 0.0) return true;
        } else {
            if (point.y < min(a.y, c.y) || point.y >= max(a.y, c.y)) continue;
            float direction = sign(c.y - a.y);
            float quadratic = endpoint.z;
            float linear = endpoint.w;
            float constant_term = a.y - point.y;
            float t;
            if (quadratic == 0.0) {
                t = clamp(-constant_term / linear, 0.0, 1.0);
            } else {
                float root = sqrt(max(linear * linear - 4.0 * quadratic * constant_term, 0.0));
                float divisor = linear + direction * root;
                t = divisor == 0.0 ? 0.0 : clamp(-2.0 * constant_term / divisor, 0.0, 1.0);
            }
            float x = mix(mix(a.x, b.x, t), mix(b.x, c.x, t), t);
            if (x > point.x) winding += int(direction);
        }
    }
    return mode > 3.5 ? (winding & 1) != 0 : winding != 0;
}

float path_coverage(const device float4 *records, float2 local, float mode, float radius,
                    float2 dx, float2 dy) {
    float coverage = 0.0;
    for (int x = 0; x < 2; ++x) {
        for (int y = 0; y < 2; ++y) {
            float2 point = local + dx * (float(x) * 0.5 - 0.25) + dy * (float(y) * 0.5 - 0.25);
            coverage += path_contains(records, point, mode, radius) ? 0.25 : 0.0;
        }
    }
    return coverage;
}

// The clip-space quad only bounds work. This writes depth and samples the
// actual cylinder or sphere with the same stable equation as headless input.
struct CanvasFragment {
    float4 color [[color(0)]];
    float depth [[depth(any)]];
};

// For axis-aligned perspective views, the cylinder equation has no screen Y
// term. Solve in the unnormalized ray parameter and retain enough information
// to apply the same distance and silhouette tests after reconstructing Y.
kernel void canvas_columns(constant CanvasMapping *mappings [[buffer(0)]],
                           device CanvasColumn *columns [[buffer(1)]],
                           constant uint2 &viewport [[buffer(2)]],
                           uint2 index [[thread_position_in_grid]]) {
    constant CanvasMapping &surface = mappings[index.y];
    float x = (float(index.x) - 0.5) * (2.0 / float(viewport.x)) - 1.0;
    float4 n = surface.inverse * float4(x, 0.0, -1.0, 1.0);
    float4 m = n + surface.inverse[2];
    float3 origin = n.xyz / n.w;
    float3 direction = m.xyz / m.w - origin;
    float k = surface.parameters.y;
    float direction_xz_squared = dot(direction.xz, direction.xz);
    float origin_xz_squared = dot(origin.xz, origin.xz);
    float a = k * direction_xz_squared;
    float b = k * dot(origin.xz, direction.xz) + direction.z;
    float c = k * origin_xz_squared + 2.0 * origin.z;
    float discriminant = b * b - a * c;
    float root = sqrt(max(discriminant, 0.0));
    float q = -b - (b < 0.0 ? -root : root);
    float t = max(q / a, abs(q) > 1e-20 ? c / q : -b / a);
    float3 point = origin + direction * t;
    float denominator = 1.0 + k * point.z;
    float x_normal = k * point.x;
    float angle = atan2(x_normal, denominator);
    float ratio = point.x / max(denominator, 1e-20);
    float small = k * ratio;
    float arc = denominator > 0.0 && abs(small) < 0.0001
        ? ratio * (1.0 - small * small / 3.0) : angle / k;
    float4 projected = surface.projection * float4(point, 1.0);
    float depth = projected.z / projected.w * 0.5 + 0.5;
    if (abs(angle) >= 3.14159265 || projected.w <= 0.0) t = -1.0;

    float vertical = surface.inverse[1].y;
    float origin_slope = vertical / n.w;
    float direction_slope = vertical / m.w - origin_slope;
    float hit_slope = origin_slope + direction_slope * t;
    float ndc_step = -2.0 / float(viewport.y);
    float3 tangent = unit(float3(denominator, 0.0, -x_normal));
    columns[index.y * (viewport.x + 2) + index.x] = {
        float4(point.xz, point.y + hit_slope, hit_slope * ndc_step),
        float4(arc, tangent.xz, depth),
        float4(t, direction_xz_squared, direction.y + direction_slope, direction_slope * ndc_step),
        float4(origin_xz_squared, origin.y + origin_slope, origin_slope * ndc_step, discriminant)
    };
}

float2 analytic_canvas(thread CanvasRaster &raster, constant CanvasMapping &surface,
                      constant CanvasAnalytic &draw, const device CanvasColumn *columns, constant float4 *edges, thread float &coverage, thread float &depth) {
    float2 chart;
    float3 right;
    float3 down;
    float3 point;
    float discriminant;
    float projected_depth;
    bool valid_distance;
    bool valid_chart;
    if (ipp_column_lookup) {
        // Include one helper-fragment column outside either viewport edge.
        uint x = uint(clamp(int(floor(raster.position.x)) + 1, 0, int(surface.options.y)));
        const device CanvasColumn &column = columns[surface.options.x - 1 + x];
        float y = raster.position.y;
        point = float3(column.point.x, fma(y, column.point.w, column.point.z), column.point.y);
        chart = float2(column.chart.x, point.y);
        right = float3(column.chart.y, 0.0, column.chart.z);
        down = float3(0.0, -1.0, 0.0);
        float dy = fma(y, column.ray.w, column.ray.z);
        float direction_squared = column.ray.y + dy * dy;
        float oy = fma(y, column.origin.z, column.origin.y);
        discriminant = column.origin.w / direction_squared;
        projected_depth = column.chart.w;
        valid_distance = column.ray.x >= 0.0 && column.ray.x * sqrt(direction_squared)
            <= sqrt(column.origin.x + oy * oy) + surface.parameters.w;
        valid_chart = true;
    } else {
        float4 n = raster.ray_origin;
        float4 m = n + surface.inverse[2];
        float3 origin = n.xyz / n.w;
        float3 direction = unit(m.xyz / m.w - origin);
        bool cylinder = ipp_canvas_cylinder || surface.parameters.x < 1.5;
        float3 axes = cylinder ? float3(1, 0, 1) : float3(1);
        float k = surface.parameters.y;
        float a = k * dot(direction * axes, direction);
        float b = k * dot(origin * axes, direction) + direction.z;
        float c = k * dot(origin * axes, origin) + 2.0 * origin.z;
        discriminant = b * b - a * c;
        float root = sqrt(max(discriminant, 0.0));
        float q = -b - (b < 0.0 ? -root : root);
        float safe_a = a != 0.0 ? a : 1.0;
        float t0 = q / safe_a;
        float t1 = abs(q) > 1e-20 ? c / q : -b / safe_a;
        bool near_root = surface.parameters.z > 0.5;
        float t = near_root ? min(t0, t1) : max(t0, t1);
        if (discriminant < 0.0) t = -b / safe_a;
        float limit = length(origin) + surface.parameters.w;
        bool valid_equation = true;
        if (a == 0.0) {
            valid_equation = b != 0.0 && near_root;
            t = b != 0.0 ? -c / (2.0 * b) : 0.0;
        }
        valid_distance = valid_equation && t >= 0.0 && t <= limit;
        t = clamp(t, 0.0, limit);
        point = origin + direction * t;

        float denominator = 1.0 + k * point.z;
        if (cylinder) {
            float x_normal = k * point.x;
            float angle = atan2(x_normal, denominator);
            valid_chart = abs(angle) < 3.14159265;
            float ratio = point.x / max(denominator, 1e-20);
            float small = k * ratio;
            float x = denominator > 0.0 && abs(small) < 0.0001
                ? ratio * (1.0 - small * small / 3.0) : angle / k;
            chart = float2(x, point.y);
            right = unit(float3(denominator, 0.0, -x_normal));
            down = float3(0.0, -1.0, 0.0);
        } else {
            float radial = length(k * point.xy);
            float angle = atan2(radial, denominator);
            valid_chart = angle < 3.14159265;
            float small = radial / max(denominator, 1e-20);
            float scale = denominator > 0.0 && small < 0.0001
                ? (1.0 - small * small / 3.0) / denominator : angle / max(radial, 1e-20);
            chart = point.xy * scale;
            float r = length(chart);
            float signed_angle = k * r;
            float sinc = abs(signed_angle) < 1e-4
                ? 1.0 - signed_angle * signed_angle / 6.0 : sin(signed_angle) / signed_angle;
            float2 radial_direction = chart / max(r, 1e-20);
            float response = cos(signed_angle) - sinc;
            float3 axis = float3(radial_direction * response, -sin(signed_angle));
            right = float3(sinc, 0.0, 0.0) + axis * radial_direction.x;
            down = -(float3(0.0, sinc, 0.0) + axis * radial_direction.y);
        }
        const float4 projected = surface.projection * float4(point, 1.0);
        projected_depth = projected.z / projected.w * 0.5 + 0.5;
        valid_chart = valid_chart && projected.w > 0.0;
    }
    depth = clamp(projected_depth, 0.0, 1.0);
    coverage = clamp(discriminant / max(fwidth(discriminant), 1e-20) + 0.5, 0.0, 1.0);
    if (!valid_chart || !valid_distance || projected_depth < 0.0 || projected_depth > 1.0)
        coverage = 0.0;

    float2 root_position = (chart + float2(surface.chart_to_root.x, -surface.chart_to_root.y))
        * surface.chart_to_root.zw;
    if (draw.clip_edges.x < 0.0) {
        float4 rectangle = draw.clip_rectangle;
        float2 distance = min(root_position - rectangle.xy, rectangle.zw - root_position);
        float2 footprint = max(fwidth(root_position), float2(1e-10));
        float2 edge = distance / footprint;
        coverage = min(coverage, clamp(min(edge.x, edge.y) + 0.5, 0.0, 1.0));
    } else {
        for (uint i = 0; i < uint(draw.clip_edges.x); ++i) {
            float3 edge = edges[i].xyz;
            if (all(edge.xy == float2(0.0))) continue;
            float distance = dot(edge, float3(root_position, 1.0));
            coverage = min(coverage, clamp(distance / max(fwidth(distance), 1e-10) + 0.5, 0.0, 1.0));
        }
    }

    raster.world = surface.world * float4(point, 1.0);
    raster.right = (surface.world * float4(right, 0.0)).xyz;
    raster.down = (surface.world * float4(down, 0.0)).xyz;
    raster.normal = cross(raster.down, raster.right);
    raster.orthonormal = surface.options.z && surface.parameters.x == 1.0;
    if (!raster.orthonormal) {
        float3 oriented = (surface.world * float4(cross(down, right), 0.0)).xyz;
        if (dot(raster.normal, oriented) < 0.0) raster.normal = -raster.normal;
    }
    return float2(dot(draw.local_x.xyz, float3(root_position, 1.0)),
                  dot(draw.local_y.xyz, float3(root_position, 1.0)));
}

static CanvasFragment canvas_shade(CanvasRaster raster, constant Lighting &scene_lighting,
                                   constant CanvasPaint &paint, texture2d<float> image,
                                   const device float4 *path, constant float4 *parameters,
                                   constant CanvasMapping *surface, constant CanvasAnalytic *analytic, const device CanvasColumn *columns, texture2d<uint, access::read> owners,
                                   uint selected_layer, bool intersect_surface,
                                   constant float4 *edges, uint content) {
    float2 local = raster.local;
    float depth = raster.position.z;
    float coverage = 1.0;
    if (intersect_surface) {
        local = analytic_canvas(raster, *surface, *analytic, columns, edges, coverage, depth);
    } else {
        raster.orthonormal = 0;
    }
    float2 dx = dfdx(local);
    float2 dy = dfdy(local);
    float2 footprint = max(abs(dx) + abs(dy), float2(1.0 / 65536.0));
    float2 inside = min(local - paint.clip.xy, paint.clip.zw - local);
    float clip = clamp(min(inside.x / footprint.x, inside.y / footprint.y) + 0.5, 0.0, 1.0);

    float4 color;
    if (content == 1 || (content == 0 && paint.options.y > 2.5)) {
        if (content == 1 && clip * coverage <= 0.0) discard_fragment();
        float coverage = path_coverage(path, local, paint.options.y, paint.options.w, dx, dy);
        float4 fill = paint.tint;
        if (paint.paint_size_opacity.w > 0.5) {
            fill = authored_color(paint, local, -coverage, fill, parameters, raster, scene_lighting);
        }
        float alpha = fill.a * coverage;
        color = float4(fill.rgb * alpha, alpha);
    } else if (content == 2 || (content == 0 && paint.options.y > 0.5)) {
        constexpr sampler pixels(coord::normalized, address::clamp_to_edge, filter::linear);
        float2 uv = local / paint.bounds.zw;
        if (selected_layer != 0) {
            uint2 extent(owners.get_width(), owners.get_height());
            uint2 pixel = min(uint2(clamp(uv, 0.0, 1.0) * float2(extent)), extent - 1);
            if (owners.read(pixel).x != selected_layer) discard_fragment();
        }
        color = image.sample(pixels, uv);
        if (paint.options.y < 1.5) color.rgb *= color.a;
        color.rgb *= paint.tint.rgb;
        color *= paint.tint.a;
    } else {
        color = shape_color(paint, local, paint.options.x, parameters, raster, scene_lighting);
        float4 section = paint.section;
        if (section.x > 0.5 && section.x < 1.5 && local.x < section.y) discard_fragment();
        if (section.x > 1.5 && section.x < 2.5 && local.y < section.y) discard_fragment();
        if (section.x > 2.5 && section.x < 3.5 && local.x >= section.y) discard_fragment();
        if (section.x > 3.5 && local.y >= section.y) discard_fragment();
    }
    if (color.a * clip * coverage <= 0.0) discard_fragment();
    return {color * (clip * coverage), depth};
}

// Each rasterized primitive selects its own paint, path and coverage record.
fragment CanvasFragment canvas_fragment(CanvasRaster raster [[stage_in]],
                                       constant Lighting &lighting [[buffer(0)]],
                                       constant CanvasPaint *paints [[buffer(1)]],
                                       texture2d<float> image [[texture(0)]],
                                       const device float4 *paths [[buffer(2)]],
                                       constant float4 *parameters [[buffer(3)]],
                                       constant CanvasAnalytic *intersection [[buffer(5)]],
                                       constant CanvasMapping *mapping [[buffer(8)]],
                                       const device CanvasColumn *columns [[buffer(9)]],
                                       texture2d<uint, access::read> owners [[texture(2)]],
                                       constant uint4 *bindings [[buffer(6)]],
                                       constant float4 *edges [[buffer(7)]]) {
    uint slot = raster.slot;
    uint4 binding = bindings[slot];
    return canvas_shade(raster, lighting, paints[slot], image, paths + binding.z, parameters + (binding.w >> 16),
                        mapping, intersection + slot, columns, owners, binding.y, true, edges + ((binding.w & 0xffffu) - 1),
                        ipp_canvas_shapes_only ? 3 : 0);
}

// Ordinary Canvas coverage leaves raster depth unchanged.
fragment float4 canvas_affine_fragment(CanvasRaster raster [[stage_in]],
                                       constant Lighting &lighting [[buffer(0)]],
                                       constant CanvasPaint *paints [[buffer(1)]],
                                       texture2d<float> image [[texture(0)]],
                                       const device float4 *paths [[buffer(2)]],
                                       constant float4 *parameters [[buffer(3)]],
                                       texture2d<uint, access::read> owners [[texture(2)]],
                                       constant uint4 *bindings [[buffer(6)]],
                                       constant float4 *edges [[buffer(7)]]) {
    uint slot = raster.slot;
    uint4 binding = bindings[slot];
    return canvas_shade(raster, lighting, paints[slot], image, paths + binding.z, parameters,
                        nullptr, nullptr, nullptr, owners, binding.y, false, edges, 0).color;
}

// Keep frequent coverage and image pipelines small enough for high occupancy.
fragment CanvasFragment canvas_path_fragment(CanvasRaster raster [[stage_in]],
                                       constant Lighting &lighting [[buffer(0)]],
                                       constant CanvasPaint *paints [[buffer(1)]],
                                       texture2d<float> image [[texture(0)]],
                                       const device float4 *paths [[buffer(2)]],
                                       constant float4 *parameters [[buffer(3)]],
                                       constant CanvasAnalytic *intersection [[buffer(5)]],
                                       constant CanvasMapping *mapping [[buffer(8)]],
                                       const device CanvasColumn *columns [[buffer(9)]],
                                       texture2d<uint, access::read> owners [[texture(2)]],
                                       constant uint4 *bindings [[buffer(6)]],
                                       constant float4 *edges [[buffer(7)]]) {
    uint slot = raster.slot;
    uint4 binding = bindings[slot];
    return canvas_shade(raster, lighting, paints[slot], image, paths + binding.z, parameters + (binding.w >> 16),
                        mapping, intersection + slot, columns, owners, binding.y, true, edges + ((binding.w & 0xffffu) - 1), 1);
}

fragment float4 canvas_image_fragment(CanvasRaster raster [[stage_in]],
                                       constant Lighting &lighting [[buffer(0)]],
                                       constant CanvasPaint *paints [[buffer(1)]],
                                       texture2d<float> image [[texture(0)]],
                                       const device float4 *paths [[buffer(2)]],
                                       constant float4 *parameters [[buffer(3)]],
                                       texture2d<uint, access::read> owners [[texture(2)]],
                                       constant uint4 *bindings [[buffer(6)]],
                                       constant float4 *edges [[buffer(7)]]) {
    uint slot = raster.slot;
    uint4 binding = bindings[slot];
    return canvas_shade(raster, lighting, paints[slot], image, paths + binding.z, parameters,
                        nullptr, nullptr, nullptr, owners, binding.y, false, edges, 2).color;
}

struct CanvasLayerFragment {
    float4 color [[color(0)]];
    uint owner [[color(1)]];
};

fragment CanvasLayerFragment canvas_layer_fragment(CanvasRaster raster [[stage_in]],
                                                  constant Lighting &lighting [[buffer(0)]],
                                                  constant CanvasPaint *paints [[buffer(1)]],
                                                  texture2d<float> image [[texture(0)]],
                                                  const device float4 *paths [[buffer(2)]],
                                                  constant float4 *parameters [[buffer(3)]],
                                                  texture2d<uint, access::read> owners [[texture(2)]],
                                                  constant uint4 *bindings [[buffer(6)]],
                                       constant float4 *edges [[buffer(7)]]) {
    uint slot = raster.slot;
    uint4 binding = bindings[slot];
    CanvasFragment output = canvas_shade(raster, lighting, paints[slot], image, paths + binding.z,
                                         parameters, nullptr, nullptr, nullptr, owners, binding.y, false, edges, 0);
    return {output.color, binding.x};
}

// A color attachment load preserves every pixel outside the damage scissor.
vertex float4 canvas_clear_vertex(uint index [[vertex_id]]) {
    constexpr float2 points[3] = {float2(-1.0, -1.0), float2(3.0, -1.0), float2(-1.0, 3.0)};
    return float4(points[index], 0.0, 1.0);
}

fragment float4 canvas_clear_fragment() {
    return float4(0.0);
}
