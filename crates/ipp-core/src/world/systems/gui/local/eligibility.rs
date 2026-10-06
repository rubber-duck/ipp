//! Maintenance of the evaluated eligibility fields on `GuiBehavior`.
//!
//! `effective_enabled` and `effective_visible` combine an entity's own policy
//! with all its ancestors up to the top level of the World; an entity
//! without `GuiBehavior` contributes the defaults. `available` holds the
//! entity's structural validity and, for a control, that it has exactly one
//! control component. The GUI System refreshes the subtrees whose inputs
//! changed, in tree order, writes only fields whose values differ and reports
//! the entities whose fields it wrote. An invalid (cyclic) link is refreshed
//! like any other: each entity is visited once, so a cycle ends the walk
//! instead of repeating it.

use super::controls::identity::control_components;
use crate::EntityId;
use crate::components::registry::ComponentStorage;
use crate::world::WorldEntityState;
use std::collections::BTreeSet;

/// Inherited `(enabled, visible)` passed from a parent to its children.
type Inherited = (bool, bool);

/// Refresh the eligibility of every subtree rooted at one of `roots`, adding
/// each entity whose eligibility fields changed to `changed`.
pub(in crate::world::systems::gui) fn refresh_eligibility(
    components: &mut ComponentStorage,
    state: &WorldEntityState,
    roots: &BTreeSet<EntityId>,
    changed: &mut BTreeSet<EntityId>,
) {
    let mut visited = BTreeSet::new();
    for &top in roots {
        if !state.entities.contains_key(&top) || has_ancestor_in(state, top, roots) {
            continue;
        }
        let mut pending = vec![(top, inherited(components, state, top))];
        while let Some((entity, (enabled, visible))) = pending.pop() {
            if !visited.insert(entity) {
                continue;
            }
            let scope = (enabled, visible);
            let passed = match components.gui_behavior_mut(entity.index() as usize) {
                Some(behavior) => {
                    let effective = (behavior.enabled && scope.0, behavior.visible && scope.1);
                    let available = !state.links.invalid.contains(&entity)
                        && control_components(state, entity) <= 1;
                    if behavior.effective_enabled != effective.0 {
                        behavior.effective_enabled = effective.0;
                        changed.insert(entity);
                    }
                    if behavior.effective_visible != effective.1 {
                        behavior.effective_visible = effective.1;
                        changed.insert(entity);
                    }
                    if behavior.available != available {
                        behavior.available = available;
                        changed.insert(entity);
                    }
                    effective
                }
                None => scope,
            };
            pending.extend(
                state
                    .links
                    .children(Some(entity))
                    .map(|child| (child, passed)),
            );
        }
    }
}

/// Whether a strict ancestor of `entity` is also a refreshed root, so the
/// ancestor's walk reaches it. An entity on a cycle refreshes as its own
/// root: the ancestors it reaches are its own descendants.
fn has_ancestor_in(state: &WorldEntityState, entity: EntityId, roots: &BTreeSet<EntityId>) -> bool {
    let cyclic = state.links.invalid.contains(&entity);
    let mut found = false;
    let mut current = parent(state, entity);
    let mut steps = 0;
    while let Some(ancestor) = current {
        if ancestor == entity {
            return false;
        }
        if roots.contains(&ancestor) {
            if !cyclic {
                return true;
            }
            found = true;
        }
        steps += 1;
        if steps > state.entities.len() {
            break;
        }
        current = parent(state, ancestor);
    }
    found
}

/// Eligibility a subtree root inherits from its nearest evaluated ancestor.
fn inherited(
    components: &ComponentStorage,
    state: &WorldEntityState,
    entity: EntityId,
) -> Inherited {
    let mut current = parent(state, entity);
    let mut steps = 0;
    while let Some(ancestor) = current {
        if let Some(behavior) = components.gui_behavior(ancestor.index() as usize) {
            return (behavior.effective_enabled, behavior.effective_visible);
        }
        steps += 1;
        if steps > state.entities.len() {
            break;
        }
        current = parent(state, ancestor);
    }
    (true, true)
}

fn parent(state: &WorldEntityState, entity: EntityId) -> Option<EntityId> {
    state.links.effective(entity).and_then(|link| link.parent)
}
