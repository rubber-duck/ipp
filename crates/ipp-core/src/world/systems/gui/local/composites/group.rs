//! Groups: their items, the selection they keep in their Button items'
//! `selected` fields and the active item of a group whose items do not take
//! focus.
//!
//! A group's items are the controls below its entity in tree order, down to
//! but not into a nested group or an item: a control inside an item, such as
//! a close mark on a tab, is part of that item. Scroll views and virtual lists
//! are not items, and the controls inside them can be. An item's group is
//! therefore its nearest ancestor with a `GuiGroup`, unless an item lies
//! between them. The input router moves focus among the items that take
//! focus; the GUI System holds each group's active item and keeps single
//! selection exclusive.
//!
//! Selection stays in the items' `selected` fields. In a group that selects,
//! an operation that leaves an item selected after writing its `selected`
//! field, inserting its Button or placing it in the group writes the other
//! items' `selected` false in the same operation, so the last selection
//! written wins whether a client or the runtime wrote it. Activating an item
//! writes its `selected` true. Other changes, such as a group starting to
//! select or a container of items moving into it, write no field.
//!
//! The active item is GUI System state of a group none of whose items takes
//! focus. Pointer hover over an item makes it its group's active item, and
//! routed keys move it; the pointer leaving it does not. It paints as
//! hovered, and while another item of its group is active a pointer hovering
//! an item does not light it, so the group shows one highlight. It ends when
//! the item stops being an eligible item of such a group: removal, hiding or
//! disabling it or its group, moving it out of the group or making it or
//! another item take focus.

use crate::components::schema::FieldValue;
use crate::services::gui_input::GuiInputError;
use crate::systems::SystemOperationContext;
use crate::systems::gui::local::controls::identity::{
    GuiControl, component_incarnation, eligibility, entity_control, focusable,
};
use crate::systems::gui::local::{
    GUI_GROUP_SELECT_NONE, GuiActiveItemRecord, GuiButton, GuiControlKind, GuiEntityTarget,
    GuiGroup, GuiLocalCommand,
};
use crate::systems::gui::system_state::GuiLocalState;
use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{ComponentValue, EntityId, ErrorReason, FieldWrite};

/// Offset of `GuiButton.selected`.
const SELECTED: u32 = std::mem::offset_of!(GuiButton, selected) as u32;

/// Whether a control of `kind` can be a group item.
pub(in crate::world::systems) fn item_kind(kind: GuiControlKind) -> bool {
    !matches!(
        kind,
        GuiControlKind::ScrollView | GuiControlKind::VirtualList
    )
}

/// The authored tree and component values the group rules read: the
/// committed World, or the staged state of an operation in progress.
#[derive(Clone, Copy)]
pub(in crate::world::systems) struct GuiGroupTree<'a> {
    /// World identity and committed component storage.
    pub world: &'a WorldSimulationState,
    /// Links and component lifetimes, with any staged values.
    pub state: &'a WorldEntityState,
}

impl<'a> GuiGroupTree<'a> {
    /// The committed World.
    pub fn committed(world: &'a WorldSimulationState) -> Self {
        Self {
            world,
            state: &world.state,
        }
    }

    fn has_group(self, entity: EntityId) -> bool {
        component_incarnation(self.state, entity, ComponentValue::GUI_GROUP).is_some()
    }

    fn parent(self, entity: EntityId) -> Option<EntityId> {
        self.state
            .links
            .effective(entity)
            .and_then(|link| link.parent)
    }

    /// Whether `entity` holds a control that can be an item.
    fn is_item(self, entity: EntityId) -> bool {
        entity_control(self.world, self.state, entity)
            .is_some_and(|control| item_kind(control.kind))
    }

    /// The group `entity` would be an item of: its nearest strict ancestor
    /// with a `GuiGroup`, unless an item lies between them.
    pub fn group_of(self, entity: EntityId) -> Option<EntityId> {
        let mut between = Vec::new();
        let mut current = self.parent(entity);
        while let Some(ancestor) = current {
            if self.has_group(ancestor) {
                return (!between.into_iter().any(|entity| self.is_item(entity)))
                    .then_some(ancestor);
            }
            if between.len() >= self.state.entities.len() {
                return None;
            }
            between.push(ancestor);
            current = self.parent(ancestor);
        }
        None
    }

    /// The `GuiGroup` of `group`.
    pub fn group(self, group: EntityId) -> Option<GuiGroup> {
        match self
            .state
            .input_value(&self.world.components, group, ComponentValue::GUI_GROUP)?
        {
            ComponentValue::GuiGroup(value) => Some(value),
            _ => None,
        }
    }

    /// The items of `group` in tree order.
    pub fn items(self, group: EntityId) -> Vec<GuiControl> {
        let mut items = Vec::new();
        let mut pending: Vec<_> = self.state.links.children(Some(group)).collect();
        pending.reverse();
        let mut visited = 0;
        while let Some(entity) = pending.pop() {
            visited += 1;
            if visited > self.state.entities.len() {
                break;
            }

            // A nested group's subtree holds its own items.
            if self.state.links.invalid.contains(&entity) || self.has_group(entity) {
                continue;
            }
            // An item's own controls are parts of it.
            if let Some(control) = entity_control(self.world, self.state, entity)
                .filter(|control| item_kind(control.kind))
            {
                items.push(control);
                continue;
            }
            let children: Vec<_> = self.state.links.children(Some(entity)).collect();
            pending.extend(children.into_iter().rev());
        }
        items
    }

    /// Whether the Button on `entity` is selected.
    pub fn selected(self, entity: EntityId) -> bool {
        self.state.input_field(
            &self.world.components,
            entity,
            ComponentValue::GUI_BUTTON,
            SELECTED,
        ) == Some(FieldValue::Bool(true))
    }

    /// The group that selects the Button item `control`, if one does.
    pub fn selecting_group(self, control: GuiControl) -> Option<EntityId> {
        if control.kind != GuiControlKind::Button {
            return None;
        }
        let group = self.group_of(control.target.entity)?;
        (self.group(group)?.selection != GUI_GROUP_SELECT_NONE).then_some(group)
    }

    /// The group `control` can be the active item of: its group, when none
    /// of that group's items takes focus. A control that does not take focus
    /// in a group whose items do, such as a close mark on a tab, is no
    /// active item.
    pub fn active_group(self, control: GuiControl) -> Option<EntityId> {
        if !item_kind(control.kind) || focusable(self.world, control.target.entity) {
            return None;
        }
        let group = self.group_of(control.target.entity)?;
        self.items(group)
            .iter()
            .all(|item| !focusable(self.world, item.target.entity))
            .then_some(group)
    }
}

/// Keep the selection of every group that selects exclusive after one
/// operation: each selected Button the operation wrote `selected` on,
/// inserted or placed clears its group's other items, in entity order.
/// Restoring a saved World keeps its fields as saved.
pub(in crate::world::systems::gui) fn keep_exclusive_selection(
    context: &mut SystemOperationContext<'_>,
) -> Result<(), ErrorReason> {
    if context.is_restoring() {
        return Ok(());
    }
    let staged = &context.staged.entities_state;
    let mut written: Vec<EntityId> = staged
        .operation_components
        .iter()
        .filter(|&&(entity, component)| {
            component == ComponentValue::GUI_BUTTON
                && (staged
                    .explicit_fields
                    .contains(&(entity, component, SELECTED))
                    || staged.operation_untracked.contains(&(entity, component)))
        })
        .map(|&(entity, _)| entity)
        .collect();
    written.extend(
        staged
            .links
            .operation_changed
            .iter()
            .copied()
            .filter(|&entity| {
                component_incarnation(staged, entity, ComponentValue::GUI_BUTTON).is_some()
            }),
    );
    written.sort_unstable();
    written.dedup();
    for entity in written {
        let cleared: Vec<EntityId> = {
            let tree = GuiGroupTree {
                world: context.world_data,
                state: &context.staged.entities_state,
            };
            let Some(group) = entity_control(tree.world, tree.state, entity)
                .filter(|_| tree.selected(entity))
                .and_then(|control| tree.selecting_group(control))
            else {
                continue;
            };
            tree.items(group)
                .into_iter()
                .map(|item| item.target.entity)
                .filter(|&item| item != entity && tree.selected(item))
                .collect()
        };
        for item in cleared {
            context.staged.write_component_field(
                &context.world_data.components,
                item,
                ComponentValue::GUI_BUTTON,
                &FieldWrite {
                    offset: SELECTED,
                    value: crate::FieldValue::Bool(false),
                },
            )?;
        }
    }
    Ok(())
}

/// The write that selects the Button item `control` of a group that selects,
/// when it is not selected yet.
pub(in crate::world::systems::gui) fn selection_write(
    tree: GuiGroupTree<'_>,
    control: GuiControl,
) -> Option<FieldWrite> {
    tree.selecting_group(control)?;
    (!tree.selected(control.target.entity)).then_some(FieldWrite {
        offset: SELECTED,
        value: crate::FieldValue::Bool(true),
    })
}

impl GuiLocalState {
    /// The active item of `group`.
    pub(in crate::world::systems::gui) fn active_item(
        &self,
        group: EntityId,
    ) -> Option<GuiEntityTarget> {
        self.active
            .iter()
            .find(|(entry, _)| *entry == group)
            .map(|(_, item)| *item)
    }

    /// Whether `target` is the active item of its group.
    pub(in crate::world::systems) fn is_active(&self, target: GuiEntityTarget) -> bool {
        self.active.iter().any(|(_, item)| *item == target)
    }

    /// Active items by group entity, as the `GuiActiveItems` System query
    /// reports them.
    pub(in crate::world::systems::gui) fn active_records(&self) -> Vec<GuiActiveItemRecord> {
        let mut records: Vec<_> = self
            .active
            .iter()
            .map(|&(group, target)| GuiActiveItemRecord {
                group,
                target,
            })
            .collect();
        records.sort_by_key(|record| record.group.to_bits());
        records
    }

    /// The entities of the current active items, whose hover paint follows
    /// them.
    pub(in crate::world::systems::gui) fn active_entities(&self) -> Vec<EntityId> {
        self.active.iter().map(|(_, item)| item.entity).collect()
    }

    /// Reserve room to record one more active item.
    pub(in crate::world::systems::gui::local) fn reserve_active(
        &mut self,
    ) -> Result<(), GuiInputError> {
        self.active
            .try_reserve(1)
            .map_err(|_| GuiInputError::Capacity)
    }

    /// Make `target`, an item of `group` that does not take focus, the
    /// group's active item; returns whether that changed it. The caller
    /// advances the presentation revision.
    pub(in crate::world::systems::gui::local) fn set_active(
        &mut self,
        group: EntityId,
        target: GuiEntityTarget,
    ) -> bool {
        if self.active_item(group) == Some(target) {
            return false;
        }
        self.active.retain(|(entry, _)| *entry != group);
        self.active.push((group, target));
        true
    }

    /// Apply a routed `ActiveItem` operation on `control`, an item of `group`
    /// that does not take focus. Keys move the active item, so a new one
    /// scrolls into view like a newly focused control; pointer hover moves it
    /// without scrolling.
    pub(in crate::world::systems::gui::local) fn make_active(
        &mut self,
        command: &GuiLocalCommand,
        control: GuiControl,
        group: EntityId,
    ) -> Result<(), GuiInputError> {
        let revision = self
            .presentation_revision
            .checked_add(1)
            .ok_or(GuiInputError::Capacity)?;
        self.reserve_active()?;
        command.input.prepare_write(false)?.commit(|| {
            if self.set_active(group, control.target) {
                self.presentation_revision = revision;
                self.reveal = Some(control.target.entity);
            }
        })
    }

    /// End each active item that is no longer an eligible item of its group
    /// that does not take focus.
    pub(in crate::world::systems::gui) fn revalidate_active(
        &mut self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
    ) {
        if self.active.is_empty() {
            return;
        }
        let tree = GuiGroupTree {
            world,
            state,
        };
        let previous = self.active.len();
        self.active.retain(|&(group, target)| {
            entity_control(world, state, target.entity).is_some_and(|control| {
                control.target == target
                    && eligibility(world, target.entity).eligible()
                    && Self::interaction_policy(world, state, target.entity)
                    && tree.active_group(control) == Some(group)
            })
        });
        if self.active.len() != previous {
            self.presentation_changed(false);
        }
    }
}
