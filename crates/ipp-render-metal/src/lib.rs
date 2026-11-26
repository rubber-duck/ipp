//! Native Metal rendering of completed IPP World publications.
#![cfg(target_os = "macos")]

mod analytic_surface;
mod assets;
mod canvas;
mod canvas_batches;
mod canvas_damage;
mod canvas_images;
mod custom_mesh;
mod derived_meshes;
mod device;
mod fill_grid;
mod lighting;
mod particles;
mod paths;
mod service;
mod shaders;
mod web_images;

pub use service::{MetalFrameSummary, MetalPresentation, MetalRenderError, MetalRenderService};

pub use web_images::{MetalWebImage, private_bytes as web_image_private_bytes};
