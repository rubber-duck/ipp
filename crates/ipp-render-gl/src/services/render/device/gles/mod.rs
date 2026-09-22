use std::ffi::{CStr, c_void};
use std::marker::PhantomData;
use std::ptr;
use std::rc::Rc;

#[cfg(feature = "gui")]
pub use super::GuiBoxVertex;
use super::RenderDevice;
#[cfg(feature = "gui")]
use super::SurfaceBoxShape;
#[cfg(feature = "surfaces")]
use super::{SurfacePathDescriptor, SurfacePathInstance};
use crate::RenderError;

mod custom;
mod lighting;
mod linear_target;
mod submission;
#[cfg(feature = "surfaces")]
mod surface;

const VERTEX_SHADER: u32 = 0x8B31;
const FRAGMENT_SHADER: u32 = 0x8B30;
const COMPILE_STATUS: u32 = 0x8B81;
const LINK_STATUS: u32 = 0x8B82;
const INFO_LOG_LENGTH: u32 = 0x8B84;
const ARRAY_BUFFER: u32 = 0x8892;
const ELEMENT_ARRAY_BUFFER: u32 = 0x8893;
const STATIC_DRAW: u32 = 0x88E4;
#[cfg(feature = "gui")]
const DYNAMIC_DRAW: u32 = 0x88E8;
const FLOAT: u32 = 0x1406;
const UNSIGNED_SHORT: u32 = 0x1403;
const TRIANGLES: u32 = 0x0004;

mod loader;
use loader::Functions;

/// Native GLES 3 binding with no context, window or translation-shim dependency.
///
/// The embedding host supplies a current context and function loader. The device
/// deliberately cannot move between threads. Surface binding/presentation and
/// detection of context destruction belong to the host.
pub struct GlesRenderDevice {
    gl: Functions,
    parameter_buffer: u32,
    parameter_capacity: usize,
    max_parameter_bytes: usize,
    max_parameter_textures: usize,
    submission: submission::GlesSubmissionState,
    #[cfg(feature = "particles")]
    instance_buffer: u32,
    #[cfg(feature = "particles")]
    instance_capacity: usize,
    #[cfg(feature = "particles")]
    instance_count: i32,
    linear_target: Option<linear_target::GlesLinearTarget>,
    presentation_target: Option<(u32, u32, [i32; 4])>,
    max_viewport: [i32; 2],
    max_texture_size: u32,
    exhaustive_draw_checks: bool,
    #[cfg(feature = "surfaces")]
    surface_quad_vao: u32,
    #[cfg(feature = "gui")]
    surface_box_quad_vao: u32,
    #[cfg(feature = "gui")]
    surface_box_quad_vbo: u32,
    #[cfg(feature = "surfaces")]
    surface_instance_buffer: u32,
    #[cfg(feature = "surfaces")]
    surface_instance_capacity: usize,
    #[cfg(feature = "surfaces")]
    surface_instance_scratch: Vec<[f32; 16]>,
    #[cfg(feature = "surfaces")]
    surface_viewport: [f32; 2],
    _thread: PhantomData<Rc<()>>,
    #[cfg(feature = "shadows")]
    shadow_target: Option<(u32, [i32; 4])>,
}

/// Native linked program with cached pass-specific uniform locations.
pub struct GlesRenderProgram {
    id: u32,
    parameters: u32,
    alpha_mode: i32,
    alpha_cutoff: i32,
    parameter_locations: std::cell::RefCell<std::collections::BTreeMap<String, i32>>,
    mvp: i32,
    material: i32,
    lighting: lighting::GlesLightingLocations,
    uniforms: std::cell::RefCell<super::uniform_cache::RenderUniformCache>,
    #[cfg(feature = "skeletal-animation")]
    joints: i32,
    #[cfg(feature = "mesh-poses")]
    pose_weight: i32,
    texture: i32,
}

/// Native vertex array and its exclusively owned buffers.
pub struct GlesRenderMesh {
    vao: u32,
    buffers: [u32; 2],
    indices: i32,
    color: u32,
    normal: u32,
    #[cfg(feature = "skeletal-animation")]
    skin: [u32; 2],
    uv: u32,
    weight: u32,
}

/// Native vertex array and buffer for a retained GUI triangle batch.
#[cfg(feature = "gui")]
pub struct GlesGuiBatch {
    pub(crate) vao: u32,
    pub(crate) vbo: u32,
    pub(crate) vertex_count: i32,
    pub(crate) bytes: usize,
}

#[cfg(feature = "surfaces")]
pub struct GlesSurfacePath {
    texture: u32,
    band_texture: u32,
    vao: u32,
    segment_count: i32,
    texture_width: i32,
    band_count: u32,
    band_width: i32,
}

mod commands;
mod device_setup;
