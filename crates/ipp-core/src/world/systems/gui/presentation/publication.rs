use crate::systems::canvas::{CanvasClip, CanvasHit, CanvasTarget};
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
    SetColor,
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
    /// The slider's `value` field: a range's lower value.
    Scalar(f32),
    /// A colour control's hue, saturation, value and alpha.
    Color([f32; 4]),
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
    /// The control's own `GuiBehavior.focusable`: whether traversal and a
    /// pointer press may focus it.
    pub focusable: bool,
    /// Its focus parts, each a traversal stop: a range's two thumbs, a
    /// colour control's surfaces, else 1.
    pub focus_parts: u32,
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
    /// Coordinate index the value runs along: 0 for x, 1 for y.
    pub axis: usize,
    /// Thumb centre along `axis` at the minimum and at the maximum, before the
    /// hit's position/scale transform; the maximum's is the smaller on a
    /// vertical slider, whose minimum is at the bottom.
    pub thumb_centers: [f32; 2],
    /// Local layout thumb `[x, y, width, height]` before decorative part
    /// transforms: a range's lower thumb.
    pub thumb_rect: [f32; 4],
    /// A dial's upward pointer travel, before the hit's position/scale
    /// transform, that crosses the whole range; None for a rail. A dial's
    /// drags are relative to their press, so its rail fields go unused.
    pub dial_travel: Option<f32>,
    /// Both thumbs of a range; none for a slider with one value.
    pub range: Option<GuiSliderRange>,
}

/// A range slider's two thumbs as one completed evaluation read them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiSliderRange {
    /// The lower and the upper value.
    pub values: [f32; 2],
    /// The lower and the upper thumb's local layout rectangle.
    pub thumb_rects: [[f32; 4]; 2],
}

impl GuiSliderGeometry {
    pub(in crate::world::systems) fn new(
        slider: &super::super::local::GuiSlider,
        value: f32,
        size: [f32; 2],
    ) -> Option<Self> {
        let axis = slider.rail_axis();
        let rail = super::super::slider_rail([0.0, 0.0, size[0], size[1]], axis)?;
        let thumb_rect = rail.thumb_rect(slider.fraction(value))?;
        let range = match slider.range {
            true => Some(GuiSliderRange {
                values: [value, slider.upper],
                thumb_rects: [thumb_rect, rail.thumb_rect(slider.fraction(slider.upper))?],
            }),
            false => None,
        };
        Some(Self {
            min: slider.min,
            max: slider.max,
            step: slider.step,
            axis,
            thumb_centers: rail.thumb_centers(),
            thumb_rect,
            range,
            dial_travel: if slider.is_dial() {
                Some(
                    super::super::local::slider::slider_dial([0.0, 0.0, size[0], size[1]])?
                        .travel(),
                )
            } else {
                None
            },
        })
    }

    /// Where `value` lies in the range, from 0 at the minimum to 1 at the
    /// maximum; an empty range places every value at the minimum.
    pub(crate) fn fraction(&self, value: f32) -> f32 {
        if self.max > self.min {
            ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The fraction of the range at a control-local point: where along the
    /// thumb's travel its centre would be, clamped to the ends.
    pub(crate) fn fraction_at(&self, local: [f32; 2]) -> f32 {
        let span = self.thumb_centers[1] - self.thumb_centers[0];
        if span == 0.0 {
            return 0.0;
        }
        ((local[self.axis] - self.thumb_centers[0]) / span).clamp(0.0, 1.0)
    }
}

/// A numeric text input as one completed Canvas evaluation read it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiNumberGeometry {
    /// Local `[x, y, width, height]` of its decrement and increment parts,
    /// before the hit's position/scale transform; none without step parts.
    pub steps: Option<[[f32; 4]; 2]>,
}

impl GuiNumberGeometry {
    /// The step part at control-local `local`, if any.
    pub fn step_at(&self, local: [f32; 2]) -> Option<crate::systems::gui::local::GuiNumberStep> {
        use crate::systems::gui::local::GuiNumberStep;
        let rects = self.steps?;
        [GuiNumberStep::Decrement, GuiNumberStep::Increment]
            .into_iter()
            .find(|step| {
                let rect = rects[step.index()];
                (0..2).all(|axis| {
                    local[axis] >= rect[axis] && local[axis] < rect[axis] + rect[axis + 2]
                })
            })
    }
}

/// A control's place in its group, as one completed Canvas evaluation read it
/// from the group's `GuiGroup`, the item's fields and the GUI System.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiGroupItem {
    /// The group: the control's nearest ancestor with a `GuiGroup`.
    pub group: EntityId,
    /// The group's `axis`.
    pub axis: u32,
    /// The group's `selection`.
    pub selection: u32,
    /// The item's `GuiButton.selected`; false for other controls.
    pub selected: bool,
    /// Whether the item is its group's active item.
    pub active: bool,
}

/// Immutable control observation joined by exact CanvasTarget, never a live component lookup.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiControlObservation {
    /// The exact measured single-line layout used by label, caret and pointer selection.
    pub text: Option<(Arc<crate::systems::surface::TextLayout>, [f32; 2])>,
    pub(crate) scroll_bars: [Option<super::super::layout::scroll_bars::GuiScrollBar>; 2],
    /// Innermost authored focus-scope ancestor in this completed logical tree.
    pub focus_scope: Option<crate::EntityId>,
    /// The control's group, when it is an item of one.
    pub group: Option<GuiGroupItem>,
    /// Identity, role, routing value, eligibility and ancestry read from fields.
    pub record: GuiControlRecord,
    /// The same finalized hit geometry carried by the corresponding Canvas output.
    pub hit: CanvasHit,
    /// Presentation-local layout/theme readiness; missing resources omit dependent parts.
    /// Local semantic admission remains independent of presentation readiness.
    pub available: bool,
    /// Range and local painted rail only for a nonempty slider role.
    pub slider: Option<GuiSliderGeometry>,
    /// A numeric text input's step parts; none for every other control.
    pub number: Option<GuiNumberGeometry>,
    /// A colour control's local surfaces, before the hit's position/scale
    /// transform, as its paint placed them.
    pub(crate) color: Option<crate::systems::gui::local::color::GuiColorLayout>,
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

    /// Whether this control holds a scalar value that arrow keys step, and
    /// that the wheel steps while the control holds focus: the slider in
    /// every presentation.
    pub fn steps_value(&self) -> bool {
        matches!(self.record.value, GuiRoutingValue::Scalar(_))
    }

    /// Role-supported operations; acceptance additionally checks eligibility.
    pub fn supported_actions(&self) -> &'static [GuiSemanticActionKind] {
        use GuiSemanticActionKind::*;
        match self.record.kind {
            GuiControlKind::Button => &[Press, Focus, Blur],
            GuiControlKind::Checkbox => &[Toggle, Focus, Blur],
            GuiControlKind::Slider => &[SetScalar, Focus, Blur],
            GuiControlKind::TextInput if self.number.is_some() => &[SetScalar, Focus, Blur],
            GuiControlKind::TextInput => &[SetText, Focus, Blur],
            GuiControlKind::ScrollView => &[ScrollTo, ScrollBy],
            GuiControlKind::VirtualList => &[ScrollTo, ScrollBy, ScrollToIndex],
            GuiControlKind::Color => &[SetColor, Focus, Blur],
        }
    }
}

/// An open overlay as one completed Canvas evaluation placed it.
///
/// Hits and controls belong to the overlay when their root-first ancestry
/// contains its entity, and to its parent's subtree when it contains the
/// parent; [`Self::contains`] reads the first.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiOverlayObservation {
    /// Exact `GuiOverlay` lifetime.
    pub target: GuiEntityTarget,
    /// The parent it is placed against, or none for a top-level overlay
    /// placed against the canvas.
    pub parent: Option<EntityId>,
    /// Its `GuiOverlay.mode`.
    pub mode: u32,
    /// Plane id of the layer it paints on.
    pub layer: u32,
    /// Its ordinal in the canvas's tree-order paint walk, as a hit's
    /// `paint_order`: a control below its layer, or on its layer with a
    /// smaller ordinal, lies under it.
    pub order: u32,
    /// Final canvas bounds of its laid-out box.
    pub bounds: CanvasClip,
}

impl GuiOverlayObservation {
    /// Whether a target with root-first `ancestry` is the overlay or lies
    /// inside it, including in an overlay opened from it.
    pub fn contains(&self, ancestry: &[EntityId]) -> bool {
        ancestry.contains(&self.target.entity)
    }

    /// Whether a target with root-first `ancestry` lies inside the overlay
    /// or its parent's subtree: what a press may land on without being
    /// outside a light overlay, and where focus may move without closing it.
    pub fn holds(&self, ancestry: &[EntityId]) -> bool {
        self.contains(ancestry) || self.parent.is_some_and(|parent| ancestry.contains(&parent))
    }

    /// Whether a modal overlay keeps `control` from input: the control is
    /// not inside it and lies under it, on a lower layer or earlier on its
    /// layer.
    pub fn blocks(&self, control: &GuiControlObservation) -> bool {
        self.mode == crate::components::GuiOverlay::MODE_MODAL
            && !self.contains(&control.record.ancestry)
            && (control.hit.layer, control.hit.paint_order) < (self.layer, self.order)
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
    /// Open, visible overlays in stacking order, the topmost last: by layer,
    /// then tree order. Empty for a canvas without overlays.
    pub overlays: Arc<[GuiOverlayObservation]>,
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
