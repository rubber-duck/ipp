//! Committed control state and shared slider geometry.
//!
//! Committed checkbox and slider values are `node_data` row properties of the
//! root; the revision fence of every control and the committed string of a
//! text input are `node_tree` row properties.

use super::nodes::GuiControlValue;

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

    /// Normalized pointer value along the exact painted thumb-center rail.
    pub(crate) fn fraction_at(self, x: f32) -> Option<f32> {
        let travel = self.center_max - self.center_min;
        if !x.is_finite() || !(travel.is_finite() && travel > 0.0) {
            return None;
        }
        Some(((x - self.center_min) / travel).clamp(0.0, 1.0))
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

/// One committed control value and the revision that produced it, as read
/// from the root: checkbox and slider values come from `node_data` rows and
/// text and the revision from the node's `node_tree` row.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiControlState {
    /// Committed value; None while a former control node has non-control data.
    pub value: GuiControlValue,
    /// Monotonic revision; insertion commits revision 1.
    pub revision: u32,
}
