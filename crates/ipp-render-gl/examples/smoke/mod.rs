//! Reusable world, asset, and EGL support for the Linux smoke runner.

pub(crate) mod deferred_removal;
pub(crate) mod egl;
pub(crate) mod world;

pub(crate) mod shapes;
#[cfg(feature = "surfaces")]
#[allow(dead_code)]
pub(crate) mod surfaces;
pub(crate) mod textures;

pub(crate) mod debug_geometry;

pub(crate) mod lighting;
#[cfg(feature = "skeletal-animation")]
#[allow(dead_code)]
pub(crate) mod skinning;

#[cfg(feature = "mesh-poses")]
#[allow(dead_code)]
pub(crate) mod mesh_poses;

#[allow(dead_code)]
pub(crate) mod custom_materials;

#[cfg(feature = "particles")]
#[allow(dead_code)]
pub(crate) mod particles;
