//! State owned directly by one world system instance.

/// Active camera selection and the Host surface dimensions used for queries.
#[derive(Default)]
pub struct CameraSystemState {
    pub(in crate::world) state_changes: Vec<super::CameraStatePatch>,
    pub(in crate::world) pending_queries: Vec<crate::systems::geometry::GeometryQueryCommand>,
    pub(in crate::world) active_camera: Option<crate::EntityId>,
    pub(in crate::world) render_viewport: Option<(u32, u32)>,
}
