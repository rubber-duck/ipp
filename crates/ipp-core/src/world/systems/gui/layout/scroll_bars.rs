//! ScrollView scroll bar geometry shared by control paint and input routing.
//!
//! A ScrollView shows one track and one thumb per axis whose content
//! overflows its viewport. The vertical track runs along the viewport's
//! right edge and the horizontal track along its bottom edge; when both show,
//! each stops short of the shared corner. Track thickness is a fraction of
//! the viewport's shorter side. The thumb length is the track length times
//! the visible fraction of the content, never shorter than a few
//! thicknesses, and the thumb travels the rest of the track in proportion to
//! the committed offset over the scroll capacity.
//!
//! Nested ScrollViews keep their bars visible: a track that would lie under
//! or run into an enclosing ScrollView's track moves to that track's inner
//! edge or ends there.

use crate::systems::gui::GuiPrimitivePart;

/// Track thickness as a fraction of the viewport's shorter side.
const SCROLL_BAR_THICKNESS: f32 = 0.05;

/// Shortest thumb, in track thicknesses.
const SCROLL_THUMB_MIN_LENGTH: f32 = 2.0;

/// Track and thumb parts of one axis: 0 horizontal, 1 vertical.
pub(crate) const fn scroll_bar_parts(axis: usize) -> (GuiPrimitivePart, GuiPrimitivePart) {
    if axis == 0 {
        (
            GuiPrimitivePart::ScrollTrackX,
            GuiPrimitivePart::ScrollThumbX,
        )
    } else {
        (
            GuiPrimitivePart::ScrollTrackY,
            GuiPrimitivePart::ScrollThumbY,
        )
    }
}

/// One axis of a ScrollView's scroll bar in final logical units at zero
/// ancestor scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuiScrollBar {
    /// 0 horizontal, 1 vertical.
    pub(crate) axis: usize,
    /// Track rectangle `[x, y, width, height]`.
    pub(crate) track: [f32; 4],
    /// Thumb rectangle within the track.
    pub(crate) thumb: [f32; 4],
    /// Scroll capacity on this axis in local logical units.
    pub(crate) capacity: f32,
    /// Viewport extent on this axis in local logical units; one page.
    pub(crate) page: f32,
}

impl GuiScrollBar {
    /// Track start along the bar's axis.
    pub(crate) fn track_start(&self) -> f32 {
        self.track[self.axis]
    }

    /// Thumb start along the bar's axis.
    pub(crate) fn thumb_start(&self) -> f32 {
        self.thumb[self.axis]
    }

    /// Distance the thumb can travel along the track.
    pub(crate) fn travel(&self) -> f32 {
        (self.track[self.axis + 2] - self.thumb[self.axis + 2]).max(0.0)
    }

    /// Offset that places the thumb start at `thumb_start` along the axis,
    /// clamped to the scroll capacity.
    pub(crate) fn offset_for_thumb(&self, thumb_start: f32) -> f32 {
        let travel = self.travel();
        if travel <= 0.0 || self.capacity <= 0.0 {
            return 0.0;
        }
        ((thumb_start - self.track_start()) / travel).clamp(0.0, 1.0) * self.capacity
    }

    /// Whether the bar can scroll; a bar shown only by its theme never takes input.
    pub(crate) fn enabled(&self) -> bool {
        self.capacity > 0.0
    }
}

/// Scroll geometry of one ScrollView or VirtualList from its layout fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuiScrollExtent {
    /// Local viewport size.
    pub(crate) viewport: [f32; 2],
    /// Logical content extent.
    pub(crate) content: [f32; 2],
    /// Largest accepted offset per axis.
    pub(crate) capacity: [f32; 2],
}

/// Usable viewport of one ScrollView and its track thickness.
#[derive(Clone, Copy, Debug)]
struct GuiScrollBarFrame {
    /// Viewport `[min_x, min_y, max_x, max_y]` in final logical units.
    viewport: [f32; 4],
    /// Scroll content extents in local logical units.
    extent: [f32; 2],
    /// Track thickness in final logical units.
    thickness: f32,
}

/// Where one ScrollView's tracks sit: `edge[0]` is the right edge of the
/// vertical track column and `edge[1]` the bottom edge of the horizontal
/// track row; `end[axis]` is the furthest point the track along `axis` may
/// reach.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GuiScrollBarBounds {
    edge: [f32; 2],
    end: [f32; 2],
}

impl GuiScrollBarFrame {
    /// Tracks along the viewport's right and bottom edges.
    fn bounds(&self) -> GuiScrollBarBounds {
        let far = [self.viewport[2], self.viewport[3]];
        GuiScrollBarBounds {
            edge: far,
            end: far,
        }
    }

    /// Track rectangle along `axis` within `bounds`, stopping short of the
    /// crossing track when that axis shows too; None when nothing is left.
    fn track(&self, axis: usize, shown: [bool; 2], bounds: GuiScrollBarBounds) -> Option<[f32; 4]> {
        let thickness = self.thickness;
        let mut end = bounds.end[axis];
        if shown[1 - axis] {
            end = end.min(bounds.edge[axis] - thickness);
        }
        let start = self.viewport[axis];
        let length = end - start;
        if !length.is_finite() || length <= 0.0 {
            return None;
        }

        Some(if axis == 0 {
            [start, bounds.edge[1] - thickness, length, thickness]
        } else {
            [bounds.edge[0] - thickness, start, thickness, length]
        })
    }

    /// Bounds that keep this ScrollView's shown tracks clear of
    /// `obstacles`, enclosing tracks in this frame. Parallel tracks move
    /// first, never past the viewport's start; tracks then end before any
    /// crossing track they would still run into.
    fn clear_of(&self, shown: [bool; 2], obstacles: &[GuiScrollBar]) -> GuiScrollBarBounds {
        let mut bounds = self.bounds();
        for axis in (0..2).filter(|&axis| shown[axis]) {
            let cross = 1 - axis;
            let floor = self.viewport[cross] + self.thickness;

            // Each move only decreases the edge, so this settles within one
            // pass per obstacle.
            for _ in 0..=obstacles.len() {
                let column = self.track(axis, [axis == 0, axis == 1], bounds);
                let blocking = obstacles
                    .iter()
                    .filter(|obstacle| obstacle.axis == axis)
                    .filter(|obstacle| column.is_some_and(|rect| overlaps(rect, obstacle.track)))
                    .map(|obstacle| obstacle.track[cross])
                    .fold(bounds.edge[cross], f32::min)
                    .max(floor);
                if blocking == bounds.edge[cross] {
                    break;
                }
                bounds.edge[cross] = blocking;
            }
        }
        for axis in (0..2).filter(|&axis| shown[axis]) {
            let Some(rect) = self.track(axis, shown, bounds) else {
                continue;
            };
            for obstacle in obstacles.iter().filter(|obstacle| obstacle.axis != axis) {
                if overlaps(rect, obstacle.track) {
                    bounds.end[axis] = bounds.end[axis].min(obstacle.track[axis]);
                }
            }
        }
        bounds
    }

    fn metric_bars(
        &self,
        capacity: [f32; 2],
        page: [f32; 2],
        offset: [f32; 2],
        shown: [bool; 2],
        bounds: GuiScrollBarBounds,
    ) -> [Option<GuiScrollBar>; 2] {
        let bar = |axis: usize| {
            if !shown[axis] {
                return None;
            }
            let track = self.track(axis, shown, bounds)?;
            let length = track[axis + 2];
            let visible = if self.extent[axis] > 0.0 {
                (page[axis] / self.extent[axis]).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let thumb_length = (length * visible)
                .max(self.thickness * SCROLL_THUMB_MIN_LENGTH)
                .min(length);
            let fraction = if capacity[axis] > 0.0 {
                (offset[axis] / capacity[axis]).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut thumb = track;
            thumb[axis] += (length - thumb_length) * fraction;
            thumb[axis + 2] = thumb_length;
            Some(GuiScrollBar {
                axis,
                track,
                thumb,
                capacity: capacity[axis],
                page: page[axis],
            })
        };
        [bar(0), bar(1)]
    }
}

/// Bars of one ordinary ScrollView: an axis shows its bar while its content
/// overflows, or while `kept` because its theme styles the disabled track.
pub(crate) fn ordinary_scroll_bars(
    extent: &GuiScrollExtent,
    offset: [f32; 2],
    obstacles: &[GuiScrollBar],
    kept: [bool; 2],
) -> Vec<GuiScrollBar> {
    let [width, height] = extent.viewport;
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let frame = GuiScrollBarFrame {
        viewport: [0.0, 0.0, width, height],
        extent: extent.content,
        thickness: width.min(height) * SCROLL_BAR_THICKNESS,
    };
    let shown = std::array::from_fn(|axis| extent.capacity[axis] > 0.0 || kept[axis]);
    frame
        .metric_bars(
            extent.capacity,
            extent.viewport,
            offset,
            shown,
            frame.clear_of(shown, obstacles),
        )
        .into_iter()
        .flatten()
        .collect()
}

/// Whether two `[x, y, width, height]` rectangles share interior area.
fn overlaps(left: [f32; 4], right: [f32; 4]) -> bool {
    left[0] < right[0] + right[2]
        && right[0] < left[0] + left[2]
        && left[1] < right[1] + right[3]
        && right[1] < left[1] + left[3]
}

#[cfg(test)]
#[path = "scroll_bar_geometry_tests.rs"]
mod geometry_tests;
