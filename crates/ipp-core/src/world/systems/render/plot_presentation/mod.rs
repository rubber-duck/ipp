//! Platform-independent preparation of Plot views over immutable publications.
//! RenderServices own the layout history, borrowed frames and presentation clocks.

use crate::{EntityId, WorldPublicationId, WorldRef};

pub mod label_layout;
mod label_storage;
mod math;
pub mod plane_facing;
pub mod view_placement;

/// World-qualified presentation lifetime; never a mutable ECS lookup token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RenderEntity {
    /// Exact producing World lifetime.
    pub world: WorldRef,
    /// Generational entity within the producing World.
    pub entity: EntityId,
    /// Captured presentation component lifetime.
    pub incarnation: u64,
}

/// One composed Plot plane presented by a renderer.
#[derive(Clone)]
pub struct ScenePlotPlane<'a> {
    /// Exact chart producer identity.
    pub entity: RenderEntity,
    /// Chart component lifetime and plane identity scope.
    pub target: crate::systems::canvas::CanvasTarget,
    /// Exact completed World publication supplying resources.
    pub publication: WorldPublicationId,
    /// Composed view-local plane placement.
    pub model: [f32; 16],
    /// Chart-local to Camera-domain placement.
    pub chart_model: [f32; 16],
    /// Optional paint on this plane.
    pub paint: Option<&'a crate::systems::canvas::CanvasPaintInstance>,
    /// Retained local chart geometry used by layout and axes.
    pub geometry: &'a std::sync::Arc<crate::systems::plot::PlotPreparedGeometry>,
    /// Exact plane and its prepared paint primitives.
    pub plane: &'a crate::systems::plot::PlotPublishedPlane,
}
