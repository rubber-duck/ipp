//! ScrollView scroll bar geometry and paint, shared by skinning and input.
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
//! Content that fits shows no bar on that axis: the bar's parts resolve the
//! disabled skin state, and they paint only when the node's theme declares a
//! colour or opacity for the disabled track, which is how a theme asks for
//! always-visible bars. Disabled bars never take input, and a
//! ScrollView disabled as a whole disables its bars.
//!
//! Nested ScrollViews keep their bars visible: a track that would lie under
//! or run into an enclosing ScrollView's track moves to that track's inner
//! edge or ends there, computed against the current enclosing offsets.
//!
//! Geometry is final logical at zero ancestor scroll, like the retained
//! view. Paint carries the ScrollView's node identity with the scroll bar
//! parts, so render preparation moves bars with the ScrollView's own outer
//! scroll shift and clip, and input hit testing applies the same shift.

use super::skin::{
    GuiInteractionState, GuiSkinCursors, GuiSkinState, GuiSkinnedAppearance,
    apply_appearance_to_primitive, resolve_paint_appearance, theme_part_style,
};
use super::{GuiEvaluatedNode, GuiEvaluatedView};
use crate::systems::gui::{GuiInputTarget, GuiNodeId, GuiPartId, GuiRoot};
use crate::systems::surface::{
    GuiPrimitiveId, GuiPrimitivePart, GuiShapeFill, SurfacePrimitiveIdentity,
    SurfacePrimitiveStyle, SurfaceRenderPrimitive, gui_logical_to_surface_content,
};
use std::collections::BTreeMap;

/// Track thickness as a fraction of the viewport's shorter side.
const SCROLL_BAR_THICKNESS: f32 = 0.05;

/// Shortest thumb, in track thicknesses.
const SCROLL_THUMB_MIN_LENGTH: f32 = 2.0;

/// Default track opacity over the node's foreground colour.
const SCROLL_TRACK_ALPHA: f32 = 0.25;

/// Default thumb opacity over the node's foreground colour.
const SCROLL_THUMB_ALPHA: f32 = 0.6;

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

/// Axis of one scroll bar part, or None for every other part.
pub(crate) const fn scroll_bar_axis(part: GuiPrimitivePart) -> Option<usize> {
    match part {
        GuiPrimitivePart::ScrollTrackX | GuiPrimitivePart::ScrollThumbX => Some(0),
        GuiPrimitivePart::ScrollTrackY | GuiPrimitivePart::ScrollThumbY => Some(1),
        GuiPrimitivePart::Background
        | GuiPrimitivePart::Fill
        | GuiPrimitivePart::Label
        | GuiPrimitivePart::Icon
        | GuiPrimitivePart::FocusRing
        | GuiPrimitivePart::Caret
        | GuiPrimitivePart::Selection
        | GuiPrimitivePart::Composition => None,
    }
}

/// Scroll capacity of one ScrollView record in local logical units: the
/// clamped maximum offset from evaluated content extents over the retained
/// viewport. Other records and degenerate viewports hold still.
pub(crate) fn scroll_capacity(node: &GuiEvaluatedNode) -> [f32; 2] {
    let Some(extent) = node.content_extents else {
        return [0.0, 0.0];
    };
    let viewport = viewport_local(node);
    if !viewport.iter().chain(&extent).all(|lane| lane.is_finite()) {
        return [0.0, 0.0];
    }
    [
        (extent[0] - viewport[0]).max(0.0),
        (extent[1] - viewport[1]).max(0.0),
    ]
}

/// Retained viewport size in local logical units.
fn viewport_local(node: &GuiEvaluatedNode) -> [f32; 2] {
    [
        node.rect[2] / node.acc_scale[0].abs().max(f32::MIN_POSITIVE),
        node.rect[3] / node.acc_scale[1].abs().max(f32::MIN_POSITIVE),
    ]
}

/// Axes whose content overflows the viewport.
pub(crate) fn scroll_bar_overflow(node: &GuiEvaluatedNode) -> [bool; 2] {
    let capacity = scroll_capacity(node);
    [capacity[0] > 0.0, capacity[1] > 0.0]
}

/// Whether one ScrollView shows a bar on one axis: its content overflows,
/// or its theme styles the disabled track to keep bars visible.
pub(crate) fn scroll_bar_shown(root: &GuiRoot, node: &GuiEvaluatedNode, axis: usize) -> bool {
    if node.viewport.is_none() {
        return false;
    }
    if scroll_bar_overflow(node)[axis] {
        return true;
    }
    let (track, _) = scroll_bar_parts(axis);
    let disabled = theme_part_style(
        root,
        node.node,
        GuiPartId::state(track, GuiSkinState::Disabled),
    );
    disabled.color.is_some() || disabled.opacity.is_some()
}

/// Whether one bar takes input: its content overflows and the ScrollView
/// itself is enabled.
pub(crate) fn scroll_bar_enabled(node: &GuiEvaluatedNode, axis: usize) -> bool {
    node.enabled && scroll_bar_overflow(node)[axis]
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

    /// Move both rectangles by a final-logical shift.
    pub(crate) fn shifted(mut self, shift: [f32; 2]) -> Self {
        for rect in [&mut self.track, &mut self.thumb] {
            rect[0] += shift[0];
            rect[1] += shift[1];
        }
        self
    }
}

/// Bars of the ScrollView record at `index` in `view` for committed offsets
/// read through `offset_of`, kept clear of every enclosing ScrollView's
/// shown bars.
///
/// Records are in pre-order, so the enclosing ScrollViews are the preceding
/// shallower viewport records. Working outward-in, each ScrollView's tracks
/// compare against the already placed tracks of its enclosing ScrollViews in
/// one scrolled frame: the enclosing offsets between them move the inner
/// viewport, never the outer bars. A track that would overlap a parallel
/// enclosing track moves across to that track's inner edge, and a track
/// that would run into a crossing enclosing track ends at its inner edge;
/// the shared corner between one ScrollView's own bars follows the moved
/// tracks. Geometry stays final logical at zero ancestor scroll, like the
/// retained view.
pub(crate) fn scroll_bars_in_view(
    view: &GuiEvaluatedView,
    root: &GuiRoot,
    index: usize,
    offset_of: impl Fn(GuiNodeId) -> [f32; 2],
) -> [Option<GuiScrollBar>; 2] {
    let Some(record) = view.nodes.get(index) else {
        return [None, None];
    };
    if record.viewport.is_none() {
        return [None, None];
    }

    // Enclosing ScrollViews, outermost first, then the record itself.
    let mut chain = vec![index];
    let mut depth = record.depth;
    for (ancestor, candidate) in view.nodes[..index].iter().enumerate().rev() {
        if depth == 0 {
            break;
        }
        if candidate.depth < depth {
            depth = candidate.depth;
            if candidate.viewport.is_some() {
                chain.push(ancestor);
            }
        }
    }
    chain.reverse();

    // Tracks placed so far, in the outermost ScrollView's frame.
    let mut placed: Vec<GuiScrollBar> = Vec::new();
    let mut shift = [0.0, 0.0];
    let mut bars = [None, None];
    for (position, &at) in chain.iter().enumerate() {
        let node = &view.nodes[at];
        let offset = offset_of(node.node);
        let shown = [
            scroll_bar_shown(root, node, 0),
            scroll_bar_shown(root, node, 1),
        ];
        bars = match GuiScrollBarFrame::of(node) {
            Some(frame) => {
                let obstacles: Vec<GuiScrollBar> = placed
                    .iter()
                    .map(|bar| bar.shifted([-shift[0], -shift[1]]))
                    .collect();
                let bounds = frame.clear_of(shown, &obstacles);
                frame.bars(node, offset, shown, bounds)
            }
            None => [None, None],
        };
        if position + 1 == chain.len() {
            break;
        }

        // Hidden or suppressed ScrollViews paint no bars to avoid.
        if node.available && node.visible && !node.paint_suppressed {
            placed.extend(bars.iter().flatten().map(|bar| bar.shifted(shift)));
        }
        shift[0] -= offset[0] * node.acc_scale[0];
        shift[1] -= offset[1] * node.acc_scale[1];
    }
    bars
}

/// Usable viewport of one ScrollView record and its track thickness.
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
    /// Frame of a ScrollView record with a finite, non-empty viewport.
    fn of(node: &GuiEvaluatedNode) -> Option<Self> {
        let (Some(viewport), Some(extent)) = (node.viewport, node.content_extents) else {
            return None;
        };
        let size = [viewport[2] - viewport[0], viewport[3] - viewport[1]];
        if !viewport.iter().chain(&extent).all(|lane| lane.is_finite())
            || size[0] <= 0.0
            || size[1] <= 0.0
        {
            return None;
        }

        Some(Self {
            viewport,
            extent,
            thickness: size[0].min(size[1]) * SCROLL_BAR_THICKNESS,
        })
    }

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

    /// Bars of `node` within `bounds` for a committed offset.
    fn bars(
        &self,
        node: &GuiEvaluatedNode,
        offset: [f32; 2],
        shown: [bool; 2],
        bounds: GuiScrollBarBounds,
    ) -> [Option<GuiScrollBar>; 2] {
        let capacity = scroll_capacity(node);
        let page = viewport_local(node);
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

/// Whether two `[x, y, width, height]` rectangles share interior area.
fn overlaps(left: [f32; 4], right: [f32; 4]) -> bool {
    left[0] < right[0] + right[2]
        && right[0] < left[0] + left[2]
        && left[1] < right[1] + right[3]
        && right[1] < left[1] + left[3]
}

/// Interaction of one scroll bar part from the input cursors: hover and
/// press name the exact part, and a bar that cannot scroll is disabled.
pub(crate) fn scroll_bar_interaction(
    cursors: &GuiSkinCursors,
    target: GuiInputTarget,
    part: GuiPrimitivePart,
    node: &GuiEvaluatedNode,
) -> GuiInteractionState {
    let axis = scroll_bar_axis(part).unwrap_or(0);
    let cursor = cursors.scroll_bars.get(&target);
    GuiInteractionState {
        disabled: !scroll_bar_enabled(node, axis),
        hovered: cursor.is_some_and(|cursor| cursor.hovered == Some(part)),
        pressed: cursor.is_some_and(|cursor| cursor.pressed == Some(part)),
        focused: false,
        // Scroll bars never take focus, so they never earn the ring.
        focus_visible: false,
    }
}

/// Skinned track and thumb primitives for one ScrollView, in Surface
/// content metres at zero ancestor scroll, horizontal bar first. Empty for
/// other nodes and for axes without a shown bar. `overrides` replaces a
/// part's resolved appearance while a skin transition owns it.
pub(crate) fn scroll_bar_primitives(
    view: &GuiEvaluatedView,
    root: &GuiRoot,
    node: &GuiEvaluatedNode,
    cursors: &GuiSkinCursors,
    overrides: &BTreeMap<GuiPrimitiveId, GuiSkinnedAppearance>,
) -> Vec<SurfaceRenderPrimitive> {
    let units = view.units_per_metre;
    if node.viewport.is_none() || !units.is_finite() || units <= 0.0 {
        return Vec::new();
    }
    let target = GuiInputTarget {
        entity: view.entity,
        root_incarnation: view.root_incarnation,
        node: node.node,
    };
    let Some(index) = view
        .nodes
        .iter()
        .position(|record| record.node == node.node)
    else {
        return Vec::new();
    };
    let offset_of = |node: GuiNodeId| {
        cursors
            .scroll_bars
            .get(&GuiInputTarget {
                node,
                ..target
            })
            .map_or([0.0, 0.0], |cursor| cursor.offset)
    };
    let clip = node.clip.and_then(|clip| {
        let min = gui_logical_to_surface_content([clip[0], clip[1]], units)?;
        let max = gui_logical_to_surface_content([clip[2], clip[3]], units)?;
        Some([min[0], min[1], max[0], max[1]])
    });

    let mut primitives = Vec::new();
    for bar in scroll_bars_in_view(view, root, index, offset_of)
        .into_iter()
        .flatten()
    {
        let (track, thumb) = scroll_bar_parts(bar.axis);
        for (part, rect, alpha) in [
            (track, bar.track, SCROLL_TRACK_ALPHA),
            (thumb, bar.thumb, SCROLL_THUMB_ALPHA),
        ] {
            let id = GuiPrimitiveId {
                root_incarnation: view.root_incarnation,
                node: node.node,
                part,
            };
            let interaction = scroll_bar_interaction(cursors, target, part, node);
            let appearance = overrides
                .get(&id)
                .cloned()
                .or_else(|| resolve_paint_appearance(root, node, &interaction, part));
            // A disabled bar paints only what its theme declares.
            if interaction.disabled
                && appearance.as_ref().is_none_or(|appearance| {
                    appearance.color.is_none() && appearance.opacity.is_none()
                })
            {
                continue;
            }
            let Some(position) = gui_logical_to_surface_content([rect[0], rect[1]], units) else {
                continue;
            };
            let color = [
                node.color[0],
                node.color[1],
                node.color[2],
                node.color[3] * alpha,
            ];
            let radius = rect[2].min(rect[3]) * 0.5 / units;
            let base = SurfaceRenderPrimitive::Box {
                style: SurfacePrimitiveStyle {
                    identity: SurfacePrimitiveIdentity::Gui(id),
                    position,
                    scale: [1.0, 1.0],
                    color,
                    opacity: node.opacity,
                    clip,
                },
                size: [rect[2] / units, rect[3] / units],
                corner_radius: [radius, radius],
                border_width: 0.0,
                border_color: [0.0, 0.0, 0.0, 0.0],
                fill: GuiShapeFill::Solid(color),
                glow: None,
            };
            primitives.push(match appearance {
                Some(appearance) => apply_appearance_to_primitive(&base, &appearance, units),
                None => base,
            });
        }
    }
    primitives
}

#[cfg(test)]
#[path = "scroll_bars_tests.rs"]
mod tests;
