//! Scroll actions over the position and capacity fields of a scrolling control.

use crate::ComponentValue;
use crate::systems::gui::local::GuiLocalActionError;

/// The fields a scroll action reads, from one ScrollView or VirtualList.
pub(in crate::world::systems::gui::local) struct GuiScrollFields {
    /// Current offset per axis.
    pub offset: [f32; 2],
    /// Largest accepted offset per axis from the last layout.
    pub capacity: [f32; 2],
    /// VirtualList anchor index, anchor offset and item count.
    pub anchor: Option<(u32, f32, u32)>,
}

impl GuiScrollFields {
    pub fn read(value: &ComponentValue) -> Result<Self, GuiLocalActionError> {
        match value {
            ComponentValue::GuiScrollView(view) => Ok(Self {
                offset: [view.offset_x, view.offset_y],
                capacity: [view.capacity_x, view.capacity_y],
                anchor: None,
            }),
            ComponentValue::GuiVirtualList(list) => Ok(Self {
                offset: [list.offset_x, list.offset_y],
                capacity: [list.capacity_x, list.capacity_y],
                anchor: Some((list.anchor_index, list.anchor_offset, list.item_count)),
            }),
            _ => Err(GuiLocalActionError::UnsupportedAction),
        }
    }

    /// Clamp an offset to the capacity; an empty list has no scroll range.
    pub fn clamp(&self, requested: [f32; 2]) -> [f32; 2] {
        if self.anchor.is_some_and(|(_, _, count)| count == 0) {
            return [0.0; 2];
        }
        std::array::from_fn(|axis| requested[axis].clamp(0.0, self.capacity[axis].max(0.0)))
    }
}
