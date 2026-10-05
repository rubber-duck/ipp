//! Distance bands and refresh caps for optional Surface caching and required images.
//!
//! The metric is the World-space distance from the active camera to the Surface
//! anchor. Band 0 selects near/current presentation; affine Surfaces draw directly
//! and curved Surfaces retain current images. Cached band `k >= 1` covers
//! `[direct_distance * 2^(k-1), direct_distance * 2^k)`, ending in an unbounded
//! band. Each farther band halves the refresh cap down to a fixed floor.
//!
//! Band changes use distance hysteresis. Raster quality is independent of bands:
//! RenderService measures projected Surface pixel demand in the containing
//! device-pixel viewport, applies the authored resolution scale and its bounded
//! curvature allowance, and stabilizes image sizes before allocation. Distance
//! reduces perspective pixel demand naturally; orthographic quality stays fixed.

use super::SurfaceCache;
use crate::ErrorReason;

/// Number of cached distance bands; the last band has no upper bound.
pub const SURFACE_CACHE_MAX_BANDS: u8 = 5;

/// Relative distance past a band boundary required to leave the previous band.
pub const SURFACE_CACHE_BAND_HYSTERESIS: f32 = 0.1;

/// Largest accepted direct distance, in metres.
pub const SURFACE_CACHE_MAX_DIRECT_DISTANCE: f32 = 100_000.0;

/// Largest accepted multiplier of projected device-pixel demand.
pub const SURFACE_CACHE_MAX_RESOLUTION_SCALE: f32 = 4.0;

/// Largest accepted first-band refresh cap, in hertz.
pub const SURFACE_CACHE_MAX_REFRESH_HZ: f32 = 240.0;

/// Refresh cap below which farther bands stop halving, unless authored lower.
const REFRESH_HZ_FLOOR: f32 = 0.5;

/// Validated prepared copy of an authored [`SurfaceCache`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceCachePolicy {
    /// Camera-to-anchor distance in metres below which presentation uses near/current
    /// quality: affine Surfaces may draw directly; required curved images stay current.
    pub direct_distance: f32,
    /// Multiplier of projected device-pixel demand; 1 matches presentation density.
    pub resolution_scale: f32,
    /// Maximum content refresh rate in the first cached band, in hertz.
    pub max_refresh_hz: f32,
}

impl SurfaceCachePolicy {
    /// Validate an authored component: a finite direct distance in
    /// `0..=SURFACE_CACHE_MAX_DIRECT_DISTANCE`, and positive finite resolution scale
    /// and refresh values no larger than their maxima.
    pub fn new(component: &SurfaceCache) -> Result<Self, ErrorReason> {
        let SurfaceCache {
            direct_distance,
            resolution_scale,
            max_refresh_hz,
        } = *component;
        if !(0.0..=SURFACE_CACHE_MAX_DIRECT_DISTANCE).contains(&direct_distance)
            || !(resolution_scale > 0.0 && resolution_scale <= SURFACE_CACHE_MAX_RESOLUTION_SCALE)
            || !(max_refresh_hz > 0.0 && max_refresh_hz <= SURFACE_CACHE_MAX_REFRESH_HZ)
        {
            return Err(ErrorReason::InvalidValue);
        }

        Ok(Self {
            direct_distance,
            resolution_scale,
            max_refresh_hz,
        })
    }

    /// Band for a Surface without a previous selection: 0 uses near/current quality
    /// and `1..=SURFACE_CACHE_MAX_BANDS` select cached quality. Non-finite or negative
    /// distances select band 0; required curved images never become direct paint.
    pub fn initial_band(&self, distance: f32) -> u8 {
        self.band_with_boundary_scale(distance, 1.0)
    }

    /// Band for the current distance given the band selected previously.
    ///
    /// The previous band is kept until the distance crosses one of its
    /// boundaries by the hysteresis margin; the result then equals the band
    /// selected with boundaries moved by that margin in the direction of travel.
    pub fn band(&self, distance: f32, previous: u8) -> u8 {
        let previous = previous.min(SURFACE_CACHE_MAX_BANDS);
        let farther = self.band_with_boundary_scale(distance, 1.0 + SURFACE_CACHE_BAND_HYSTERESIS);
        if farther > previous {
            return farther;
        }

        let nearer = self.band_with_boundary_scale(distance, 1.0 - SURFACE_CACHE_BAND_HYSTERESIS);
        if nearer < previous {
            return nearer;
        }

        previous
    }

    /// Minimum World-time interval between content repaints for a band, in
    /// seconds; nondecreasing in band. Near/current quality (band 0) has no cap.
    pub fn refresh_interval_at(&self, band: u8) -> f64 {
        if band == 0 {
            return 0.0;
        }

        1.0 / f64::from(halved(self.max_refresh_hz, REFRESH_HZ_FLOOR, band))
    }

    fn band_with_boundary_scale(&self, distance: f32, scale: f32) -> u8 {
        if !distance.is_finite() || distance < 0.0 {
            return 0;
        }

        if self.direct_distance == 0.0 {
            return 1;
        }

        let mut boundary = self.direct_distance * scale;
        let mut band = 0;
        while band < SURFACE_CACHE_MAX_BANDS && distance >= boundary {
            band += 1;
            boundary *= 2.0;
        }

        band
    }
}

/// First-band value halved per farther band down to `min(floor, value)`.
fn halved(value: f32, floor: f32, band: u8) -> f32 {
    let halvings = band.clamp(1, SURFACE_CACHE_MAX_BANDS) - 1;
    (value / f32::from(1_u16 << halvings)).max(floor.min(value))
}

#[cfg(test)]
#[path = "cache_policy_tests.rs"]
mod tests;
