//! Image demand from projected Surface bounds in physical viewport pixels.
//!
//! Bounds are analytic provider enclosures, projected with fixed corner work;
//! no mesh sampling, triangle scans or occlusion queries are involved. Uniform
//! content density preserves image aspect. This is an approximate footprint,
//! not a guarantee of one texel per pixel at every point of a curved mapping.
//! The actual draw mesh still obeys the provider's geometric error bound.
//!
//! A viewport-clipped rectangle determines allocation priority independently.
//! Keeping full projected dimensions for density prevents offscreen movement
//! from blurring the visible part. Eye-plane crossings request capped quality.

use super::{
    screen_bounds::{self, SurfaceScreenBounds},
    texture_cache::SURFACE_CACHE_MAX_DIMENSION,
};
use crate::RenderError;
use ipp_core::{WorldViewport, systems::surface::Surface};

pub(in crate::services::render) fn surface_demand(
    geometry: &dyn Surface,
    offsets: &[f64],
    mvp: [f32; 16],
    viewport: WorldViewport,
) -> Result<[f64; 2], RenderError> {
    demand(
        screen_bounds::surface(geometry, offsets, mvp)?,
        geometry.physical_extent(),
        viewport,
    )
}

/// An already composed content-to-clip transform, including nested Canvas slots.
pub(in crate::services::render) fn plane_demand(
    extent: [f32; 2],
    mvp: [f32; 16],
    viewport: WorldViewport,
) -> Result<[f64; 2], RenderError> {
    demand(
        screen_bounds::plane([0.0, 0.0, extent[0], extent[1]], mvp)?,
        extent.map(f64::from),
        viewport,
    )
}

fn demand(
    bounds: SurfaceScreenBounds,
    extent: [f64; 2],
    viewport: WorldViewport,
) -> Result<[f64; 2], RenderError> {
    if extent.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || viewport.width == 0
        || viewport.height == 0
    {
        return Err(RenderError::UnavailableOutput);
    }
    let density = match bounds {
        SurfaceScreenBounds::Invisible => return Ok([1.0; 2]),
        SurfaceScreenBounds::EyePlane => {
            f64::from(SURFACE_CACHE_MAX_DIMENSION) / extent[0].max(extent[1])
        }
        SurfaceScreenBounds::Rect {
            min,
            max,
        } => {
            let pixels = [f64::from(viewport.width), f64::from(viewport.height)];
            (0..2)
                .map(|axis| (max[axis] - min[axis]) * 0.5 * pixels[axis] / extent[axis])
                .fold(0.0_f64, f64::max)
        }
    };
    Ok(extent.map(|v| (v * density).max(1.0)))
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
        if same_aspect && (0..2).any(|i| dimensions[i] > f64::from(previous[i]) + 1e-3) {
            // Use the existing ~10% allocation headroom as a growth band,
            // rounded down to an active-size step that fits that headroom.
            // Small motion then reuses quality as well as storage. Initial
            // exact pixel sizes remain useful for overlays and 1:1 text.
            let capacity = image_capacity(previous, limit);
            let padded = (f64::from(capacity[0].max(capacity[1])) / 8.0).floor() * 8.0;
            let growth = (padded / dimensions[0].max(dimensions[1])).max(1.0);
            let growth = growth.min(f64::from(limit) / dimensions[0].max(dimensions[1]));
            dimensions = dimensions.map(|v| v * growth);
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
