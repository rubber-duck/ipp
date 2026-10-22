//! Viewport-clipped projected bounds for allocation priority. No occlusion or
//! mesh processing; separated shells share one enclosure and do not multiply
//! a Surface's priority by its image count.

use crate::RenderError;
use ipp_core::systems::surface::Surface;

pub(in crate::services::render) fn surface(
    geometry: &dyn Surface,
    offsets: &[f64],
    mvp: [f32; 16],
) -> Result<f64, RenderError> {
    Ok(super::screen_bounds::surface(geometry, offsets, mvp)?.coverage())
}

/// Small overlays count only their occupied, ancestor-clipped rectangle.
pub(in crate::services::render) fn plane(clip: [f32; 4], mvp: [f32; 16]) -> f64 {
    super::screen_bounds::plane(clip, mvp).map_or(0.0, |bounds| bounds.coverage())
}

#[cfg(test)]
#[path = "coverage_tests.rs"]
mod tests;
