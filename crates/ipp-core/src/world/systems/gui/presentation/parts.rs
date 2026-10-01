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
/// The skin base parts are the authored skin identities. Transient text-input
/// paint (caret, selection and provisional composition) has its own parts so
/// every primitive of one entity keeps a distinct identity per Canvas; those
/// parts are paint identities only and resolve their skin through `Label`.
/// All parts remain independent of painter order and generated primitive
/// indices, and the set may grow beyond control parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiPrimitivePart {
    /// Resizable background of the whole entity rectangle.
    Background,
    /// Slider value fill, drawn between the track and thumb.
    Fill,
    /// Text or control label.
    Label,
    /// Drawing or bitmap icon/content.
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
        }
    }
}

/// Base parts in part-index order.
pub const GUI_BASE_PARTS: [GuiPrimitivePart; 9] = [
    GuiPrimitivePart::Background,
    GuiPrimitivePart::Fill,
    GuiPrimitivePart::Label,
    GuiPrimitivePart::Icon,
    GuiPrimitivePart::FocusRing,
    GuiPrimitivePart::ScrollTrackX,
    GuiPrimitivePart::ScrollThumbX,
    GuiPrimitivePart::ScrollTrackY,
    GuiPrimitivePart::ScrollThumbY,
];

/// Index of a base part in [`GUI_BASE_PARTS`].
///
/// Text-input overlay parts are paint identities, not skin parts: they
/// resolve through the `Label` they annotate.
pub const fn base_part_index(part: GuiPrimitivePart) -> u32 {
    match part {
        GuiPrimitivePart::Background => 0,
        GuiPrimitivePart::Fill => 1,
        GuiPrimitivePart::Label
        | GuiPrimitivePart::Caret
        | GuiPrimitivePart::Selection
        | GuiPrimitivePart::Composition => 2,
        GuiPrimitivePart::Icon => 3,
        GuiPrimitivePart::FocusRing => 4,
        GuiPrimitivePart::ScrollTrackX => 5,
        GuiPrimitivePart::ScrollThumbX => 6,
        GuiPrimitivePart::ScrollTrackY => 7,
        GuiPrimitivePart::ScrollThumbY => 8,
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
    ) -> impl Iterator<Item = Self> {
        variant
            .map(|variant| Self::variant(part, state, variant))
            .into_iter()
            .chain([Self::state(part, state), Self::base(part)])
    }
}

/// One skin part property in stable property order. The first
/// [`Self::APPEARANCE_COUNT`] are appearance properties, which every paint part
/// row keeps at the same index; the rest configure transitions.
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
    /// `corner_radius`: Vec2, non-negative.
    CornerRadius,
    /// `border_width`: F32, non-negative.
    BorderWidth,
    /// `border_color`: Vec4 in 0..=1.
    BorderColor,
    /// `fill_mode`: F32; 0 solid, 1 linear, 2 radial.
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
    /// `glow_radius`: F32, non-negative.
    GlowRadius,
    /// `glow_falloff`: F32, non-negative.
    GlowFalloff,
    /// `motion`: Asset; animation clip of the transition into this state.
    Motion,
    /// `duration`: F32, non-negative seconds.
    Duration,
    /// `easing`: F32; 0 linear, 1 smoothstep.
    Easing,
    /// `track`: F32; whole first track, leaving room for every channel.
    Track,
    /// `time`: F32, non-negative clip time of this state's destination.
    Time,
}

impl GuiPartProperty {
    /// Number of part properties.
    pub const COUNT: u32 = 23;

    /// Number of appearance properties, which lead the layout.
    pub const APPEARANCE_COUNT: u32 = 18;

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
        Self::Motion,
        Self::Duration,
        Self::Easing,
        Self::Track,
        Self::Time,
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
            Self::Motion => "motion",
            Self::Duration => "duration",
            Self::Easing => "easing",
            Self::Track => "track",
            Self::Time => "time",
        }
    }

    /// Exact storage type.
    pub const fn kind(self) -> DynamicPropertyKind {
        match self {
            Self::Color
            | Self::BorderColor
            | Self::GradientColor0
            | Self::GradientColor1
            | Self::GlowColor => DynamicPropertyKind::Vec4,
            Self::Scale | Self::CornerRadius | Self::GradientStart | Self::GradientEnd => {
                DynamicPropertyKind::Vec2
            }
            Self::Asset | Self::Motion => DynamicPropertyKind::Asset,
            Self::Opacity
            | Self::AlignX
            | Self::BorderWidth
            | Self::FillMode
            | Self::GradientRadius
            | Self::GlowIntensity
            | Self::GlowRadius
            | Self::GlowFalloff
            | Self::Duration
            | Self::Easing
            | Self::Track
            | Self::Time => DynamicPropertyKind::F32,
        }
    }

    /// Whether this is an appearance property rather than transition motion.
    pub const fn appearance(self) -> bool {
        (self as u32) < Self::APPEARANCE_COUNT
    }

    /// Whether numeric animation may target it: every property except the
    /// asset references.
    pub const fn numeric_animatable(self) -> bool {
        !matches!(self, Self::Asset | Self::Motion)
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
            | P::Duration
            | P::Time,
            DynamicValue::F32(v),
        ) => *v >= 0.0,
        (P::FillMode, DynamicValue::F32(v)) => matches!(*v, 0.0 | 1.0 | 2.0),
        (P::Easing, DynamicValue::F32(v)) => matches!(*v, 0.0 | 1.0),
        (P::Track, DynamicValue::F32(v)) => skin_motion_base_track(*v).is_some(),
        (P::CornerRadius, DynamicValue::Vec2(v)) => v.iter().all(|v| *v >= 0.0),
        (
            P::Color | P::BorderColor | P::GradientColor0 | P::GradientColor1 | P::GlowColor,
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

/// Convert an authored F32 base track only when the colour, opacity and scale
/// tracks fit; motion that also animates alignment checks its fourth track.
pub(crate) fn skin_motion_base_track(value: f32) -> Option<u32> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
        return None;
    }
    let base = u32::try_from(value as u64).ok()?;
    base.checked_add(2)?;
    Some(base)
}

#[cfg(test)]
#[path = "parts_tests.rs"]
mod tests;
