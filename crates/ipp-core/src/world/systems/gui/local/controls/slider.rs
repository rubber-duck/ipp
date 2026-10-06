//! Shared slider geometry and pure value mapping.
//!
//! Paint and pointer routing read one contained-thumb rail, so a press at the
//! painted thumb centre maps back to the committed value.
//!
//! # Value-to-position mapping
//!
//! The thumb is a square three quarters of the control's smaller side and
//! stays inside the control. Its centre travels linearly in the value from
//! half a thumb in from the minimum's end of the control, its left or, on a
//! vertical slider, its bottom, to half a thumb in from the opposite end. The
//! middle of the range is therefore at the middle of the control. Clients that
//! place scale ticks, labels or a bipolar slider's origin mark beside the rail
//! use this mapping; the rail and the hit region are the whole control length.
//!
//! # Range
//!
//! A range's two thumbs follow the same mapping, each at its own value, with
//! the fill between their centres ([`GuiSliderRail::fill_rect`] from the
//! lower value to the upper one); the origin does not apply. On a vertical
//! rail the lower value's thumb lies below the upper one's.
//!
//! # Dial
//!
//! A dial draws in the square of the control's smaller side at its top-left
//! corner, the whole control when it is square, so a longer control keeps
//! room beside or beneath the dial for client content such as a readout. The
//! value runs clockwise over a 270-degree sweep from the minimum at half past
//! seven to the maximum at half past four, leaving the gap centred at the
//! bottom, without wrapping. From the outside in: a tick ring half an em in
//! from the square's edge, a quarter-em gap, then the value ring, whose centre
//! line the track and value arcs share and the pointer reaches.
//!
//! A press on a dial leaves its value. Dragging changes it relative to where
//! the press began: upward travel of [`DIAL_DRAG_SIDES`] times the dial's side
//! crosses the whole range, up increasing and down decreasing, and horizontal
//! travel does nothing. The value never jumps to the pointer's angle. A drag
//! that reaches a bound continues from that bound, so reversing direction
//! responds at once. Drags snap to the step as rail drags do.

/// Slider-thumb edge as a fraction of the control's smaller side, which
/// leaves finite centre travel on narrow controls.
const SLIDER_THUMB_EDGE: f32 = 0.75;

/// Shared slider geometry for paint and pointer-to-value routing.
///
/// The contained thumb travels by its centre between `centers[0]` at the
/// minimum and `centers[1]` at the maximum along `axis`; on a vertical rail
/// the maximum is above the minimum, so the second centre is the smaller
/// coordinate. Keeping that interval in one helper prevents pointer-down at a
/// painted thumb centre from changing the committed value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuiSliderRail {
    /// Coordinate index the value runs along: 0 for x, 1 for y.
    axis: usize,
    thumb_edge: f32,
    /// Thumb centre along `axis` at the minimum and at the maximum.
    centers: [f32; 2],
    /// The control's start and length across `axis`.
    across: [f32; 2],
}

impl GuiSliderRail {
    /// Thumb centre along the rail's axis at the minimum and at the maximum.
    pub(crate) fn thumb_centers(self) -> [f32; 2] {
        self.centers
    }

    /// Painted rail: the whole control length, centred across the axis,
    /// `thickness` thick but never thicker than the control.
    pub(crate) fn rail_rect(self, thickness: f32) -> [f32; 4] {
        let start = self.centers[0].min(self.centers[1]) - self.thumb_edge * 0.5;
        self.rect(start, self.travel() + self.thumb_edge, self.band(thickness))
    }

    /// Filled rail of `thickness` between the origin and the committed value,
    /// both as fractions of the range, or nothing while they coincide inside
    /// the rail.
    ///
    /// The value end is the thumb centre. The origin end is the thumb centre
    /// at the origin, except that an origin at a bound starts at that end of
    /// the rail: the rail spans the whole control rectangle, so a fill from
    /// the minimum starts where the rail starts rather than at the first thumb
    /// centre, and at the minimum value it still reaches under half the thumb.
    ///
    /// Decision (ipp-jtst.7): the fill is deliberately not inset by the rail
    /// border. It shares the rail's height and corner radius so the two read
    /// as one shape, and the flush start is the accepted ipp-jtst.2 behavior;
    /// the border stays visible on the unfilled remainder to mark travel.
    pub(crate) fn fill_rect(self, origin: f32, fraction: f32, thickness: f32) -> Option<[f32; 4]> {
        if !origin.is_finite() || !fraction.is_finite() {
            return None;
        }

        // Distances from the minimum's end of the rail towards the maximum.
        let travel = self.travel();
        let thumb = |fraction: f32| self.thumb_edge * 0.5 + fraction.clamp(0.0, 1.0) * travel;
        let from = if origin <= 0.0 {
            0.0
        } else if origin >= 1.0 {
            travel + self.thumb_edge
        } else {
            thumb(origin)
        };
        let to = thumb(fraction);
        let (near, far) = (from.min(to), from.max(to));
        if far <= near {
            return None;
        }

        let half = self.thumb_edge * 0.5;
        let start = if self.axis == 0 {
            self.centers[0] - half + near
        } else {
            self.centers[0] + half - far
        };
        Some(self.rect(start, far - near, self.band(thickness)))
    }

    /// Painted thumb rectangle for a normalized committed value.
    pub(crate) fn thumb_rect(self, fraction: f32) -> Option<[f32; 4]> {
        if !fraction.is_finite() {
            return None;
        }
        let center =
            self.centers[0] + fraction.clamp(0.0, 1.0) * (self.centers[1] - self.centers[0]);
        let across = [
            self.across[0] + (self.across[1] - self.thumb_edge) * 0.5,
            self.thumb_edge,
        ];
        Some(self.rect(center - self.thumb_edge * 0.5, self.thumb_edge, across))
    }

    /// Distance the thumb centre travels from the minimum to the maximum.
    fn travel(self) -> f32 {
        (self.centers[1] - self.centers[0]).abs()
    }

    /// The painted rail's start and thickness across the axis: centred,
    /// `thickness` thick but never thicker than the control.
    fn band(self, thickness: f32) -> [f32; 2] {
        let thickness = thickness.clamp(0.0, self.across[1]);
        [
            self.across[0] + (self.across[1] - thickness) * 0.5,
            thickness,
        ]
    }

    /// The rectangle from `start` for `length` along the axis and over
    /// `across` (start and length) across it.
    fn rect(self, start: f32, length: f32, across: [f32; 2]) -> [f32; 4] {
        if self.axis == 0 {
            [start, across[0], length, across[1]]
        } else {
            [across[0], start, across[1], length]
        }
    }
}

/// Resolve the finite contained-thumb geometry for one retained slider rect
/// whose value runs along `axis`: 0 for x, 1 for y with the minimum at the
/// bottom.
pub(crate) fn slider_rail(rect: [f32; 4], axis: usize) -> Option<GuiSliderRail> {
    if !rect.iter().all(|value| value.is_finite()) || rect[2] <= 0.0 || rect[3] <= 0.0 || axis > 1 {
        return None;
    }
    let thumb_edge = rect[2].min(rect[3]) * SLIDER_THUMB_EDGE;
    let low = rect[axis] + thumb_edge * 0.5;
    let high = rect[axis] + rect[axis + 2] - thumb_edge * 0.5;
    let across = 1 - axis;
    Some(GuiSliderRail {
        axis,
        thumb_edge,
        centers: if axis == 0 {
            [low, high]
        } else {
            [high, low]
        },
        across: [rect[across], rect[across + 2]],
    })
}

/// The dial's first end, at the minimum, in turns clockwise from twelve
/// o'clock: half past seven.
const DIAL_START: f32 = 0.625;

/// The dial's sweep from the minimum to the maximum in turns, clockwise.
pub(crate) const DIAL_SWEEP: f32 = 0.75;

/// Upward pointer travel that moves a dial's value across its whole range,
/// in dial sides: the control's size, and with it the skin's em, sets the
/// drag's sensitivity.
pub(crate) const DIAL_DRAG_SIDES: f32 = 2.5;

/// Margin of the tick ring from the dial square's edge, in ems.
pub(crate) const DIAL_INSET_EMS: f32 = 0.5;

/// Gap between the tick ring and the value ring, in ems.
const DIAL_GAP_EMS: f32 = 0.25;

/// Shared dial geometry for paint and drag routing: the square the dial
/// draws in and the value-to-angle mapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuiSliderDial {
    /// The square's top-left corner.
    origin: [f32; 2],
    /// The square's side: the control's smaller side.
    side: f32,
}

impl GuiSliderDial {
    /// Upward pointer travel that crosses the whole range.
    pub(crate) fn travel(self) -> f32 {
        self.side * DIAL_DRAG_SIDES
    }

    /// The angle of a value fraction in turns clockwise from twelve o'clock.
    pub(crate) fn angle(fraction: f32) -> f32 {
        DIAL_START + fraction.clamp(0.0, 1.0) * DIAL_SWEEP
    }

    /// The whole sweep's start and extent in turns.
    pub(crate) fn sweep() -> [f32; 2] {
        [DIAL_START, DIAL_SWEEP]
    }

    /// Start and extent in turns of the arc between the origin and the value,
    /// both as fractions of the range, or nothing while they coincide.
    pub(crate) fn value_arc(origin: f32, fraction: f32) -> Option<[f32; 2]> {
        if !origin.is_finite() || !fraction.is_finite() {
            return None;
        }
        let [origin, fraction] = [origin, fraction].map(|value| value.clamp(0.0, 1.0));
        let (near, far) = (origin.min(fraction), origin.max(fraction));
        (far > near).then(|| [Self::angle(near), (far - near) * DIAL_SWEEP])
    }

    /// The square around the circle of `radius` about the dial's centre.
    pub(crate) fn square(self, radius: f32) -> [f32; 4] {
        let radius = radius.max(0.0);
        let centre = self.side * 0.5;
        [
            self.origin[0] + centre - radius,
            self.origin[1] + centre - radius,
            2.0 * radius,
            2.0 * radius,
        ]
    }

    /// Outer radius of the tick ring for a control font of `font_size`.
    pub(crate) fn ticks_radius(self, font_size: f32) -> f32 {
        (self.side * 0.5 - DIAL_INSET_EMS * font_size).max(0.0)
    }

    /// Radius of the value ring's centre line inside ticks `tick_length` long
    /// and around a value arc `value_width` thick.
    pub(crate) fn ring_radius(self, font_size: f32, tick_length: f32, value_width: f32) -> f32 {
        let gap = DIAL_GAP_EMS * font_size;
        (self.ticks_radius(font_size) - tick_length - gap - value_width * 0.5).max(0.0)
    }
}

/// The dial of one retained slider rect: the square of its smaller side at
/// its top-left corner.
pub(crate) fn slider_dial(rect: [f32; 4]) -> Option<GuiSliderDial> {
    if !rect.iter().all(|value| value.is_finite()) || rect[2] <= 0.0 || rect[3] <= 0.0 {
        return None;
    }
    Some(GuiSliderDial {
        origin: [rect[0], rect[1]],
        side: rect[2].min(rect[3]),
    })
}

/// A dial drag in progress: the pointer's vertical position where the drag
/// began, or last passed a bound, and the value fraction there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuiDialDrag {
    /// Control-local vertical pointer position.
    pub origin: f32,
    /// Value fraction at `origin`.
    pub start: f32,
}

impl GuiDialDrag {
    /// The value fraction with the pointer at control-local vertical position
    /// `y`, on a dial whose whole range takes `travel`, and the drag that
    /// continues from there: past a bound the fraction stays at the bound and
    /// the drag restarts from the pointer.
    pub(crate) fn turned(self, y: f32, travel: f32) -> Option<(f32, Self)> {
        if !y.is_finite() || !travel.is_finite() || travel <= 0.0 {
            return None;
        }
        let fraction = self.start + (self.origin - y) / travel;
        if !fraction.is_finite() {
            return None;
        }
        if (0.0..=1.0).contains(&fraction) {
            return Some((fraction, self));
        }
        let fraction = fraction.clamp(0.0, 1.0);
        Some((
            fraction,
            Self {
                origin: y,
                start: fraction,
            },
        ))
    }
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

/// Move `current` by `steps` strides of `step` within the range and snap the
/// result to the step's grid from `min`; a zero step moves by a hundredth of
/// the range without snapping.
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
