//! Resolved appearance of one GUI paint part and its interaction state.

use crate::services::asset_management::AssetSource;

/// Resolved interaction state with fixed precedence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiSkinState {
    /// The control cannot interact.
    Disabled,
    /// A pointer currently presses the control.
    Pressed,
    /// A pointer currently hovers the control.
    Hovered,
    /// The enabled control has no pointer interaction.
    Idle,
}

impl GuiSkinState {
    /// Resolve disabled over pressed over hovered over idle.
    pub fn resolve(disabled: bool, pressed: bool, hovered: bool) -> Self {
        if disabled {
            Self::Disabled
        } else if pressed {
            Self::Pressed
        } else if hovered {
            Self::Hovered
        } else {
            Self::Idle
        }
    }
}

/// Appearance properties resolved for one part. A control's parts resolve
/// through its kind's default look, so only properties no row sets are absent;
/// paint gives those the primitive's neutral value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiPartStyle {
    /// Optional linear RGBA override.
    pub color: Option<[f32; 4]>,
    /// Optional opacity multiplier.
    pub opacity: Option<f32>,
    /// Optional per-axis scale.
    pub scale: Option<[f32; 2]>,
    /// Optional horizontal alignment along the control, clamped to -1..=1 when
    /// painted. Only the checkbox indicator reads it: -1 and +1 centre it in
    /// the left- and right-most height-square cells of a wider control.
    pub align_x: Option<f32>,
    /// Optional drawing or bitmap source.
    pub asset: Option<AssetSource>,
    /// Optional per-axis corner radii `[rx, ry]` in logical units.
    pub corner_radius: Option<[f32; 2]>,
    /// Optional border width in logical units.
    pub border_width: Option<f32>,
    /// Optional straight linear RGBA border color.
    pub border_color: Option<[f32; 4]>,
    /// Optional fill mode: 0.0 = solid, 1.0 = linear gradient, 2.0 = radial gradient,
    /// 3.0 = hue along the gradient axis, 4.0 = saturation-value field of `fill_hue`.
    ///
    /// Solid paints the colour. Like every property, the mode resolves
    /// independently through the candidate chain, so a state that paints its
    /// own colour over an inherited gradient declares mode 0 explicitly.
    pub fill_mode: Option<f32>,
    /// Optional gradient start point (or radial center) in local shape
    /// logical units.
    pub gradient_start: Option<[f32; 2]>,
    /// Optional gradient end point in local shape logical units.
    pub gradient_end: Option<[f32; 2]>,
    /// Optional gradient stop 0 straight linear RGBA color.
    pub gradient_color0: Option<[f32; 4]>,
    /// Optional gradient stop 1 straight linear RGBA color.
    pub gradient_color1: Option<[f32; 4]>,
    /// Optional radial gradient radius in local shape logical units.
    pub gradient_radius: Option<f32>,
    /// Optional straight linear RGBA glow color.
    pub glow_color: Option<[f32; 4]>,
    /// Optional glow intensity multiplier (>= 0.0).
    pub glow_intensity: Option<f32>,
    /// Optional outward glow radius in logical units (>= 0.0).
    pub glow_radius: Option<f32>,
    /// Optional glow falloff exponent (>= 0.0).
    pub glow_falloff: Option<f32>,
    /// Optional inward glow radius in logical units (>= 0.0), over the fill.
    pub glow_inner_radius: Option<f32>,
    /// Optional 45-degree cut per corner `[tl, tr, br, bl]` in logical units.
    pub corner_cut: Option<[f32; 4]>,
    /// Optional accent span per corner `[tl, tr, br, bl]` in logical units.
    pub corner_accent: Option<[f32; 4]>,
    /// Optional border width inside accent spans in logical units; absent keeps
    /// the border width.
    pub corner_accent_width: Option<f32>,
    /// Optional shape: 0.0 = box, 1.0 = stroke, 2.0 = arc.
    pub shape: Option<f32>,
    /// Optional first stroke segment `[x0, y0, x1, y1]` normalised to the part.
    pub stroke_a: Option<[f32; 4]>,
    /// Optional second stroke segment `[x0, y0, x1, y1]` normalised to the part.
    pub stroke_b: Option<[f32; 4]>,
    /// Optional arc start in turns, clockwise from twelve o'clock.
    pub arc_start: Option<f32>,
    /// Optional signed arc sweep in turns.
    pub arc_sweep: Option<f32>,
    /// Optional arc dash pattern `[cells per turn, duty]`.
    pub arc_dashes: Option<[f32; 2]>,
    /// Optional saturation-value fill hue in turns from red.
    pub fill_hue: Option<f32>,
    /// Optional checker cell side in logical units; zero paints no checker.
    pub checker_size: Option<f32>,
    /// Optional straight linear RGBA colour of the checker's corner cell.
    pub checker_color0: Option<[f32; 4]>,
    /// Optional straight linear RGBA colour of the checker's other cells.
    pub checker_color1: Option<[f32; 4]>,
}

impl GuiPartStyle {
    /// Return whether no property is present.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Multiply every length property by `factor`: border and accent widths,
    /// corner radii, cuts and accent spans, both glow reaches, the gradient
    /// geometry and the checker's cells. Colours, opacity, `scale`, alignment,
    /// modes, the normalised stroke points, the arc's angles and dash pattern and
    /// the fill hue are not lengths.
    pub(in crate::world::systems::gui) fn scale_lengths(&mut self, factor: f32) {
        fn each<const N: usize>(value: &mut Option<[f32; N]>, factor: f32) {
            if let Some(lanes) = value {
                lanes.iter_mut().for_each(|lane| *lane *= factor);
            }
        }

        for length in [
            &mut self.border_width,
            &mut self.gradient_radius,
            &mut self.glow_radius,
            &mut self.glow_inner_radius,
            &mut self.corner_accent_width,
            &mut self.checker_size,
        ]
        .into_iter()
        .flatten()
        {
            *length *= factor;
        }
        each(&mut self.corner_radius, factor);
        each(&mut self.gradient_start, factor);
        each(&mut self.gradient_end, factor);
        each(&mut self.corner_cut, factor);
        each(&mut self.corner_accent, factor);
    }

    /// Fill every absent property from a less specific source.
    pub(in crate::world::systems::gui) fn inherit(&mut self, next: &Self) {
        fn or<T: Clone>(target: &mut Option<T>, next: &Option<T>) {
            if target.is_none() {
                target.clone_from(next);
            }
        }

        or(&mut self.color, &next.color);
        or(&mut self.opacity, &next.opacity);
        or(&mut self.scale, &next.scale);
        or(&mut self.align_x, &next.align_x);
        or(&mut self.asset, &next.asset);
        or(&mut self.corner_radius, &next.corner_radius);
        or(&mut self.border_width, &next.border_width);
        or(&mut self.border_color, &next.border_color);
        or(&mut self.fill_mode, &next.fill_mode);
        or(&mut self.gradient_start, &next.gradient_start);
        or(&mut self.gradient_end, &next.gradient_end);
        or(&mut self.gradient_color0, &next.gradient_color0);
        or(&mut self.gradient_color1, &next.gradient_color1);
        or(&mut self.gradient_radius, &next.gradient_radius);
        or(&mut self.glow_color, &next.glow_color);
        or(&mut self.glow_intensity, &next.glow_intensity);
        or(&mut self.glow_radius, &next.glow_radius);
        or(&mut self.glow_falloff, &next.glow_falloff);
        or(&mut self.glow_inner_radius, &next.glow_inner_radius);
        or(&mut self.corner_cut, &next.corner_cut);
        or(&mut self.corner_accent, &next.corner_accent);
        or(&mut self.corner_accent_width, &next.corner_accent_width);
        or(&mut self.shape, &next.shape);
        or(&mut self.stroke_a, &next.stroke_a);
        or(&mut self.stroke_b, &next.stroke_b);
        or(&mut self.arc_start, &next.arc_start);
        or(&mut self.arc_sweep, &next.arc_sweep);
        or(&mut self.arc_dashes, &next.arc_dashes);
        or(&mut self.fill_hue, &next.fill_hue);
        or(&mut self.checker_size, &next.checker_size);
        or(&mut self.checker_color0, &next.checker_color0);
        or(&mut self.checker_color1, &next.checker_color1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_precedence_is_disabled_over_pressed_over_hovered_over_idle() {
        use GuiSkinState::*;

        for (disabled, pressed, hovered, expected) in [
            (true, true, true, Disabled),
            (true, false, false, Disabled),
            (false, true, true, Pressed),
            (false, true, false, Pressed),
            (false, false, true, Hovered),
            (false, false, false, Idle),
        ] {
            assert_eq!(
                GuiSkinState::resolve(disabled, pressed, hovered),
                expected,
                "disabled={disabled}, pressed={pressed}, hovered={hovered}"
            );
        }
    }

    #[test]
    fn inherit_fills_only_absent_properties_from_the_less_specific_source() {
        let mut specific = GuiPartStyle {
            color: Some([1.0, 0.0, 0.0, 1.0]),
            fill_mode: Some(0.0),
            ..Default::default()
        };
        specific.corner_cut = Some([1.0, 0.0, 1.0, 0.0]);
        specific.inherit(&GuiPartStyle {
            color: Some([0.0, 0.0, 1.0, 1.0]),
            opacity: Some(0.5),
            fill_mode: Some(1.0),
            glow_radius: Some(4.0),
            corner_cut: Some([2.0; 4]),
            corner_accent: Some([3.0; 4]),
            stroke_b: Some([0.0, 0.0, 1.0, 1.0]),
            arc_sweep: Some(0.75),
            arc_dashes: Some([48.0, 0.25]),
            ..Default::default()
        });
        assert_eq!(
            specific,
            GuiPartStyle {
                color: Some([1.0, 0.0, 0.0, 1.0]),
                opacity: Some(0.5),
                fill_mode: Some(0.0),
                glow_radius: Some(4.0),
                corner_cut: Some([1.0, 0.0, 1.0, 0.0]),
                corner_accent: Some([3.0; 4]),
                stroke_b: Some([0.0, 0.0, 1.0, 1.0]),
                arc_sweep: Some(0.75),
                arc_dashes: Some([48.0, 0.25]),
                ..Default::default()
            }
        );
    }
}
