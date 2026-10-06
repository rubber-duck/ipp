//! Scrolling a newly focused control into view.
//!
//! When focus moves to a control, every ScrollView and VirtualList that
//! contains it scrolls, innermost first, by the least distance that shows the
//! control's box in its viewport. It happens in the layout pass of the frame
//! whose mutation boundary moved focus, on that pass's placement, so the
//! revealed offset is an ordinary normalized position: layout writes it to
//! the position fields with the list's anchor, like any position it settles.
//!
//! The box is the control's layout box under the visual translation and
//! scale of the entities between it and the scroll view, as Canvas places
//! it. An outer scroll view sees that box clipped by the viewports of the
//! inner ones, so it shows what they show. A box larger than a viewport shows
//! its start, and one already covering the viewport leaves it in place. A
//! control in a VirtualList item that the list does not lay out, such as one
//! repeating another item's index, is revealed through its item's index as
//! scroll-to-index places that item; without either there is nothing to
//! reveal.

use super::entity_layout::GuiEntityLayoutView;
use super::scroll_layout::GuiScrollFieldPosition;
use super::virtual_list::GuiVirtualListLayout;
use crate::EntityId;
use crate::world::WorldSimulationState;
use std::sync::Arc;

/// A box `[min_x, min_y, max_x, max_y]` in one entity's local logical units.
pub(super) type GuiRevealBox = [f32; 4];

/// The control focus moved to, inside at least one scrolling control.
pub(super) struct GuiReveal {
    /// The newly focused control.
    target: EntityId,
    /// Its root-first ancestry, ending with the control.
    ancestry: Arc<[EntityId]>,
}

impl GuiReveal {
    /// The reveal of `target`, when a ScrollView or VirtualList contains it.
    pub fn new(world: &WorldSimulationState, target: EntityId) -> Option<Self> {
        let ancestry =
            crate::systems::gui::local::controls::identity::ancestry(&world.state, target);
        let reveal = Self {
            target,
            ancestry,
        };
        reveal
            .containers()
            .iter()
            .any(|&entity| GuiScrollFieldPosition::read(&world.components, entity).is_some())
            .then_some(reveal)
    }

    /// The target's ancestors, root first, without the target.
    fn containers(&self) -> &[EntityId] {
        &self.ancestry[..self.ancestry.len() - 1]
    }

    /// The target's branch below `scroll`: from the child of `scroll` on its
    /// path down to the target. None unless `scroll` contains the target.
    fn branch(&self, scroll: EntityId) -> Option<&[EntityId]> {
        let at = self
            .containers()
            .iter()
            .position(|&entity| entity == scroll)?;
        Some(&self.ancestry[at + 1..])
    }

    /// Whether `scroll` contains the target.
    pub fn inside(&self, scroll: EntityId) -> bool {
        self.branch(scroll).is_some()
    }

    /// The target's box in the content of `scroll`, while the placements of
    /// its children are still content positions: before the scroll view
    /// subtracts its offset and adds its padding. None while the target or
    /// an entity between them is not laid out.
    pub fn content_box(
        &self,
        view: &GuiEntityLayoutView,
        world: &WorldSimulationState,
        scroll: EntityId,
    ) -> Option<GuiRevealBox> {
        let path = self.branch(scroll)?;
        let size = view
            .placements
            .get(&self.target)
            .filter(|placement| placement.available)?
            .size;

        // Map the box up the branch, one parent frame at a time.
        let mut rect = [0.0, 0.0, size[0], size[1]];
        for (step, &entity) in path.iter().enumerate().rev() {
            let placement = view.placements.get(&entity);
            if placement.is_some_and(|placement| !placement.available) {
                return None;
            }
            rect = into_parent(
                rect,
                placement.map_or([0.0; 2], |placement| placement.origin),
                world.components.canvas_style(entity.index() as usize),
            );

            // An inner scroll view, already settled, shows only its viewport.
            let parent = step.checked_sub(1).map(|step| path[step]);
            if let Some(parent) = parent
                && let Some(inner) = view.scrolls.get(&parent)
            {
                let start = view
                    .placements
                    .get(&parent)
                    .map_or([0.0; 2], |placement| placement.content_offset);
                rect = clamp_to(
                    rect,
                    [
                        start[0],
                        start[1],
                        start[0] + inner.viewport[0],
                        start[1] + inner.viewport[1],
                    ],
                );
            }
        }

        rect.iter().all(|value| value.is_finite()).then_some(rect)
    }

    /// The box `list`, the layout of `scroll`, gives the item of the
    /// target's branch through that item's index.
    pub fn item_box(
        &self,
        world: &WorldSimulationState,
        scroll: EntityId,
        list: &GuiVirtualListLayout<EntityId>,
    ) -> Option<GuiRevealBox> {
        let item = *self.branch(scroll)?.first()?;
        let index = world
            .components
            .gui_virtual_item(item.index() as usize)?
            .index;
        if index >= list.item_count {
            return None;
        }

        // The item spans its main-axis slot; the cross axis does not scroll.
        let mut rect = [0.0; 4];
        rect[list.axis] = list.position(index);
        rect[list.axis + 2] = list.position(index + 1);
        rect.iter().all(|value| value.is_finite()).then_some(rect)
    }
}

/// `rect` in a child's local units in its parent's: the child's layout
/// origin, then its visual translation and scale, as Canvas places it.
fn into_parent(
    rect: GuiRevealBox,
    origin: [f32; 2],
    style: Option<&crate::systems::canvas::CanvasStyle>,
) -> GuiRevealBox {
    let (translation, scale) = style.map_or(([0.0; 2], [1.0; 2]), |style| {
        ([style.x, style.y], [style.scale_x, style.scale_y])
    });
    let mut parent = [0.0; 4];
    for axis in 0..2 {
        let start = origin[axis] + translation[axis] + rect[axis] * scale[axis];
        let end = origin[axis] + translation[axis] + rect[axis + 2] * scale[axis];
        parent[axis] = start.min(end);
        parent[axis + 2] = start.max(end);
    }
    parent
}

/// `rect` limited to `clip`; a box outside it collapses onto its nearest edge.
fn clamp_to(rect: GuiRevealBox, clip: GuiRevealBox) -> GuiRevealBox {
    std::array::from_fn(|index| {
        let axis = index % 2;
        rect[index].max(clip[axis]).min(clip[axis + 2])
    })
}
