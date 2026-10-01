//! Shared slider geometry and pure value mapping.
//!
//! Paint and pointer routing read one contained-thumb rail, so a press at the
//! painted thumb centre maps back to the committed value.

/// Slider-thumb edge as a fraction of the retained control height.
const SLIDER_THUMB_EDGE: f32 = 0.75;
/// Upper bound that leaves finite centre travel on narrow controls.
const SLIDER_THUMB_MAX_WIDTH: f32 = 0.75;

/// Shared slider geometry for paint and pointer-to-value routing.
///
/// The contained thumb travels by its center between `center_min` and
/// `center_max`. Keeping that interval in one helper prevents pointer-down at
/// a painted thumb center from changing the committed value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuiSliderRail {
    thumb_edge: f32,
    center_min: f32,
    center_max: f32,
    top: f32,
    height: f32,
}

impl GuiSliderRail {
    /// Filled track from the rail's left edge to the committed thumb center.
    ///
    /// The track spans the whole control rectangle, so the fill starts where
    /// the track starts rather than at the first thumb center; at the minimum
    /// value it still reaches under half the thumb.
    ///
    /// Decision (ipp-jtst.7): the fill is deliberately not inset by the track
    /// border. It shares the track's height and corner radius so the two read
    /// as one shape, and the flush start is the accepted ipp-jtst.2 behavior;
    /// the border stays visible on the unfilled remainder to mark travel.
    pub(crate) fn fill_rect(self, fraction: f32, height: f32) -> Option<[f32; 4]> {
        if !fraction.is_finite() || !height.is_finite() || height <= 0.0 {
            return None;
        }
        let half_thumb = self.thumb_edge * 0.5;
        Some([
            self.center_min - half_thumb,
            self.top + (self.height - height) * 0.5,
            half_thumb + fraction.clamp(0.0, 1.0) * (self.center_max - self.center_min),
            height,
        ])
    }

    /// Painted thumb rectangle for a normalized committed value.
    pub(crate) fn thumb_rect(self, fraction: f32) -> Option<[f32; 4]> {
        if !fraction.is_finite() {
            return None;
        }
        let center =
            self.center_min + fraction.clamp(0.0, 1.0) * (self.center_max - self.center_min);
        Some([
            center - self.thumb_edge * 0.5,
            self.top + (self.height - self.thumb_edge) * 0.5,
            self.thumb_edge,
            self.thumb_edge,
        ])
    }
}

/// Resolve the finite contained-thumb geometry for one retained slider rect.
pub(crate) fn slider_rail(rect: [f32; 4]) -> Option<GuiSliderRail> {
    if !rect.iter().all(|value| value.is_finite()) || rect[2] <= 0.0 || rect[3] <= 0.0 {
        return None;
    }
    let thumb_edge = (rect[3] * SLIDER_THUMB_EDGE).min(rect[2] * SLIDER_THUMB_MAX_WIDTH);
    Some(GuiSliderRail {
        thumb_edge,
        center_min: rect[0] + thumb_edge * 0.5,
        center_max: rect[0] + rect[2] - thumb_edge * 0.5,
        top: rect[1],
        height: rect[3],
    })
}

pub(crate) fn value_at(min: f32, max: f32, step: f32, fraction: f32, current: f32) -> Option<f32> {
    let mut value = min + fraction * (max - min);
    if fraction <= 0.0 {
        value = min;
    } else if fraction >= 1.0 {
        value = max;
    } else if step > 0.0 {
        let snapped = ((value - min) / step).round() * step + min;
        value = if current.is_finite()
            && current >= min
            && current <= max
            && (value - current).abs() <= (value - snapped).abs()
        {
            current
        } else {
            snapped
        };
    }
    value = value.clamp(min, max);
    value.is_finite().then_some(value)
}

pub(crate) fn nudge(min: f32, max: f32, step: f32, current: f32, steps: f32) -> Option<f32> {
    let stride = if step > 0.0 {
        step
    } else {
        (max - min) / 100.0
    };
    if !stride.is_finite() || stride == 0.0 {
        return None;
    }
    let mut value = (current + steps * stride).clamp(min, max);
    if step > 0.0 {
        value = ((value - min) / step).round() * step + min;
        value = value.clamp(min, max);
    }
    value.is_finite().then_some(value)
}
