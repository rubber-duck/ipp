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

use ipp_core::services::asset_management::quadratic::{QuadraticContour, QuadraticSegment};

use super::device::SurfacePathDescriptor;

pub(super) const BAND_COUNT: usize = 16;

/// Header texels per path: an offset and count for each horizontal and vertical band.
pub(super) const BAND_HEADER_TEXELS: u32 = (BAND_COUNT * 2 * 2) as u32;

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
