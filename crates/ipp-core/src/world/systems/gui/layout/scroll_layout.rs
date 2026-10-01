//! Scroll geometry and normalized position of scrolling controls.
//!
//! Layout reads a ScrollView's or VirtualList's position fields, evaluates its
//! viewport, content and capacity, clamps the position and, for a list,
//! reconciles offset and anchor and derives the wanted item range. It writes
//! the results back to the same fields in its pass.
//!
//! A VirtualList's anchor keeps visible content in place while item extents
//! change. Whichever of offset and anchor was written since the last layout
//! wins: an anchor write moves the offset to the anchored item, an offset
//! write re-anchors at the new offset, and when neither changed the anchor is
//! kept. Before the first layout of a control lifetime a non-default anchor
//! wins, so restored positions keep their anchored item.

use super::virtual_list::GuiVirtualListLayout;
use crate::components::registry::ComponentStorage;
use crate::{ComponentValue, EntityId};

/// Evaluated geometry and normalized position of one scrolling control.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::world::systems) struct GuiScrollLayout {
    /// ScrollView or VirtualList.
    pub component: u16,
    /// Component lifetime these values belong to.
    pub incarnation: u64,
    /// Local viewport size.
    pub viewport: [f32; 2],
    /// Logical content extent, estimating unrealized items.
    pub content: [f32; 2],
    /// Largest accepted offset per axis.
    pub capacity: [f32; 2],
    /// Normalized offset.
    pub offset: [f32; 2],
    /// VirtualList placement, anchor and wanted range.
    pub list: Option<GuiScrollListLayout>,
}

/// VirtualList part of a scroll layout.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::world::systems) struct GuiScrollListLayout {
    /// Realized item placement, a reconstructible cache.
    pub items: GuiVirtualListLayout<EntityId>,
    /// First visible item.
    pub anchor_index: u32,
    /// Offset within the anchor item.
    pub anchor_offset: f32,
    /// Inclusive first and exclusive last wanted item.
    pub range: (u32, u32),
}

/// Position fields as they are stored before this layout.
#[derive(Clone, Copy, Debug)]
pub(super) struct GuiScrollFieldPosition {
    pub offset: [f32; 2],
    pub anchor: Option<(u32, f32)>,
}

impl GuiScrollFieldPosition {
    pub fn read(components: &ComponentStorage, entity: EntityId) -> Option<(u16, Self)> {
        let index = entity.index() as usize;
        if let Some(list) = components.gui_virtual_list(index) {
            return Some((
                ComponentValue::GUI_VIRTUAL_LIST,
                Self {
                    offset: [list.offset_x, list.offset_y],
                    anchor: Some((list.anchor_index, list.anchor_offset)),
                },
            ));
        }
        components.gui_scroll_view(index).map(|view| {
            (
                ComponentValue::GUI_SCROLL_VIEW,
                Self {
                    offset: [view.offset_x, view.offset_y],
                    anchor: None,
                },
            )
        })
    }
}

impl GuiScrollLayout {
    /// Normalize the stored position against freshly evaluated geometry.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn settle(
        component: u16,
        incarnation: u64,
        stored: GuiScrollFieldPosition,
        previous: Option<&Self>,
        viewport: [f32; 2],
        content: [f32; 2],
        axis: u32,
        items: Option<GuiVirtualListLayout<EntityId>>,
    ) -> Self {
        let capacity = std::array::from_fn(|dimension| {
            if axis == 2 || axis as usize == dimension {
                (content[dimension] - viewport[dimension]).max(0.0)
            } else {
                0.0
            }
        });
        let clamp = |offset: [f32; 2]| -> [f32; 2] {
            std::array::from_fn(|dimension| {
                let value = offset[dimension];
                if value.is_finite() {
                    value.clamp(0.0, capacity[dimension])
                } else {
                    0.0
                }
            })
        };
        let previous = previous.filter(|previous| {
            previous.component == component && previous.incarnation == incarnation
        });
        let mut layout = Self {
            component,
            incarnation,
            viewport,
            content,
            capacity,
            offset: clamp(stored.offset),
            list: None,
        };
        let Some(items) = items else {
            return layout;
        };
        if items.item_count == 0 {
            layout.offset = [0.0; 2];
            layout.list = Some(GuiScrollListLayout {
                range: items.wanted_range(0.0),
                items,
                anchor_index: 0,
                anchor_offset: 0.0,
            });
            return layout;
        }
        let anchor = stored.anchor.unwrap_or((0, 0.0));
        let (anchor_written, offset_written) = match previous {
            Some(previous) => (
                previous
                    .list
                    .as_ref()
                    .is_none_or(|list| (list.anchor_index, list.anchor_offset) != anchor),
                previous.offset != stored.offset,
            ),
            None => (anchor != (0, 0.0), true),
        };
        let main = items.axis;
        let mut offset = stored.offset;
        if anchor_written || !offset_written {
            let index = anchor.0.min(items.item_count - 1);
            let within = if anchor.1.is_finite() {
                anchor.1.max(0.0)
            } else {
                0.0
            };
            offset[main] = items.anchored_offset(index, within);
        }
        layout.offset = clamp(offset);
        let (anchor_index, anchor_offset) = items.anchor(layout.offset[main]);
        layout.list = Some(GuiScrollListLayout {
            range: items.wanted_range(layout.offset[main]),
            items,
            anchor_index,
            anchor_offset,
        });
        layout
    }

    /// Write the evaluated values to the control's fields, touching only
    /// fields whose stored values differ.
    pub(super) fn write(&self, components: &mut ComponentStorage, entity: EntityId) {
        let index = entity.index() as usize;
        match self.component {
            ComponentValue::GUI_SCROLL_VIEW => {
                if let Some(view) = components.gui_scroll_view_mut(index) {
                    set(&mut view.offset_x, self.offset[0]);
                    set(&mut view.offset_y, self.offset[1]);
                    set(&mut view.viewport_x, self.viewport[0]);
                    set(&mut view.viewport_y, self.viewport[1]);
                    set(&mut view.content_x, self.content[0]);
                    set(&mut view.content_y, self.content[1]);
                    set(&mut view.capacity_x, self.capacity[0]);
                    set(&mut view.capacity_y, self.capacity[1]);
                }
            }
            ComponentValue::GUI_VIRTUAL_LIST => {
                if let Some(list) = components.gui_virtual_list_mut(index) {
                    set(&mut list.offset_x, self.offset[0]);
                    set(&mut list.offset_y, self.offset[1]);
                    set(&mut list.viewport_x, self.viewport[0]);
                    set(&mut list.viewport_y, self.viewport[1]);
                    set(&mut list.content_x, self.content[0]);
                    set(&mut list.content_y, self.content[1]);
                    set(&mut list.capacity_x, self.capacity[0]);
                    set(&mut list.capacity_y, self.capacity[1]);
                    if let Some(items) = &self.list {
                        set(&mut list.anchor_index, items.anchor_index);
                        set(&mut list.anchor_offset, items.anchor_offset);
                        set(&mut list.range_first, items.range.0);
                        set(&mut list.range_last, items.range.1);
                    }
                }
            }
            _ => {}
        }
    }
}

fn set<T: PartialEq>(field: &mut T, value: T) {
    if *field != value {
        *field = value;
    }
}
