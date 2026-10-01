//! Stateless completed-publication queries, not action or presentation authority.
//! Unknown Spatial footprints conservatively return Unavailable rather than clicking through.

mod camera;
mod canvas;
mod projection;
mod traversal;

pub use projection::{GuiProjectedPoint, project_composed_point};
pub(crate) use traversal::GuiQueryWorlds;
pub use traversal::query_composed_input;

use crate::systems::canvas::CanvasHit;
use crate::systems::gui::presentation::GuiControlObservation;
use crate::{
    EntityId, ErrorReason, OutputRef, ViewDescriptor, WorldAttachmentToken, WorldPublicationId,
    WorldRef,
};

/// Explicit interaction blocker identity; rendered meshes are not implicit blockers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiPickingBlocker {
    /// Exact producing World.
    pub world: WorldRef,
    /// Generational geometry owner.
    pub entity: EntityId,
    /// Exact PickingGeometry lifetime.
    pub incarnation: u64,
}

/// Input context options shared by all nested Camera and Canvas domains.
///
/// There is no caller work budget. The Host keeps the attachment graph a tree,
/// so one query enters each World at most once, and its work is bounded by
/// what the point can reach: the entered publications and their attachment
/// edges, the Canvas hits of entered Canvases and these explicit blockers,
/// looked up by entity rather than by scanning published geometry.
#[derive(Clone, Copy, Debug, Default)]
pub struct GuiQueryOptions<'a> {
    /// Only these published picking shapes occlude panel input.
    pub blockers: &'a [GuiPickingBlocker],
}

/// One exact completed edge; distances never cross a nested Camera domain boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiQueryStep {
    /// Exact attachment write identity, including its World lifetimes.
    pub token: WorldAttachmentToken,
    /// Parent publication containing this edge.
    pub publication: WorldPublicationId,
    /// Surface intersection distance in its containing Camera; Canvas/Spatial steps have none.
    pub distance: Option<f64>,
}

/// A borrowed immutable observation ready for the separate input service's admission checks.
pub struct GuiQueryHit<'a> {
    /// Canvas output containing this hit.
    pub output: OutputRef,
    /// Exact completed Canvas source.
    pub publication: WorldPublicationId,
    /// Point in the published Canvas logical coordinate domain.
    pub point: [f32; 2],
    /// Finalized local geometry, including scroll-part identity.
    pub hit: &'a CanvasHit,
    /// Exact GUI observation, when this hit represents an ordinary control.
    pub control: Option<&'a GuiControlObservation>,
    /// Ordered root-to-target path, suitable for later proof validation, not itself a proof.
    pub path: Vec<GuiQueryStep>,
}

/// Why a known footprint cannot deliver input. It must not expose a target behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiQueryBlockReason {
    /// Explicit PickingGeometry blocker.
    PickingGeometry,
    /// Known Surface or Canvas footprint with unavailable output/resource/control state.
    Unavailable,
    /// Panel content without a hit: a Camera ray met a presented Surface, but
    /// nothing on it or behind it took the point. A panel is opaque to the
    /// scene; the path names the nearest panel met.
    Panel,
}

/// Conservative failures for which no safe complete footprint is available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiQueryUnavailable {
    /// The completed composition reached one World twice. The Host rejects
    /// attachment cycles and competing parents, so this is an inconsistent
    /// composition, never a miss and never a work budget.
    RepeatedWorld,
    /// An unavailable spatial contribution might occlude the query.
    SpatialBranch,
    /// Required completed data or geometry is invalid or unavailable.
    Data(ErrorReason),
}

/// Geometry outcome only; even a Hit still requires current routed admission.
pub enum GuiQueryOutcome<'a> {
    /// Frontmost eligible immutable local hit.
    Hit(GuiQueryHit<'a>),
    /// Known occluding footprint; no fall-through or replay.
    Blocked {
        /// Blocking policy.
        reason: GuiQueryBlockReason,
        /// Path to the blocking footprint.
        path: Vec<GuiQueryStep>,
    },
    /// Complete readable geometry contains no hit, explicit blocker or panel.
    Miss,
    /// Cannot safely establish a miss or target.
    Unavailable(GuiQueryUnavailable),
}

/// Exact CPU source view, independent of physical presentation/context completion.
pub struct GuiQueryResult<'a> {
    /// Resolved root or explicitly requested retained headless view.
    pub view: ViewDescriptor,
    /// Immutable query observation.
    pub outcome: GuiQueryOutcome<'a>,
}
