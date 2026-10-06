//! Shared colour-control geometry, value mapping and colour model.
//!
//! Paint and pointer routing read one arrangement, so a press at the painted
//! marker or thumb maps back to the committed value, and paint evaluates the
//! swatch with the model the renderer evaluates the colour fields with, so the
//! swatch shows the colour under the marker.
//!
//! # Arrangement
//!
//! Lengths are ems of the control's font, so the arrangement keeps its
//! proportions at every unit scale. The surfaces lie half an em inside the
//! control's box, which keeps the marker and the thumbs inside it at every
//! value, and one em apart. The saturation-value field takes the box's width
//! less the rails beside it at its right, the hue rail and, when shown, the
//! alpha rail, each 1.5 em wide and as tall as the field. The swatch runs
//! along the bottom under all of them, 1.5 em tall. An unsized control has a
//! square field 9 em across: 15 by 12.5 em with the alpha rail, and 12.5 em
//! square without. A resized control grows or shrinks the field; the rails
//! and the swatch keep their thickness.
//!
//! # Values
//!
//! The field's saturation rises from its left edge to its right and its value
//! from its bottom edge to its top, so the top-left corner is white, the
//! top-right the pure hue and the bottom edge black. The rails rise from their
//! bottom edge to their top: the hue from red through yellow, green, cyan,
//! blue and magenta to red, and the alpha from transparent to the opaque
//! colour. The marker's centre is the field's point of the saturation and
//! value, and each thumb's centre is its rail's point of its value, so the
//! colour under each is the colour the value names.
//!
//! A pointer addresses the surface whose rectangle, grown by half the gap on
//! every side, holds it: the zones meet halfway across each gap and reach the
//! box's edge, and a drag past a surface's edge holds the value at that edge.
//! The swatch is no part and takes no input.
//!
//! # Keys
//!
//! On the field, Left and Right move the saturation and Down and Up the value
//! by a hundredth, a thousandth with Shift, and Home and End take the
//! saturation to 0 and 1 along the same axis. On a rail, Right and Up raise
//! its value and Left and Down lower it by the same steps, and Home and End
//! take it to 0 and 1. The wheel over the focused rail steps it; over the
//! field it scrolls, as the field has no one axis to step.

/// Margin of the surfaces from the control's box, in ems: half a gap, at
/// least the marker's radius and half a thumb.
pub(crate) const COLOR_INSET_EMS: f32 = 0.5;

/// Gap between the surfaces, in ems.
pub(crate) const COLOR_GAP_EMS: f32 = 1.0;

/// Width of a rail, in ems.
pub(crate) const COLOR_RAIL_EMS: f32 = 1.5;

/// Height of the swatch, in ems.
pub(crate) const COLOR_SWATCH_EMS: f32 = 1.5;

/// Side of an unsized control's field, in ems.
pub(crate) const COLOR_FIELD_EMS: f32 = 9.0;

/// Diameter of the field's marker, in ems.
pub(crate) const COLOR_MARKER_EMS: f32 = 0.75;

/// Height of a rail's thumb, a bar across the rail, in ems.
pub(crate) const COLOR_THUMB_EMS: f32 = 0.5;

/// Step of the arrow keys and the wheel on every channel.
pub(crate) const COLOR_STEP: f32 = 0.01;

/// Step of the arrow keys and the wheel with Shift on every channel.
pub(crate) const COLOR_FINE_STEP: f32 = 0.001;

/// Channel indices in field order.
pub(crate) const HUE: usize = 0;
pub(crate) const SATURATION: usize = 1;
pub(crate) const VALUE: usize = 2;
pub(crate) const ALPHA: usize = 3;

/// The size of an unsized colour control with a font of `font_size`.
pub(crate) fn intrinsic_size(font_size: f32, alpha_rail: bool) -> [f32; 2] {
    let rails = 1.0 + f32::from(u8::from(alpha_rail));
    let side = 2.0 * COLOR_INSET_EMS + COLOR_FIELD_EMS;
    [
        (side + rails * (COLOR_GAP_EMS + COLOR_RAIL_EMS)) * font_size,
        (side + COLOR_GAP_EMS + COLOR_SWATCH_EMS) * font_size,
    ]
}

/// The surfaces of one colour control's box, `[x, y, width, height]` in
/// control-local logical units, shared by paint and pointer routing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuiColorLayout {
    /// The saturation-value field.
    pub field: [f32; 4],
    /// The hue rail.
    pub hue: [f32; 4],
    /// The alpha rail, when shown.
    pub alpha: Option<[f32; 4]>,
    /// The swatch.
    pub swatch: [f32; 4],
    /// The marker's diameter.
    pub marker: f32,
    /// A thumb's height.
    pub thumb: f32,
    /// The gap between surfaces.
    gap: f32,
}

impl GuiColorLayout {
    /// The arrangement in a box of `size` for a font of `font_size`.
    pub(crate) fn new(size: [f32; 2], font_size: f32, alpha_rail: bool) -> Self {
        let em = |ems: f32| ems * font_size.max(0.0);
        let [inset, gap, rail, swatch] = [
            COLOR_INSET_EMS,
            COLOR_GAP_EMS,
            COLOR_RAIL_EMS,
            COLOR_SWATCH_EMS,
        ]
        .map(em);
        let width = (size[0] - 2.0 * inset).max(0.0);
        let height = (size[1] - 2.0 * inset).max(0.0);
        let rails = 1.0 + f32::from(u8::from(alpha_rail));
        let field_width = (width - rails * (gap + rail)).max(0.0);
        let tall = (height - gap - swatch).max(0.0);
        let hue = [inset + field_width + gap, inset, rail, tall];
        Self {
            field: [inset, inset, field_width, tall],
            hue,
            alpha: alpha_rail.then_some([hue[0] + rail + gap, inset, rail, tall]),
            swatch: [inset, inset + tall + gap, width, swatch],
            marker: em(COLOR_MARKER_EMS),
            thumb: em(COLOR_THUMB_EMS),
            gap,
        }
    }

    /// The surface of focus part `part`: the field 0, the hue rail 1 and the
    /// alpha rail 2.
    pub(crate) fn surface(&self, part: u32) -> Option<[f32; 4]> {
        match part {
            0 => Some(self.field),
            1 => Some(self.hue),
            2 => self.alpha,
            _ => None,
        }
    }

    /// The focus part whose zone holds control-local `local`: its surface
    /// grown by half the gap on every side. None over the swatch.
    pub(crate) fn part_at(&self, local: [f32; 2]) -> Option<u32> {
        let reach = self.gap * 0.5;
        (0..3).find(|&part| {
            self.surface(part).is_some_and(|[x, y, width, height]| {
                local[0] >= x - reach
                    && local[0] < x + width + reach
                    && local[1] >= y - reach
                    && local[1] < y + height + reach
            })
        })
    }

    /// The channels a pointer at control-local `local` sets on part `part`,
    /// in field order: the field's saturation and value, or a rail's hue or
    /// alpha, each clamped to its surface's edges.
    pub(crate) fn channels_at(&self, part: u32, local: [f32; 2]) -> [Option<f32>; 4] {
        let mut channels = [None; 4];
        let Some([x, y, width, height]) = self.surface(part) else {
            return channels;
        };
        let along = |offset: f32, extent: f32| {
            if extent > 0.0 && offset.is_finite() {
                (offset / extent).clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        let rise = 1.0 - along(local[1] - y, height);
        match part {
            0 => {
                channels[SATURATION] = Some(along(local[0] - x, width));
                channels[VALUE] = Some(rise);
            }
            1 => channels[HUE] = Some(rise),
            _ => channels[ALPHA] = Some(rise),
        }
        channels
    }

    /// The marker's square: centred on the field's point of `saturation` and
    /// `value`.
    pub(crate) fn marker_rect(&self, saturation: f32, value: f32) -> [f32; 4] {
        let [x, y, width, height] = self.field;
        let centre = [
            x + saturation.clamp(0.0, 1.0) * width,
            y + (1.0 - value.clamp(0.0, 1.0)) * height,
        ];
        [
            centre[0] - self.marker * 0.5,
            centre[1] - self.marker * 0.5,
            self.marker,
            self.marker,
        ]
    }

    /// A thumb's bar across `rail`, centred on its point of `fraction`.
    pub(crate) fn thumb_rect(&self, rail: [f32; 4], fraction: f32) -> [f32; 4] {
        let [x, y, width, height] = rail;
        let centre = y + (1.0 - fraction.clamp(0.0, 1.0)) * height;
        [x, centre - self.thumb * 0.5, width, self.thumb]
    }
}

/// The HSV model's sRGB-encoded colour, as the renderer's colour fields
/// evaluate it: the hue's pure colour at full saturation and value, mixed
/// with white by `1 - saturation` and darkened by `value`. Hue is in turns,
/// taken modulo one turn.
pub(crate) fn hsv_to_srgb(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let turn = hue - hue.floor();
    [0.0, 2.0 / 3.0, 1.0 / 3.0].map(|offset: f32| {
        let shifted = turn + offset;
        let pure = ((shifted - shifted.floor()) * 6.0 - 3.0).abs() - 1.0;
        let pure = pure.clamp(0.0, 1.0);
        (1.0 + (pure - 1.0) * saturation) * value
    })
}

/// One sRGB-encoded channel decoded to linear light.
pub(crate) fn srgb_to_linear(encoded: f32) -> f32 {
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// The straight linear RGB of an HSV colour, as solid fills and gradient stops
/// take it.
pub(crate) fn hsv_to_linear(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    hsv_to_srgb(hue, saturation, value).map(srgb_to_linear)
}

/// `current` moved by `steps` strides of `step` and kept in `0..=1`.
pub(crate) fn nudge(current: f32, steps: f32, step: f32) -> f32 {
    (current + steps * step).clamp(0.0, 1.0)
}

#[cfg(test)]
#[path = "color_tests.rs"]
mod tests;
