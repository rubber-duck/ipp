//! State owned directly by one world system instance.

/// World-local diagnostics; each Skin component owns its evaluated palette.
#[derive(Default)]
pub struct SkinningSystemState {
    pub(super) components: crate::world::component_query::ComponentQuery<crate::components::Skin>,
    pub(super) palette_scratch: Vec<[f32; 16]>,
    pub(in crate::world) diagnostics: Vec<crate::RenderDiagnostic>,
}
