//! State owned directly by one world system instance.

use crate::world::systems::asset_dependencies::AssetSourceKeyCache;

/// Prepared render inputs, diagnostics and world-specific render settings.
#[derive(Default)]
pub struct RenderSystemState {
    pub(super) entries: Vec<super::preparation::RenderEntry>,
    pub(super) debug_entries: Vec<super::preparation::DebugGeometryEntry>,
    pub(super) light_entries: Vec<super::preparation::LightEntry>,
    pub(super) entries_ready: bool,
    pub(super) compatibility_diagnostics: Vec<super::RenderDiagnostic>,
    pub(in crate::world) mesh_keys: AssetSourceKeyCache,
    #[cfg(feature = "mesh-poses")]
    pub(in crate::world) pose_mesh_keys: AssetSourceKeyCache,
    pub(in crate::world) texture_keys: AssetSourceKeyCache,
    pub(in crate::world) items: Vec<super::RenderItem>,
    pub(in crate::world) debug_items: Vec<super::DebugRenderItem>,
    #[cfg(feature = "surfaces")]
    pub(in crate::world) surface_items: Vec<crate::SurfaceRenderItem>,
    #[cfg(feature = "surfaces")]
    pub(in crate::world) surface_layout_cache:
        crate::systems::surface::rendering::SurfaceLayoutCache,
    /// Revisions and interaction priority published with `surface_items`.
    #[cfg(feature = "surfaces")]
    pub(in crate::world) surface_cache_inputs: super::surface_cache_inputs::SurfaceCacheInputs,
    pub(in crate::world) diagnostics: Vec<super::RenderDiagnostic>,
    pub(in crate::world) render_state: crate::RenderState,
    pub(in crate::world) state_changes: Vec<crate::RenderStatePatch>,
}
