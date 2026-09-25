//! Compiled skin rows of [`GuiRoot`](super::GuiRoot): root-owned theme parts
//! and per-node part state.
//!
//! A theme is a client-chosen `u32` handle owning one [`GuiThemePartRow`] per
//! authored [`GuiPartId`] at slot `theme_slot * GuiPartId::COUNT + part`; the
//! root allocates theme slots monotonically, so a removed theme's slots are
//! never reused within the incarnation. Nodes reference a theme through their
//! `theme` style property instead of copying its parts.
//!
//! A [`GuiPartRow`] holds what one node adds for one base part: overriding
//! appearance, which takes precedence over the theme's resolved value for
//! every state and variant, and the live channels skin transitions animate.
//! Rows are keyed by their `node` and `part` properties, so the lookup index
//! rebuilds from the table on restore.

use super::super::layout::GuiSkinState;
use crate::components::rows::SchemaRow;
use crate::components::schema::FieldError;
use crate::services::asset_management::AssetSource;
use crate::systems::surface::GuiPrimitivePart;
use crate::{DynamicPropertyKind, DynamicValue, ErrorReason};
use std::collections::BTreeMap;

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
    /// A checked checkbox.
    Checked,
    /// An unchecked checkbox.
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

/// One skin part property, in [`GuiThemePartRow`] layout order. The first
/// [`Self::APPEARANCE_COUNT`] are appearance properties, which per-node part
/// rows share at the same index; the rest configure transitions.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiPartProperty {
    /// `color`: Vec4 in 0..=1; the solid fill or stroke colour.
    Color = 0,
    /// `opacity`: F32 in 0..=1.
    Opacity,
    /// `scale`: Vec2.
    Scale,
    /// `align_x`: F32; checkbox indicator placement, clamped when painted.
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

    /// Row layout name, as generated clients see it.
    pub const fn name(self) -> &'static str {
        GuiThemePartRow::LAYOUT.properties[self as usize].name
    }

    /// Exact storage type.
    pub const fn kind(self) -> DynamicPropertyKind {
        GuiThemePartRow::LAYOUT.properties[self as usize].kind
    }

    /// Whether this is an appearance property rather than transition motion.
    pub const fn appearance(self) -> bool {
        (self as u32) < Self::APPEARANCE_COUNT
    }

    /// Whether numeric animation and overlays may target it: every property
    /// except the asset references.
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

/// Appearance and motion of one [`GuiPartId`] of one theme. Every part
/// property is optional; `theme` is the owning theme's handle, the same for
/// every row of a theme slot.
#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
pub struct GuiThemePartRow {
    /// Linear RGBA solid fill or stroke colour.
    pub color: Option<[f32; 4]>,
    /// Opacity multiplier.
    pub opacity: Option<f32>,
    /// Per-axis scale.
    pub scale: Option<[f32; 2]>,
    /// Checkbox indicator placement from -1 (start) to 1 (end).
    pub align_x: Option<f32>,
    /// Drawing or bitmap source.
    pub asset: Option<AssetSource>,
    /// Per-axis corner radii in local Surface metres.
    pub corner_radius: Option<[f32; 2]>,
    /// Border width in local Surface metres.
    pub border_width: Option<f32>,
    /// Straight linear RGBA border colour.
    pub border_color: Option<[f32; 4]>,
    /// 0 solid, 1 linear gradient, 2 radial gradient.
    pub fill_mode: Option<f32>,
    /// Gradient start point (or radial centre) in local shape metres.
    pub gradient_start: Option<[f32; 2]>,
    /// Gradient end point in local shape metres.
    pub gradient_end: Option<[f32; 2]>,
    /// Gradient stop 0 colour.
    pub gradient_color0: Option<[f32; 4]>,
    /// Gradient stop 1 colour.
    pub gradient_color1: Option<[f32; 4]>,
    /// Radial gradient radius in local shape metres.
    pub gradient_radius: Option<f32>,
    /// Glow colour.
    pub glow_color: Option<[f32; 4]>,
    /// Glow intensity multiplier.
    pub glow_intensity: Option<f32>,
    /// Outward glow radius in local Surface metres.
    pub glow_radius: Option<f32>,
    /// Glow falloff exponent.
    pub glow_falloff: Option<f32>,
    /// Animation clip of the transition into this state.
    pub motion: Option<AssetSource>,
    /// Crossfade duration in seconds.
    pub duration: Option<f32>,
    /// Crossfade easing: 0 linear, 1 smoothstep.
    pub easing: Option<f32>,
    /// First of the colour, opacity, scale and alignment tracks.
    pub track: Option<f32>,
    /// Clip time that represents this state's destination.
    pub time: Option<f32>,
    /// Owning theme handle.
    pub theme: u32,
}

impl GuiThemePartRow {
    /// Layout index of the `theme` key property.
    pub const THEME: u32 = GuiPartProperty::COUNT;

    /// Empty row of one theme.
    pub fn for_theme(theme: u32) -> Self {
        Self {
            theme,
            ..Self::default()
        }
    }

    /// Whether no part property is present.
    pub fn is_empty(&self) -> bool {
        GuiPartProperty::ALL
            .into_iter()
            .all(|property| matches!(self.property(property.index()), Ok(None)))
    }

    /// Check every present property against its range.
    pub fn validate(&self) -> Result<(), ErrorReason> {
        for property in GuiPartProperty::ALL {
            if let Some(value) = self
                .property(property.index())
                .map_err(|_| ErrorReason::InvalidField)?
            {
                validate_part_property(property, &value)?;
            }
        }
        Ok(())
    }
}

/// One live channel of a [`GuiPartRow`]: the numeric appearance skin
/// transitions sample, in layout order after the appearance overrides.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiPartChannel {
    /// `live_color`: Vec4 in 0..=1.
    Color = GuiPartProperty::APPEARANCE_COUNT,
    /// `live_opacity`: F32 in 0..=1.
    Opacity,
    /// `live_scale`: Vec2.
    Scale,
    /// `live_align_x`: F32.
    AlignX,
}

impl GuiPartChannel {
    /// Every channel in layout and track order.
    pub const ALL: [Self; 4] = [Self::Color, Self::Opacity, Self::Scale, Self::AlignX];

    /// Layout index in [`GuiPartRow`].
    pub const fn index(self) -> u32 {
        self as u32
    }

    /// Appearance property this channel animates, which also gives its range.
    pub const fn property(self) -> GuiPartProperty {
        match self {
            Self::Color => GuiPartProperty::Color,
            Self::Opacity => GuiPartProperty::Opacity,
            Self::Scale => GuiPartProperty::Scale,
            Self::AlignX => GuiPartProperty::AlignX,
        }
    }
}

/// One property of a [`GuiPartRow`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiPartRowProperty {
    /// An appearance override.
    Override(GuiPartProperty),
    /// A live channel.
    Channel(GuiPartChannel),
    /// The `node` or `part` key.
    Key,
}

impl GuiPartRowProperty {
    /// Number of part row properties.
    pub const COUNT: u32 = GuiPartRow::PART + 1;

    /// Property at a layout index.
    pub const fn from_index(index: u32) -> Option<Self> {
        if index < GuiPartProperty::APPEARANCE_COUNT {
            Some(Self::Override(GuiPartProperty::ALL[index as usize]))
        } else if index < GuiPartRow::NODE {
            Some(Self::Channel(
                GuiPartChannel::ALL[(index - GuiPartProperty::APPEARANCE_COUNT) as usize],
            ))
        } else if index < Self::COUNT {
            Some(Self::Key)
        } else {
            None
        }
    }

    /// Layout index in [`GuiPartRow`]; None for the keys, which share this
    /// variant.
    pub const fn index(self) -> Option<u32> {
        match self {
            Self::Override(property) => Some(property.index()),
            Self::Channel(channel) => Some(channel.index()),
            Self::Key => None,
        }
    }

    /// Whether numeric animation and overlays may target it: numeric
    /// overrides and every channel.
    pub const fn numeric_animatable(self) -> bool {
        match self {
            Self::Override(property) => property.numeric_animatable(),
            Self::Channel(_) => true,
            Self::Key => false,
        }
    }
}

/// What one node adds to one base part: appearance overrides, which take
/// precedence over its theme for every state and variant, and the live
/// channels skin transitions animate. The root keeps the channels present
/// exactly while the node's theme declares motion for the part, so
/// transitions always find them; they are not overrides and paint only
/// while a transition owns them.
#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
pub struct GuiPartRow {
    /// Overriding solid fill or stroke colour.
    pub color: Option<[f32; 4]>,
    /// Overriding opacity.
    pub opacity: Option<f32>,
    /// Overriding scale.
    pub scale: Option<[f32; 2]>,
    /// Overriding checkbox indicator placement.
    pub align_x: Option<f32>,
    /// Overriding drawing or bitmap source.
    pub asset: Option<AssetSource>,
    /// Overriding corner radii.
    pub corner_radius: Option<[f32; 2]>,
    /// Overriding border width.
    pub border_width: Option<f32>,
    /// Overriding border colour.
    pub border_color: Option<[f32; 4]>,
    /// Overriding fill mode.
    pub fill_mode: Option<f32>,
    /// Overriding gradient start.
    pub gradient_start: Option<[f32; 2]>,
    /// Overriding gradient end.
    pub gradient_end: Option<[f32; 2]>,
    /// Overriding gradient stop 0.
    pub gradient_color0: Option<[f32; 4]>,
    /// Overriding gradient stop 1.
    pub gradient_color1: Option<[f32; 4]>,
    /// Overriding radial gradient radius.
    pub gradient_radius: Option<f32>,
    /// Overriding glow colour.
    pub glow_color: Option<[f32; 4]>,
    /// Overriding glow intensity.
    pub glow_intensity: Option<f32>,
    /// Overriding glow radius.
    pub glow_radius: Option<f32>,
    /// Overriding glow falloff.
    pub glow_falloff: Option<f32>,
    /// Live animated colour.
    pub live_color: Option<[f32; 4]>,
    /// Live animated opacity.
    pub live_opacity: Option<f32>,
    /// Live animated scale.
    pub live_scale: Option<[f32; 2]>,
    /// Live animated checkbox indicator placement.
    pub live_align_x: Option<f32>,
    /// Owning node identity.
    pub node: u32,
    /// Base part index in [`GUI_BASE_PARTS`].
    pub part: u32,
}

impl GuiPartRow {
    /// Layout index of the `node` key property.
    pub const NODE: u32 = GuiPartProperty::APPEARANCE_COUNT + 4;

    /// Layout index of the `part` key property.
    pub const PART: u32 = Self::NODE + 1;

    /// Empty row of one node and base part.
    pub fn keyed(node: u32, part: GuiPrimitivePart) -> Self {
        Self {
            node,
            part: base_part_index(part),
            ..Self::default()
        }
    }

    /// Whether any appearance override is present.
    pub fn has_overrides(&self) -> bool {
        (0..GuiPartProperty::APPEARANCE_COUNT)
            .any(|index| matches!(self.property(index), Ok(Some(_))))
    }

    /// Whether every live channel is present.
    pub fn has_channels(&self) -> bool {
        self.live_color.is_some()
            && self.live_opacity.is_some()
            && self.live_scale.is_some()
            && self.live_align_x.is_some()
    }

    /// Make the live channels present, keeping present ones and starting
    /// absent ones from the part's base appearance in `base`, or neutral
    /// values; they paint only while a transition writes them.
    pub fn open_channels(&mut self, base: Option<&GuiThemePartRow>) {
        let base = base.cloned().unwrap_or_default();
        self.live_color
            .get_or_insert(base.color.unwrap_or([1.0, 1.0, 1.0, 1.0]));
        self.live_opacity.get_or_insert(base.opacity.unwrap_or(1.0));
        self.live_scale
            .get_or_insert(base.scale.unwrap_or([1.0, 1.0]));
        self.live_align_x.get_or_insert(base.align_x.unwrap_or(0.0));
    }

    /// Clear every live channel.
    pub fn close_channels(&mut self) {
        self.live_color = None;
        self.live_opacity = None;
        self.live_scale = None;
        self.live_align_x = None;
    }

    /// Check the key, every present override and every present channel.
    pub fn validate(&self) -> Result<(), ErrorReason> {
        if base_part(self.part).is_none() || self.node == 0 {
            return Err(ErrorReason::InvalidField);
        }
        for index in 0..GuiPartRow::NODE {
            let Some(value) = self
                .property(index)
                .map_err(|_| ErrorReason::InvalidField)?
            else {
                continue;
            };
            validate_part_row_property(index, &value)?;
        }
        Ok(())
    }
}

/// Accept a value for one non-key part row property.
pub(crate) fn validate_part_row_property(
    index: u32,
    value: &DynamicValue,
) -> Result<(), ErrorReason> {
    match GuiPartRowProperty::from_index(index) {
        Some(GuiPartRowProperty::Override(property)) => validate_part_property(property, value),
        Some(GuiPartRowProperty::Channel(channel)) => {
            validate_part_property(channel.property(), value)
        }
        Some(GuiPartRowProperty::Key) | None => Err(ErrorReason::InvalidField),
    }
}

/// Sparse edit of one theme part or one node's part overrides: `None` clears
/// a property and `Some` sets it. Properties not listed are preserved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiPartPatch {
    /// Changes in property order.
    pub changes: BTreeMap<GuiPartProperty, Option<DynamicValue>>,
}

impl GuiPartPatch {
    /// Set one property.
    pub fn set(mut self, property: GuiPartProperty, value: DynamicValue) -> Self {
        self.changes.insert(property, Some(value));
        self
    }

    /// Clear one property.
    pub fn clear(mut self, property: GuiPartProperty) -> Self {
        self.changes.insert(property, None);
        self
    }

    /// Whether the patch changes nothing.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Check every set value against its property.
    pub fn validate(&self) -> Result<(), ErrorReason> {
        for (property, value) in &self.changes {
            if let Some(value) = value {
                validate_part_property(*property, value)?;
            }
        }
        Ok(())
    }

    /// Apply the patch to a row whose part properties share
    /// [`GuiPartProperty`] indices.
    pub(crate) fn apply<R: SchemaRow>(&self, row: &mut R) -> Result<(), FieldError> {
        for (property, value) in &self.changes {
            match value {
                Some(value) => row.set_property(property.index(), value.clone())?,
                None => row.clear_property(property.index())?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "part_rows_tests.rs"]
mod tests;
