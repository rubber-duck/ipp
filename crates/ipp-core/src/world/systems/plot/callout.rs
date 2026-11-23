//! Authored callout appearance, independent of data-mark paint and data binding.
use crate::ErrorReason;
use crate::components::schema::ComponentLifecycle;
use ipp_schema_derive::SchemaComponent;

/// Optional chart-local panel and leader appearance; dimensions scale with the frame font.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct PlotCalloutStyle {
    /// Horizontal padding in font-size units.
    pub padding_x: f32,
    /// Vertical padding in font-size units.
    pub padding_y: f32,
    /// Corner radius in font-size units.
    pub radius: f32,
    /// Border width in font-size units; zero delegates edges entirely to the paint.
    pub border_width: f32,
    /// Fallback panel red.
    pub red: f32,
    /// Fallback panel green.
    pub green: f32,
    /// Fallback panel blue.
    pub blue: f32,
    /// Fallback panel alpha.
    pub alpha: f32,
    /// Text red.
    pub text_red: f32,
    /// Text green.
    pub text_green: f32,
    /// Text blue.
    pub text_blue: f32,
    /// Text alpha.
    pub text_alpha: f32,
    /// Connector red.
    pub leader_red: f32,
    /// Connector green.
    pub leader_green: f32,
    /// Connector blue.
    pub leader_blue: f32,
    /// Connector alpha.
    pub leader_alpha: f32,
    /// Connector width in font-size units.
    pub leader_width: f32,
}

impl Default for PlotCalloutStyle {
    fn default() -> Self {
        Self {
            padding_x: 0.65,
            padding_y: 0.4,
            radius: 0.35,
            border_width: 0.0,
            red: 0.025,
            green: 0.04,
            blue: 0.05,
            alpha: 0.94,
            text_red: 0.8,
            text_green: 0.9,
            text_blue: 0.94,
            text_alpha: 1.0,
            leader_red: 0.4,
            leader_green: 0.5,
            leader_blue: 0.55,
            leader_alpha: 0.5,
            leader_width: 0.035,
        }
    }
}

impl ComponentLifecycle for PlotCalloutStyle {
    fn validate(&self) -> Result<(), ErrorReason> {
        if [
            self.padding_x,
            self.padding_y,
            self.radius,
            self.border_width,
            self.leader_width,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
            || [
                self.red,
                self.green,
                self.blue,
                self.alpha,
                self.text_red,
                self.text_green,
                self.text_blue,
                self.text_alpha,
                self.leader_red,
                self.leader_green,
                self.leader_blue,
                self.leader_alpha,
            ]
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }
}

impl PlotCalloutStyle {
    pub(super) fn color(&self) -> [f32; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }

    pub(super) fn text_color(&self) -> [f32; 4] {
        [
            self.text_red,
            self.text_green,
            self.text_blue,
            self.text_alpha,
        ]
    }

    pub(super) fn leader_color(&self) -> [f32; 4] {
        [
            self.leader_red,
            self.leader_green,
            self.leader_blue,
            self.leader_alpha,
        ]
    }
}

/// Native sizing includes every authored line without changing the label text.
pub(super) fn text_extent(text: &str, font: f32) -> [f32; 2] {
    [
        text.lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0) as f32
            * font
            * 0.62,
        text.lines().count().max(1) as f32 * font * 1.2,
    ]
}
