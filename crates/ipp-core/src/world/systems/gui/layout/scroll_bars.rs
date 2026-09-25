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
//! Geometry is final logical at zero ancestor scroll, like the retained
//! view. Paint carries the ScrollView's node identity with the scroll bar
//! parts, so render preparation moves bars with the ScrollView's own outer
//! scroll shift and clip, and input hit testing applies the same shift.

use super::skin::{
    GuiInteractionState, GuiSkinCursors, GuiSkinState, GuiSkinnedAppearance,
    apply_appearance_to_primitive, resolve_paint_appearance, theme_part_style,
};
use super::{GuiEvaluatedNode, GuiEvaluatedView};
use crate::systems::gui::{GuiInputTarget, GuiPartId, GuiRoot};
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
        | GuiPrimitivePart::FocusRing => None,
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

/// Bars one ScrollView shows, per axis, for a committed offset. `shown`
/// selects the axes, normally from [`scroll_bar_shown`].
pub(crate) fn scroll_bars(
    node: &GuiEvaluatedNode,
    offset: [f32; 2],
    shown: [bool; 2],
) -> [Option<GuiScrollBar>; 2] {
    let (Some(viewport), Some(extent)) = (node.viewport, node.content_extents) else {
        return [None, None];
    };
    let size = [viewport[2] - viewport[0], viewport[3] - viewport[1]];
    if !viewport.iter().chain(&extent).all(|lane| lane.is_finite())
        || size[0] <= 0.0
        || size[1] <= 0.0
    {
        return [None, None];
    }

    let thickness = size[0].min(size[1]) * SCROLL_BAR_THICKNESS;
    let capacity = scroll_capacity(node);
    let page = viewport_local(node);
    let bar = |axis: usize| {
        if !shown[axis] {
            return None;
        }
        let corner = if shown[1 - axis] {
            thickness
        } else {
            0.0
        };
        let track = if axis == 0 {
            [
                viewport[0],
                viewport[3] - thickness,
                size[0] - corner,
                thickness,
            ]
        } else {
            [
                viewport[2] - thickness,
                viewport[1],
                thickness,
                size[1] - corner,
            ]
        };
        let length = track[axis + 2];
        if length <= 0.0 {
            return None;
        }
        let visible = if extent[axis] > 0.0 {
            (page[axis] / extent[axis]).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let thumb_length = (length * visible)
            .max(thickness * SCROLL_THUMB_MIN_LENGTH)
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
    let offset = cursors
        .scroll_bars
        .get(&target)
        .map_or([0.0, 0.0], |cursor| cursor.offset);
    let shown = [
        scroll_bar_shown(root, node, 0),
        scroll_bar_shown(root, node, 1),
    ];
    let clip = node.clip.and_then(|clip| {
        let min = gui_logical_to_surface_content([clip[0], clip[1]], units)?;
        let max = gui_logical_to_surface_content([clip[2], clip[3]], units)?;
        Some([min[0], min[1], max[0], max[1]])
    });

    let mut primitives = Vec::new();
    for bar in scroll_bars(node, offset, shown).into_iter().flatten() {
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
                Some(appearance) => apply_appearance_to_primitive(&base, &appearance),
                None => base,
            });
        }
    }
    primitives
}

#[cfg(test)]
#[path = "scroll_bars_tests.rs"]
mod tests;
