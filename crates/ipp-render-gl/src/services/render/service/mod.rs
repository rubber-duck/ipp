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
#[cfg(feature = "surfaces")]
mod surface_cache;

use std::fmt;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

#[cfg(any(test, feature = "diagnostics"))]
use super::custom_material::CustomMaterialFallback;

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

/// Host-owned rendering of evaluated World inputs through one graphics context.
///
/// Resource providers prepare GPU representations before graphics readiness.
/// Drawing applies conservative frustum culling; LOD and visibility-driven residency
/// remain unimplemented. Context recovery preserves logical resource identities.
pub struct RenderService<D: RenderDevice> {
    pub(super) device: Rc<RefCell<D>>,
    asset_context_active: Rc<Cell<bool>>,
    pub(super) recipe_scratch: Vec<(RenderShaderConfig, bool)>,
    pub(super) program_keys: Vec<AssetKey>,
    // Rebuilt from this World's demand before submission. Keys only: payload and
    // device borrows still end before resource invalidation or World mutation.
    pub(super) program_lookup: Vec<Option<AssetKey>>,
    pub(super) custom_materials:
        BTreeMap<ipp_core::EntityId, super::custom_material::PreparedCustomMaterial>,
    /// Retained custom-material fallbacks, so each changed reason is logged once.
    #[cfg(any(test, feature = "diagnostics"))]
    pub(super) custom_fallbacks: BTreeMap<ipp_core::EntityId, CustomMaterialFallback>,
    uploads: super::frame_statistics::RenderUploadCounter,
    /// Statistics of the last completed render.
    #[cfg(any(test, feature = "diagnostics"))]
    statistics: super::frame_statistics::RenderStatistics,
    /// Context-wide retained Surface residency, maintained per World render.
    #[cfg(all(feature = "surfaces", any(test, feature = "diagnostics")))]
    retained_surface_residency: surface::RetainedSurfaceResidency,
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
    pub(super) surface_program: Option<D::Program>,
    #[cfg(feature = "surfaces")]
    surface_instance_program: Option<D::Program>,
    #[cfg(feature = "surfaces")]
    surface_bitmap_program: Option<D::Program>,
    #[cfg(feature = "surfaces")]
    surface_cache_program: Option<D::Program>,
    /// Whole-Surface cache images shared by every World on this context.
    #[cfg(feature = "surfaces")]
    surface_cache: super::surface_cache::SurfaceTextureCache<D::SurfaceCacheTarget>,
    /// Reused per-frame cache planning inputs.
    #[cfg(feature = "surfaces")]
    surface_cache_inputs: Vec<super::surface_cache::SurfaceCacheInput>,
    /// Resources whose primitives the last Surface submission skipped because
    /// they were not resident; a cache repaint records them as incomplete.
    #[cfg(feature = "surfaces")]
    surface_missing: Vec<ipp_core::services::asset_management::AssetKey>,
    /// The last Surface submission drew a text run analytically because its
    /// atlas entries were not all resident.
    #[cfg(feature = "gui")]
    surface_analytic_text: bool,
    /// Program drawing GUI boxes and atlas glyphs.
    #[cfg(feature = "gui")]
    surface_gui_program: Option<D::Program>,
    #[cfg(feature = "gui")]
    gui_batch_cache: BTreeMap<ipp_core::WorldId, super::gui_batch::GuiBatchRenderCache<D>>,
    #[cfg(feature = "gui")]
    pub(super) glyph_atlas: super::glyph_atlas::GlyphAtlas<D>,
    #[cfg(feature = "gui")]
    glyph_batch_cache: BTreeMap<ipp_core::WorldId, super::glyph_atlas::GlyphBatchRenderCache>,
    /// Painter-order work of the Surface being submitted.
    #[cfg(feature = "gui")]
    surface_ops: Vec<surface::SurfaceOp>,
    /// Glyph misses, population queue and outcomes of the current World frame.
    #[cfg(feature = "gui")]
    glyph_frame: super::glyph_atlas::GlyphFrameWork,
    /// Per-frame population allowance, shared by every World on this context.
    #[cfg(feature = "gui")]
    glyph_population: super::glyph_atlas::GlyphPopulationBudget,
    /// Surfaces the current frame submitted; `None` until submission reaches them.
    #[cfg(feature = "surfaces")]
    submitted_surfaces: Option<std::collections::BTreeSet<ipp_core::EntityId>>,
    /// Each World's last drawn Surface paint revisions and identity orders.
    #[cfg(feature = "surfaces")]
    surface_paint: BTreeMap<ipp_core::WorldId, super::retained_surfaces::SurfacePaintTracker>,
    /// Each World's retained analytic glyph instance streams.
    #[cfg(feature = "surfaces")]
    analytic_glyphs: BTreeMap<ipp_core::WorldId, super::analytic_glyphs::AnalyticGlyphCache<D>>,
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
