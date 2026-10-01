//! Reusable world, asset, and EGL support for the Linux smoke runner.

pub(crate) mod deferred_removal;
pub(crate) mod egl;
pub(crate) mod frame_stats;
#[allow(dead_code)]
pub(crate) mod selection;
pub(crate) mod world;

#[allow(dead_code)]
pub(crate) mod error_checks;
pub(crate) mod shapes;
#[allow(dead_code)]
pub(crate) mod surface_cache_target;
#[allow(dead_code)]
pub(crate) mod surfaces;
pub(crate) mod textures;

pub(crate) mod debug_geometry;

pub(crate) mod lighting;
#[allow(dead_code)]
pub(crate) mod skinning;

#[allow(dead_code)]
pub(crate) mod mesh_poses;

#[allow(dead_code)]
pub(crate) mod custom_materials;

#[allow(dead_code)]
pub(crate) mod particles;
