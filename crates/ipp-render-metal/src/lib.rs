//! Native Metal rendering of completed IPP World publications.
#![cfg(target_os = "macos")]

use ipp_render_scene::analytic_surface;
mod assets;
mod canvas;
mod canvas_batches;
use ipp_render_scene::canvas_damage;
mod canvas_images;
mod custom_mesh;
mod derived_meshes;
mod device;
use ipp_render_scene::lighting;
mod particles;
mod paths;
mod service;
mod shaders;
mod web_images;

pub use service::{MetalFrameSummary, MetalPresentation, MetalRenderError, MetalRenderService};

pub use web_images::{MetalWebImage, private_bytes as web_image_private_bytes};
