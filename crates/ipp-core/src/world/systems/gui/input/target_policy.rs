//! Shared full-fence liveness and evaluated-eligibility policy for GUI input.

use super::super::{GuiLayoutSystem, GuiRoot};
use super::system::GuiInputTarget;
use crate::world::WorldSimulationState;
use crate::{ComponentValue, EntityId};

/// Current status of one fully fenced input target.
pub(super) enum GuiTargetStatus {
    /// Producer identity and evaluated eligibility all match.
    Eligible(GuiRoot),
    /// Entity, root incarnation, node, or node lifetime no longer matches.
    Removed,
    /// The target identity survives but is disabled, hidden, or unavailable.
    Ineligible,
}

/// Borrowed target classification used when a caller does not need to retain
/// an owned producer root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GuiTargetValidity {
    Eligible,
    Removed,
    Ineligible,
}

/// Cloned producer root with its component incarnation, if live.
pub(super) fn producer_root(
    sim: &WorldSimulationState,
    entity: EntityId,
) -> Option<(u64, GuiRoot)> {
    let incarnation = sim
        .state
        .entities
        .get(&entity)?
        .input(ComponentValue::GUI_ROOT)?
        .incarnation;
    let ComponentValue::GuiRoot(root) =
        sim.state
            .producer_value(&sim.components, entity, ComponentValue::GUI_ROOT)?
    else {
        return None;
    };
    Some((incarnation, root))
}

/// Validate producer identity and authored eligibility without retained layout.
///
/// Lifecycle callbacks use this synchronous subset before the next evaluation.
pub(super) fn producer_status(
    sim: &WorldSimulationState,
    target: &GuiInputTarget,
) -> GuiTargetStatus {
    let Some((incarnation, root)) = producer_root(sim, target.entity) else {
        return GuiTargetStatus::Removed;
    };
    status_in_root(root, incarnation, target)
}

/// Validate a target against an owned root observed at a lifecycle boundary.
pub(super) fn status_in_root(
    root: GuiRoot,
    incarnation: u64,
    target: &GuiInputTarget,
) -> GuiTargetStatus {
    match producer_validity(&root, incarnation, target) {
        GuiTargetValidity::Eligible => GuiTargetStatus::Eligible(root),
        GuiTargetValidity::Removed => GuiTargetStatus::Removed,
        GuiTargetValidity::Ineligible => GuiTargetStatus::Ineligible,
    }
}

/// Validate producer identity and authored eligibility against one borrowed
/// panel root. Batch callers can reuse the same restored producer snapshot for
/// every retained target on that panel.
pub(super) fn producer_validity(
    root: &GuiRoot,
    incarnation: u64,
    target: &GuiInputTarget,
) -> GuiTargetValidity {
    if incarnation != target.root_incarnation {
        return GuiTargetValidity::Removed;
    }
    let Some(node) = root.nodes().node(target.node) else {
        return GuiTargetValidity::Removed;
    };
    if node.lifetime != target.lifetime {
        return GuiTargetValidity::Removed;
    }
    let Some(style) = root.style(target.node) else {
        return GuiTargetValidity::Ineligible;
    };
    if !style.enabled || style.opacity <= 0.0 {
        return GuiTargetValidity::Ineligible;
    }
    GuiTargetValidity::Eligible
}

/// Validate one target against producer identity and the matching evaluated view.
///
/// Missing views and records fail closed: unavailable measurement never leaves a
/// stale input target eligible.
pub(super) fn evaluated_status(
    sim: &WorldSimulationState,
    layout: &GuiLayoutSystem,
    target: &GuiInputTarget,
) -> GuiTargetStatus {
    let Some((incarnation, root)) = producer_root(sim, target.entity) else {
        return GuiTargetStatus::Removed;
    };
    match evaluated_validity(&root, incarnation, layout, target) {
        GuiTargetValidity::Eligible => GuiTargetStatus::Eligible(root),
        GuiTargetValidity::Removed => GuiTargetStatus::Removed,
        GuiTargetValidity::Ineligible => GuiTargetStatus::Ineligible,
    }
}

/// Validate one target against a borrowed producer root and retained layout.
/// Missing views and records fail closed.
pub(super) fn evaluated_validity(
    root: &GuiRoot,
    incarnation: u64,
    layout: &GuiLayoutSystem,
    target: &GuiInputTarget,
) -> GuiTargetValidity {
    match producer_validity(root, incarnation, target) {
        GuiTargetValidity::Eligible => {}
        status => return status,
    }
    let Some(view) = layout.view(target.entity) else {
        return GuiTargetValidity::Ineligible;
    };
    if view.root_incarnation != target.root_incarnation {
        return GuiTargetValidity::Removed;
    }
    if !view.available {
        return GuiTargetValidity::Ineligible;
    }
    let Some(node) = view
        .nodes
        .iter()
        .find(|node| node.node == target.node && node.lifetime == target.lifetime)
    else {
        return GuiTargetValidity::Ineligible;
    };
    if !node.available || !node.visible || !node.enabled {
        return GuiTargetValidity::Ineligible;
    }
    GuiTargetValidity::Eligible
}

/// Resolve the current evaluated identity for public entity/node observations.
pub(super) fn current_target(
    sim: &WorldSimulationState,
    layout: &GuiLayoutSystem,
    entity: EntityId,
    node: super::super::GuiNodeId,
) -> Option<GuiInputTarget> {
    let view = layout.view(entity)?;
    let record = view.nodes.iter().find(|record| record.node == node)?;
    let target = GuiInputTarget {
        entity,
        root_incarnation: view.root_incarnation,
        node,
        lifetime: record.lifetime,
    };
    matches!(
        evaluated_status(sim, layout, &target),
        GuiTargetStatus::Eligible(_)
    )
    .then_some(target)
}
