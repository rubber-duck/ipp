//! Authored control measurement shared by layout, paint and routed input.

use crate::components::schema::ComponentLifecycle;
use crate::world::WorldSimulationState;
use crate::{EntityId, ErrorReason};
use ipp_schema_derive::SchemaComponent;

/// Skin geometry on a shared theme entity or directly on a control.
/// Negative one leaves a field unspecified. A control overrides its theme;
/// absent geometry resolves to zero. Theme lengths follow GuiMetrics.em,
/// control lengths are absolute, and ratios are dimensionless.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct GuiMetrics {
    /// Design em of shared lengths; zero keeps them absolute. Control overrides are absolute.
    pub em: f32,
    /// Minimum intrinsic control width.
    pub min_width: f32,
    /// Minimum intrinsic control height.
    pub min_height: f32,
    /// Text input left inset.
    pub label_inset: f32,
    /// Gap between a checkbox box and label.
    pub label_gap: f32,
    /// Slider rail thickness.
    pub rail_thickness: f32,
    /// Slider thumb side relative to the smaller control side.
    pub thumb_ratio: f32,
    /// Checkbox indicator side relative to its box.
    pub checkbox_ratio: f32,
    /// Button or text input icon side relative to control height.
    pub icon_ratio: f32,
    /// Numeric step cell width relative to control height.
    pub number_step_ratio: f32,
    /// Inset of a dial tick ring.
    pub dial_inset: f32,
    /// Gap between dial ticks and value ring.
    pub dial_gap: f32,
    /// Caret and composition underline thickness.
    pub caret_width: f32,
    /// Numeric step mark side.
    pub step_mark: f32,
    /// Colour control surface inset.
    pub color_inset: f32,
    /// Gap between colour surfaces.
    pub color_gap: f32,
    /// Colour rail width.
    pub color_rail: f32,
    /// Colour swatch height.
    pub color_swatch: f32,
    /// Intrinsic colour field side.
    pub color_field: f32,
    /// Colour field marker diameter.
    pub color_marker: f32,
    /// Colour rail thumb height.
    pub color_thumb: f32,
    /// Scroll track thickness.
    pub scroll_thickness: f32,
    /// Scroll track inset from the far side.
    pub scroll_inset: f32,
    /// Scroll track inset at each end.
    pub scroll_end_inset: f32,
    /// Scroll track tip inset relative to thickness.
    pub scroll_tip_ratio: f32,
    /// Minimum scroll thumb length relative to thickness.
    pub scroll_min_thumb_ratio: f32,
}

impl Default for GuiMetrics {
    fn default() -> Self {
        Self {
            em: 0.0,
            min_width: -1.0,
            min_height: -1.0,
            label_inset: -1.0,
            label_gap: -1.0,
            rail_thickness: -1.0,
            thumb_ratio: -1.0,
            checkbox_ratio: -1.0,
            icon_ratio: -1.0,
            number_step_ratio: -1.0,
            dial_inset: -1.0,
            dial_gap: -1.0,
            caret_width: -1.0,
            step_mark: -1.0,
            color_inset: -1.0,
            color_gap: -1.0,
            color_rail: -1.0,
            color_swatch: -1.0,
            color_field: -1.0,
            color_marker: -1.0,
            color_thumb: -1.0,
            scroll_thickness: -1.0,
            scroll_inset: -1.0,
            scroll_end_inset: -1.0,
            scroll_tip_ratio: -1.0,
            scroll_min_thumb_ratio: -1.0,
        }
    }
}

impl ComponentLifecycle for GuiMetrics {
    fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
        self.validate()
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if !self.em.is_finite() || self.em < 0.0 {
            return Err(ErrorReason::InvalidValue);
        }
        let values = [
            self.min_width,
            self.min_height,
            self.label_inset,
            self.label_gap,
            self.rail_thickness,
            self.thumb_ratio,
            self.checkbox_ratio,
            self.icon_ratio,
            self.number_step_ratio,
            self.dial_inset,
            self.dial_gap,
            self.caret_width,
            self.step_mark,
            self.color_inset,
            self.color_gap,
            self.color_rail,
            self.color_swatch,
            self.color_field,
            self.color_marker,
            self.color_thumb,
            self.scroll_thickness,
            self.scroll_inset,
            self.scroll_end_inset,
            self.scroll_tip_ratio,
            self.scroll_min_thumb_ratio,
        ];
        if values
            .iter()
            .all(|value| value.is_finite() && (*value == -1.0 || *value >= 0.0))
        {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

impl GuiMetrics {
    /// Resolve authored geometry once at the consumer's current font size.
    pub(in crate::world::systems) fn resolve(
        world: &WorldSimulationState,
        entity: EntityId,
        font_size: f32,
    ) -> Self {
        let own = world.components.gui_metrics(entity.index() as usize);
        let theme = world
            .components
            .gui_skin(entity.index() as usize)
            .map(|skin| skin.theme)
            .filter(|theme| world.state.entities.contains_key(theme));
        let shared = theme.and_then(|theme| world.components.gui_metrics(theme.index() as usize));
        let scale = shared.map_or(1.0, |metrics| {
            super::component::em_scale(metrics.em, font_size)
        });
        let field = |get: fn(&Self) -> f32, ratio: bool| {
            own.map(get)
                .filter(|value| *value >= 0.0)
                .unwrap_or_else(|| {
                    shared.map(get).filter(|value| *value >= 0.0).unwrap_or(0.0)
                        * if ratio {
                            1.0
                        } else {
                            scale
                        }
                })
        };
        Self {
            em: 0.0,
            min_width: field(|value| value.min_width, false),
            min_height: field(|value| value.min_height, false),
            label_inset: field(|value| value.label_inset, false),
            label_gap: field(|value| value.label_gap, false),
            rail_thickness: field(|value| value.rail_thickness, false),
            thumb_ratio: field(|value| value.thumb_ratio, true),
            checkbox_ratio: field(|value| value.checkbox_ratio, true),
            icon_ratio: field(|value| value.icon_ratio, true),
            number_step_ratio: field(|value| value.number_step_ratio, true),
            dial_inset: field(|value| value.dial_inset, false),
            dial_gap: field(|value| value.dial_gap, false),
            caret_width: field(|value| value.caret_width, false),
            step_mark: field(|value| value.step_mark, false),
            color_inset: field(|value| value.color_inset, false),
            color_gap: field(|value| value.color_gap, false),
            color_rail: field(|value| value.color_rail, false),
            color_swatch: field(|value| value.color_swatch, false),
            color_field: field(|value| value.color_field, false),
            color_marker: field(|value| value.color_marker, false),
            color_thumb: field(|value| value.color_thumb, false),
            scroll_thickness: field(|value| value.scroll_thickness, false),
            scroll_inset: field(|value| value.scroll_inset, false),
            scroll_end_inset: field(|value| value.scroll_end_inset, false),
            scroll_tip_ratio: field(|value| value.scroll_tip_ratio, true),
            scroll_min_thumb_ratio: field(|value| value.scroll_min_thumb_ratio, true),
        }
    }
}
