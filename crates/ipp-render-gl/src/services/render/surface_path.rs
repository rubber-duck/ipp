//! Renderer-owned packing for shared quadratic contours.
//!
//! One packed atlas holds every path of a font or drawing in two textures that
//! the analytic curve shader (`shaders/surface.frag`) reads with `texelFetch`.
//!
//! **Curve texels** hold `[p1.x, p1.y, p2.x, p2.y]` for one quadratic segment
//! as signed fixed-point integers. Consecutive segments of a contour share
//! their endpoints, following the Slug reference layout: a segment's end point
//! is the `xy` of the next texel, and each contour ends with one terminator
//! texel holding its closing point. Lines store their end point as the control
//! point, and the shader solves a segment as a line exactly when its control
//! equals its end, so float cancellation cannot turn a line into a parabola.
//!
//! **Precision.** Every coordinate is `integer * curve_scale`, where
//! `curve_scale` is a power of two chosen per atlas. When all coordinates are
//! representable exactly with 16-bit integers (TrueType outlines in font units,
//! including their half-unit implied on-curve points, for any em size up to
//! 16384 units), the atlas uses 16-bit texels and the shader sees the same
//! `f32` values as the source contours. Otherwise it uses 32-bit texels. A
//! power-of-two multiple of an `f32` is itself an `f32`, so these are exact
//! unless the coordinates span more than 31 bits of binary magnitude; then the
//! smallest fractions round to 2^-31 of the largest coordinate, finer than
//! `f32` resolves at that magnitude. Exact texels do not guarantee bit-identical
//! coverage: GPU compilers may contract or reorder the shader's float
//! arithmetic differently, which can move roots near curve tangents slightly.
//!
//! **Bands** are one unsigned channel. Each path owns [`BAND_HEADER_TEXELS`]
//! header texels at its `band_offset`: a list offset relative to `band_offset`
//! and a curve count for each of 16 horizontal then 16 vertical bands. List
//! entries are curve texel indices relative to the path's first curve texel.
//! The atlas uses 16-bit bands when every value fits and 32-bit bands
//! otherwise; the encoding is the same.
//!
//! Dense collections of closed contours also have a two-dimensional lookup.
//! The final two header lanes contain the grid size and relative tile-header
//! offset (zero grid size selects ordinary bands). Each tile has horizontal
//! and vertical offset/count pairs. A list retains original curve order, but
//! omits entire contours beyond the ray's antialiasing footprint: their closed
//! winding cancels and their edge weight is zero. No contours are split into
//! separately blended draws. Minified paths use the original bands. GPU callers
//! also retain bands when optional tiles exceed the device's texture extent.

use ipp_core::services::asset_management::quadratic::{QuadraticContour, QuadraticSegment};

use super::device::SurfacePathDescriptor;

pub(super) const BAND_COUNT: usize = 16;
pub(super) const TILE_COUNT: usize = 64;
const TILE_METADATA: usize = BAND_COUNT * 4;

/// Header texels per path: an offset and count for each horizontal and vertical band.
pub(super) const BAND_HEADER_TEXELS: u32 = (BAND_COUNT * 2 * 2 + 2) as u32;

const I16_LIMIT: f64 = i16::MAX as f64;
const I32_LIMIT: f64 = i32::MAX as f64;

/// Fixed-point curve texels of one packed atlas.
#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceCurveTexels {
    /// `RGBA16I` texels.
    Int16(Vec<[i16; 4]>),
    /// `RGBA32I` texels.
    Int32(Vec<[i32; 4]>),
}

impl SurfaceCurveTexels {
    /// Number of texels.
    pub fn len(&self) -> usize {
        match self {
            Self::Int16(texels) => texels.len(),
            Self::Int32(texels) => texels.len(),
        }
    }

    /// Whether the atlas has no curve texels.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes of texel data, excluding device row padding.
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Int16(texels) => std::mem::size_of_val(texels.as_slice()),
            Self::Int32(texels) => std::mem::size_of_val(texels.as_slice()),
        }
    }
}

/// Single-channel unsigned band texels of one packed atlas.
#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceBandTexels {
    /// `R16UI` texels.
    Uint16(Vec<u16>),
    /// `R32UI` texels.
    Uint32(Vec<u32>),
}

impl SurfaceBandTexels {
    /// Number of texels.
    pub fn len(&self) -> usize {
        match self {
            Self::Uint16(texels) => texels.len(),
            Self::Uint32(texels) => texels.len(),
        }
    }

    /// Whether the atlas has no band texels.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes of texel data, excluding device row padding.
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Uint16(texels) => std::mem::size_of_val(texels.as_slice()),
            Self::Uint32(texels) => std::mem::size_of_val(texels.as_slice()),
        }
    }
}

/// Device-independent texture contents for a packed path atlas.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfacePathTexels {
    /// Fixed-point curve texels.
    pub curves: SurfaceCurveTexels,
    /// Power-of-two factor converting curve texel integers to path units.
    pub curve_scale: f32,
    /// Band headers and lists.
    pub bands: SurfaceBandTexels,
}

impl SurfacePathTexels {
    /// Bytes of both textures' data, excluding device row padding.
    pub fn byte_len(&self) -> usize {
        self.curves.byte_len() + self.bands.byte_len()
    }
}

/// Texels for a set of paths and each path's lookup descriptor, in input order.
pub struct SurfacePathAtlas {
    /// Texture contents shared by every path.
    pub texels: SurfacePathTexels,
    /// Curve and band ranges per input path.
    pub descriptors: Vec<SurfacePathDescriptor>,
}

/// Pack paths given as bounds `[min.x, min.y, max.x, max.y]` and contours.
pub fn pack_surface_paths<'a>(
    paths: impl IntoIterator<Item = ([f32; 4], &'a [QuadraticContour])>,
) -> SurfacePathAtlas {
    pack_surface_paths_inner(paths, true)
}

/// Prefer tiles only when their atlas fits the device's texture extent. Unknown
/// limits retain the original bands; the device still validates actual uploads.
pub(super) fn pack_surface_paths_with_limit<'a>(
    paths: impl IntoIterator<Item = ([f32; 4], &'a [QuadraticContour])>,
    texture_edge: u32,
) -> SurfacePathAtlas {
    if texture_edge == 0 {
        return pack_surface_paths_inner(paths, false);
    }

    let paths: Vec<_> = paths.into_iter().collect();
    let atlas = pack_surface_paths_inner(paths.iter().copied(), true);
    let capacity = u64::from(texture_edge).pow(2);
    if atlas.texels.bands.len() as u64 <= capacity {
        return atlas;
    }

    // Repack every descriptor together: removing tile lists changes subsequent
    // paths' band offsets. Drop the oversized allocation before rebuilding.
    drop(atlas);
    pack_surface_paths_inner(paths, false)
}

fn pack_surface_paths_inner<'a>(
    paths: impl IntoIterator<Item = ([f32; 4], &'a [QuadraticContour])>,
    tiles_enabled: bool,
) -> SurfacePathAtlas {
    let mut points = Vec::new();
    let mut ranges = Vec::new();
    let mut bounds = Vec::new();
    for (path_bounds, contours) in paths {
        let start = points.len();
        push_contour_texels(contours, &mut points);
        ranges.push((start, points.len() - start));
        bounds.push(path_bounds);
    }

    let (curves, curve_scale) = quantize(&points);
    let extents = curve_extents(&points, &curves, curve_scale);

    let mut bands = Vec::new();
    let mut lists: [Vec<u32>; BAND_COUNT] = Default::default();
    let mut descriptors = Vec::with_capacity(ranges.len());
    for (&(start, count), path_bounds) in ranges.iter().zip(&bounds) {
        let band_offset = bands.len();
        bands.resize(band_offset + BAND_HEADER_TEXELS as usize, 0);
        for (group, axis) in [1, 0].into_iter().enumerate() {
            fill_band_lists(
                &mut lists,
                &extents[start..start + count],
                path_bounds[axis],
                path_bounds[axis + 2],
                axis,
            );
            let mut previous: Option<(usize, &[u32])> = None;
            for (band, list) in lists.iter().enumerate() {
                // Adjacent bands often cross the same curves; share one list.
                let list_offset = match previous {
                    Some((offset, earlier)) if earlier == list.as_slice() => offset,
                    _ => {
                        let offset = bands.len() - band_offset;
                        bands.extend_from_slice(list);
                        offset
                    }
                };
                previous = Some((list_offset, list));
                let header = band_offset + (group * BAND_COUNT + band) * 2;
                bands[header] = list_offset as u32;
                bands[header + 1] = list.len() as u32;
            }
        }
        // Bound extra lookup storage relative to the existing band lists. Large
        // spanning contours can make tiling less useful; they keep exact bands.
        let budget = (bands.len() - band_offset).saturating_mul(8);
        if let Some(tiles) = tiles_enabled
            .then(|| fill_tile_lists(&extents[start..start + count], *path_bounds, budget))
            .flatten()
        {
            let header = bands.len();
            bands[band_offset + TILE_METADATA] = TILE_COUNT as u32;
            bands[band_offset + TILE_METADATA + 1] = (header - band_offset) as u32;
            bands.resize(header + TILE_COUNT * TILE_COUNT * 4, 0);
            for (tile, lists) in tiles.iter().enumerate() {
                for (axis, list) in lists.iter().enumerate() {
                    let index = header + tile * 4 + axis * 2;
                    bands[index] = (bands.len() - band_offset) as u32;
                    bands[index + 1] = list.len() as u32;
                    bands.extend_from_slice(list);
                }
            }
        }
        descriptors.push(SurfacePathDescriptor::new(
            [start as u32, count as u32],
            band_offset as u32,
        ));
    }

    let bands = if bands.iter().all(|&value| value <= u32::from(u16::MAX)) {
        SurfaceBandTexels::Uint16(bands.into_iter().map(|value| value as u16).collect())
    } else {
        SurfaceBandTexels::Uint32(bands)
    };

    SurfacePathAtlas {
        texels: SurfacePathTexels {
            curves,
            curve_scale,
            bands,
        },
        descriptors,
    }
}

/// One unquantized curve texel; a terminator only supplies its predecessor's end.
struct PointTexel {
    lanes: [f32; 4],
    terminator: bool,
}

/// Quantized `[min.x, min.y, max.x, max.y]` hull extents per curve texel;
/// terminators, which start no segment, get an empty extent.
fn curve_extents(
    points: &[PointTexel],
    curves: &SurfaceCurveTexels,
    curve_scale: f32,
) -> Vec<[f32; 4]> {
    let value = |texel: usize, lane: usize| match curves {
        SurfaceCurveTexels::Int16(texels) => f32::from(texels[texel][lane]) * curve_scale,
        SurfaceCurveTexels::Int32(texels) => texels[texel][lane] as f32 * curve_scale,
    };
    (0..points.len())
        .map(|texel| {
            if points[texel].terminator {
                return [
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ];
            }
            let [x1, y1, x2, y2] = [0, 1, 2, 3].map(|lane| value(texel, lane));
            let [x3, y3] = [0, 1].map(|lane| value(texel + 1, lane));
            [
                x1.min(x2).min(x3),
                y1.min(y2).min(y3),
                x1.max(x2).max(x3),
                y1.max(y2).max(y3),
            ]
        })
        .collect()
}

/// Rebuild `lists` with the path-relative curves whose hull meets each of the
/// equal bands between `low` and `high` on `axis`, in ascending curve order.
fn fill_band_lists(
    lists: &mut [Vec<u32>; BAND_COUNT],
    extents: &[[f32; 4]],
    low: f32,
    high: f32,
    axis: usize,
) {
    let extent = high - low;
    let band_low = |band: usize| low + extent * band as f32 / BAND_COUNT as f32;
    let band_high = |band: usize| low + extent * (band + 1) as f32 / BAND_COUNT as f32;
    let estimate = |value: f32| ((value - low) / extent * BAND_COUNT as f32).floor();
    for list in lists.iter_mut() {
        list.clear();
    }
    for (local, hull) in extents.iter().enumerate() {
        let (minimum, maximum) = (hull[axis], hull[axis + 2]);
        if minimum > maximum {
            continue;
        }
        // Estimate the bands, then widen by one on each side so the exact
        // boundary comparisons below decide every rounding-sensitive case.
        let (first, last) = match (estimate(minimum), estimate(maximum)) {
            (first, last) if first.is_finite() && last.is_finite() => (
                (first - 1.0).clamp(0.0, (BAND_COUNT - 1) as f32) as usize,
                (last + 1.0).clamp(0.0, (BAND_COUNT - 1) as f32) as usize,
            ),
            _ => (0, BAND_COUNT - 1),
        };
        for (band, list) in lists.iter_mut().enumerate().take(last + 1).skip(first) {
            if maximum >= band_low(band) && minimum <= band_high(band) {
                list.push(local as u32);
            }
        }
    }
}

/// Conservative closed-contour ray candidates for each tile, in curve order.
/// Empty hulls terminate contours. The ray margin covers one full tile;
/// the shader selects these lists only when its pixel footprint fits a tile.
fn fill_tile_lists(
    extents: &[[f32; 4]],
    bounds: [f32; 4],
    budget: usize,
) -> Option<Vec<[Vec<u32>; 2]>> {
    if extents.len() < 8192 || extents.iter().filter(|hull| hull[0] > hull[2]).count() < 128 {
        return None;
    }
    let size = [bounds[2] - bounds[0], bounds[3] - bounds[1]];
    if !bounds.iter().all(|value| value.is_finite())
        || size
            .iter()
            .any(|&value| !value.is_finite() || value < 1.0 / 65536.0)
    {
        return None;
    }
    let cell = size.map(|value| value / TILE_COUNT as f32);
    let range = |hull: [f32; 4], axis: usize, margin: f32| {
        let estimate = |value: f32| ((value - bounds[axis]) / cell[axis]).floor();
        // Widen before exact tests, including rounding at cell boundaries.
        let first =
            (estimate(hull[axis]) - margin - 1.0).clamp(0.0, (TILE_COUNT - 1) as f32) as usize;
        let last =
            (estimate(hull[axis + 2]) + margin + 1.0).clamp(0.0, (TILE_COUNT - 1) as f32) as usize;
        first..=last
    };
    let meets = |hull: [f32; 4], axis: usize, index: usize, margin: f32| {
        let low = bounds[axis] + cell[axis] * (index as f32 - margin);
        let high = bounds[axis] + cell[axis] * (index as f32 + 1.0 + margin);
        hull[axis + 2] >= low && hull[axis] <= high
    };
    let mut tiles: Vec<[Vec<u32>; 2]> = (0..TILE_COUNT * TILE_COUNT)
        .map(|_| Default::default())
        .collect();
    let mut entries = TILE_COUNT * TILE_COUNT * 4;
    let mut first = 0;
    for (end, hull) in extents.iter().enumerate() {
        if hull[0] <= hull[2] {
            continue;
        }
        let contour = extents[first..end].iter().fold(
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
            |bounds, curve| {
                [
                    bounds[0].min(curve[0]),
                    bounds[1].min(curve[1]),
                    bounds[2].max(curve[2]),
                    bounds[3].max(curve[3]),
                ]
            },
        );
        for (local, curve) in extents.iter().enumerate().take(end).skip(first) {
            // Horizontal rays filter a curve's Y hull and its whole
            // contour's X hull; vertical rays exchange the axes.
            for (axis, ray) in [
                [contour[0], curve[1], contour[2], curve[3]],
                [curve[0], contour[1], curve[2], contour[3]],
            ]
            .into_iter()
            .enumerate()
            {
                // Root eligibility uses the sample centre on the perpendicular
                // axis; only the ray axis has antialiasing support to pad.
                let margin = if axis == 0 {
                    [1.0, 0.0]
                } else {
                    [0.0, 1.0]
                };
                for y in range(ray, 1, margin[1]).filter(|&y| meets(ray, 1, y, margin[1])) {
                    for x in range(ray, 0, margin[0]).filter(|&x| meets(ray, 0, x, margin[0])) {
                        entries += 1;
                        if entries > budget {
                            return None;
                        }
                        tiles[y * TILE_COUNT + x][axis].push(local as u32);
                    }
                }
            }
        }
        first = end + 1;
    }
    Some(tiles)
}

/// Append shared-endpoint texels for `contours`, closing open contours with a line.
fn push_contour_texels(contours: &[QuadraticContour], points: &mut Vec<PointTexel>) {
    for contour in contours {
        if contour.segments.is_empty() {
            continue;
        }
        let mut start = contour.start;
        for segment in &contour.segments {
            let (control, end) = match *segment {
                QuadraticSegment::Line {
                    to,
                } => (to, to),
                QuadraticSegment::Quadratic {
                    control,
                    to,
                } => (control, to),
            };
            points.push(PointTexel {
                lanes: [start[0], start[1], control[0], control[1]],
                terminator: false,
            });
            start = end;
        }
        if start != contour.start {
            let to = contour.start;
            points.push(PointTexel {
                lanes: [start[0], start[1], to[0], to[1]],
                terminator: false,
            });
        }
        let end = contour.start;
        points.push(PointTexel {
            lanes: [end[0], end[1], end[0], end[1]],
            terminator: true,
        });
    }
}

/// Convert points to the narrowest exact fixed-point texels, see the module docs.
fn quantize(points: &[PointTexel]) -> (SurfaceCurveTexels, f32) {
    let mut largest = 0.0f64;
    let mut fraction_bits = 0i32;
    for value in points.iter().flat_map(|point| point.lanes) {
        if value.is_finite() {
            largest = largest.max(f64::from(value.abs()));
            fraction_bits = fraction_bits.max(f32_fraction_bits(value));
        }
    }

    let range_bits = |limit: f64| {
        if largest == 0.0 {
            return 0;
        }
        let mut bits = (limit / largest).log2().floor() as i32;
        while largest * 2f64.powi(bits) > limit {
            bits -= 1;
        }
        bits.clamp(-120, 120)
    };
    let fixed = |bits: i32, value: f32| {
        if value.is_finite() {
            (f64::from(value) * 2f64.powi(bits)).round()
        } else {
            0.0
        }
    };

    if fraction_bits <= range_bits(I16_LIMIT) {
        let texels = points
            .iter()
            .map(|point| point.lanes.map(|value| fixed(fraction_bits, value) as i16))
            .collect();
        (SurfaceCurveTexels::Int16(texels), 2f32.powi(-fraction_bits))
    } else {
        let bits = fraction_bits.min(range_bits(I32_LIMIT));
        let texels = points
            .iter()
            .map(|point| point.lanes.map(|value| fixed(bits, value) as i32))
            .collect();
        (SurfaceCurveTexels::Int32(texels), 2f32.powi(-bits))
    }
}

/// Binary fraction digits needed to represent `value` exactly; at least zero.
fn f32_fraction_bits(value: f32) -> i32 {
    if value == 0.0 || value.fract() == 0.0 {
        return 0;
    }
    let bits = value.to_bits();
    let biased = ((bits >> 23) & 0xff) as i32;
    let (exponent, mantissa) = if biased == 0 {
        (-126, bits & 0x7f_ffff)
    } else {
        (biased - 127, (bits & 0x7f_ffff) | 0x80_0000)
    };
    23 - mantissa.trailing_zeros() as i32 - exponent
}

#[cfg(test)]
#[path = "surface_path_tests.rs"]
mod tests;

pub(super) async fn pack_surface_paths_async<'a>(
    paths: impl IntoIterator<Item = ([f32; 4], &'a [QuadraticContour])>,
) -> SurfacePathAtlas {
    let mut budget = ipp_core::services::asset_management::decode::DecodeBudget::default();
    let mut points = Vec::new();
    let mut ranges = Vec::new();
    let mut bounds = Vec::new();
    for (path_bounds, contours) in paths {
        let start = points.len();
        push_contour_texels_async(contours, &mut points).await;
        ranges.push((start, points.len() - start));
        bounds.push(path_bounds);
    }

    let (curves, curve_scale) = quantize_async(&points).await;
    let extents = curve_extents_async(&points, &curves, curve_scale).await;

    let mut bands = Vec::new();
    let mut lists: [Vec<u32>; BAND_COUNT] = Default::default();
    let mut descriptors = Vec::with_capacity(ranges.len());
    for (&(start, count), path_bounds) in ranges.iter().zip(&bounds) {
        let band_offset = bands.len();
        bands.resize(band_offset + BAND_HEADER_TEXELS as usize, 0);
        for (group, axis) in [1, 0].into_iter().enumerate() {
            fill_band_lists_async(
                &mut lists,
                &extents[start..start + count],
                path_bounds[axis],
                path_bounds[axis + 2],
                axis,
            )
            .await;
            let mut previous: Option<(usize, &[u32])> = None;
            for (band, list) in lists.iter().enumerate() {
                // Adjacent bands often cross the same curves; share one list.
                let list_offset = match previous {
                    Some((offset, earlier)) if earlier == list.as_slice() => offset,
                    _ => {
                        let offset = bands.len() - band_offset;
                        bands.extend_from_slice(list);
                        offset
                    }
                };
                previous = Some((list_offset, list));
                let header = band_offset + (group * BAND_COUNT + band) * 2;
                bands[header] = list_offset as u32;
                bands[header + 1] = list.len() as u32;
            }
        }
        budget.advance(0).await;
        descriptors.push(SurfacePathDescriptor::new(
            [start as u32, count as u32],
            band_offset as u32,
        ));
    }

    let bands = if bands.iter().all(|&value| value <= u32::from(u16::MAX)) {
        {
            let mut converted = Vec::with_capacity(bands.len());
            for value in bands {
                converted.push(value as u16);
                budget.advance(0).await;
            }
            SurfaceBandTexels::Uint16(converted)
        }
    } else {
        SurfaceBandTexels::Uint32(bands)
    };

    SurfacePathAtlas {
        texels: SurfacePathTexels {
            curves,
            curve_scale,
            bands,
        },
        descriptors,
    }
}

async fn push_contour_texels_async(contours: &[QuadraticContour], points: &mut Vec<PointTexel>) {
    let mut budget = ipp_core::services::asset_management::decode::DecodeBudget::default();
    for contour in contours {
        budget.advance(0).await;
        if contour.segments.is_empty() {
            continue;
        }
        let mut start = contour.start;
        for segment in &contour.segments {
            budget.advance(0).await;
            let (control, end) = match *segment {
                QuadraticSegment::Line {
                    to,
                } => (to, to),
                QuadraticSegment::Quadratic {
                    control,
                    to,
                } => (control, to),
            };
            points.push(PointTexel {
                lanes: [start[0], start[1], control[0], control[1]],
                terminator: false,
            });
            start = end;
        }
        if start != contour.start {
            let to = contour.start;
            points.push(PointTexel {
                lanes: [start[0], start[1], to[0], to[1]],
                terminator: false,
            });
        }
        let end = contour.start;
        points.push(PointTexel {
            lanes: [end[0], end[1], end[0], end[1]],
            terminator: true,
        });
    }
}

async fn fill_band_lists_async(
    lists: &mut [Vec<u32>; BAND_COUNT],
    extents: &[[f32; 4]],
    low: f32,
    high: f32,
    axis: usize,
) {
    let mut budget = ipp_core::services::asset_management::decode::DecodeBudget::default();
    let extent = high - low;
    let band_low = |band: usize| low + extent * band as f32 / BAND_COUNT as f32;
    let band_high = |band: usize| low + extent * (band + 1) as f32 / BAND_COUNT as f32;
    let estimate = |value: f32| ((value - low) / extent * BAND_COUNT as f32).floor();
    for list in lists.iter_mut() {
        list.clear();
    }
    for (local, hull) in extents.iter().enumerate() {
        budget.advance(0).await;
        let (minimum, maximum) = (hull[axis], hull[axis + 2]);
        if minimum > maximum {
            continue;
        }
        // Estimate the bands, then widen by one on each side so the exact
        // boundary comparisons below decide every rounding-sensitive case.
        let (first, last) = match (estimate(minimum), estimate(maximum)) {
            (first, last) if first.is_finite() && last.is_finite() => (
                (first - 1.0).clamp(0.0, (BAND_COUNT - 1) as f32) as usize,
                (last + 1.0).clamp(0.0, (BAND_COUNT - 1) as f32) as usize,
            ),
            _ => (0, BAND_COUNT - 1),
        };
        for (band, list) in lists.iter_mut().enumerate().take(last + 1).skip(first) {
            if maximum >= band_low(band) && minimum <= band_high(band) {
                list.push(local as u32);
            }
        }
    }
}

async fn quantize_async(points: &[PointTexel]) -> (SurfaceCurveTexels, f32) {
    let mut budget = ipp_core::services::asset_management::decode::DecodeBudget::default();
    let mut largest = 0.0f64;
    let mut fraction_bits = 0i32;
    for point in points {
        for value in point.lanes {
            if value.is_finite() {
                largest = largest.max(f64::from(value.abs()));
                fraction_bits = fraction_bits.max(f32_fraction_bits(value));
            }
        }
        budget.advance(0).await;
    }
    let range_bits = |limit: f64| {
        if largest == 0.0 {
            return 0;
        }
        let mut bits = (limit / largest).log2().floor() as i32;
        while largest * 2f64.powi(bits) > limit {
            bits -= 1;
        }
        bits.clamp(-120, 120)
    };
    let fixed = |bits: i32, value: f32| {
        if value.is_finite() {
            (f64::from(value) * 2f64.powi(bits)).round()
        } else {
            0.0
        }
    };
    if fraction_bits <= range_bits(I16_LIMIT) {
        let mut texels = Vec::with_capacity(points.len());
        for point in points {
            texels.push(point.lanes.map(|value| fixed(fraction_bits, value) as i16));
            budget.advance(0).await;
        }
        (SurfaceCurveTexels::Int16(texels), 2f32.powi(-fraction_bits))
    } else {
        let bits = fraction_bits.min(range_bits(I32_LIMIT));
        let mut texels = Vec::with_capacity(points.len());
        for point in points {
            texels.push(point.lanes.map(|value| fixed(bits, value) as i32));
            budget.advance(0).await;
        }
        (SurfaceCurveTexels::Int32(texels), 2f32.powi(-bits))
    }
}

async fn curve_extents_async(
    points: &[PointTexel],
    curves: &SurfaceCurveTexels,
    curve_scale: f32,
) -> Vec<[f32; 4]> {
    let value = |texel: usize, lane: usize| match curves {
        SurfaceCurveTexels::Int16(texels) => f32::from(texels[texel][lane]) * curve_scale,
        SurfaceCurveTexels::Int32(texels) => texels[texel][lane] as f32 * curve_scale,
    };
    let mut output = Vec::with_capacity(points.len());
    let mut budget = ipp_core::services::asset_management::decode::DecodeBudget::default();
    for (texel, point) in points.iter().enumerate() {
        let extent = if point.terminator {
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ]
        } else {
            let [x1, y1, x2, y2] = [0, 1, 2, 3].map(|lane| value(texel, lane));
            let [x3, y3] = [0, 1].map(|lane| value(texel + 1, lane));
            [
                x1.min(x2).min(x3),
                y1.min(y2).min(y3),
                x1.max(x2).max(x3),
                y1.max(y2).max(y3),
            ]
        };
        output.push(extent);
        budget.advance(0).await;
    }
    output
}
