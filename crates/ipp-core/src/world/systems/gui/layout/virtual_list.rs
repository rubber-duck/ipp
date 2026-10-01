//! VirtualList item placement and range math.
//!
//! A VirtualList is a scrolling viewport whose content extent derives from
//! its authored item count and per-item extent estimate rather than from its
//! children. Every item occupies the estimate along the main axis except a
//! declared child, whose measured extent replaces the estimate while it is
//! declared. A child's item index is its declared item index, so child order is
//! index order; children past the item count and later children repeating an
//! index are not laid out and paint nothing.
//!
//! The position of item `i` is `i * estimate` plus the sum of
//! `measured - estimate` over the declared children before it. Layout keeps
//! only the declared children, in index order, with their positions, so
//! every query here is a binary search or a walk bounded by the declared
//! window, never by the item count.
//!
//! Positions and offsets are in the list's local logical units along its
//! main axis, the space of its scroll offset. The persisted
//! anchor (first visible item and offset into it) maps to a scroll offset
//! through the current positions, so a measurement above the viewport moves
//! the offset with the content it measured and visible content stays put.

/// Main axis of a list without a valid `axis` property: vertical.
const DEFAULT_AXIS: usize = 1;

/// One declared child laid out at its item index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuiVirtualItem<Identity> {
    /// Item index of the declared child.
    pub(crate) index: u32,
    /// Declared child identity.
    pub(crate) node: Identity,
    /// Main-axis position of its leading edge.
    pub(crate) position: f32,
    /// Measured main-axis extent.
    pub(crate) extent: f32,
}

/// Evaluated item placement of one VirtualList, retained with its view.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GuiVirtualListLayout<Identity> {
    /// Authored item count.
    pub(crate) item_count: u32,
    /// Main-axis extent estimate per item, positive.
    pub(crate) item_extent: f32,
    /// Items wanted beyond each end of the visible range.
    pub(crate) overscan: u32,
    /// Main axis: 0 horizontal, 1 vertical.
    pub(crate) axis: usize,
    /// Main-axis viewport extent.
    pub(crate) viewport: f32,
    /// Declared children in ascending index order.
    pub(crate) items: Vec<GuiVirtualItem<Identity>>,
}

impl<Identity: Copy> GuiVirtualListLayout<Identity> {
    /// Construct retained placement independently of the author's storage format.
    /// Item indices are supplied explicitly; identities never determine positions.
    pub(crate) fn from_parameters(
        item_count: u32,
        item_extent: f32,
        overscan: u32,
        axis: usize,
        viewport: [f32; 2],
    ) -> Self {
        let axis = if axis <= 1 {
            axis
        } else {
            DEFAULT_AXIS
        };
        Self {
            item_count,
            item_extent: if item_extent.is_finite() && item_extent > 0.0 {
                item_extent
            } else {
                1.0
            },
            overscan,
            axis,
            viewport: viewport[axis],
            items: Vec::new(),
        }
    }

    /// Item index a child with order key `order` takes, or None when it
    /// lies past the count or repeats an index an earlier child took.
    /// Children must arrive in order-key order.
    pub(crate) fn accepts(&self, order: u32) -> Option<u32> {
        let after_last = self.items.last().is_none_or(|last| order > last.index);
        (order < self.item_count && after_last).then_some(order)
    }

    /// Main-axis position of an index past every declared child so far.
    pub(crate) fn next_position(&self, index: u32) -> f32 {
        index as f32 * self.item_extent + self.delta_before(self.items.len())
    }

    /// Content-box-local origin of a main-axis position.
    pub(crate) fn local_point(&self, position: f32) -> [f32; 2] {
        let mut point = [0.0, 0.0];
        point[self.axis] = position;
        point
    }

    /// Record one declared child at `index` with its measured main extent,
    /// or the estimate when it could not measure.
    pub(crate) fn push(&mut self, index: u32, node: Identity, size: [f32; 2], measured: bool) {
        let extent = size[self.axis];
        let extent = if measured && extent.is_finite() && extent >= 0.0 {
            extent
        } else {
            self.item_extent
        };
        let position = self.next_position(index);
        self.items.push(GuiVirtualItem {
            index,
            node,
            position,
            extent,
        });
    }

    /// Scrollable content size: the viewport's cross extent and the total
    /// main extent of every item.
    pub(crate) fn content_size(&self, viewport: [f32; 2]) -> [f32; 2] {
        let mut size = viewport;
        size[self.axis] = self.content_extent();
        size
    }

    /// Total main extent of every item.
    pub(crate) fn content_extent(&self) -> f32 {
        self.item_count as f32 * self.item_extent + self.delta_before(self.items.len())
    }

    /// Sum of `measured - estimate` over the first `count` declared items.
    fn delta_before(&self, count: usize) -> f32 {
        count
            .checked_sub(1)
            .and_then(|last| self.items.get(last))
            .map_or(0.0, |item| {
                item.position + item.extent - (item.index as f32 + 1.0) * self.item_extent
            })
    }

    /// Main-axis position of one item's leading edge.
    pub(crate) fn position(&self, index: u32) -> f32 {
        let at = self.items.partition_point(|item| item.index < index);
        match self.items.get(at) {
            Some(item) if item.index == index => item.position,
            _ => index as f32 * self.item_extent + self.delta_before(at),
        }
    }

    /// Item containing a main-axis offset, clamped to the item range; 0 for
    /// an empty list.
    pub(crate) fn item_at(&self, offset: f32) -> u32 {
        let Some(last) = self.item_count.checked_sub(1) else {
            return 0;
        };
        let offset = if offset.is_finite() {
            offset.max(0.0)
        } else {
            0.0
        };

        let at = self.items.partition_point(|item| item.position <= offset);
        if let Some(item) = at.checked_sub(1).map(|at| self.items[at])
            && offset < item.position + item.extent
        {
            return item.index;
        }

        // Between declared items every item takes the estimate.
        let base = at.checked_sub(1).map_or(0, |at| self.items[at].index + 1);
        let limit = self
            .items
            .get(at)
            .map_or(self.item_count, |item| item.index);
        let steps = ((offset - self.position(base)) / self.item_extent).floor();
        let index = if steps.is_finite() && steps > 0.0 {
            base.saturating_add(steps.min(u32::MAX as f32) as u32)
        } else {
            base
        };
        index.min(limit.saturating_sub(1).max(base)).min(last)
    }

    /// Anchor of a main-axis offset: the item containing it and the offset
    /// into that item.
    pub(crate) fn anchor(&self, offset: f32) -> (u32, f32) {
        let index = self.item_at(offset);
        (index, (offset - self.position(index)).max(0.0))
    }

    /// Main-axis offset of a persisted anchor under the current positions,
    /// before clamping to the scroll capacity.
    pub(crate) fn anchored_offset(&self, index: u32, offset: f32) -> f32 {
        let index = index.min(self.item_count.saturating_sub(1));
        let offset = if offset.is_finite() {
            offset.max(0.0)
        } else {
            0.0
        };
        self.position(index) + offset
    }

    /// Wanted item range `[first, last)` for a main-axis scroll offset: the
    /// visible items widened by the overscan on each side. Empty for an
    /// empty list.
    pub(crate) fn wanted_range(&self, offset: f32) -> (u32, u32) {
        if self.item_count == 0 {
            return (0, 0);
        }

        let first_visible = self.item_at(offset);
        let end = offset + self.viewport.max(0.0);
        let mut last_visible = self.item_at(end);
        if last_visible > first_visible && self.position(last_visible) >= end {
            last_visible -= 1;
        }
        (
            first_visible.saturating_sub(self.overscan),
            last_visible
                .saturating_add(1)
                .saturating_add(self.overscan)
                .min(self.item_count),
        )
    }
}

#[cfg(test)]
#[path = "virtual_list_tests.rs"]
mod tests;
