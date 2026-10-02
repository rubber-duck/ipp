//! Keyboard routing of groups.
//!
//! A group's items are the controls its completed observations name it the
//! group of. A group whose items take focus is one Tab stop: traversal keeps
//! the focused item while focus is inside the group, else the group's
//! selected item, else its first. Arrow keys along the group's axis, Home and
//! End on a focused item move focus among the eligible items that take focus
//! and stop at the ends; in a group whose selection follows, the item focus
//! moves to is selected in the same frame.
//!
//! A group none of whose items takes focus has an active item instead, held
//! by the GUI System. While the group is the one in the topmost open light or
//! modal overlay of the focused control's canvas (see [`driven_group`]), the
//! axis keys, Home and End move the active item, and Enter, or Space when the
//! focused control is not a text input, activates it before the focused
//! control. Without an active item a step starts at the
//! group's selected item, or the first or last. A focused text input keeps
//! Left, Right, Home and End for editing and a focused slider keeps its
//! arrows, Home and End. Keys no group uses stay with the focused control and
//! are unhandled where it has no use for them.
//!
//! The context records the active item it last moved by key or hover until
//! its World settles this context's routed work, so keys routed against the
//! same completed publication continue from it; after that the publication
//! names the System's active item.

use super::routing::{GuiInputRouter, GuiRoutingContext, Target, enqueue, semantic_view};
use super::{GuiInputError, GuiPhysicalKey, GuiRoutingDelivery, GuiRoutingDisposition};
use crate::components::GuiOverlay;
use crate::systems::canvas::CanvasHitKind;
use crate::systems::gui::local::{
    GUI_GROUP_HORIZONTAL, GUI_GROUP_SELECT_FOLLOW, GUI_GROUP_VERTICAL, GuiControlKind,
    GuiLocalAction, GuiLocalCommand,
};
use crate::systems::gui::presentation::{GuiCanvasSemanticView, GuiControlObservation};
use crate::{EntityId, HostRuntime, ViewDescriptor};
use std::sync::Arc;

/// Where an arrow, Home or End moves among a group's items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupMove {
    Previous,
    Next,
    First,
    Last,
}

impl GroupMove {
    /// The move `key` makes along `axis`. A text input keeps Left, Right,
    /// Home and End for editing.
    fn of(key: GuiPhysicalKey, axis: u32, text: bool) -> Option<Self> {
        let horizontal = axis != GUI_GROUP_VERTICAL;
        let vertical = axis != GUI_GROUP_HORIZONTAL;
        Some(match key {
            GuiPhysicalKey::Left if horizontal && !text => Self::Previous,
            GuiPhysicalKey::Right if horizontal && !text => Self::Next,
            GuiPhysicalKey::Up if vertical => Self::Previous,
            GuiPhysicalKey::Down if vertical => Self::Next,
            GuiPhysicalKey::Home if !text => Self::First,
            GuiPhysicalKey::End if !text => Self::Last,
            _ => return None,
        })
    }

    /// The index this move reaches among `count` items from `current`,
    /// stopping at the ends. Without a current item a step starts at `start`,
    /// the selected item, or else at the end it moves from.
    fn index(self, count: usize, current: Option<usize>, start: Option<usize>) -> Option<usize> {
        let last = count.checked_sub(1)?;
        Some(match (self, current) {
            (Self::First, _) => 0,
            (Self::Last, _) => last,
            (Self::Previous, Some(index)) => index.saturating_sub(1),
            (Self::Next, Some(index)) => (index + 1).min(last),
            (Self::Previous, None) => start.unwrap_or(last),
            (Self::Next, None) => start.unwrap_or(0),
        })
    }
}

type Observation = Arc<GuiControlObservation>;

/// The eligible items of `group` in `view`, in tree order, that take focus
/// or that do not.
fn items(view: &GuiCanvasSemanticView, group: EntityId, focusable: bool) -> Vec<&Observation> {
    view.controls
        .iter()
        .filter(|control| {
            control.available
                && control.hit.eligible
                && control.record.focusable == focusable
                && control.group.is_some_and(|item| item.group == group)
        })
        .collect()
}

/// Whether `control` is a selected item.
fn selected(control: &Observation) -> bool {
    control.group.is_some_and(|item| item.selected)
}

/// The group whose active item the focused control's keys move and activate:
/// a group none of whose items takes focus, with an eligible item inside the
/// topmost open light or modal overlay of the focused control's canvas; the
/// latest such group in tree order when that overlay holds several. Without
/// one no group takes the focused control's keys.
///
/// Open overlays are those the canvas's completed observations list, which
/// are visible ones. A manual overlay, such as a toast stack, and a hint
/// never take the keys of a field, so a toast or tooltip opening above an
/// option list leaves the keys with the list.
pub(super) fn driven_group(view: &GuiCanvasSemanticView) -> Option<EntityId> {
    let overlay = view.overlays.iter().rev().find(|overlay| {
        overlay.mode == GuiOverlay::MODE_LIGHT || overlay.mode == GuiOverlay::MODE_MODAL
    })?;

    // Groups with an item that takes focus have no active item.
    let focusing: Vec<EntityId> = view
        .controls
        .iter()
        .filter(|control| control.record.focusable)
        .filter_map(|control| control.group.map(|item| item.group))
        .collect();
    view.controls.iter().rev().find_map(|control| {
        let item = control.group?;
        (control.available
            && control.hit.eligible
            && !control.record.focusable
            && !focusing.contains(&item.group)
            && overlay.contains(&control.record.ancestry))
        .then_some(item.group)
    })
}

/// Keyboard traversal candidates, in order, with one stop for each group
/// whose items take focus: the focused item while focus is in the group,
/// else the group's selected item, else its first.
pub(super) fn tab_stops(candidates: Vec<Target>, focus: Option<&Target>) -> Vec<Target> {
    let same = |left: &Target, right: &Target| {
        left.control.record.target.world == right.control.record.target.world
            && left.path == right.path
    };
    let group = |target: &Target| target.control.group.map(|item| item.group);

    // Each group's stop: its candidate index and preference, lowest first.
    let mut stops: Vec<(usize, u8)> = Vec::new();
    let mut slots = vec![None; candidates.len()];
    for (index, candidate) in candidates.iter().enumerate() {
        let Some(item) = candidate.control.group else {
            continue;
        };
        let focused = focus.is_some_and(|focus| {
            same(focus, candidate) && focus.control.record.target == candidate.control.record.target
        });
        let preference = if focused {
            0
        } else if item.selected {
            1
        } else {
            2
        };
        let slot = stops.iter().position(|&(stop, _)| {
            same(&candidates[stop], candidate) && group(&candidates[stop]) == Some(item.group)
        });
        slots[index] = Some(match slot {
            Some(slot) => {
                if preference < stops[slot].1 {
                    stops[slot] = (index, preference);
                }
                slot
            }
            None => {
                stops.push((index, preference));
                stops.len() - 1
            }
        });
    }
    candidates
        .into_iter()
        .enumerate()
        .filter(|(index, _)| slots[*index].is_none_or(|slot| stops[slot].0 == *index))
        .map(|(_, candidate)| candidate)
        .collect()
}

/// Record a hovered item that does not take focus as the context's latest
/// active item, so keys routed before its World settles continue from it.
pub(super) fn note_hover(context: &mut GuiRoutingContext, target: &Target) {
    if target.control.group.is_some() && !target.control.record.focusable {
        context.active = Some(target.clone());
    }
}

impl GuiInputRouter {
    /// Route `key` for the groups of the focused control `focus`: the active
    /// item of the group it drives, or focus among the items of its own
    /// group. None leaves the key to the focused control.
    pub(super) fn group_key(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        view: ViewDescriptor,
        focus: &Target,
        key: GuiPhysicalKey,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<Option<GuiRoutingDisposition>, GuiInputError> {
        let kind = focus.control.record.kind;
        let text = kind == GuiControlKind::TextInput;
        // Value controls keep their own arrows, Home and End.
        let slider = matches!(kind, GuiControlKind::Slider | GuiControlKind::Color);
        let semantic = semantic_view(host, view, &focus.path)?;
        let target = |control: &Observation| Target {
            control: control.clone(),
            path: focus.path.clone(),
            source: view.publication,
            part: CanvasHitKind::Entity,
            focus_part: None,
            step: None,
        };

        if let Some(group) = driven_group(&semantic) {
            let items = items(&semantic, group, false);
            let current = context
                .active
                .as_ref()
                .and_then(|active| {
                    items
                        .iter()
                        .position(|item| item.record.target == active.control.record.target)
                })
                .or_else(|| {
                    items
                        .iter()
                        .position(|item| item.group.is_some_and(|item| item.active))
                });
            let activates = key == GuiPhysicalKey::Enter || (key == GuiPhysicalKey::Space && !text);
            if activates
                && let Some(index) = current
                && let Some(action) = activation(items[index].record.kind)
            {
                let item = target(items[index]);
                self.action(host, context, &item, action, delivery)?;
                return Ok(Some(GuiRoutingDisposition::Routed {
                    target: item.control.record.target,
                }));
            }
            let membership = items[0].group.expect("group item");
            if !slider && let Some(step) = GroupMove::of(key, membership.axis, text) {
                let start = items.iter().position(|item| selected(item));
                let index = step
                    .index(items.len(), current, start)
                    .expect("a driven group has an item");
                let item = target(items[index]);
                if current != Some(index) {
                    let input = self.input(host, context, &item, delivery)?;
                    enqueue(
                        host,
                        context.queue_owner,
                        item.control.record.target,
                        GuiLocalCommand::active_item(input)?,
                    )?;
                    if membership.selection == GUI_GROUP_SELECT_FOLLOW {
                        self.select(host, context, &item, delivery)?;
                    }
                    context.active = Some(item.clone());
                }
                return Ok(Some(GuiRoutingDisposition::Routed {
                    target: item.control.record.target,
                }));
            }
        }

        // A focused slider or colour control keeps its own arrows, Home and
        // End.
        if slider {
            return Ok(None);
        }
        let Some(membership) = focus.control.group else {
            return Ok(None);
        };
        let Some(step) = GroupMove::of(key, membership.axis, text) else {
            return Ok(None);
        };
        let items = items(&semantic, membership.group, true);
        let current = items
            .iter()
            .position(|item| item.record.target == focus.control.record.target);
        let start = items.iter().position(|item| selected(item));
        let Some(index) = step.index(items.len(), current, start) else {
            return Ok(Some(GuiRoutingDisposition::Routed {
                target: focus.control.record.target,
            }));
        };
        let item = target(items[index]);
        if current != Some(index) {
            self.focus(host, context, item.clone(), true, delivery)?;
            if membership.selection == GUI_GROUP_SELECT_FOLLOW {
                self.select(host, context, &item, delivery)?;
            }
        }
        Ok(Some(GuiRoutingDisposition::Routed {
            target: item.control.record.target,
        }))
    }

    /// Select a Button item of a group whose selection follows arrow
    /// movement; other items have no selection to write.
    fn select(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        item: &Target,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        if item.control.record.kind != GuiControlKind::Button || selected(&item.control) {
            return Ok(());
        }
        let input = self.input(host, context, item, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            item.control.record.target,
            GuiLocalCommand::select(input)?,
        )
    }
}

/// The action that activates an item of `kind`, as a press of it would.
fn activation(kind: GuiControlKind) -> Option<GuiLocalAction> {
    match kind {
        GuiControlKind::Button => Some(GuiLocalAction::Press),
        GuiControlKind::Checkbox => Some(GuiLocalAction::Toggle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::GroupMove;

    #[test]
    fn moves_stop_at_the_ends_and_start_at_the_selected_item() {
        use GroupMove::*;
        assert_eq!(Next.index(3, Some(2), None), Some(2));
        assert_eq!(Previous.index(3, Some(0), None), Some(0));
        assert_eq!(Next.index(3, None, Some(1)), Some(1));
        assert_eq!(Previous.index(3, None, None), Some(2));
        assert_eq!(Next.index(3, None, None), Some(0));
        assert_eq!(First.index(3, Some(2), Some(1)), Some(0));
        assert_eq!(Last.index(3, None, Some(0)), Some(2));
        assert_eq!(Next.index(0, None, None), None);
    }
}
