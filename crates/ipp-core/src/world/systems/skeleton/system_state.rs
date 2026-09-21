//! State owned directly by one world system instance.

/// Per-world observations; each Skeleton component owns its evaluated pose buffers.
#[derive(Default)]
pub struct SkeletonSystemState {
    pub(super) components:
        crate::world::component_query::ComponentQuery<crate::components::Skeleton>,
    pub(super) local_scratch: Vec<crate::components::Transform>,
    pub(in crate::world) skeleton_diagnostics: Vec<crate::RenderDiagnostic>,
}
