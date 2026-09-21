//! Shared right-handed camera and transform mathematics for CPU and GL consumers.

mod component;
pub use component::Camera;

mod update;

use crate::{EntityId, ErrorReason, components::Transform, components::schema::ComponentLifecycle};

mod navigation;

pub(crate) use navigation::navigate;

/// Camera-local navigation applied to the active camera's producer components.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CameraMotion {
    /// Orbit about the implied focus pivot, yaw then pitch in radians.
    Rotate {
        /// Rotation about camera-local up.
        yaw: f32,
        /// Rotation about camera-local right after yaw.
        pitch: f32,
    },
    /// Move the scene with a normalized viewport right/down drag.
    Pan {
        /// Horizontal viewport delta, positive right.
        x: f32,
        /// Vertical viewport delta, positive down.
        y: f32,
        /// Nonzero viewport width in pixels.
        width: u32,
        /// Nonzero viewport height in pixels.
        height: u32,
    },
    /// Change perspective focus distance or orthographic extent exponentially.
    Zoom {
        /// Natural logarithmic delta; positive values zoom out.
        amount: f32,
    },
}

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

/// Final effective camera prepared for a host-owned viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreparedCamera {
    /// Explicitly selected camera entity.
    pub entity: EntityId,
    /// Column-major projection * inverse(camera TRS), using GL clip space.
    pub view_projection: [f32; 16],
}

#[cfg(test)]
mod camera_math_tests;

mod system_state;
pub use system_state::CameraSystemState;

mod system;
pub use system::{CameraSystem, CameraSystemFactory};

pub(in crate::world) use update::CameraReadAccess;

mod camera_math;
pub(crate) use camera_math::CameraAffineTransform;
#[cfg(feature = "skeletal-animation")]
pub use camera_math::inverse_model_matrix;
pub use camera_math::{model_matrix, multiply, prepare, prepare_affine};

#[cfg(feature = "skeletal-animation")]
pub(crate) use camera_math::invertible;
