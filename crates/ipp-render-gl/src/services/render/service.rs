//! Host-owned GL rendering through one graphics context.
//!
//! The service boundary types live here; lifecycle, frame, surface and shadow
//! submissions each own their implementation file.

use std::fmt;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use super::materials::custom_material::CustomMaterialFallback;

use super::assets::export::RenderAssetExportDelay;
use super::assets::loaders::GlMeshData;
use super::frame::scratch::RenderFrameScratch;
use super::materials::shader::RenderShaderConfig;
use crate::RenderDevice;
use ipp_core::services::asset_management::AssetKey;

// Preserve the authored dark sRGB background when clearing the linear intermediate.
pub(super) const BACKGROUND: [f32; 4] = [0.003095975, 0.004400849, 0.007194409, 1.0];

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

pub(super) type CameraTarget<T> = (T, [u32; 2], [u32; 2]);

/// Host-owned rendering of evaluated World inputs through one graphics context.
///
/// Resource providers prepare GPU representations before graphics readiness.
/// Drawing applies conservative frustum culling; LOD and visibility-driven residency
/// remain unimplemented. Context recovery preserves logical resource identities.
pub struct RenderService<D: RenderDevice> {
    pub(super) inclusions: super::outputs::inclusion::OutputInclusions,
    pub(super) device: Rc<RefCell<D>>,
    pub(super) asset_context_active: super::assets::context::RenderAssetContext,
    pub(super) asset_export_delay: Option<RenderAssetExportDelay>,
    #[cfg(feature = "instrumentation")]
    pub(super) asset_export_staging_gate: bool,
    pub(super) recipe_scratch: Vec<(RenderShaderConfig, bool)>,
    pub(super) program_demand: Option<super::prepare::ProgramDemand>,
    pub(super) prepared_output: Option<ipp_core::OutputRef>,
    // Rebuilt from this World's demand before submission. Keys only: payload and
    // device borrows still end before resource invalidation or World mutation.
    pub(super) program_lookup: Vec<Option<AssetKey>>,
    pub(super) custom_materials: BTreeMap<
        super::outputs::scene::RenderEntity,
        super::materials::custom_material::PreparedCustomMaterial,
    >,
    /// Retained custom-material fallbacks, so each changed reason is logged once.
    pub(super) custom_fallbacks:
        BTreeMap<super::outputs::scene::RenderEntity, CustomMaterialFallback>,
    pub(super) uploads: super::statistics::RenderUploadCounter,
    /// Statistics of the last completed render.
    pub(super) statistics: super::statistics::RenderStatistics,
    #[cfg(feature = "instrumentation")]
    pub(super) gpu_capture: super::gpu_profiling::RenderGpuCapture,
    pub(super) frame_scratch: RenderFrameScratch,
    pub(super) light_selections:
        BTreeMap<ipp_core::OutputRef, super::lighting::selection::LightSelectionState>,
    pub(super) shadow_capacity_limit: usize,
    pub(super) particle_quad: Option<GlMeshData<D>>,
    pub(super) shadow_map: Option<D::ShadowMap>,
    pub(super) shadow_map_size: u32,
    pub(super) camera_targets: BTreeMap<ipp_core::OutputRef, CameraTarget<D::SurfaceCacheTarget>>,
    pub(super) camera_completed: std::collections::BTreeSet<ipp_core::OutputRef>,
    pub(super) debug: crate::services::render::frame::debug_geometry::DebugGeometryRenderCache<D>,
    pub(super) generated_paths: super::assets::generated_paths::GeneratedPathCache<D>,
    pub(super) generated_meshes: super::assets::generated_meshes::GeneratedMeshCache<D>,
    pub(super) plot_label_layouts:
        BTreeMap<ipp_core::OutputRef, super::plot::label_layout::PlotLabelLayoutState>,
    pub(super) plot_plane_caches:
        BTreeMap<super::plot::planes::PlotPlaneKey, super::plot::planes::PlotPlaneCache<D>>,
    pub(super) surface_program: Option<D::Program>,
    pub(super) surface_instance_program: Option<D::Program>,
    pub(super) surface_bitmap_program: Option<D::Program>,
    pub(super) surface_cache_program: Option<D::Program>,
    pub(super) surface_image_program: Option<D::Program>,
    pub(super) required_image_demand: BTreeMap<ipp_core::OutputRef, usize>,
    pub(super) camera_used: BTreeMap<ipp_core::OutputRef, f64>,
    pub(super) projected_visible: std::collections::BTreeSet<ipp_core::OutputRef>,
    pub(super) projected_surfaces:
        BTreeMap<ipp_core::OutputRef, super::surface::projected::ProjectedSurface<D>>,
    pub(super) projected_repainting: Option<ipp_core::OutputRef>,
    /// Whole-Surface cache images shared by every World on this context.
    pub(super) surface_cache:
        super::surface::texture_cache::SurfaceTextureCache<D::SurfaceCacheTarget>,
    pub(super) canvas_caches:
        BTreeMap<ipp_core::OutputRef, super::surface::cache_refresh::CanvasCacheState>,
    pub(super) canvas_cache_frame: super::surface::cache_refresh::CanvasCacheFrame,
    /// Resources whose primitives the last Surface submission skipped because
    /// they were not resident; a cache repaint records them as incomplete.
    pub(super) surface_missing: Vec<ipp_core::services::asset_management::AssetKey>,
    /// Failed submissions attributable only to nonresident primitive resources.
    pub(super) surface_missing_draws: u32,
    /// The last Surface submission drew a text run analytically because its
    /// atlas entries were not all resident.
    pub(super) surface_analytic_text: bool,
    /// The last Surface submission had no usable retained GUI storage, so it
    /// skipped its boxes and drew its text analytically.
    pub(super) surface_gui_unretained: bool,
    /// The canvas program drawing GUI boxes, strokes and arcs with their custom
    /// paints, and the paints it holds.
    pub(super) canvas_paints: super::canvas::paint::CanvasPaintPrograms<D>,
    /// The static program drawing retained atlas glyphs.
    pub(super) gui_glyph_program: Option<D::Program>,
    /// Canvas entities whose paint draws their colour, with the reason; a changed
    /// reason is logged once.
    pub(super) canvas_paint_fallbacks: BTreeMap<
        (ipp_core::OutputRef, ipp_core::EntityId),
        super::canvas::paint::CanvasPaintFallback,
    >,
    pub(super) gui_batch_cache:
        BTreeMap<ipp_core::OutputRef, super::retained::shape_batches::GuiBatchRenderCache<D>>,
    pub(super) glyph_atlas: super::retained::glyph_atlas::GlyphAtlas<D>,
    pub(super) glyph_batch_cache:
        BTreeMap<ipp_core::OutputRef, super::retained::glyph_atlas::GlyphBatchRenderCache>,
    /// Painter-order work of the Surface being submitted.
    pub(super) surface_ops: Vec<super::canvas::draw::SurfaceOp>,
    /// Glyph misses, population queue and outcomes of the current World frame.
    pub(super) glyph_frame: super::retained::glyph_atlas::GlyphFrameWork,
    /// Per-frame population allowance, shared by every World on this context.
    pub(super) glyph_population: super::retained::glyph_atlas::GlyphPopulationBudget,
    /// Each World's last drawn Surface paint revisions and identity orders.
    pub(super) surface_paint:
        BTreeMap<ipp_core::OutputRef, super::retained::surface_paint::SurfacePaintTracker>,
    /// Each World's retained analytic glyph instance streams.
    pub(super) analytic_glyphs:
        BTreeMap<ipp_core::OutputRef, super::retained::analytic_glyphs::AnalyticGlyphCache<D>>,
}

pub(super) fn prepared_normal(item: &ipp_core::RenderItem) -> Result<&[f32; 16], RenderError> {
    if item.particle.is_some() {
        return Ok(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]);
    }
    item.normal
        .as_ref()
        .map_err(|_| RenderError::InvalidTransform)
}

pub(super) fn prepared_model(item: &ipp_core::RenderItem) -> &[f32; 16] {
    if item.particle.is_some() {
        return &[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
    }
    &item.model
}
