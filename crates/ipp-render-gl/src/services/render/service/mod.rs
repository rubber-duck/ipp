//! Host-owned GL rendering through one graphics context.
//!
//! The service boundary types live here; lifecycle, frame, surface and shadow
//! submissions each own their implementation file.

#[cfg(feature = "instrumentation")]
mod gpu_profiling;
#[cfg(feature = "instrumentation")]
pub use gpu_profiling::{
    RenderGlCallWindow, RenderGlStopReason, RenderGpuIdentity, RenderGpuSample, RenderGpuSampling,
    RenderGpuScope,
};

mod asset_export;
pub use asset_export::RenderAssetExportDelay;

mod canvas_composition;
mod composition;
mod frame;
mod inclusion;
mod lifecycle;
mod plot_planes;
mod projected_surface;
mod shadow;
mod surface;
mod surface_cache;

use std::fmt;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use super::custom_material::CustomMaterialFallback;

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
    /// The exact selected output or its publication is no longer available.
    UnavailableOutput,
    /// Deselect the previous Host explicitly before preparing another catalog.
    HostCatalogMismatch,
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
            Self::UnavailableOutput => f.write_str("selected publication is unavailable"),
            Self::HostCatalogMismatch => f.write_str("renderer is bound to another Host catalog"),
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
    inclusions: inclusion::OutputInclusions,
    pub(super) device: Rc<RefCell<D>>,
    asset_context_active: super::asset_context::RenderAssetContext,
    asset_export_delay: Option<RenderAssetExportDelay>,
    #[cfg(feature = "instrumentation")]
    asset_export_staging_gate: bool,
    pub(super) recipe_scratch: Vec<(RenderShaderConfig, bool)>,
    pub(super) program_demand: Option<super::program_assets::ProgramDemand>,
    pub(super) prepared_output: Option<ipp_core::OutputRef>,
    // Rebuilt from this World's demand before submission. Keys only: payload and
    // device borrows still end before resource invalidation or World mutation.
    pub(super) program_lookup: Vec<Option<AssetKey>>,
    pub(super) custom_materials:
        BTreeMap<super::scene::RenderEntity, super::custom_material::PreparedCustomMaterial>,
    /// Retained custom-material fallbacks, so each changed reason is logged once.
    pub(super) custom_fallbacks: BTreeMap<super::scene::RenderEntity, CustomMaterialFallback>,
    uploads: super::frame_statistics::RenderUploadCounter,
    /// Statistics of the last completed render.
    statistics: super::frame_statistics::RenderStatistics,
    #[cfg(feature = "instrumentation")]
    gpu_capture: gpu_profiling::RenderGpuCapture,
    frame_scratch: RenderFrameScratch,
    pub(super) light_selections:
        BTreeMap<ipp_core::OutputRef, super::light_selection::LightSelectionState>,
    shadow_capacity_limit: usize,
    particle_quad: Option<GlMeshData<D>>,
    shadow_map: Option<D::ShadowMap>,
    shadow_map_size: u32,
    pub(super) camera_targets: BTreeMap<ipp_core::OutputRef, (D::SurfaceCacheTarget, [u32; 2])>,
    pub(super) camera_completed: std::collections::BTreeSet<ipp_core::OutputRef>,
    debug: crate::services::render::debug_geometry::DebugGeometryRenderCache<D>,
    generated_paths: super::generated_paths::GeneratedPathCache<D>,
    generated_meshes: super::generated_meshes::GeneratedMeshCache<D>,
    pub(super) plot_label_layouts:
        BTreeMap<ipp_core::OutputRef, super::plot_label_layout::PlotLabelLayoutState>,
    plot_plane_caches: BTreeMap<plot_planes::PlotPlaneKey, plot_planes::PlotPlaneCache<D>>,
    pub(super) surface_program: Option<D::Program>,
    surface_instance_program: Option<D::Program>,
    surface_bitmap_program: Option<D::Program>,
    surface_cache_program: Option<D::Program>,
    surface_image_program: Option<D::Program>,
    projected_surfaces: BTreeMap<ipp_core::OutputRef, projected_surface::ProjectedSurface<D>>,
    projected_repainting: Option<ipp_core::OutputRef>,
    /// Whole-Surface cache images shared by every World on this context.
    surface_cache: super::surface_cache::SurfaceTextureCache<D::SurfaceCacheTarget>,
    canvas_caches: BTreeMap<ipp_core::OutputRef, surface_cache::CanvasCacheState>,
    canvas_cache_frame: surface_cache::CanvasCacheFrame,
    /// Resources whose primitives the last Surface submission skipped because
    /// they were not resident; a cache repaint records them as incomplete.
    surface_missing: Vec<ipp_core::services::asset_management::AssetKey>,
    /// Failed submissions attributable only to nonresident primitive resources.
    surface_missing_draws: u32,
    /// The last Surface submission drew a text run analytically because its
    /// atlas entries were not all resident.
    surface_analytic_text: bool,
    /// The last Surface submission had no usable retained GUI storage, so it
    /// skipped its boxes and drew its text analytically.
    surface_gui_unretained: bool,
    /// The canvas program drawing GUI boxes, strokes and arcs with their custom
    /// paints, and the paints it holds.
    canvas_paints: super::canvas_paint::CanvasPaintPrograms<D>,
    /// The static program drawing retained atlas glyphs.
    gui_glyph_program: Option<D::Program>,
    /// Canvas entities whose paint draws their colour, with the reason; a changed
    /// reason is logged once.
    canvas_paint_fallbacks: BTreeMap<
        (ipp_core::OutputRef, ipp_core::EntityId),
        super::canvas_paint::CanvasPaintFallback,
    >,
    gui_batch_cache: BTreeMap<ipp_core::OutputRef, super::gui_batch::GuiBatchRenderCache<D>>,
    pub(super) glyph_atlas: super::glyph_atlas::GlyphAtlas<D>,
    glyph_batch_cache: BTreeMap<ipp_core::OutputRef, super::glyph_atlas::GlyphBatchRenderCache>,
    /// Painter-order work of the Surface being submitted.
    surface_ops: Vec<surface::SurfaceOp>,
    /// Glyph misses, population queue and outcomes of the current World frame.
    glyph_frame: super::glyph_atlas::GlyphFrameWork,
    /// Per-frame population allowance, shared by every World on this context.
    glyph_population: super::glyph_atlas::GlyphPopulationBudget,
    /// Each World's last drawn Surface paint revisions and identity orders.
    surface_paint: BTreeMap<ipp_core::OutputRef, super::retained_surfaces::SurfacePaintTracker>,
    /// Each World's retained analytic glyph instance streams.
    analytic_glyphs: BTreeMap<ipp_core::OutputRef, super::analytic_glyphs::AnalyticGlyphCache<D>>,
}
fn prepared_normal(item: &ipp_core::RenderItem) -> Result<&[f32; 16], RenderError> {
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
    if item.particle.is_some() {
        return &[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
    }
    &item.model
}
