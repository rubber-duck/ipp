//! Host-owned GL rendering through one graphics context.
//!
//! The service boundary types live here; lifecycle, frame, surface and shadow
//! submissions each own their implementation file.

mod frame;
mod lifecycle;
#[cfg(feature = "shadows")]
mod shadow;
#[cfg(feature = "surfaces")]
mod surface;

use std::fmt;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

#[cfg(feature = "particles")]
use super::assets::GlMeshData;
use super::frame_scratch::RenderFrameScratch;
use super::shader::RenderShaderConfig;
use crate::RenderDevice;
use ipp_core::services::asset_management::AssetKey;

// Preserve the authored dark sRGB background when clearing the linear intermediate.
const BACKGROUND: [f32; 4] = [0.003095975, 0.004400849, 0.007194409, 1.0];
/// RenderService initialization or frame submission failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// The graphics context is unavailable; the host must suspend rendering.
    ContextLost,
    /// The viewport must have positive dimensions representable by GL.
    InvalidViewport,
    /// An effective world transform cannot produce a finite model matrix.
    InvalidTransform,
    /// A final effective mesh reference had no retained CPU payload.
    MissingMesh,
    /// A final effective texture reference had no retained CPU payload.
    MissingTexture,
    /// A graphics operation, capability check or shader compilation failed.
    RenderDevice(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContextLost => f.write_str("GL context lost"),
            Self::InvalidViewport => f.write_str("invalid GL viewport"),
            Self::InvalidTransform => f.write_str("invalid renderable transform"),
            Self::MissingMesh => f.write_str("renderable mesh has no retained CPU asset"),
            Self::MissingTexture => f.write_str("renderable texture has no retained CPU asset"),
            Self::RenderDevice(message) => write!(f, "GL device: {message}"),
        }
    }
}

impl std::error::Error for RenderError {}
/// Work submitted by one successful render call.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RenderStats {
    /// GPU draw submissions, in stable prepared order.
    pub draw_calls: u32,
    /// Submitted triangles, including repeated instances of shared geometry.
    pub triangles: u32,
    /// Vertex/index/pixel uploads accounted by this submission, including pending
    /// shared service work once. Later worlds do not recount the same allocation.
    pub uploaded_bytes: u32,
    /// Instances skipped because their mesh could not acquire GPU residency.
    /// CPU geometry remains usable; resource reload permits another upload.
    pub failed_draw_calls: u32,
    /// Selected shadow-requesting lights left unshadowed by capacity/allocation fallback.
    pub unshadowed_lights: u32,
    /// The selected camera cannot represent this viewport; the frame is clear.
    /// Selection and session state remain available for repair or resize.
    pub invalid_camera: bool,
    /// Depth-only mesh submissions, separate from visible forward draws.
    #[cfg(feature = "shadows")]
    pub shadow_draw_calls: u32,
    /// Private depth allocation estimate (four bytes per texel).
    #[cfg(feature = "shadows")]
    pub shadow_resident_bytes: u32,
    /// Private debug GPU bytes, separately bounded and absent from world assets.
    pub debug_resident_bytes: u32,
}
/// Host-owned rendering of evaluated World inputs through one graphics context.
///
/// Resource providers prepare GPU representations before graphics readiness.
/// Drawing applies conservative frustum culling; LOD and visibility-driven residency
/// remain unimplemented. Context recovery preserves logical resource identities.
pub struct RenderService<D: RenderDevice> {
    pub(super) device: Rc<RefCell<D>>,
    asset_context_active: Rc<Cell<bool>>,
    custom_program_count: Rc<Cell<usize>>,
    pub(super) recipe_scratch: Vec<(RenderShaderConfig, bool)>,
    pub(super) program_keys: Vec<AssetKey>,
    // Rebuilt from this World's demand before submission. Keys only: payload and
    // device borrows still end before resource invalidation or World mutation.
    pub(super) program_lookup: Vec<Option<AssetKey>>,
    pub(super) custom_materials:
        BTreeMap<ipp_core::EntityId, super::custom_material::PreparedCustomMaterial>,
    pub(super) custom_diagnostics: BTreeMap<ipp_core::EntityId, String>,
    uploaded: Rc<Cell<u32>>,
    frame_scratch: RenderFrameScratch,
    #[cfg(feature = "particles")]
    pub(super) particle_quad_metadata:
        Option<ipp_core::services::asset_management::mesh_metadata::MeshMetadata>,
    light_selections: BTreeMap<ipp_core::WorldId, super::light_selection::LightSelectionState>,
    #[cfg(feature = "shadows")]
    shadow_capacity_limit: usize,
    #[cfg(feature = "particles")]
    particle_quad: Option<GlMeshData<D>>,
    #[cfg(feature = "shadows")]
    shadow_map: Option<D::ShadowMap>,
    #[cfg(feature = "shadows")]
    shadow_map_size: u32,
    debug: crate::services::render::debug_geometry::DebugGeometryRenderCache<D>,
    #[cfg(feature = "surfaces")]
    surface_program: Option<D::Program>,
    #[cfg(feature = "surfaces")]
    surface_instance_program: Option<D::Program>,
    #[cfg(feature = "surfaces")]
    surface_bitmap_program: Option<D::Program>,
    #[cfg(feature = "gui")]
    surface_box_program: Option<D::Program>,
}
fn prepared_normal(item: &ipp_core::RenderItem) -> Result<&[f32; 16], RenderError> {
    #[cfg(feature = "particles")]
    if item.particle.is_some() {
        return Ok(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]);
    }
    item.normal
        .as_ref()
        .map_err(|_| RenderError::InvalidTransform)
}

fn prepared_model(item: &ipp_core::RenderItem) -> &[f32; 16] {
    #[cfg(feature = "particles")]
    if item.particle.is_some() {
        return &[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
    }
    &item.model
}
