//! Compact GUI paint-part vocabulary shared by themes, skins, motion and paint.
//!
//! A part identity names one paint part of a GUI entity, optionally qualified by
//! an interaction state and a checked variant. Part identities and property
//! indices are stable row keys: theme, skin and motion rows address parts by
//! [`GuiPartId::index`] and appearance lanes by [`GuiPartProperty::index`]. The
//! vocabulary names paint parts rather than control kinds, so richer primitive
//! shapes can extend it without changing existing indices.

use super::part_style::GuiSkinState;
use crate::{DynamicPropertyKind, DynamicValue, ErrorReason};

/// Stable named paint part of one GUI entity's generated primitives.
///
/// The skin base parts are the authored skin identities, including the
/// text-input caret and selection. Provisional composition has its own part so
/// every primitive of one entity keeps a distinct identity per Canvas; that
/// part is a paint identity only and resolves its skin through `Label`, whose
/// text it underlines. All parts remain independent of painter order and
/// generated primitive indices, and the set may grow beyond control parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiPrimitivePart {
    /// Resizable background of the whole entity rectangle.
    Background,
    /// Value fill, drawn between the track and thumb: a slider rail's bar or a
    /// dial's value arc, and a colour control's swatch of its colour.
    Fill,
    /// Text or control label.
    Label,
    /// Drawing or bitmap icon/content; a slider's thumb, a dial's pointer or
    /// a colour control's rail thumb.
    Icon,
    /// Focus indicator painted independently of the background.
    FocusRing,
    /// Text-input caret bar, committed or at the end of a provisional run.
    Caret,
    /// Text-input selection highlight, over committed text or over the
    /// active clause of a provisional composition.
    Selection,
    /// Provisional composition glyph run.
    Composition,
    /// ScrollView horizontal scroll bar track.
    ScrollTrackX,
    /// ScrollView horizontal scroll bar thumb.
    ScrollThumbX,
    /// ScrollView vertical scroll bar track.
    ScrollTrackY,
    /// ScrollView vertical scroll bar thumb.
    ScrollThumbY,
    /// A value control's whole travel where its Background is not the rail:
    /// the track arc beneath a dial's value arc, and a colour control's
    /// saturation-value field and hue and alpha rails, whose fills its value
    /// gives.
    Track,
    /// Marks along a value control's travel: a dial's tick ring.
    Ticks,
    /// A numeric text input's decrement part at the field's start, a division
    /// of its frame that takes its own pointer state.
    Decrement,
    /// A numeric text input's increment part at the field's end.
    Increment,
    /// The mark drawn in the decrement part, a minus stroke.
    DecrementMark,
    /// The mark drawn in the increment part, a plus stroke.
    IncrementMark,
    /// The mark of a point on a two-dimensional travel: a colour control's
    /// marker on its saturation-value field.
    Marker,
}

impl GuiPrimitivePart {
    /// Stable part name; a skin base part's name is its authored skin name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Fill => "fill",
            Self::Label => "label",
            Self::Icon => "icon",
            Self::FocusRing => "focusRing",
            Self::Caret => "caret",
            Self::Selection => "selection",
            Self::Composition => "composition",
            Self::ScrollTrackX => "scrollTrackX",
            Self::ScrollThumbX => "scrollThumbX",
            Self::ScrollTrackY => "scrollTrackY",
            Self::ScrollThumbY => "scrollThumbY",
            Self::Track => "track",
            Self::Ticks => "ticks",
            Self::Decrement => "decrement",
            Self::Increment => "increment",
            Self::DecrementMark => "decrementMark",
            Self::IncrementMark => "incrementMark",
            Self::Marker => "marker",
        }
    }
}

/// Base parts in part-index order.
pub const GUI_BASE_PARTS: [GuiPrimitivePart; 18] = [
    GuiPrimitivePart::Background,
    GuiPrimitivePart::Fill,
    GuiPrimitivePart::Label,
    GuiPrimitivePart::Icon,
    GuiPrimitivePart::FocusRing,
    GuiPrimitivePart::ScrollTrackX,
    GuiPrimitivePart::ScrollThumbX,
    GuiPrimitivePart::ScrollTrackY,
    GuiPrimitivePart::ScrollThumbY,
    GuiPrimitivePart::Caret,
    GuiPrimitivePart::Selection,
    GuiPrimitivePart::Track,
    GuiPrimitivePart::Ticks,
    GuiPrimitivePart::Decrement,
    GuiPrimitivePart::Increment,
    GuiPrimitivePart::DecrementMark,
    GuiPrimitivePart::IncrementMark,
    GuiPrimitivePart::Marker,
];

/// Index of a base part in [`GUI_BASE_PARTS`].
///
/// The provisional composition underline is a paint identity, not a skin
/// part: it resolves through the `Label` it underlines.
pub const fn base_part_index(part: GuiPrimitivePart) -> u32 {
    match part {
        GuiPrimitivePart::Background => 0,
        GuiPrimitivePart::Fill => 1,
        GuiPrimitivePart::Label | GuiPrimitivePart::Composition => 2,
        GuiPrimitivePart::Icon => 3,
        GuiPrimitivePart::FocusRing => 4,
        GuiPrimitivePart::ScrollTrackX => 5,
        GuiPrimitivePart::ScrollThumbX => 6,
        GuiPrimitivePart::ScrollTrackY => 7,
        GuiPrimitivePart::ScrollThumbY => 8,
        GuiPrimitivePart::Caret => 9,
        GuiPrimitivePart::Selection => 10,
        GuiPrimitivePart::Track => 11,
        GuiPrimitivePart::Ticks => 12,
        GuiPrimitivePart::Decrement => 13,
        GuiPrimitivePart::Increment => 14,
        GuiPrimitivePart::DecrementMark => 15,
        GuiPrimitivePart::IncrementMark => 16,
        GuiPrimitivePart::Marker => 17,
    }
}

/// Base part at an index of [`GUI_BASE_PARTS`].
pub const fn base_part(index: u32) -> Option<GuiPrimitivePart> {
    if index < GUI_BASE_PARTS.len() as u32 {
        Some(GUI_BASE_PARTS[index as usize])
    } else {
        None
    }
}

/// Interaction states in qualifier order.
const PART_STATES: [GuiSkinState; 4] = [
    GuiSkinState::Idle,
    GuiSkinState::Hovered,
    GuiSkinState::Pressed,
    GuiSkinState::Disabled,
];

const fn state_index(state: GuiSkinState) -> u32 {
    match state {
        GuiSkinState::Idle => 0,
        GuiSkinState::Hovered => 1,
        GuiSkinState::Pressed => 2,
        GuiSkinState::Disabled => 3,
    }
}

/// Checked/unchecked variant qualifying a state-qualified part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiPartVariant {
    /// A checked selection, such as a checked checkbox.
    Checked,
    /// An unchecked selection, such as an unchecked checkbox.
    Unchecked,
}

/// Enumerated skin part identity: a base part, optionally qualified by an
/// interaction state and, under a state, by a checked variant.
///
/// Resolution looks a property up at (part, state, variant), then (part,
/// state), then (part); the unqualified base part is the least specific. A
/// variant never qualifies a part without a state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GuiPartId {
    /// Stable primitive part.
    pub part: GuiPrimitivePart,
    /// Interaction state, or None for the base part.
    pub state: Option<GuiSkinState>,
    /// Checked variant; only with a state.
    pub variant: Option<GuiPartVariant>,
}

impl GuiPartId {
    /// Qualifiers per base part: the base, four states, and four states with
    /// each of two variants.
    pub const QUALIFIERS: u32 = 13;

    /// Number of part identities.
    pub const COUNT: u32 = GUI_BASE_PARTS.len() as u32 * Self::QUALIFIERS;

    /// The unqualified base part.
    pub const fn base(part: GuiPrimitivePart) -> Self {
        Self {
            part,
            state: None,
            variant: None,
        }
    }

    /// A state-qualified part.
    pub const fn state(part: GuiPrimitivePart, state: GuiSkinState) -> Self {
        Self {
            part,
            state: Some(state),
            variant: None,
        }
    }

    /// A state- and variant-qualified part.
    pub const fn variant(
        part: GuiPrimitivePart,
        state: GuiSkinState,
        variant: GuiPartVariant,
    ) -> Self {
        Self {
            part,
            state: Some(state),
            variant: Some(variant),
        }
    }

    /// Dense index in `0..COUNT`; None for a variant without a state.
    pub const fn index(self) -> Option<u32> {
        let qualifier = match (self.state, self.variant) {
            (None, None) => 0,
            (None, Some(_)) => return None,
            (Some(state), variant) => {
                1 + state_index(state) * 3
                    + match variant {
                        None => 0,
                        Some(GuiPartVariant::Checked) => 1,
                        Some(GuiPartVariant::Unchecked) => 2,
                    }
            }
        };
        Some(base_part_index(self.part) * Self::QUALIFIERS + qualifier)
    }

    /// Part identity at a dense index.
    pub const fn from_index(index: u32) -> Option<Self> {
        let Some(part) = base_part(index / Self::QUALIFIERS) else {
            return None;
        };
        let qualifier = index % Self::QUALIFIERS;
        if qualifier == 0 {
            return Some(Self::base(part));
        }
        let state = PART_STATES[((qualifier - 1) / 3) as usize];
        Some(match (qualifier - 1) % 3 {
            0 => Self::state(part, state),
            1 => Self::variant(part, state, GuiPartVariant::Checked),
            _ => Self::variant(part, state, GuiPartVariant::Unchecked),
        })
    }

    /// Lookup chain for one property, most specific first: (part, state,
    /// variant) when a variant applies, then (part, state), then (part).
    pub fn candidates(
        part: GuiPrimitivePart,
        state: GuiSkinState,
        variant: Option<GuiPartVariant>,
    ) -> impl Iterator<Item = Self> + Clone {
        variant
            .map(|variant| Self::variant(part, state, variant))
            .into_iter()
            .chain([Self::state(part, state), Self::base(part)])
    }
}

/// One skin part appearance property in stable property order, which every
/// paint part row keeps at the same index.
///
/// Lengths are final logical units unless stated otherwise. Per-corner Vec4
/// properties run `[top-left, top-right, bottom-right, bottom-left]` in the part's
/// own orientation, and paint clamps each corner's length so the lengths of two
/// corners never overlap along the side they share.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiPartProperty {
    /// `color`: Vec4 in 0..=1; the solid fill or stroke colour.
    Color = 0,
    /// `opacity`: F32 in 0..=1.
    Opacity,
    /// `scale`: Vec2.
    Scale,
    /// `align_x`: F32; horizontal placement of an indicator part, clamped when painted.
    AlignX,
    /// `asset`: Asset; drawing or bitmap source.
    Asset,
    /// `corner_radius`: Vec2, non-negative; elliptical radii of every uncut corner.
    CornerRadius,
    /// `border_width`: F32, non-negative; border ring width, or stroke thickness.
    BorderWidth,
    /// `border_color`: Vec4 in 0..=1.
    BorderColor,
    /// `fill_mode`: F32; 0 solid, 1 linear, 2 radial, 3 hue, 4 saturation-value.
    /// Hue paints the hue circle along the gradient axis, from red at
    /// `gradient_start` through yellow, green, cyan, blue and magenta to red at
    /// `gradient_end`. Saturation-value paints `fill_hue` over the part rectangle,
    /// saturation rising to the right and value upward, in the part's own
    /// orientation. Both paint the HSV model's opaque sRGB colour at every point,
    /// which `color` does not change; `opacity` and an inherited tint or opacity
    /// apply as to every fill.
    FillMode,
    /// `gradient_start`: Vec2.
    GradientStart,
    /// `gradient_end`: Vec2.
    GradientEnd,
    /// `gradient_color0`: Vec4 in 0..=1.
    GradientColor0,
    /// `gradient_color1`: Vec4 in 0..=1.
    GradientColor1,
    /// `gradient_radius`: F32, non-negative.
    GradientRadius,
    /// `glow_color`: Vec4 in 0..=1.
    GlowColor,
    /// `glow_intensity`: F32, non-negative.
    GlowIntensity,
    /// `glow_radius`: F32, non-negative; outward reach from the outer contour.
    GlowRadius,
    /// `glow_falloff`: F32, non-negative.
    GlowFalloff,
    /// `glow_inner_radius`: F32, non-negative; inward reach from the outer contour,
    /// over the fill and beneath the border.
    GlowInnerRadius,
    /// `corner_cut`: Vec4, non-negative; 45-degree cut length per corner. An uncut
    /// corner keeps `corner_radius`.
    CornerCut,
    /// `corner_accent`: Vec4, non-negative; span per corner, along both edges from
    /// the rectangle's corner, where the border is `corner_accent_width` thick.
    CornerAccent,
    /// `corner_accent_width`: F32, non-negative; border width inside accent spans,
    /// which keep `border_width` while it is absent.
    CornerAccentWidth,
    /// `shape`: F32; 0 box, 1 stroke, 2 arc. A stroke and an arc are painted by
    /// the fill, `border_width` thick, without a border ring.
    Shape,
    /// `stroke_a`: Vec4 in 0..=1; first stroke segment `[x0, y0, x1, y1]`
    /// normalised to the part rectangle, with butt caps. Segments meeting at an end
    /// point leave a notch outside the joint unless one extends past it.
    StrokeA,
    /// `stroke_b`: Vec4 in 0..=1; second stroke segment, painting nothing when
    /// its end points coincide.
    StrokeB,
    /// `arc_start`: F32, any finite value; the arc's first end in turns,
    /// clockwise from twelve o'clock, taken modulo one turn, so a looping clip
    /// from 0 to 1 turns an arc without a jump. Absent is 0. The arc's ring has
    /// its outer edge on the circle inscribed in the shorter side of the part
    /// rectangle and is `border_width` thick, with butt ends.
    ArcStart,
    /// `arc_sweep`: F32, any finite value; the arc's extent in turns from
    /// `arc_start`, clockwise when positive and counter-clockwise when negative.
    /// A whole turn or more is the full ring and zero paints nothing; absent is
    /// the full ring.
    ArcSweep,
    /// `arc_dashes`: Vec2 `[cells, duty]`; dash cells per turn laid from
    /// `arc_start` along the sweep, non-negative, zero for a solid arc, and the
    /// fraction in 0..=1 of each cell its centred dash covers. A whole number of
    /// cells divides a full ring evenly. Absent is solid.
    ArcDashes,
    /// `fill_hue`: F32, any finite value; the saturation-value fill's hue in turns
    /// from red, taken modulo one turn. Absent is red.
    FillHue,
    /// `checker_size`: F32, non-negative; cell side of a checkerboard beneath a
    /// box's fill, inside its border, from the part's top-left corner. Zero or
    /// absent paints none. A translucent fill composites over it in linear light,
    /// its alpha being linear coverage. A box with a checker paints no corner
    /// accents, and strokes and arcs paint no checker.
    CheckerSize,
    /// `checker_color0`: Vec4 in 0..=1; the corner cell's colour. Absent is the
    /// light grey of sRGB `#CCCCCC`.
    CheckerColor0,
    /// `checker_color1`: Vec4 in 0..=1; the other cells' colour. Absent is the
    /// grey of sRGB `#999999`.
    CheckerColor1,
}

impl GuiPartProperty {
    /// Number of part properties.
    pub const COUNT: u32 = 32;

    /// Every property in layout order; `ALL[i] as u32 == i`.
    pub const ALL: [Self; Self::COUNT as usize] = [
        Self::Color,
        Self::Opacity,
        Self::Scale,
        Self::AlignX,
        Self::Asset,
        Self::CornerRadius,
        Self::BorderWidth,
        Self::BorderColor,
        Self::FillMode,
        Self::GradientStart,
        Self::GradientEnd,
        Self::GradientColor0,
        Self::GradientColor1,
        Self::GradientRadius,
        Self::GlowColor,
        Self::GlowIntensity,
        Self::GlowRadius,
        Self::GlowFalloff,
        Self::GlowInnerRadius,
        Self::CornerCut,
        Self::CornerAccent,
        Self::CornerAccentWidth,
        Self::Shape,
        Self::StrokeA,
        Self::StrokeB,
        Self::ArcStart,
        Self::ArcSweep,
        Self::ArcDashes,
        Self::FillHue,
        Self::CheckerSize,
        Self::CheckerColor0,
        Self::CheckerColor1,
    ];

    /// Property at a layout index.
    pub const fn from_index(index: u32) -> Option<Self> {
        if index < Self::COUNT {
            Some(Self::ALL[index as usize])
        } else {
            None
        }
    }

    /// Layout index of this property.
    pub const fn index(self) -> u32 {
        self as u32
    }

    /// Row property name, as generated clients see it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Opacity => "opacity",
            Self::Scale => "scale",
            Self::AlignX => "align_x",
            Self::Asset => "asset",
            Self::CornerRadius => "corner_radius",
            Self::BorderWidth => "border_width",
            Self::BorderColor => "border_color",
            Self::FillMode => "fill_mode",
            Self::GradientStart => "gradient_start",
            Self::GradientEnd => "gradient_end",
            Self::GradientColor0 => "gradient_color0",
            Self::GradientColor1 => "gradient_color1",
            Self::GradientRadius => "gradient_radius",
            Self::GlowColor => "glow_color",
            Self::GlowIntensity => "glow_intensity",
            Self::GlowRadius => "glow_radius",
            Self::GlowFalloff => "glow_falloff",
            Self::GlowInnerRadius => "glow_inner_radius",
            Self::CornerCut => "corner_cut",
            Self::CornerAccent => "corner_accent",
            Self::CornerAccentWidth => "corner_accent_width",
            Self::Shape => "shape",
            Self::StrokeA => "stroke_a",
            Self::StrokeB => "stroke_b",
            Self::ArcStart => "arc_start",
            Self::ArcSweep => "arc_sweep",
            Self::ArcDashes => "arc_dashes",
            Self::FillHue => "fill_hue",
            Self::CheckerSize => "checker_size",
            Self::CheckerColor0 => "checker_color0",
            Self::CheckerColor1 => "checker_color1",
        }
    }

    /// Exact storage type.
    pub const fn kind(self) -> DynamicPropertyKind {
        match self {
            Self::Color
            | Self::BorderColor
            | Self::GradientColor0
            | Self::GradientColor1
            | Self::GlowColor
            | Self::CornerCut
            | Self::CornerAccent
            | Self::StrokeA
            | Self::StrokeB
            | Self::CheckerColor0
            | Self::CheckerColor1 => DynamicPropertyKind::Vec4,
            Self::Scale
            | Self::CornerRadius
            | Self::GradientStart
            | Self::GradientEnd
            | Self::ArcDashes => DynamicPropertyKind::Vec2,
            Self::Asset => DynamicPropertyKind::Asset,
            Self::Opacity
            | Self::AlignX
            | Self::BorderWidth
            | Self::FillMode
            | Self::GradientRadius
            | Self::GlowIntensity
            | Self::GlowRadius
            | Self::GlowFalloff
            | Self::GlowInnerRadius
            | Self::CornerAccentWidth
            | Self::Shape
            | Self::ArcStart
            | Self::ArcSweep
            | Self::FillHue
            | Self::CheckerSize => DynamicPropertyKind::F32,
        }
    }

    /// Whether numeric animation may target it: every property except the
    /// asset reference.
    pub const fn numeric_animatable(self) -> bool {
        !matches!(self, Self::Asset)
    }
}

/// Accept a value for one part property: exact type, finite, valid asset
/// source and the property's range.
pub(crate) fn validate_part_property(
    property: GuiPartProperty,
    value: &DynamicValue,
) -> Result<(), ErrorReason> {
    use GuiPartProperty as P;

    if value.kind() != property.kind() {
        return Err(ErrorReason::InvalidField);
    }
    value.validate().map_err(|_| ErrorReason::InvalidValue)?;

    let unit = |v: &f32| (0.0..=1.0).contains(v);
    let valid = match (property, value) {
        (P::Opacity, DynamicValue::F32(v)) => unit(v),
        (
            P::BorderWidth
            | P::GradientRadius
            | P::GlowIntensity
            | P::GlowRadius
            | P::GlowFalloff
            | P::GlowInnerRadius
            | P::CornerAccentWidth
            | P::CheckerSize,
            DynamicValue::F32(v),
        ) => *v >= 0.0,
        (P::FillMode, DynamicValue::F32(v)) => matches!(*v, 0.0 | 1.0 | 2.0 | 3.0 | 4.0),
        (P::Shape, DynamicValue::F32(v)) => matches!(*v, 0.0 | 1.0 | 2.0),
        (P::CornerRadius, DynamicValue::Vec2(v)) => v.iter().all(|v| *v >= 0.0),
        (P::ArcDashes, DynamicValue::Vec2([cells, duty])) => *cells >= 0.0 && unit(duty),
        (P::CornerCut | P::CornerAccent, DynamicValue::Vec4(v)) => v.iter().all(|v| *v >= 0.0),
        (
            P::Color
            | P::BorderColor
            | P::GradientColor0
            | P::GradientColor1
            | P::GlowColor
            | P::StrokeA
            | P::StrokeB
            | P::CheckerColor0
            | P::CheckerColor1,
            DynamicValue::Vec4(v),
        ) => v.iter().all(unit),
        _ => true,
    };

    if valid {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

#[cfg(test)]
#[path = "parts_tests.rs"]
mod tests;
