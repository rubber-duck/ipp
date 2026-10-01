use crate::systems::canvas::{CanvasHit, CanvasTarget};
use crate::systems::gui::local::{GuiControlKind, GuiEntityTarget};
use crate::{EntityId, OutputRef};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Actions supported by a role, independent of current eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum GuiSemanticActionKind {
    Press,
    Toggle,
    SetScalar,
    SetText,
    Focus,
    Blur,
    ScrollTo,
    ScrollBy,
    ScrollToIndex,
}

/// The value input routing reads from a control's fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GuiRoutingValue {
    /// A control whose routing needs no value.
    None,
    /// The slider's `value` field.
    Scalar(f32),
    /// The scroll position and the capacity of its last layout.
    Scroll {
        /// Offset per axis.
        offset: [f32; 2],
        /// Largest accepted offset per axis.
        capacity: [f32; 2],
    },
}

/// A control as one completed Canvas evaluation read it from its fields:
/// identity, role, routing value, eligibility and ancestry. Routing and
/// rendering read only this record; it confers no action authority.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiControlRecord {
    /// Exact control lifetime.
    pub target: GuiEntityTarget,
    /// Control role.
    pub kind: GuiControlKind,
    /// Value input routing reads.
    pub value: GuiRoutingValue,
    /// Evaluated `GuiBehavior.effective_enabled`.
    pub enabled: bool,
    /// Evaluated `GuiBehavior.effective_visible`.
    pub visible: bool,
    /// Evaluated `GuiBehavior.available`.
    pub available: bool,
    /// Core logical ancestors, root first and including this entity.
    pub ancestry: Arc<[EntityId]>,
}

impl GuiControlRecord {
    /// The scroll offset and capacity of a scrolling control.
    pub fn scroll(&self) -> Option<([f32; 2], [f32; 2])> {
        match self.value {
            GuiRoutingValue::Scroll {
                offset,
                capacity,
            } => Some((offset, capacity)),
            _ => None,
        }
    }
}

/// Slider constraints observed in the same completed publication as its value and hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiSliderGeometry {
    /// Inclusive minimum.
    pub min: f32,
    /// Inclusive maximum.
    pub max: f32,
    /// Step, or zero for continuous input.
    pub step: f32,
    /// Inclusive thumb-center X interval before the hit's position/scale transform.
    pub thumb_centers: [f32; 2],
    /// Local layout thumb `[x, y, width, height]` before decorative part transforms.
    pub thumb_rect: [f32; 4],
}

impl GuiSliderGeometry {
    pub(in crate::world::systems) fn new(
        slider: &super::super::local::GuiSlider,
        value: f32,
        size: [f32; 2],
    ) -> Option<Self> {
        let rail = super::super::slider_rail([0.0, 0.0, size[0], size[1]])?;
        let first = rail.thumb_rect(0.0)?;
        let last = rail.thumb_rect(1.0)?;
        let fraction = if slider.max > slider.min {
            ((value - slider.min) / (slider.max - slider.min)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Some(Self {
            min: slider.min,
            max: slider.max,
            step: slider.step,
            thumb_centers: [first[0] + first[2] * 0.5, last[0] + last[2] * 0.5],
            thumb_rect: rail.thumb_rect(fraction)?,
        })
    }
}

/// Immutable control observation joined by exact CanvasTarget, never a live component lookup.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiControlObservation {
    /// The exact measured single-line layout used by label, caret and pointer selection.
    pub text: Option<(Arc<crate::systems::surface::TextLayout>, [f32; 2])>,
    pub(crate) scroll_bars: [Option<super::super::layout::scroll_bars::GuiScrollBar>; 2],
    /// Innermost authored focus-scope ancestor in this completed logical tree.
    pub focus_scope: Option<crate::EntityId>,
    /// Identity, role, routing value, eligibility and ancestry read from fields.
    pub record: GuiControlRecord,
    /// The same finalized hit geometry carried by the corresponding Canvas output.
    pub hit: CanvasHit,
    /// Presentation-local layout/theme readiness; missing resources omit dependent parts.
    /// Local semantic admission remains independent of presentation readiness.
    pub available: bool,
    /// Range and local painted rail only for a nonempty slider role.
    pub slider: Option<GuiSliderGeometry>,
}

impl GuiControlObservation {
    pub(crate) fn scroll_hit(
        &self,
        part: crate::systems::canvas::CanvasPart,
        paint_order: u32,
    ) -> Option<CanvasHit> {
        use crate::systems::canvas::{CanvasAxis, CanvasHitKind, CanvasPart};
        let (axis, thumb) = match part {
            CanvasPart::ScrollTrackX => (0, false),
            CanvasPart::ScrollThumbX => (0, true),
            CanvasPart::ScrollTrackY => (1, false),
            CanvasPart::ScrollThumbY => (1, true),
            _ => return None,
        };
        let bar = self.scroll_bars[axis]?;
        let rect = if thumb {
            bar.thumb
        } else {
            bar.track
        };
        let axis = if axis == 0 {
            CanvasAxis::Horizontal
        } else {
            CanvasAxis::Vertical
        };
        let mut hit = self.hit.clone();
        hit.kind = if thumb {
            CanvasHitKind::ScrollThumb {
                axis,
            }
        } else {
            CanvasHitKind::ScrollTrack {
                axis,
            }
        };
        hit.paint_order = paint_order;
        for dimension in 0..2 {
            let start = hit.position[dimension] + rect[dimension] * hit.scale[dimension];
            let end = start + rect[dimension + 2] * hit.scale[dimension];
            hit.bounds[dimension] = start.min(end);
            hit.bounds[dimension + 2] = start.max(end);
        }
        Some(hit)
    }

    /// Role-supported operations; acceptance additionally checks eligibility.
    pub fn supported_actions(&self) -> &'static [GuiSemanticActionKind] {
        use GuiSemanticActionKind::*;
        match self.record.kind {
            GuiControlKind::Button => &[Press, Focus, Blur],
            GuiControlKind::Checkbox => &[Toggle, Focus, Blur],
            GuiControlKind::Slider => &[SetScalar, Focus, Blur],
            GuiControlKind::TextInput => &[SetText, Focus, Blur],
            GuiControlKind::ScrollView => &[ScrollTo, ScrollBy],
            GuiControlKind::VirtualList => &[ScrollTo, ScrollBy, ScrollToIndex],
        }
    }
}

/// The canvas's completed control observations in core painter order.
#[derive(Clone, Debug)]
pub struct GuiCanvasSemanticView {
    /// The canvas output in the completed World publication.
    pub selection: OutputRef,
    /// Matches that CanvasPublication's input revision, including non-geometric control changes.
    pub input_revision: u64,
    /// Shared records retain unchanged control observations.
    pub controls: Arc<[Arc<GuiControlObservation>]>,
}

impl GuiCanvasSemanticView {
    /// Join a Canvas hit only to its exact control component lifetime.
    pub fn control(&self, target: CanvasTarget) -> Option<&GuiControlObservation> {
        self.controls
            .iter()
            .find(|control| control.hit.target == target)
            .map(Arc::as_ref)
    }
}

impl PartialEq for GuiCanvasSemanticView {
    fn eq(&self, other: &Self) -> bool {
        self.selection == other.selection && self.input_revision == other.input_revision
    }
}

/// GUI observations published through CanvasSystem's ordinary System chunk.
/// Read this chunk and the selected Canvas output from the same WorldPublication.
#[derive(Clone, Debug, Default)]
pub struct GuiCanvasPublication {
    /// Local retained semantic version, not simulation time or admission authority.
    pub revision: u64,
    /// Immutable scopes; unchanged frames share this entire map.
    pub views: Arc<BTreeMap<OutputRef, Arc<GuiCanvasSemanticView>>>,
}

impl PartialEq for GuiCanvasPublication {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
    }
}
