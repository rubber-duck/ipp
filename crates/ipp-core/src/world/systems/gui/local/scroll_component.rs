use super::component::CONTROL_REQUIREMENTS;
use crate::ErrorReason;
use crate::components::schema::ComponentLifecycle;
use ipp_schema_derive::SchemaComponent;

/// Ordinary scrolling viewport. The committed position and the geometry of the
/// last layout are ordinary fields; `_x` is the horizontal axis and `_y` the
/// vertical one.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct GuiScrollView {
    /// Enabled axes: horizontal 0, vertical 1, both 2.
    pub axis: u32,
    /// Committed horizontal logical offset, clamped to the capacity by layout.
    pub offset_x: f32,
    /// Committed vertical logical offset, clamped to the capacity by layout.
    pub offset_y: f32,
    /// Local viewport width from the last layout.
    pub viewport_x: f32,
    /// Local viewport height from the last layout.
    pub viewport_y: f32,
    /// Logical content width from the last layout.
    pub content_x: f32,
    /// Logical content height from the last layout.
    pub content_y: f32,
    /// Largest accepted horizontal offset from the last layout.
    pub capacity_x: f32,
    /// Largest accepted vertical offset from the last layout.
    pub capacity_y: f32,
}

impl Default for GuiScrollView {
    fn default() -> Self {
        Self {
            axis: 1,
            offset_x: 0.0,
            offset_y: 0.0,
            viewport_x: 0.0,
            viewport_y: 0.0,
            content_x: 0.0,
            content_y: 0.0,
            capacity_x: 0.0,
            capacity_y: 0.0,
        }
    }
}

impl ComponentLifecycle for GuiScrollView {
    fn required_components() -> &'static [u16] {
        &CONTROL_REQUIREMENTS
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if self.axis <= 2 {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// Sparse list; realized children carry explicit item indices. The committed
/// position, the geometry of the last layout and the wanted item range are
/// ordinary fields; `_x` is the horizontal axis and `_y` the vertical one.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct GuiVirtualList {
    /// Logical number of items, independent of realization.
    pub item_count: u32,
    /// Positive main-axis extent estimate for an unrealized item.
    pub item_extent: f32,
    /// Additional wanted items on each side of the visible interval.
    pub overscan: u32,
    /// Horizontal 0 or vertical 1.
    pub axis: u32,
    /// Committed horizontal logical offset, clamped to the capacity by layout.
    pub offset_x: f32,
    /// Committed vertical logical offset, clamped to the capacity by layout.
    pub offset_y: f32,
    /// First visible item, which layout keeps in place when extents change.
    pub anchor_index: u32,
    /// Logical offset within the anchor item.
    pub anchor_offset: f32,
    /// Local viewport width from the last layout.
    pub viewport_x: f32,
    /// Local viewport height from the last layout.
    pub viewport_y: f32,
    /// Logical content width from the last layout, estimating unrealized items.
    pub content_x: f32,
    /// Logical content height from the last layout, estimating unrealized items.
    pub content_y: f32,
    /// Largest accepted horizontal offset from the last layout.
    pub capacity_x: f32,
    /// Largest accepted vertical offset from the last layout.
    pub capacity_y: f32,
    /// Inclusive first wanted item from the last layout.
    pub range_first: u32,
    /// Exclusive last wanted item from the last layout.
    pub range_last: u32,
}

impl Default for GuiVirtualList {
    fn default() -> Self {
        Self {
            item_count: 0,
            item_extent: 1.0,
            overscan: 0,
            axis: 1,
            offset_x: 0.0,
            offset_y: 0.0,
            anchor_index: 0,
            anchor_offset: 0.0,
            viewport_x: 0.0,
            viewport_y: 0.0,
            content_x: 0.0,
            content_y: 0.0,
            capacity_x: 0.0,
            capacity_y: 0.0,
            range_first: 0,
            range_last: 0,
        }
    }
}

impl ComponentLifecycle for GuiVirtualList {
    fn required_components() -> &'static [u16] {
        &CONTROL_REQUIREMENTS
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if self.item_count <= 1 << 24
            && self.item_extent.is_finite()
            && self.item_extent > 0.0
            && self.axis <= 1
            && (self.item_count as f32 * self.item_extent).is_finite()
        {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// Item placement within the immediate ordinary VirtualList parent.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiVirtualItem {
    /// Logical item index; entity identity and sibling order do not encode it.
    pub index: u32,
}

impl ComponentLifecycle for GuiVirtualItem {}
