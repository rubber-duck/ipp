//! Shared device-pixel demand for optional affine caches and required Surface images.
//!
//! A 16×16 provider grid seeds image selection. Required images then measure
//! their actual tessellated triangles, retaining their uploaded positions for
//! quality checks when the view changes. For a triangle's perspective mapping,
//! the screen Jacobian is an affine numerator divided by homogeneous W². The
//! maximum vertex spectral norm of that numerator divided by minimum vertex W²
//! bounds magnification throughout the triangle, including diagonal/shear stretch.
//! This computes the scale needed for at least one texture texel per screen pixel
//! in every direction on the rendered mapping. Provider approximation is separate.
//! At most 32 occupied offsets are sampled; larger sets and near-plane crossings
//! request the cap instead of doing unbounded work or dividing unstable W.
//!
//! Images keep uniform content density/aspect, round their longest dimension up
//! in eight-pixel steps, grow as soon as raw demand exceeds resident dimensions,
//! and shrink below 75%. Device, aggregate image and mesh limits may reduce quality.

use super::texture_cache::SURFACE_CACHE_MAX_DIMENSION;
use crate::RenderError;
use ipp_core::{WorldViewport, systems::surface::Surface};

const CELLS: usize = 16;
const MAX_OFFSETS: usize = 32;

pub(in crate::services::render) fn surface_demand(
    geometry: &dyn Surface,
    offsets: &[f64],
    mvp: [f32; 16],
    viewport: WorldViewport,
) -> Result<[f64; 2], RenderError> {
    let extent = geometry.physical_extent();
    if offsets.len() > MAX_OFFSETS {
        return Ok(capped_demand(extent));
    }
    let mut demand = [1.0_f64; 2];
    for &offset in offsets {
        let sampled = sample_demand(extent, mvp, viewport, |uv| {
            geometry
                .sample(uv, offset)
                .map(|s| s.position)
                .map_err(|_| RenderError::UnavailableOutput)
        })?;
        for axis in 0..2 {
            demand[axis] = demand[axis].max(sampled[axis]);
        }
    }
    Ok(demand)
}

/// An already composed content-to-clip transform, including nested Canvas slots.
pub(in crate::services::render) fn plane_demand(
    extent: [f32; 2],
    mvp: [f32; 16],
    viewport: WorldViewport,
) -> Result<[f64; 2], RenderError> {
    sample_demand(extent.map(f64::from), mvp, viewport, |uv| {
        Ok([uv[0], uv[1], 0.0])
    })
}

fn capped_demand(extent: [f64; 2]) -> [f64; 2] {
    let density = f64::from(SURFACE_CACHE_MAX_DIMENSION) / extent[0].max(extent[1]);
    extent.map(|v| v * density)
}

fn sample_demand(
    extent: [f64; 2],
    mvp: [f32; 16],
    viewport: WorldViewport,
    mut sample: impl FnMut([f64; 2]) -> Result<[f64; 3], RenderError>,
) -> Result<[f64; 2], RenderError> {
    if extent.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || mvp.iter().any(|v| !v.is_finite())
        || viewport.width == 0
        || viewport.height == 0
    {
        return Err(RenderError::UnavailableOutput);
    }
    let mut positions = Vec::with_capacity((CELLS + 1).pow(2));
    for y in 0..=CELLS {
        for x in 0..=CELLS {
            positions.push(
                sample([
                    x as f64 / CELLS as f64 * extent[0],
                    y as f64 / CELLS as f64 * extent[1],
                ])?
                .map(|v| v as f32),
            );
        }
    }
    grid_demand(&positions, CELLS, extent, mvp, viewport)
}

/// Exact uploaded grid positions, used only when geometry or view quality changes.
pub(super) fn grid_demand(
    positions: &[[f32; 3]],
    cells: usize,
    extent: [f64; 2],
    mvp: [f32; 16],
    viewport: WorldViewport,
) -> Result<[f64; 2], RenderError> {
    let points: Vec<[f64; 4]> = positions
        .iter()
        .map(|p| {
            std::array::from_fn(|row| {
                (0..3)
                    .map(|col| f64::from(mvp[col * 4 + row]) * f64::from(p[col]))
                    .sum::<f64>()
                    + f64::from(mvp[12 + row])
            })
        })
        .collect();
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(RenderError::UnavailableOutput);
    }
    let mut front = false;
    let mut behind = false;
    for p in &points {
        let safe = p[3] > 1e-8 && p[2] + p[3] > 0.0;
        front |= safe;
        behind |= !safe;
    }
    if front && behind {
        return Ok(capped_demand(extent));
    }
    if !front {
        return Ok([1.0; 2]);
    }
    let step = extent.map(|v| v / cells as f64);

    let pixels = [
        f64::from(viewport.width) * 0.5,
        f64::from(viewport.height) * 0.5,
    ];
    let mut density = 0.0_f64;
    for y in 0..cells {
        for x in 0..cells {
            let top = y * (cells + 1) + x;
            let bottom = top + cells + 1;
            // The same diagonal and winding as mesh::tessellate.
            for [origin, horizontal, vertical] in [
                [points[top], points[top + 1], points[bottom]],
                [points[bottom + 1], points[bottom], points[top + 1]],
            ] {
                let du: [f64; 4] = std::array::from_fn(|i| (horizontal[i] - origin[i]) / step[0]);
                let dv: [f64; 4] = std::array::from_fn(|i| (vertical[i] - origin[i]) / step[1]);
                let minimum_w = origin[3].min(horizontal[3]).min(vertical[3]);
                for vertex in [origin, horizontal, vertical] {
                    let numerator: [[f64; 2]; 2] = std::array::from_fn(|i| {
                        [
                            pixels[i] * (du[i] * vertex[3] - vertex[i] * du[3]),
                            pixels[i] * (dv[i] * vertex[3] - vertex[i] * dv[3]),
                        ]
                    });
                    density = density.max(spectral_norm(numerator) / minimum_w.powi(2));
                }
            }
        }
    }
    Ok(extent.map(|v| (v * density).max(1.0)))
}

/// Largest singular value of a 2×2 matrix, including shear and diagonal stretch.
fn spectral_norm(matrix: [[f64; 2]; 2]) -> f64 {
    let [[a, b], [c, d]] = matrix;
    let column_u = a * a + c * c;
    let column_v = b * b + d * d;
    let cross = a * b + c * d;
    ((column_u + column_v + (column_u - column_v).hypot(2.0 * cross)) * 0.5).sqrt()
}

pub(in crate::services::render) fn image_size(
    demand: [f64; 2],
    resolution_scale: f32,
    limit: u32,
    previous: Option<[u32; 2]>,
) -> Option<[u32; 2]> {
    let limit = limit.min(SURFACE_CACHE_MAX_DIMENSION);
    let mut dimensions = demand.map(|v| v * f64::from(resolution_scale));
    if limit == 0 || dimensions.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return None;
    }
    let capped_scale = (f64::from(limit) / dimensions[0].max(dimensions[1])).min(1.0);
    dimensions = dimensions.map(|v| v * capped_scale);
    if let Some(previous) = previous.filter(|s| !s.contains(&0) && s.iter().all(|v| *v <= limit)) {
        let aspect = dimensions[0] / dimensions[1];
        let previous_aspect = f64::from(previous[0]) / f64::from(previous[1]);
        let same_aspect = (aspect - previous_aspect).abs() <= 2.0 / f64::from(previous[1]);
        let stable = (0..2).all(|i| {
            dimensions[i] <= f64::from(previous[i]) + 1e-3
                && dimensions[i] >= f64::from(previous[i]) * 0.75
        });
        if same_aspect && stable {
            return Some(previous);
        }
    }
    let longest = dimensions[0].max(dimensions[1]);
    let quantized = ((longest - 1e-3) / 8.0).ceil().max(1.0) * 8.0;
    let scale = quantized.min(f64::from(limit)) / longest;
    dimensions = dimensions.map(|v| v * scale);
    Some(dimensions.map(|v| (v - 1e-3).ceil().clamp(1.0, f64::from(limit)) as u32))
}

/// Allocate about 10% headroom on growth; active quality remains independent.
pub(in crate::services::render) fn image_capacity(size: [u32; 2], limit: u32) -> [u32; 2] {
    let limit = limit.min(SURFACE_CACHE_MAX_DIMENSION);
    size.map(|v| (v.saturating_mul(11).div_ceil(10)).min(limit))
}

pub(in crate::services::render) fn fits(size: [u32; 2], capacity: [u32; 2]) -> bool {
    (0..2).all(|i| size[i] <= capacity[i])
}

#[cfg(test)]
#[path = "quality_tests.rs"]
mod tests;
