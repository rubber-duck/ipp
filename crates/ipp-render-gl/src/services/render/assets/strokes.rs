//! Compact retained round-capped strokes in the existing two-texture device storage.
//!
//! The signed RGBA texture stores bit-exact f32 endpoint pairs, with the shared
//! radius in texel zero. The unsigned texture stores a one-dimensional hierarchy
//! of conservative candidate lists. Its axis minimizes expected segment overlap.
//! Each bin includes a full neighbouring-bin margin in addition to stroke radius;
//! the fragment shader chooses a level at least as wide as its pixel footprint.
//! Coarser levels therefore retain subpixel spikes and every original segment.
//! Storage is bounded by 32 list/header texels per segment, plus the shared header;
//! spanning strokes and device limits select fewer bins, down to one complete list.

use super::paths::{
    BAND_HEADER_TEXELS, SurfaceBandTexels, SurfaceCurveTexels, SurfacePathAtlas, SurfacePathTexels,
};
use crate::SurfacePathDescriptor;

pub(super) fn pack_stroke(
    bounds: [f32; 4],
    segments: &[[f32; 4]],
    radius: f32,
    limit: u32,
) -> SurfacePathAtlas {
    let mut points = Vec::with_capacity(segments.len() + 1);
    points.push([radius.to_bits() as i32, 0, 0, 0]);
    points.extend(segments.iter().map(|p| p.map(|v| v.to_bits() as i32)));
    let extent = [
        f64::from(bounds[2]) - f64::from(bounds[0]),
        f64::from(bounds[3]) - f64::from(bounds[1]),
    ];
    let overlap = |axis: usize| {
        segments
            .iter()
            .map(|p| {
                ((f64::from(p[axis + 2]) - f64::from(p[axis])).abs() + 2.0 * f64::from(radius))
                    / extent[axis].max(f64::MIN_POSITIVE)
            })
            .sum::<f64>()
    };
    let axis = usize::from(overlap(1) < overlap(0));
    let device_limit = (limit as usize).saturating_mul(limit as usize);
    let budget =
        (segments.len().saturating_mul(32) + BAND_HEADER_TEXELS as usize).min(device_limit);
    let mut bins = segments.len().next_power_of_two().clamp(1, 256);
    let bands = loop {
        if let Some(bands) = bin_hierarchy(bounds, segments, radius, axis, bins, budget) {
            break bands;
        }
        if bins == 1 {
            // Keep the complete geometry; the device reports an unsupported size.
            break bin_hierarchy(bounds, segments, radius, axis, 1, usize::MAX)
                .expect("one complete list");
        }
        bins /= 2;
    };
    let bands = if bands.iter().all(|&v| v <= u16::MAX as u32) {
        SurfaceBandTexels::Uint16(bands.into_iter().map(|v| v as u16).collect())
    } else {
        SurfaceBandTexels::Uint32(bands)
    };
    SurfacePathAtlas {
        descriptors: vec![SurfacePathDescriptor::new([0, points.len() as u32], 0)],
        texels: SurfacePathTexels {
            curves: SurfaceCurveTexels::Int32(points),
            curve_scale: 1.0,
            bands,
        },
    }
}

fn bin_hierarchy(
    bounds: [f32; 4],
    segments: &[[f32; 4]],
    radius: f32,
    axis: usize,
    mut bins: usize,
    budget: usize,
) -> Option<Vec<u32>> {
    let mut bands = vec![0; BAND_HEADER_TEXELS as usize];
    bands[0] = axis as u32;
    let low = f64::from(bounds[axis]);
    let extent = (f64::from(bounds[axis + 2]) - low).max(f64::MIN_POSITIVE);
    let mut level = 0;
    loop {
        if bands.len().checked_add(bins * 2)? > budget {
            return None;
        }
        let width = extent / bins as f64;
        let header = bands.len();
        bands[2 + level * 2] = bins as u32;
        bands[3 + level * 2] = header as u32;
        bands.resize(header + bins * 2, 0);
        for bin in 0..bins {
            let start = bands.len();
            let a = low + (bin as f64 - 1.0) * width;
            let b = low + (bin as f64 + 2.0) * width;
            for (index, p) in segments.iter().enumerate() {
                if f64::from(p[axis].max(p[axis + 2])) + f64::from(radius) >= a
                    && f64::from(p[axis].min(p[axis + 2])) - f64::from(radius) <= b
                {
                    if bands.len() >= budget {
                        return None;
                    }
                    bands.push(index as u32 + 1);
                }
            }
            bands[header + bin * 2] = start as u32;
            bands[header + bin * 2 + 1] = (bands.len() - start) as u32;
        }
        level += 1;
        if bins == 1 {
            break;
        }
        bins /= 2;
    }
    bands[1] = level as u32;
    Some(bands)
}

#[cfg(test)]
#[path = "strokes_tests.rs"]
mod tests;
