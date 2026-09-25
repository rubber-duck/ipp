//! Compiled per-node rows of [`GuiRoot`](super::GuiRoot): node style and the
//! scalar kind-specific node data. Both tables use slot = node id, like the
//! node's `node_tree` row, and a node's rows are inserted and removed with it.
//!
//! Property index is declaration order and is mirrored by
//! [`GuiNodeStyleProperty`] and [`GuiNodeDataProperty`]; a row field offset
//! addresses one property of one node.

use super::node_tree::GuiNodeTreeProperty;
use super::nodes::{GuiNodeKind, GuiNodePatch, GuiNodeStyle};
use crate::components::rows::SchemaRow;
use crate::components::schema::FieldError;
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
    /// Handle of the root theme skinning this node; a handle without a live
    /// theme resolves as no theme.
    pub theme: Option<u32>,
    /// Whether this node bounds keyboard traversal: Tab and BackTab from a
    /// focused descendant cycle within the node's subtree.
    pub focus_scope: bool,
}

impl Default for GuiNodeStyleRow {
    fn default() -> Self {
        Self::from(&GuiNodeStyle::default())
    }
}

impl From<&GuiNodeStyle> for GuiNodeStyleRow {
    /// The row holding a complete authored style.
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
            position: style.position,
            scale: style.scale,
            padding: style.padding,
            margin: style.margin,
            theme: style.theme,
            focus_scope: style.focus_scope,
        }
    }
}

impl From<&GuiNodeStyleRow> for GuiNodeStyle {
    /// The authored style a row holds.
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
            position: row.position,
            scale: row.scale,
            theme: row.theme,
            focus_scope: row.focus_scope,
        }
    }
}

impl GuiNodeStyleRow {
    /// Apply a patch's style members, preserving omitted ones; an explicit
    /// clear makes an optional property absent. Node data is not style.
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
        set(&mut self.position, &patch.position);
        set(&mut self.scale, &patch.scale);
        set(&mut self.theme, &patch.theme);
        set(&mut self.focus_scope, &patch.focus_scope);
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

/// One style change of a patch: `None` preserves the property, `Some(None)`
/// clears an optional property and `Some(Some(value))` sets it.
pub type GuiNodeStyleChange = Option<Option<DynamicValue>>;

impl GuiNodePatch {
    /// Style change for one row property, in the row's value representation.
    #[cfg(test)]
    pub(crate) fn style_change(&self, property: GuiNodeStyleProperty) -> GuiNodeStyleChange {
        use crate::components::rows::RowPropertyValue;
        use GuiNodeStyleProperty as P;

        fn required<T: RowPropertyValue>(value: &Option<T>) -> GuiNodeStyleChange {
            value.as_ref().map(|value| Some(value.to_dynamic()))
        }
        fn optional<T: RowPropertyValue>(value: &Option<Option<T>>) -> GuiNodeStyleChange {
            value
                .as_ref()
                .map(|value| value.as_ref().map(RowPropertyValue::to_dynamic))
        }

        match property {
            P::Enabled => required(&self.enabled),
            P::Width => optional(&self.width),
            P::Height => optional(&self.height),
            P::MinWidth => optional(&self.min_width),
            P::MinHeight => optional(&self.min_height),
            P::MaxWidth => optional(&self.max_width),
            P::MaxHeight => optional(&self.max_height),
            P::Flex => optional(&self.flex),
            P::AlignX => optional(&self.align_x),
            P::AlignY => optional(&self.align_y),
            P::Color => required(&self.color),
            P::BackgroundColor => optional(&self.background_color),
            P::Opacity => required(&self.opacity),
            P::FontSize => required(&self.font_size),
            P::Asset => optional(&self.asset),
            P::Position => required(&self.position),
            P::Scale => required(&self.scale),
            P::Padding => optional(&self.padding),
            P::Margin => optional(&self.margin),
            P::Theme => optional(&self.theme),
            P::FocusScope => required(&self.focus_scope),
        }
    }

    /// Replace the style change for one row property. Clearing a required
    /// property or a value of the wrong kind is rejected.
    pub fn set_style_change(
        &mut self,
        property: GuiNodeStyleProperty,
        change: GuiNodeStyleChange,
    ) -> Result<(), FieldError> {
        use crate::components::rows::RowPropertyValue;
        use GuiNodeStyleProperty as P;

        fn required<T: RowPropertyValue>(
            target: &mut Option<T>,
            change: GuiNodeStyleChange,
        ) -> Result<(), FieldError> {
            *target = match change {
                None => None,
                Some(None) => return Err(FieldError::WrongType),
                Some(Some(value)) => Some(T::from_dynamic(value)?),
            };
            Ok(())
        }
        fn optional<T: RowPropertyValue>(
            target: &mut Option<Option<T>>,
            change: GuiNodeStyleChange,
        ) -> Result<(), FieldError> {
            *target = match change {
                None => None,
                Some(None) => Some(None),
                Some(Some(value)) => Some(Some(T::from_dynamic(value)?)),
            };
            Ok(())
        }

        match property {
            P::Enabled => required(&mut self.enabled, change),
            P::Width => optional(&mut self.width, change),
            P::Height => optional(&mut self.height, change),
            P::MinWidth => optional(&mut self.min_width, change),
            P::MinHeight => optional(&mut self.min_height, change),
            P::MaxWidth => optional(&mut self.max_width, change),
            P::MaxHeight => optional(&mut self.max_height, change),
            P::Flex => optional(&mut self.flex, change),
            P::AlignX => optional(&mut self.align_x, change),
            P::AlignY => optional(&mut self.align_y, change),
            P::Color => required(&mut self.color, change),
            P::BackgroundColor => optional(&mut self.background_color, change),
            P::Opacity => required(&mut self.opacity, change),
            P::FontSize => required(&mut self.font_size, change),
            P::Asset => optional(&mut self.asset, change),
            P::Position => required(&mut self.position, change),
            P::Scale => required(&mut self.scale, change),
            P::Padding => optional(&mut self.padding, change),
            P::Margin => optional(&mut self.margin, change),
            P::Theme => optional(&mut self.theme, change),
            P::FocusScope => required(&mut self.focus_scope, change),
        }
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
    /// `theme`: optional U32 theme handle; command-owned.
    Theme,
    /// `focus_scope`: Bool.
    FocusScope,
}

impl GuiNodeStyleProperty {
    /// Number of node style properties.
    pub const COUNT: u32 = 21;

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
        Self::Theme,
        Self::FocusScope,
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

/// Data row with every property absent, as for nodes without scalars.
pub(crate) const EMPTY_NODE_DATA: GuiNodeDataRow = GuiNodeDataRow {
    image_size: None,
    checked: None,
    value: None,
    min: None,
    max: None,
    step: None,
};

impl GuiNodeDataRow {
    /// Data of an Image node.
    pub fn image(size: [f32; 2]) -> Self {
        Self {
            image_size: Some(size),
            ..Self::default()
        }
    }

    /// Data of a Checkbox node.
    pub fn checkbox(checked: bool) -> Self {
        Self {
            checked: Some(checked),
            ..Self::default()
        }
    }

    /// Data of a Slider node.
    pub fn slider(value: f32, min: f32, max: f32, step: f32) -> Self {
        Self {
            value: Some(value),
            min: Some(min),
            max: Some(max),
            step: Some(step),
            ..Self::default()
        }
    }

    /// Keep values of properties `kind` uses, fill newly used ones with
    /// placeholders (unchecked, a unit image, a zero slider) and clear the
    /// rest. The result always passes [`Self::validate_for`] when the kept
    /// values do.
    pub fn conform(&mut self, kind: GuiNodeKind) {
        let used = |property: GuiNodeDataProperty| property.used_by(kind);
        fn keep<T: Copy>(value: &mut Option<T>, used: bool, placeholder: T) {
            *value = match (used, *value) {
                (true, Some(value)) => Some(value),
                (true, None) => Some(placeholder),
                (false, _) => None,
            };
        }

        keep(
            &mut self.image_size,
            used(GuiNodeDataProperty::ImageSize),
            [1.0, 1.0],
        );
        keep(&mut self.checked, used(GuiNodeDataProperty::Checked), false);
        keep(&mut self.value, used(GuiNodeDataProperty::Value), 0.0);
        keep(&mut self.min, used(GuiNodeDataProperty::Min), 0.0);
        keep(&mut self.max, used(GuiNodeDataProperty::Max), 0.0);
        keep(&mut self.step, used(GuiNodeDataProperty::Step), 0.0);
    }

    /// Row properties to write, in an order that keeps `min <= value <= max`
    /// valid after every write when moving from `self` to `target`: widen the
    /// range, set the value, then narrow the range. Unchanged properties are
    /// skipped.
    pub fn write_order(&self, target: &Self) -> impl Iterator<Item = GuiNodeDataProperty> {
        use GuiNodeDataProperty as P;

        let lower = |a: Option<f32>, b: Option<f32>| matches!((a, b), (Some(a), Some(b)) if b < a);
        let widen_min = lower(self.min, target.min);
        let widen_max = lower(target.max, self.max);
        let order = [
            (P::ImageSize, true),
            (P::Checked, true),
            (P::Min, widen_min),
            (P::Max, widen_max),
            (P::Value, true),
            (P::Min, !widen_min),
            (P::Max, !widen_max),
            (P::Step, true),
        ];
        let changed = move |property: GuiNodeDataProperty| {
            self.property(property.index()).ok().flatten()
                != target.property(property.index()).ok().flatten()
        };
        order
            .into_iter()
            .filter(move |&(property, now)| now && changed(property))
            .map(|(property, _)| property)
    }

    /// Whether exactly the properties of `kind` are present.
    pub fn matches_kind(&self, kind: GuiNodeKind) -> bool {
        GuiNodeDataProperty::ALL
            .into_iter()
            .all(|property| self.present(property) == property.used_by(kind))
    }

    /// Whether one property is present.
    pub fn present(&self, property: GuiNodeDataProperty) -> bool {
        match property {
            GuiNodeDataProperty::ImageSize => self.image_size.is_some(),
            GuiNodeDataProperty::Checked => self.checked.is_some(),
            GuiNodeDataProperty::Value => self.value.is_some(),
            GuiNodeDataProperty::Min => self.min.is_some(),
            GuiNodeDataProperty::Max => self.max.is_some(),
            GuiNodeDataProperty::Step => self.step.is_some(),
        }
    }

    /// Check presence for `kind`, each property's range, and the slider rule
    /// `min <= value <= max`.
    pub fn validate_for(&self, kind: GuiNodeKind) -> Result<(), ErrorReason> {
        if !self.matches_kind(kind) {
            return Err(ErrorReason::InvalidField);
        }
        self.validate_values()
    }

    /// Check each present property's range and, when the slider values are
    /// all present, `min <= value <= max`; presence is not checked.
    pub fn validate_values(&self) -> Result<(), ErrorReason> {
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

    /// Whether nodes of `kind` carry this property.
    pub fn used_by(self, kind: GuiNodeKind) -> bool {
        match self {
            Self::ImageSize => kind == GuiNodeKind::Image,
            Self::Checked => kind == GuiNodeKind::Checkbox,
            Self::Value | Self::Min | Self::Max | Self::Step => kind == GuiNodeKind::Slider,
        }
    }
}

/// One property of a node's rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiNodeRowProperty {
    /// A `node_style` property.
    Style(GuiNodeStyleProperty),
    /// A `node_data` property.
    Data(GuiNodeDataProperty),
    /// A `node_tree` property.
    Tree(GuiNodeTreeProperty),
}

impl GuiNodeRowProperty {
    /// Whether numeric animation and overlays may target it: numeric style
    /// properties and `image_size`.
    pub const fn numeric_animatable(self) -> bool {
        match self {
            Self::Style(property) => !matches!(
                property,
                GuiNodeStyleProperty::Enabled
                    | GuiNodeStyleProperty::Asset
                    | GuiNodeStyleProperty::Theme
                    | GuiNodeStyleProperty::FocusScope
            ),
            Self::Data(property) => !property.command_owned(),
            Self::Tree(_) => false,
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
