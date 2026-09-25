//! CPU references of the GUI shape material shader math, used only by tests
//! to check fills and glows without a renderer.

use super::{GuiShapeFill, GuiShapeGlow};

/// Reference evaluation of [`GuiShapeFill`].
pub(super) trait GuiShapeFillSampling {
    /// Whether this gradient is degenerate (zero length or zero radius).
    fn is_degenerate(&self) -> bool;

    /// Evaluate the fill color at a local shape space position in metres.
    fn sample(&self, point: [f32; 2]) -> [f32; 4];
}

/// Reference evaluation of [`GuiShapeGlow`].
pub(super) trait GuiShapeGlowSampling {
    /// Glow color at a distance outside the shape edge.
    fn sample_intensity(&self, distance_outside: f32) -> [f32; 4];
}

impl GuiShapeFillSampling for GuiShapeFill {
    /// Whether this gradient is degenerate (zero length or zero radius).
    ///
    /// Degenerate linear gradients (start == end) safely evaluate to `start_color`.
    /// Degenerate radial gradients (radius <= 0) safely evaluate to `start_color`.
    fn is_degenerate(&self) -> bool {
        match self {
            Self::Solid(_) => false,
            Self::LinearGradient {
                start,
                end,
                ..
            } => {
                let dx = end[0] - start[0];
                let dy = end[1] - start[1];
                dx * dx + dy * dy <= 1e-12
            }
            Self::RadialGradient {
                radius,
                ..
            } => *radius <= 1e-6,
        }
    }

    /// Evaluate the fill color at a local shape space position [x, y] in metres.
    ///
    /// Degenerate linear gradients (zero distance between start and end) and
    /// degenerate radial gradients (zero radius) evaluate to `start_color`.
    fn sample(&self, point: [f32; 2]) -> [f32; 4] {
        match self {
            Self::Solid(color) => *color,
            Self::LinearGradient {
                start,
                end,
                start_color,
                end_color,
            } => {
                let d = [end[0] - start[0], end[1] - start[1]];
                let len_sq = d[0] * d[0] + d[1] * d[1];
                if len_sq <= 1e-12 {
                    return *start_color;
                }
                let p = [point[0] - start[0], point[1] - start[1]];
                let t = ((p[0] * d[0] + p[1] * d[1]) / len_sq).clamp(0.0, 1.0);
                interpolate_color(*start_color, *end_color, t)
            }
            Self::RadialGradient {
                center,
                radius,
                start_color,
                end_color,
            } => {
                if *radius <= 1e-6 {
                    return *start_color;
                }
                let d = [point[0] - center[0], point[1] - center[1]];
                let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
                let t = (dist / radius).clamp(0.0, 1.0);
                interpolate_color(*start_color, *end_color, t)
            }
        }
    }
}

impl GuiShapeGlowSampling for GuiShapeGlow {
    /// Sample the glow intensity at a distance `d >= 0.0` outside the shape edge.
    /// Returns straight linear RGBA with alpha attenuated by distance falloff.
    fn sample_intensity(&self, distance_outside: f32) -> [f32; 4] {
        if distance_outside <= 0.0 {
            let a = (self.color[3] * self.intensity).clamp(0.0, 1.0);
            return [self.color[0], self.color[1], self.color[2], a];
        }
        let cutoff = self.cutoff_distance();
        if cutoff <= 0.0 || distance_outside >= cutoff {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let norm = (1.0 - distance_outside / cutoff).clamp(0.0, 1.0);
        let factor = if self.falloff == 1.0 {
            norm
        } else {
            norm.powf(self.falloff)
        };
        let a = (self.color[3] * self.intensity * factor).clamp(0.0, 1.0);
        [self.color[0], self.color[1], self.color[2], a]
    }
}

fn interpolate_color(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}
