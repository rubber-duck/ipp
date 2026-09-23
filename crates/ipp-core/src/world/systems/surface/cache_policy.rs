//! Distance bands, texel density and refresh caps for opted-in Surface caches.
//!
//! The metric is the World-space distance from the active camera's World
//! translation to the evaluated Surface anchor (the entity centre). Distances
//! below [`SurfaceCachePolicy::direct_distance`] select direct presentation
//! (band 0). Cached band `k >= 1` covers
//! `[direct_distance * 2^(k-1), direct_distance * 2^k)`, and band
//! [`SURFACE_CACHE_MAX_BANDS`] extends to infinity. Each band halves the
//! texel density and the refresh cap of the previous one down to fixed
//! floors, so neither increases with distance. A zero direct distance caches
//! at every distance in band 1.
//!
//! Band changes use hysteresis: leaving the previous band requires crossing
//! its boundary by [`SURFACE_CACHE_BAND_HYSTERESIS`] of the boundary distance,
//! so small camera movements around a boundary keep the current band and its
//! cache resolution. Orthographic cameras use the same distance rule.

use super::SurfaceCache;
use crate::ErrorReason;

/// Number of cached distance bands; the last band has no upper bound.
pub const SURFACE_CACHE_MAX_BANDS: u8 = 5;

/// Relative distance past a band boundary required to leave the previous band.
pub const SURFACE_CACHE_BAND_HYSTERESIS: f32 = 0.1;

/// Largest accepted direct distance, in metres.
pub const SURFACE_CACHE_MAX_DIRECT_DISTANCE: f32 = 100_000.0;

/// Largest accepted first-band texel density, per metre.
pub const SURFACE_CACHE_MAX_TEXELS_PER_METRE: f32 = 16_384.0;

/// Largest accepted first-band refresh cap, in hertz.
pub const SURFACE_CACHE_MAX_REFRESH_HZ: f32 = 240.0;

/// Density below which farther bands stop halving, unless authored lower.
const TEXELS_PER_METRE_FLOOR: f32 = 16.0;

/// Refresh cap below which farther bands stop halving, unless authored lower.
const REFRESH_HZ_FLOOR: f32 = 0.5;

/// Validated prepared copy of an authored [`SurfaceCache`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceCachePolicy {
    /// Camera-to-anchor distance in metres below which presentation stays direct.
    pub direct_distance: f32,
    /// Cache texel density in the first cached band, per Surface metre.
    pub texels_per_metre: f32,
    /// Maximum content refresh rate in the first cached band, in hertz.
    pub max_refresh_hz: f32,
}

impl SurfaceCachePolicy {
    /// Validate an authored component: a finite direct distance in
    /// `0..=SURFACE_CACHE_MAX_DIRECT_DISTANCE`, and positive finite density
    /// and refresh values no larger than their maxima.
    pub fn new(component: &SurfaceCache) -> Result<Self, ErrorReason> {
        let SurfaceCache {
            direct_distance,
            texels_per_metre,
            max_refresh_hz,
        } = *component;
        if !(0.0..=SURFACE_CACHE_MAX_DIRECT_DISTANCE).contains(&direct_distance)
            || !(texels_per_metre > 0.0 && texels_per_metre <= SURFACE_CACHE_MAX_TEXELS_PER_METRE)
            || !(max_refresh_hz > 0.0 && max_refresh_hz <= SURFACE_CACHE_MAX_REFRESH_HZ)
        {
            return Err(ErrorReason::InvalidValue);
        }

        Ok(Self {
            direct_distance,
            texels_per_metre,
            max_refresh_hz,
        })
    }

    /// Band for a Surface without a previous selection: 0 is direct and
    /// `1..=SURFACE_CACHE_MAX_BANDS` are cached. Non-finite or negative
    /// distances select direct presentation.
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

    /// Cache texel density for a band, per Surface metre; nonincreasing in band.
    pub fn texels_per_metre_at(&self, band: u8) -> f32 {
        halved(self.texels_per_metre, TEXELS_PER_METRE_FLOOR, band)
    }

    /// Minimum World-time interval between content repaints for a band, in
    /// seconds; nondecreasing in band. Direct presentation (band 0) has none.
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
