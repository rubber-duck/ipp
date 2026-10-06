//! World-level camera System settings and their committed transitions.

use crate::EntityId;

/// Sparse committed camera-system settings; omitted fields did not change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CameraStatePatch {
    /// The newly selected entity, present only when selection changed.
    pub active_camera: Option<EntityId>,
}

/// A camera-system transition committed at stage 0, without command correlation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraStateChange {
    /// Evaluated frame number.
    pub tick: u64,
    /// Only system settings changed by this committed transition.
    pub changes: CameraStatePatch,
}
