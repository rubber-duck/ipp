//! Resolved appearance of one GUI paint part and its interaction state.

use crate::services::asset_management::AssetSource;

/// Default focus-ring border width in logical units.
pub const FOCUS_BORDER_WIDTH: f32 = 0.005;

/// Default focus-ring border colour.
pub const FOCUS_BORDER_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

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

/// Appearance properties resolved for one part; absent properties fall back
/// to control defaults when painted.
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
    /// Optional fill mode: 0.0 = solid, 1.0 = linear gradient, 2.0 = radial gradient.
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
}

impl GuiPartStyle {
    /// Return whether no property is present.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
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
        specific.inherit(&GuiPartStyle {
            color: Some([0.0, 0.0, 1.0, 1.0]),
            opacity: Some(0.5),
            fill_mode: Some(1.0),
            glow_radius: Some(4.0),
            ..Default::default()
        });
        assert_eq!(
            specific,
            GuiPartStyle {
                color: Some([1.0, 0.0, 0.0, 1.0]),
                opacity: Some(0.5),
                fill_mode: Some(0.0),
                glow_radius: Some(4.0),
                ..Default::default()
            }
        );
    }
}
