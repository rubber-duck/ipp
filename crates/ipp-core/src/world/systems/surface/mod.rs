//! Headless two-dimensional Surface geometry, concrete providers and cache policy.

mod cache_policy;
mod component;
mod curved_component;
mod curved_geometry;
mod flat_surface;
mod geometry;
mod providers;
mod system;

pub use cache_policy::{
    SURFACE_CACHE_BAND_HYSTERESIS, SURFACE_CACHE_MAX_BANDS, SURFACE_CACHE_MAX_DIRECT_DISTANCE,
    SURFACE_CACHE_MAX_REFRESH_HZ, SURFACE_CACHE_MAX_RESOLUTION_SCALE, SurfaceCachePolicy,
};
pub use component::{FlatSurface, SurfaceCache};
pub use curved_component::{CylinderSurface, SphereSurface};
pub use system::{SurfaceSystem, SurfaceSystemFactory};

pub use geometry::{Surface, SurfaceDomain, SurfaceGeometry, SurfaceIntersection, SurfaceSample};
pub(crate) use providers::provider;
pub use providers::{SURFACE_PROVIDERS, from_component, is_provider, publication_is_current};

#[cfg(test)]
mod geometry_tests;

#[cfg(test)]
mod curved_geometry_tests;
