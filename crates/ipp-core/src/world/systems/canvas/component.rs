use crate::ErrorReason;
use crate::components::rows::{Rows, SchemaRow};
use crate::components::schema::{ComponentAssetReference, ComponentLifecycle};
use crate::services::asset_management::service::AssetDemandSelection;
use crate::services::asset_management::{drawing::DRAWING_TYPE, font::FONT_TYPE};
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Optional authored placement, tint, rectangular clipping and layer of one
/// Canvas entity. An entity without it paints with the identity style: no
/// translation, unit scale, white tint, full opacity, no clip and its parent's
/// layer. Tint and opacity multiply down the tree; translation and scale
/// compose; ordinary clips intersect through every ancestor.
///
/// `layer` is a nonnegative parent-relative offset. Zero inherits its parent's
/// level; equal sums share one group across ordinary trees in the same canvas.
/// GUI overlays establish independent priority and clipping scopes. Logical
/// sums use u64 so nested u32 offsets do not overflow; occupied groups are
/// published as consecutive u32 physical ranks. Surface layer spacing applies
/// to those ranks rather than authored offsets.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct CanvasStyle {
    /// Parent-local logical horizontal translation.
    pub x: f32,
    /// Parent-local logical vertical translation, positive downwards.
    pub y: f32,
    /// Signed horizontal visual scale.
    pub scale_x: f32,
    /// Signed vertical visual scale.
    pub scale_y: f32,
    /// Straight linear red tint, multiplied into descendants.
    pub red: f32,
    /// Straight linear green tint.
    pub green: f32,
    /// Straight linear blue tint.
    pub blue: f32,
    /// Straight alpha tint.
    pub alpha: f32,
    /// Opacity inherited by descendants.
    pub opacity: f32,
    /// Whether the local clipping rectangle participates in ancestor intersection.
    pub clipped: bool,
    /// Local clipping minimum X.
    pub clip_min_x: f32,
    /// Local clipping minimum Y.
    pub clip_min_y: f32,
    /// Local clipping maximum X.
    pub clip_max_x: f32,
    /// Local clipping maximum Y.
    pub clip_max_y: f32,
    /// Nonnegative offset from the parent's resolved component layer.
    /// Structural: animation never writes it. Zero inherits.
    pub layer: u32,
}

impl Default for CanvasStyle {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            alpha: 1.0,
            opacity: 1.0,
            clipped: false,
            clip_min_x: 0.0,
            clip_min_y: 0.0,
            clip_max_x: 0.0,
            clip_max_y: 0.0,
            layer: 0,
        }
    }
}

impl CanvasStyle {
    /// Compose this style's translation and scale onto a parent's canvas
    /// `position` and `scale`: the translation moves in the parent's scaled
    /// space and the scales multiply.
    pub(in crate::world::systems) fn compose(
        &self,
        position: [f32; 2],
        scale: [f32; 2],
    ) -> ([f32; 2], [f32; 2]) {
        (
            [
                position[0] + scale[0] * self.x,
                position[1] + scale[1] * self.y,
            ],
            [scale[0] * self.scale_x, scale[1] * self.scale_y],
        )
    }
}

impl ComponentLifecycle for CanvasStyle {
    fn animatable_field(offset: u32) -> bool {
        offset != std::mem::offset_of!(Self, layer) as u32
    }

    fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
        // Every field is independently bounded, so a write out of range is
        // refused where it happens.
        self.validate()
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if [
            self.x,
            self.y,
            self.scale_x,
            self.scale_y,
            self.clip_min_x,
            self.clip_min_y,
            self.clip_max_x,
            self.clip_max_y,
        ]
        .iter()
        .all(|value| value.is_finite())
            && [self.red, self.green, self.blue, self.alpha, self.opacity]
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// Evaluated Canvas-local logical bounds of one laid-out entity, after visual
/// transforms and scrolling. The Canvas System writes them each pass for every
/// entity GUI layout places; GUI controls and Canvas leaves require them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, SchemaComponent)]
pub struct CanvasBounds {
    /// Canvas-local logical minimum X.
    pub x: f32,
    /// Canvas-local logical minimum Y, positive downwards.
    pub y: f32,
    /// Logical width.
    pub width: f32,
    /// Logical height.
    pub height: f32,
}

impl ComponentLifecycle for CanvasBounds {
    fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
        self.validate()
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if [self.x, self.y].iter().all(|value| value.is_finite()) {
            nonnegative(&[self.width, self.height])
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// Components every Canvas leaf requires; missing ones are inserted with defaults.
const LEAF_REQUIREMENTS: &[u16] = &[crate::ComponentValue::CANVAS_BOUNDS];

/// Headlessly measured basic text on an ordinary Canvas entity.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct CanvasText {
    /// Preserved Unicode source text.
    pub text: Arc<str>,
    /// Immutable font source.
    pub source: Arc<str>,
    /// Font source variant.
    pub variant: u32,
    /// Logical units per em.
    pub font_size: f32,
}

impl Default for CanvasText {
    fn default() -> Self {
        Self {
            text: Arc::default(),
            source: Arc::default(),
            variant: 0,
            font_size: 0.1,
        }
    }
}

/// A compact client-positioned glyph, independent of entity and painter identities.
#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
pub struct CanvasGlyphRow {
    /// Original immutable font glyph identity.
    pub glyph_id: u32,
    /// Logical origin relative to the entity.
    pub position: [f32; 2],
    /// Optional straight linear RGBA, multiplied by the inherited tint.
    pub color: Option<[f32; 4]>,
}

/// Client-positioned glyphs retained as one compact ordinary component.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct CanvasGlyphRun {
    /// Immutable font source.
    pub source: Arc<str>,
    /// Font source variant.
    pub variant: u32,
    /// Logical units per em.
    pub font_size: f32,
    /// Glyph payload in ascending row-slot order.
    #[schema(rows)]
    pub glyphs: Rows<CanvasGlyphRow>,
}

impl Default for CanvasGlyphRun {
    fn default() -> Self {
        Self {
            source: Arc::default(),
            variant: 0,
            font_size: 0.1,
            glyphs: Rows::default(),
        }
    }
}

/// An immutable quadratic drawing in Canvas-local logical coordinates.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct CanvasDrawing {
    /// Immutable drawing source.
    pub source: Arc<str>,
    /// Drawing source variant.
    pub variant: u32,
}

/// An immutable RGBA bitmap with a logical display size.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct CanvasBitmap {
    /// Immutable bitmap source.
    pub source: Arc<str>,
    /// Bitmap source variant.
    pub variant: u32,
    /// Logical display width before visual scaling.
    pub width: f32,
    /// Logical display height before visual scaling.
    pub height: f32,
}

impl Default for CanvasBitmap {
    fn default() -> Self {
        Self {
            source: Arc::default(),
            variant: 0,
            width: 1.0,
            height: 1.0,
        }
    }
}

/// A resource-independent filled rounded rectangle.
///
/// The box paints white under the inherited tint. On an entity that also has a
/// `GuiSkin` and no control, the skin's resolved Background part paints this box
/// instead, over its size or its layout bounds, so the skin's rows replace the
/// plain fill and these radii.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct CanvasBox {
    /// Logical display width before visual scaling.
    pub width: f32,
    /// Logical display height before visual scaling.
    pub height: f32,
    /// Corner horizontal radius in final logical units.
    pub radius_x: f32,
    /// Corner vertical radius in final logical units.
    pub radius_y: f32,
}

impl Default for CanvasBox {
    fn default() -> Self {
        Self {
            width: 1.0,
            height: 1.0,
            radius_x: 0.0,
            radius_y: 0.0,
        }
    }
}

impl ComponentLifecycle for CanvasBox {
    fn required_components() -> &'static [u16] {
        LEAF_REQUIREMENTS
    }

    fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
        self.validate()
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        nonnegative(&[self.width, self.height, self.radius_x, self.radius_y])
    }
}

/// A custom paint for the fill of this entity's own box: its `CanvasBox`, or the
/// Background part of its skin or control.
///
/// The paint is an immutable shader-definition asset with a paint body, a small
/// fragment function the renderer calls for the fill colour; `properties` are its
/// named inputs, written, animated and saved like custom-material properties. The
/// box's colour stays where it is authored, in the style tint or the skin row, and
/// becomes the function's colour input; with a paint the fill is otherwise solid,
/// so a gradient, colour field or checker of the same part is not drawn. Coverage,
/// border, glow, clip and opacity stay the renderer's. Until the paint is usable,
/// and whenever the renderer cannot admit it, the box draws its colour. The
/// renderer's paint guide, `CANVAS_PAINTS.md`, owns the function interface.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct CanvasPaint {
    /// Immutable shader-definition source with a paint body.
    pub source: Arc<str>,
    /// Selected definition variant.
    pub variant: u32,
    /// Independently editable named paint inputs.
    #[schema(ignore)]
    pub properties: crate::DynamicProperties,
}

impl ComponentLifecycle for CanvasPaint {
    fn supports_numeric_property(offset: u32) -> bool {
        crate::components::dynamic_properties::is_dynamic_field(offset)
            && offset != crate::components::dynamic_properties::DYNAMIC_METADATA
    }

    fn validate_numeric_properties(
        &self,
        fields: &[(u32, crate::components::schema::FieldValue)],
    ) -> Result<(), ErrorReason> {
        // Only existing numeric properties change in place; the source and its
        // variant are resource fields.
        for (offset, field) in fields {
            let crate::components::schema::FieldValue::Dynamic(value) = field else {
                return Err(ErrorReason::InvalidField);
            };
            if !Self::supports_numeric_property(*offset)
                || value.kind() == crate::DynamicPropertyKind::Asset
                || self
                    .properties
                    .get_key(*offset)
                    .is_none_or(|previous| previous.kind() != value.kind())
            {
                return Err(ErrorReason::InvalidField);
            }
            value.validate().map_err(|_| ErrorReason::InvalidValue)?;
        }
        Ok(())
    }

    fn supports_dynamic_properties() -> bool {
        true
    }

    fn dynamic_properties(&self) -> Option<&crate::DynamicProperties> {
        Some(&self.properties)
    }

    fn dynamic_properties_mut(&mut self) -> Option<&mut crate::DynamicProperties> {
        Some(&mut self.properties)
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        crate::services::asset_management::service::validate_source(&self.source)
    }

    /// Dynamic properties are indexed state: each write is checked where it
    /// happens, by its own value.
    fn validates_after_operation() -> bool {
        false
    }

    fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
        self.validate()
    }

    fn asset_references() -> &'static [ComponentAssetReference] {
        &[ComponentAssetReference {
            kind: crate::services::asset_management::shader::SHADER_TYPE.0,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        if !self.source.is_empty() {
            AssetDemandSelection::insert_into(
                demand,
                crate::services::asset_management::shader::SHADER_TYPE,
                &self.source,
                self.variant,
            );
        }
        self.properties.resource_demand(demand);
    }
}

macro_rules! canvas_resource {
    ($component:ty, $kind:path, $validate:expr) => {
        canvas_resource!(
            $component,
            $kind,
            $validate,
            |_: &$component, _: u32| Ok(())
        );
    };
    ($component:ty, $kind:path, $validate:expr, $validate_field:expr) => {
        impl ComponentLifecycle for $component {
            fn required_components() -> &'static [u16] {
                LEAF_REQUIREMENTS
            }

            fn asset_references() -> &'static [ComponentAssetReference] {
                &[ComponentAssetReference {
                    kind: $kind.0,
                    source_offset: std::mem::offset_of!(Self, source) as u32,
                    variant_offset: std::mem::offset_of!(Self, variant) as u32,
                }]
            }

            fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
                if !self.source.is_empty() {
                    AssetDemandSelection::insert_into(demand, $kind, &self.source, self.variant);
                }
            }

            fn validate_field(&self, offset: u32) -> Result<(), ErrorReason> {
                ($validate_field)(self, offset)
            }

            fn validate(&self) -> Result<(), ErrorReason> {
                ($validate)(self)
            }
        }
    };
}

canvas_resource!(
    CanvasText,
    FONT_TYPE,
    |value: &CanvasText| positive(&[value.font_size]),
    |value: &CanvasText, _| positive(&[value.font_size])
);
canvas_resource!(CanvasGlyphRun, FONT_TYPE, |value: &CanvasGlyphRun| {
    positive(&[value.font_size])?;
    for (_, glyph) in value.glyphs.iter() {
        if !glyph.position.iter().all(|value| value.is_finite())
            || glyph.color.is_some_and(|color| {
                color
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            })
        {
            return Err(ErrorReason::InvalidValue);
        }
    }
    Ok(())
});
canvas_resource!(CanvasDrawing, DRAWING_TYPE, |_: &CanvasDrawing| Ok(()));
canvas_resource!(
    CanvasBitmap,
    crate::TEXTURE_TYPE,
    |value: &CanvasBitmap| positive(&[value.width, value.height]),
    |value: &CanvasBitmap, _| positive(&[value.width, value.height])
);

fn positive(values: &[f32]) -> Result<(), ErrorReason> {
    if values.iter().all(|value| value.is_finite() && *value > 0.0) {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

fn nonnegative(values: &[f32]) -> Result<(), ErrorReason> {
    if values
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
    {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layer_is_structural_and_every_other_style_field_animates() {
        let layer = std::mem::offset_of!(CanvasStyle, layer) as u32;
        assert!(!CanvasStyle::animatable_field(layer));
        assert!(CanvasStyle::animatable_field(
            std::mem::offset_of!(CanvasStyle, opacity) as u32
        ));
        assert!(CanvasStyle::animatable_field(
            std::mem::offset_of!(CanvasStyle, clip_max_y) as u32
        ));
    }
}
