//! Compiled per-node rows of [`GuiRoot`](super::GuiRoot): node style and the
//! scalar kind-specific node data. Both tables use slot = node id, and a node's
//! rows are inserted and removed with the node.
//!
//! Property index is declaration order and is mirrored by
//! [`GuiNodeStyleProperty`] and [`GuiNodeDataProperty`]; a row field offset
//! addresses one property of one node.

use super::nodes::{GuiNodeContent, GuiNodePatch, GuiNodeStyle};
use crate::components::rows::SchemaRow;
use crate::services::asset_management::AssetSource;
use crate::{DynamicPropertyKind, DynamicValue, ErrorReason};

/// Authoritative style of one node, including its visual transform.
///
/// Layout properties cause reflow; `position` and `scale` move paint and hit
/// regions together without reflow.
#[derive(Clone, Debug, PartialEq, SchemaRow)]
pub struct GuiNodeStyleRow {
    /// Effective interactivity; false skips hit testing and activation.
    pub enabled: bool,
    /// Explicit width in local metres.
    pub width: Option<f32>,
    /// Explicit height in local metres.
    pub height: Option<f32>,
    /// Minimum width in local metres.
    pub min_width: Option<f32>,
    /// Minimum height in local metres.
    pub min_height: Option<f32>,
    /// Maximum width in local metres.
    pub max_width: Option<f32>,
    /// Maximum height in local metres.
    pub max_height: Option<f32>,
    /// Main-axis share of a parent Row or Column's remaining space.
    pub flex: Option<f32>,
    /// Alignment X factor from -1.0 (start) to 1.0 (end).
    pub align_x: Option<f32>,
    /// Alignment Y factor from -1.0 (start) to 1.0 (end).
    pub align_y: Option<f32>,
    /// Foreground / text colour (RGBA 0.0..=1.0).
    pub color: [f32; 4],
    /// Background fill colour (RGBA 0.0..=1.0).
    pub background_color: Option<[f32; 4]>,
    /// Content opacity (0.0..=1.0).
    pub opacity: f32,
    /// Font size in local metres per em.
    pub font_size: f32,
    /// Bound asset reference (font, drawing, or image).
    pub asset: Option<AssetSource>,
    /// Visual translation in local metres; never reflows.
    pub position: [f32; 2],
    /// Visual axis-aligned scale; never reflows.
    pub scale: [f32; 2],
    /// Content padding [top, right, bottom, left].
    pub padding: Option<[f32; 4]>,
    /// Outer margin [top, right, bottom, left].
    pub margin: Option<[f32; 4]>,
}

impl Default for GuiNodeStyleRow {
    fn default() -> Self {
        Self::from(&GuiNodeStyle::default())
    }
}

impl From<&GuiNodeStyle> for GuiNodeStyleRow {
    /// Complete row for a style with an identity visual transform.
    fn from(style: &GuiNodeStyle) -> Self {
        Self {
            enabled: style.enabled,
            width: style.width,
            height: style.height,
            min_width: style.min_width,
            min_height: style.min_height,
            max_width: style.max_width,
            max_height: style.max_height,
            flex: style.flex,
            align_x: style.align_x,
            align_y: style.align_y,
            color: style.color,
            background_color: style.background_color,
            opacity: style.opacity,
            font_size: style.font_size,
            asset: style.asset.clone(),
            position: [0.0, 0.0],
            scale: [1.0, 1.0],
            padding: style.padding,
            margin: style.margin,
        }
    }
}

impl From<&GuiNodeStyleRow> for GuiNodeStyle {
    /// Style members of a row; the visual transform stays on the row.
    fn from(row: &GuiNodeStyleRow) -> Self {
        Self {
            enabled: row.enabled,
            width: row.width,
            height: row.height,
            min_width: row.min_width,
            min_height: row.min_height,
            max_width: row.max_width,
            max_height: row.max_height,
            padding: row.padding,
            margin: row.margin,
            flex: row.flex,
            align_x: row.align_x,
            align_y: row.align_y,
            color: row.color,
            background_color: row.background_color,
            opacity: row.opacity,
            font_size: row.font_size,
            asset: row.asset.clone(),
        }
    }
}

impl GuiNodeStyleRow {
    /// Apply a patch's style members, preserving omitted ones; an explicit
    /// clear makes an optional property absent. Content is not row state.
    pub fn apply(&mut self, patch: &GuiNodePatch) {
        fn set<T: Clone>(target: &mut T, value: &Option<T>) {
            if let Some(value) = value {
                *target = value.clone();
            }
        }

        set(&mut self.enabled, &patch.enabled);
        set(&mut self.width, &patch.width);
        set(&mut self.height, &patch.height);
        set(&mut self.min_width, &patch.min_width);
        set(&mut self.min_height, &patch.min_height);
        set(&mut self.max_width, &patch.max_width);
        set(&mut self.max_height, &patch.max_height);
        set(&mut self.flex, &patch.flex);
        set(&mut self.align_x, &patch.align_x);
        set(&mut self.align_y, &patch.align_y);
        set(&mut self.color, &patch.color);
        set(&mut self.background_color, &patch.background_color);
        set(&mut self.opacity, &patch.opacity);
        set(&mut self.font_size, &patch.font_size);
        set(&mut self.asset, &patch.asset);
        set(&mut self.padding, &patch.padding);
        set(&mut self.margin, &patch.margin);
    }

    /// Check every present property against its declared range.
    pub fn validate(&self) -> Result<(), ErrorReason> {
        for property in GuiNodeStyleProperty::ALL {
            let value = self
                .property(property as u32)
                .map_err(|_| ErrorReason::InvalidField)?;
            if let Some(value) = value {
                validate_node_style_property(property, &value)?;
            }
        }

        Ok(())
    }
}

/// Typed index of one [`GuiNodeStyleRow`] property, in layout order.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiNodeStyleProperty {
    /// `enabled`: Bool.
    Enabled = 0,
    /// `width`: optional F32, non-negative.
    Width,
    /// `height`: optional F32, non-negative.
    Height,
    /// `min_width`: optional F32, non-negative.
    MinWidth,
    /// `min_height`: optional F32, non-negative.
    MinHeight,
    /// `max_width`: optional F32, non-negative.
    MaxWidth,
    /// `max_height`: optional F32, non-negative.
    MaxHeight,
    /// `flex`: optional F32, non-negative.
    Flex,
    /// `align_x`: optional F32.
    AlignX,
    /// `align_y`: optional F32.
    AlignY,
    /// `color`: Vec4 in 0..=1.
    Color,
    /// `background_color`: optional Vec4 in 0..=1.
    BackgroundColor,
    /// `opacity`: F32 in 0..=1.
    Opacity,
    /// `font_size`: F32, positive.
    FontSize,
    /// `asset`: optional Asset.
    Asset,
    /// `position`: Vec2.
    Position,
    /// `scale`: Vec2.
    Scale,
    /// `padding`: optional Vec4, non-negative.
    Padding,
    /// `margin`: optional Vec4.
    Margin,
}

impl GuiNodeStyleProperty {
    /// Number of node style properties.
    pub const COUNT: u32 = 19;

    /// Every property in layout order; `ALL[i] as u32 == i`.
    pub const ALL: [Self; Self::COUNT as usize] = [
        Self::Enabled,
        Self::Width,
        Self::Height,
        Self::MinWidth,
        Self::MinHeight,
        Self::MaxWidth,
        Self::MaxHeight,
        Self::Flex,
        Self::AlignX,
        Self::AlignY,
        Self::Color,
        Self::BackgroundColor,
        Self::Opacity,
        Self::FontSize,
        Self::Asset,
        Self::Position,
        Self::Scale,
        Self::Padding,
        Self::Margin,
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
        GuiNodeStyleRow::LAYOUT.properties[self as usize].name
    }

    /// Exact storage type.
    pub const fn kind(self) -> DynamicPropertyKind {
        GuiNodeStyleRow::LAYOUT.properties[self as usize].kind
    }

    /// Whether the property may be absent.
    pub const fn optional(self) -> bool {
        GuiNodeStyleRow::LAYOUT.properties[self as usize].optional
    }
}

/// Accept a value for one node style property: exact type, finite, valid
/// asset source and the property's range.
pub(crate) fn validate_node_style_property(
    property: GuiNodeStyleProperty,
    value: &DynamicValue,
) -> Result<(), ErrorReason> {
    use GuiNodeStyleProperty as P;

    if value.kind() != property.kind() {
        return Err(ErrorReason::InvalidField);
    }
    value.validate().map_err(|_| ErrorReason::InvalidValue)?;

    let unit = |v: &f32| (0.0..=1.0).contains(v);
    let valid = match (property, value) {
        (P::Opacity, DynamicValue::F32(v)) => unit(v),
        (P::FontSize, DynamicValue::F32(v)) => *v > 0.0,
        (
            P::Width
            | P::Height
            | P::MinWidth
            | P::MinHeight
            | P::MaxWidth
            | P::MaxHeight
            | P::Flex,
            DynamicValue::F32(v),
        ) => *v >= 0.0,
        (P::Color | P::BackgroundColor, DynamicValue::Vec4(v)) => v.iter().all(unit),
        (P::Padding, DynamicValue::Vec4(v)) => v.iter().all(|v| *v >= 0.0),
        _ => true,
    };

    if valid {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

/// Scalar kind-specific values of one node. Presence follows the node kind:
/// `image_size` for Image, `checked` for Checkbox, `value`, `min`, `max` and
/// `step` for Slider, nothing otherwise.
///
/// `checked` and `value` are the committed control values; they and the
/// slider range change only through GUI commands. `image_size` is an
/// ordinary numeric property.
#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
pub struct GuiNodeDataRow {
    /// Image display size in local metres; positive.
    pub image_size: Option<[f32; 2]>,
    /// Committed checkbox state.
    pub checked: Option<bool>,
    /// Committed slider value, within `min..=max`.
    pub value: Option<f32>,
    /// Slider minimum.
    pub min: Option<f32>,
    /// Slider maximum; not below `min`.
    pub max: Option<f32>,
    /// Slider step increment, or 0.0 for continuous; non-negative.
    pub step: Option<f32>,
}

impl GuiNodeDataRow {
    /// Scalar parts of authored content: the initial data of a new node.
    pub fn from_content(content: &GuiNodeContent) -> Self {
        match content {
            GuiNodeContent::Image {
                size,
            } => Self {
                image_size: Some(*size),
                ..Self::default()
            },
            GuiNodeContent::Checkbox {
                checked,
            } => Self {
                checked: Some(*checked),
                ..Self::default()
            },
            GuiNodeContent::Slider {
                value,
                min,
                max,
                step,
            } => Self {
                value: Some(*value),
                min: Some(*min),
                max: Some(*max),
                step: Some(*step),
                ..Self::default()
            },
            _ => Self::default(),
        }
    }

    /// Content of `content`'s kind carrying this row's scalar values; None
    /// when presence does not match the kind.
    pub fn to_content(&self, content: &GuiNodeContent) -> Option<GuiNodeContent> {
        if !self.matches_kind(content) {
            return None;
        }

        Some(match content {
            GuiNodeContent::Image {
                ..
            } => GuiNodeContent::Image {
                size: self.image_size?,
            },
            GuiNodeContent::Checkbox {
                ..
            } => GuiNodeContent::Checkbox {
                checked: self.checked?,
            },
            GuiNodeContent::Slider {
                ..
            } => GuiNodeContent::Slider {
                value: self.value?,
                min: self.min?,
                max: self.max?,
                step: self.step?,
            },
            other => other.clone(),
        })
    }

    /// Whether exactly the properties of `content`'s kind are present.
    pub fn matches_kind(&self, content: &GuiNodeContent) -> bool {
        let image = matches!(content, GuiNodeContent::Image { .. });
        let checkbox = matches!(content, GuiNodeContent::Checkbox { .. });
        let slider = matches!(content, GuiNodeContent::Slider { .. });
        self.image_size.is_some() == image
            && self.checked.is_some() == checkbox
            && [self.value, self.min, self.max, self.step]
                .iter()
                .all(|property| property.is_some() == slider)
    }

    /// Check presence for `content`'s kind, each property's range, and the
    /// slider rules `min <= value <= max`.
    pub fn validate_for(&self, content: &GuiNodeContent) -> Result<(), ErrorReason> {
        if !self.matches_kind(content) {
            return Err(ErrorReason::InvalidField);
        }
        for property in GuiNodeDataProperty::ALL {
            let value = self
                .property(property as u32)
                .map_err(|_| ErrorReason::InvalidField)?;
            if let Some(value) = value {
                validate_node_data_property(property, &value)?;
            }
        }
        if let (Some(value), Some(min), Some(max)) = (self.value, self.min, self.max)
            && !(min <= max && (min..=max).contains(&value))
        {
            return Err(ErrorReason::InvalidValue);
        }

        Ok(())
    }
}

/// Typed index of one [`GuiNodeDataRow`] property, in layout order.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiNodeDataProperty {
    /// `image_size`: optional Vec2, positive; Image only.
    ImageSize = 0,
    /// `checked`: optional Bool; Checkbox only; committed value.
    Checked,
    /// `value`: optional F32; Slider only; committed value.
    Value,
    /// `min`: optional F32; Slider only.
    Min,
    /// `max`: optional F32; Slider only.
    Max,
    /// `step`: optional F32, non-negative; Slider only.
    Step,
}

impl GuiNodeDataProperty {
    /// Number of node data properties.
    pub const COUNT: u32 = 6;

    /// Every property in layout order; `ALL[i] as u32 == i`.
    pub const ALL: [Self; Self::COUNT as usize] = [
        Self::ImageSize,
        Self::Checked,
        Self::Value,
        Self::Min,
        Self::Max,
        Self::Step,
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
        GuiNodeDataRow::LAYOUT.properties[self as usize].name
    }

    /// Exact storage type.
    pub const fn kind(self) -> DynamicPropertyKind {
        GuiNodeDataRow::LAYOUT.properties[self as usize].kind
    }

    /// Whether only GUI commands may change it: committed control values and
    /// the slider range they are validated against.
    pub const fn command_owned(self) -> bool {
        !matches!(self, Self::ImageSize)
    }
}

/// One property of a node's rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiNodeRowProperty {
    /// A `node_style` property.
    Style(GuiNodeStyleProperty),
    /// A `node_data` property.
    Data(GuiNodeDataProperty),
}

impl GuiNodeRowProperty {
    /// Whether numeric animation and overlays may target it: numeric style
    /// properties and `image_size`.
    pub const fn numeric_animatable(self) -> bool {
        match self {
            Self::Style(property) => !matches!(
                property,
                GuiNodeStyleProperty::Enabled | GuiNodeStyleProperty::Asset
            ),
            Self::Data(property) => !property.command_owned(),
        }
    }
}

/// A node property addressed by a row field offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GuiNodePropertyRef {
    /// Node whose row holds the property.
    pub node: super::nodes::GuiNodeId,
    /// Row and property.
    pub property: GuiNodeRowProperty,
}

/// Accept a value for one node data property: exact type, finite and the
/// property's own range. Cross-property slider rules belong to the row.
pub(crate) fn validate_node_data_property(
    property: GuiNodeDataProperty,
    value: &DynamicValue,
) -> Result<(), ErrorReason> {
    if value.kind() != property.kind() {
        return Err(ErrorReason::InvalidField);
    }
    value.validate().map_err(|_| ErrorReason::InvalidValue)?;

    let valid = match (property, value) {
        (GuiNodeDataProperty::ImageSize, DynamicValue::Vec2(size)) => size.iter().all(|v| *v > 0.0),
        (GuiNodeDataProperty::Step, DynamicValue::F32(step)) => *step >= 0.0,
        _ => true,
    };

    if valid {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

#[cfg(test)]
#[path = "node_rows_tests.rs"]
mod tests;
