//! Control identity, eligibility, focus parts and ancestry read from the
//! component store.
//!
//! A control is an entity with a control component. Its identity is the lowest
//! control component present and that component's incarnation; its eligibility
//! is the evaluated `GuiBehavior` fields; its focus parts follow its fields.
//! Nothing here keeps a copy of any of them.

use super::component::CONTROL_COMPONENTS;
use super::{GuiControlKind, GuiEntityTarget};
use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{EntityId, WorldRef};
use std::sync::Arc;

/// A control's exact target and role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::world::systems) struct GuiControl {
    /// Exact control lifetime.
    pub target: GuiEntityTarget,
    /// Role of the control component.
    pub kind: GuiControlKind,
}

/// Evaluated inherited eligibility stored on a control's `GuiBehavior`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::world::systems) struct GuiEligibility {
    /// The control and all its ancestors in the World are enabled.
    pub enabled: bool,
    /// The control and all its ancestors in the World are visible.
    pub visible: bool,
    /// The control is structurally valid and unambiguous.
    pub available: bool,
}

impl GuiEligibility {
    /// Whether actions and input may apply.
    pub fn eligible(self) -> bool {
        self.enabled && self.visible && self.available
    }
}

/// Current incarnation of a present component.
pub(in crate::world::systems) fn component_incarnation(
    state: &WorldEntityState,
    entity: EntityId,
    component: u16,
) -> Option<u64> {
    state
        .entities
        .get(&entity)?
        .input(component)
        .map(|input| input.incarnation)
}

/// Number of control components present on an entity.
pub(super) fn control_components(state: &WorldEntityState, entity: EntityId) -> usize {
    CONTROL_COMPONENTS
        .iter()
        .filter(|component| component_incarnation(state, entity, **component).is_some())
        .count()
}

/// The entity's control: its lowest present control component.
pub(in crate::world::systems) fn entity_control(
    world: &WorldSimulationState,
    state: &WorldEntityState,
    entity: EntityId,
) -> Option<GuiControl> {
    let (component, incarnation) = CONTROL_COMPONENTS.iter().find_map(|&component| {
        component_incarnation(state, entity, component).map(|incarnation| (component, incarnation))
    })?;
    Some(GuiControl {
        target: GuiEntityTarget {
            world: WorldRef {
                id: world.id,
                incarnation: world.identity,
            },
            entity,
            component,
            incarnation,
        },
        kind: GuiControlKind::of_component(component)?,
    })
}

/// Evaluated eligibility of an entity; an entity without `GuiBehavior` has the defaults.
pub(in crate::world::systems) fn eligibility(
    world: &WorldSimulationState,
    entity: EntityId,
) -> GuiEligibility {
    world
        .components
        .gui_behavior(entity.index() as usize)
        .map_or(
            GuiEligibility {
                enabled: true,
                visible: true,
                available: true,
            },
            |behavior| GuiEligibility {
                enabled: behavior.effective_enabled,
                visible: behavior.effective_visible,
                available: behavior.available,
            },
        )
}

/// Whether an entity's control takes focus: its own `GuiBehavior.focusable`,
/// true without one.
pub(in crate::world::systems) fn focusable(world: &WorldSimulationState, entity: EntityId) -> bool {
    world
        .components
        .gui_behavior(entity.index() as usize)
        .is_none_or(|behavior| behavior.focusable)
}

/// The number of focus parts of a control: the stops within it that its focus
/// names, each a Tab stop with its own focus ring, pointer feedback and arrow
/// keys. A range slider has two thumbs, and a colour control its field, its
/// hue rail and its alpha rail when shown; every other control is one part,
/// part 0, and behaves as a control without parts.
pub(in crate::world::systems) fn focus_parts(
    world: &WorldSimulationState,
    control: GuiControl,
) -> u32 {
    let index = control.target.entity.index() as usize;
    match control.kind {
        GuiControlKind::Slider => world
            .components
            .gui_slider(index)
            .map_or(1, super::GuiSlider::thumbs),
        GuiControlKind::Color => world
            .components
            .gui_color(index)
            .map_or(1, super::GuiColor::parts),
        _ => 1,
    }
}

/// Root-first core ancestry, including the entity itself.
pub(in crate::world::systems) fn ancestry(
    state: &WorldEntityState,
    entity: EntityId,
) -> Arc<[EntityId]> {
    let mut ancestry = Vec::new();
    let mut current = Some(entity);
    while let Some(ancestor) = current {
        if ancestry.len() > state.entities.len() {
            break;
        }
        ancestry.push(ancestor);
        current = state.links.effective(ancestor).and_then(|link| link.parent);
    }
    ancestry.reverse();
    ancestry.into()
}
