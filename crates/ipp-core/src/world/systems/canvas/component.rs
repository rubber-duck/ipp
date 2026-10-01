use crate::ErrorReason;
use crate::components::rows::{Rows, SchemaRow};
use crate::components::schema::{ComponentAssetReference, ComponentLifecycle};
use crate::services::asset_management::service::AssetDemandSelection;
use crate::services::asset_management::{drawing::DRAWING_TYPE, font::FONT_TYPE};
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Optional authored placement, tint and rectangular clipping of one Canvas
/// entity. An entity without it paints with the identity style: no translation,
/// unit scale, white tint, full opacity and no clip. Tint and opacity multiply
/// down the tree; translation and scale compose; clips intersect.
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
        }
    }
}

impl ComponentLifecycle for CanvasStyle {
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
#[cfg(feature = "gui")]
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

#[cfg(feature = "gui")]
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
const LEAF_REQUIREMENTS: &[u16] = &[
    #[cfg(feature = "gui")]
    crate::ComponentValue::CANVAS_BOUNDS,
];

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
