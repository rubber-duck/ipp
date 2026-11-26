//! Plot presentation: camera-facing planes, view placement and label layout.

pub(super) mod planes;
pub(super) use ipp_core::systems::render::plot_presentation::{
    label_layout, plane_facing, view_placement,
};

#[cfg(test)]
mod profile_tests;
