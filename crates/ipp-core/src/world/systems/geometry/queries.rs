//! Correlated CPU geometry observations of the final effective frame.

use crate::{EntityId, ErrorReason};

/// A normalized top-left viewport position, independent of host time and resize.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryPickQuery {
    /// Horizontal position in 0..=1.
    pub x: f32,
    /// Vertical position in 0..=1.
    pub y: f32,
    /// Supplied viewport width in physical pixels.
    pub width: u32,
    /// Supplied viewport height in physical pixels.
    pub height: u32,
    /// Include the camera-facing world plane through a successful hit.
    pub include_view_plane: bool,
}

/// A world-space plane represented by a point and a nonzero normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldPlane {
    /// World-space point on the plane.
    pub point: [f32; 3],
    /// GeometryPlane normal; geometry picks return a unit camera-forward vector.
    pub normal: [f32; 3],
}

/// Project a viewport ray onto a world-space plane without changing scene state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraProjectQuery {
    /// Horizontal normalized viewport coordinate; may extend outside 0..=1.
    pub x: f32,
    /// Vertical normalized viewport coordinate, increasing downwards.
    pub y: f32,
    /// Host viewport width in physical pixels.
    pub width: u32,
    /// Host viewport height in physical pixels.
    pub height: u32,
    /// Finite world-space plane; the normal must be nonzero.
    pub plane: WorldPlane,
}

/// A correlated projection observation of the final effective camera.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraProjectOutcome {
    /// Original query identity.
    pub request_id: u64,
    /// Evaluated frame number.
    pub tick: u64,
    /// Camera used, including when projection fails.
    pub camera: Option<EntityId>,
    /// Intersection, no unique forward intersection, or explicit failure.
    pub result: Result<Option<[f32; 3]>, ErrorReason>,
}

/// Nearest intersection measured in world space from the camera-plane ray origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryPickHit {
    /// Explicit interaction target.
    pub entity: EntityId,
    /// World-space point.
    pub position: [f32; 3],
    /// World-space distance, including the target's nonuniform scale.
    pub distance: f32,
    /// Stable zero-based primitive index in the picking geometry definition.
    pub part: u32,
    /// Present only when requested by the query.
    pub view_plane: Option<WorldPlane>,
}

/// A terminal response evaluated using the final camera selected for this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct GeometryPickOutcome {
    /// Original request identity.
    pub request_id: u64,
    /// Evaluated frame number.
    pub tick: u64,
    /// Camera used, including when geometry evaluation fails.
    pub camera: Option<EntityId>,
    /// Hit, successful miss, or explicit failure.
    pub result: Result<Option<GeometryPickHit>, ErrorReason>,
}
