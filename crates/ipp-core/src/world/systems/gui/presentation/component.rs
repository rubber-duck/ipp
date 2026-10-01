use super::parts::validate_part_property;
use super::{GuiPartId, GuiPartProperty, GuiPartStyle, GuiPrimitivePart};
use crate::components::rows::{Rows, SchemaRow};
use crate::components::schema::{ComponentAssetReference, ComponentLifecycle};
use crate::services::asset_management::service::AssetDemandSelection;
use crate::services::asset_management::{AssetSource, font::FONT_TYPE};
use crate::{EntityId, ErrorReason};
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;
use std::sync::Arc;

/// One compact appearance row. `part` encodes a GuiPartId, not an entity or node.
/// Property lanes retain GuiPartProperty order for ordinary row-addressed animation.
#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
#[allow(missing_docs)]
pub struct GuiPaintPart {
    pub color: Option<[f32; 4]>,
    pub opacity: Option<f32>,
    pub scale: Option<[f32; 2]>,
    pub align_x: Option<f32>,
    pub asset: Option<AssetSource>,
    pub corner_radius: Option<[f32; 2]>,
    pub border_width: Option<f32>,
    pub border_color: Option<[f32; 4]>,
    pub fill_mode: Option<f32>,
    pub gradient_start: Option<[f32; 2]>,
    pub gradient_end: Option<[f32; 2]>,
    pub gradient_color0: Option<[f32; 4]>,
    pub gradient_color1: Option<[f32; 4]>,
    pub gradient_radius: Option<f32>,
    pub glow_color: Option<[f32; 4]>,
    pub glow_intensity: Option<f32>,
    pub glow_radius: Option<f32>,
    pub glow_falloff: Option<f32>,
    pub part: u32,
}

impl GuiPaintPart {
    /// Set a qualified theme part or an unqualified per-control override part.
    pub fn keyed(part: GuiPartId) -> Result<Self, ErrorReason> {
        Ok(Self {
            part: part.index().ok_or(ErrorReason::InvalidValue)?,
            ..Self::default()
        })
    }

    pub(super) fn style(&self) -> GuiPartStyle {
        GuiPartStyle {
            color: self.color,
            opacity: self.opacity,
            scale: self.scale,
            align_x: self.align_x,
            asset: self.asset.clone(),
            corner_radius: self.corner_radius,
            border_width: self.border_width,
            border_color: self.border_color,
            fill_mode: self.fill_mode,
            gradient_start: self.gradient_start,
            gradient_end: self.gradient_end,
            gradient_color0: self.gradient_color0,
            gradient_color1: self.gradient_color1,
            gradient_radius: self.gradient_radius,
            glow_color: self.glow_color,
            glow_intensity: self.glow_intensity,
            glow_radius: self.glow_radius,
            glow_falloff: self.glow_falloff,
        }
    }
}

fn validate_parts(parts: &Rows<GuiPaintPart>, overrides: bool) -> Result<(), ErrorReason> {
    let mut identities = BTreeSet::new();
    for (_, row) in parts.iter() {
        let identity = GuiPartId::from_index(row.part).ok_or(ErrorReason::InvalidValue)?;
        if !identities.insert(row.part) || (overrides && identity.state.is_some()) {
            return Err(ErrorReason::InvalidValue);
        }
        if row.asset.as_ref().is_some_and(|source| {
            !matches!(
                identity.part,
                GuiPrimitivePart::Background
                    | GuiPrimitivePart::Fill
                    | GuiPrimitivePart::Icon
                    | GuiPrimitivePart::FocusRing
            ) || (source.kind != crate::services::asset_management::drawing::DRAWING_TYPE
                && source.kind != crate::TEXTURE_TYPE)
        }) {
            return Err(ErrorReason::InvalidValue);
        }
        for property in GuiPartProperty::ALL
            .into_iter()
            .filter(|property| property.appearance())
        {
            if let Some(value) = row
                .property(property.index())
                .map_err(|_| ErrorReason::InvalidField)?
            {
                validate_part_property(property, &value)?;
            }
        }
    }
    Ok(())
}

fn demand_parts(parts: &Rows<GuiPaintPart>, demand: &mut BTreeSet<AssetDemandSelection>) {
    for (_, row) in parts.iter() {
        if let Some(source) = &row.asset {
            AssetDemandSelection::insert_into(demand, source.kind, &source.uri, source.variant);
        }
    }
}

/// A shared ordinary theme entity. Controls borrow its sparse qualified part rows.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiTheme {
    /// Base/state/variant appearance rows; no entity structure or control values.
    #[schema(rows)]
    pub parts: Rows<GuiPaintPart>,
}

impl ComponentLifecycle for GuiTheme {
    fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
        // A row write is refused where it happens rather than leaving an
        // invalid table behind.
        self.validate()
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        validate_parts(&self.parts, false)
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        demand_parts(&self.parts, demand);
    }
}

/// One control's shared-theme reference and sparse appearance overrides.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiSkin {
    /// Ordinary same-World generational theme entity; zero selects control defaults.
    pub theme: EntityId,
    /// Unqualified part overrides win over every theme state and variant.
    #[schema(rows)]
    pub parts: Rows<GuiPaintPart>,
    /// Sparse internal transition channels, excluded from authoring and persistence.
    #[schema(ignore)]
    pub runtime: super::super::motion::GuiSkinRuntime,
}

impl ComponentLifecycle for GuiSkin {
    fn accepts_null_entity(offset: u32) -> bool {
        offset == std::mem::offset_of!(Self, theme) as u32
    }

    fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
        // A row write is refused where it happens rather than leaving an
        // invalid table behind.
        self.validate()
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        validate_parts(&self.parts, true)
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        demand_parts(&self.parts, demand);
    }
}

/// Label typography inherited down the ordinary entity tree.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct GuiFont {
    /// Immutable font source owned through the ordinary asset-demand lifecycle.
    pub source: Arc<str>,
    /// Source variant.
    pub variant: u32,
    /// Logical units per em, independent of Surface physical dimensions.
    pub font_size: f32,
}

impl Default for GuiFont {
    fn default() -> Self {
        Self {
            source: Arc::default(),
            variant: 0,
            font_size: 14.0,
        }
    }
}

impl ComponentLifecycle for GuiFont {
    fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
        self.validate()
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if self.font_size.is_finite() && self.font_size > 0.0 {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }

    fn asset_references() -> &'static [ComponentAssetReference] {
        &[ComponentAssetReference {
            kind: FONT_TYPE.0,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        if !self.source.is_empty() {
            AssetDemandSelection::insert_into(demand, FONT_TYPE, &self.source, self.variant);
        }
    }
}

#[cfg(test)]
#[path = "component_tests.rs"]
mod tests;
