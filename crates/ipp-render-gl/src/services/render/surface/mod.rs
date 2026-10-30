//! Surface presentation: analytic intersections, whole-Surface image caches,
//! refresh scheduling, sampled meshes and image quality.

pub(super) mod allocation;
pub(super) mod analytic;
pub(super) mod cache_refresh;
pub(super) mod coverage;
pub(super) mod damage;
mod mesh;
pub(super) mod projected;
pub(super) mod quality;
mod screen_bounds;
pub(super) mod texture_cache;
