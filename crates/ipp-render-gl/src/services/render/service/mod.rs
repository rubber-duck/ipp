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
    /// Analytic Surface glyph instance streams count on every draw that uploads them.
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
    /// Batches submitted for GUI primitives.
    #[cfg(feature = "gui")]
    pub gui_batches: u32,
    /// GUI box primitives whose CPU geometry was regenerated, plus glyph batches
    /// rebuilt after a text edit or the retirement of an atlas page they sampled.
    #[cfg(feature = "gui")]
    pub gui_rebuilds: u32,
    /// Number of GPU batch buffers allocated or replaced during this frame.
    #[cfg(feature = "gui")]
    pub gui_allocations: u32,
    /// Resident bytes of retained GUI box and glyph batch GPU buffers across every
    /// World presented through this context.
    #[cfg(feature = "gui")]
    pub gui_resident_bytes: u32,
    /// Distinct glyph atlas entries that visible text demanded but did not find
    /// during this submission, including entries the population budget or a failure
    /// back-off defers to a later frame.
    #[cfg(feature = "gui")]
    pub glyph_misses: u32,
    /// Glyph atlas entries rasterized during this submission, before the main pass.
    /// A frame populates at least [`crate::glyph_atlas::MIN_POPULATES_PER_FRAME`]
    /// missing entries, then as many as the population time budget covers, up to
    /// [`crate::glyph_atlas::MAX_POPULATES_PER_FRAME`]; later entries count as misses
    /// and their text stays analytic until a following frame populates them.
    #[cfg(feature = "gui")]
    pub glyph_populates: u32,
    /// Recoverable glyph atlas allocation or rasterization failures during this
    /// submission. Each glyph backs off and its text uses analytic glyphs meanwhile.
    #[cfg(feature = "gui")]
    pub glyph_population_failures: u32,
    /// Glyph atlas pages retired since the previous completed submission: idle past
    /// the configured limit, reclaimed under allocation pressure or released when no
    /// World demands any glyph. Context loss is not counted.
    #[cfg(feature = "gui")]
    pub glyph_page_retirements: u32,
    /// Number of resident glyph atlas pages shared by every World on this context.
    #[cfg(feature = "gui")]
    pub glyph_pages: u32,
    /// Total resident bytes occupied by the shared glyph atlas page textures: one
    /// byte per texel of single-channel coverage.
    #[cfg(feature = "gui")]
    pub glyph_resident_bytes: usize,
    /// Opted-in Surfaces repainted into their cache images during this submission,
    /// before the main pass. Each repaint also counts its primitive draws.
    #[cfg(feature = "surfaces")]
    pub surface_cache_repaints: u32,
    /// Opted-in Surfaces composited from an unchanged cache image, without repainting.
    /// A composite is one draw call of two triangles.
    #[cfg(feature = "surfaces")]
    pub surface_cache_reuses: u32,
    /// Opted-in visible Surfaces presented directly: inside their direct distance,
    /// under GUI interaction, after a fallback or without cache support. Culled
    /// Surfaces count in none of the cache counters.
    #[cfg(feature = "surfaces")]
    pub surface_cache_direct: u32,
    /// Opted-in Surfaces presented directly because the byte budget, a zero budget,
    /// or a recoverable allocation, repaint or composite failure and its retry
    /// interval left no usable image. Also counted in `surface_cache_direct`.
    #[cfg(feature = "surfaces")]
    pub surface_cache_fallbacks: u32,
    /// Cache images created or resized during this submission; each is repainted
    /// before it is shown.
    #[cfg(feature = "surfaces")]
    pub surface_cache_allocations: u32,
    /// Resident cache images across every World presented through this context,
    /// after this submission's releases.
    #[cfg(feature = "surfaces")]
    pub surface_cache_entries: u32,
    /// Resident bytes of cache images across every World presented through this
    /// context, four per texel.
    #[cfg(feature = "surfaces")]
    pub surface_cache_resident_bytes: u32,
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
    #[cfg(feature = "gui")]
    surface_box_program: Option<D::Program>,
    #[cfg(feature = "gui")]
    gui_batch_cache: BTreeMap<ipp_core::WorldId, super::gui_batch::GuiBatchRenderCache<D>>,
    #[cfg(feature = "gui")]
    surface_text_program: Option<D::Program>,
    #[cfg(feature = "gui")]
    pub(super) glyph_atlas: super::glyph_atlas::GlyphAtlas<D>,
    #[cfg(feature = "gui")]
    glyph_batch_cache: BTreeMap<ipp_core::WorldId, super::glyph_atlas::GlyphBatchRenderCache<D>>,
    /// Glyph misses, population queue and outcomes of the current World frame.
    #[cfg(feature = "gui")]
    glyph_frame: super::glyph_atlas::GlyphFrameWork,
    /// Per-frame population allowance, shared by every World on this context.
    #[cfg(feature = "gui")]
    glyph_population: super::glyph_atlas::GlyphPopulationBudget,
    /// Surfaces the current frame submitted; `None` until submission reaches them.
    #[cfg(feature = "gui")]
    submitted_surfaces: Option<std::collections::BTreeSet<ipp_core::EntityId>>,
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
