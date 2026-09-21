//! Per-entity mesh, material and texture declarations.

use ipp_schema_derive::SchemaComponent;

/// Opaque, single-sided linear-light RGB factor.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct UnlitMaterial {
    /// Linear red factor in 0..=1.
    pub r: f32,
    /// Linear green factor in 0..=1.
    pub g: f32,
    /// Linear blue factor in 0..=1.
    pub b: f32,
}

impl Default for UnlitMaterial {
    fn default() -> Self {
        Self {
            r: 1.0,
            g: 1.0,
            b: 1.0,
        }
    }
}

impl crate::components::schema::ComponentLifecycle for UnlitMaterial {
    fn validate(&self) -> Result<(), crate::ErrorReason> {
        if [self.r, self.g, self.b]
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        {
            Ok(())
        } else {
            Err(crate::ErrorReason::InvalidValue)
        }
    }
}

/// Named immutable mesh and per-entity variant selection.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct MeshInstance {
    /// Owned source URI; empty means no selection.
    pub source: String,
    /// Per-entity mesh variant.
    pub variant: u32,
}

impl crate::components::schema::ComponentLifecycle for MeshInstance {
    fn required_components() -> &'static [u16] {
        &[crate::ComponentValue::BOUNDING_GEOMETRY]
    }

    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[crate::components::schema::ComponentAssetReference {
            kind: 1,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
    }

    fn validate_field(&self, offset: u32) -> Result<(), crate::ErrorReason> {
        if offset == std::mem::offset_of!(Self, source) as u32 {
            crate::services::asset_management::service::validate_source(&self.source)?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), crate::ErrorReason> {
        crate::services::asset_management::service::validate_source(&self.source)
    }

    fn resource_demand(
        &self,
        demand: &mut std::collections::BTreeSet<
            crate::services::asset_management::service::AssetDemandSelection,
        >,
    ) {
        if !self.source.is_empty() {
            demand.insert(
                crate::services::asset_management::service::AssetDemandSelection::new(
                    crate::AssetResourceKind::Mesh,
                    &self.source,
                    self.variant,
                ),
            );
        }
    }
}

/// Legacy base-color texture selection; prefer `BaseColorTexture`.
/// Ignored for rendering when `BaseColorTexture` is also present.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct UnlitTexture {
    /// Owned source URI; empty means no selection.
    pub source: String,
    /// Per-entity texture variant.
    pub variant: u32,
}

impl crate::components::schema::ComponentLifecycle for UnlitTexture {
    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[crate::components::schema::ComponentAssetReference {
            kind: 2,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
    }

    fn validate_field(&self, offset: u32) -> Result<(), crate::ErrorReason> {
        if offset == std::mem::offset_of!(Self, source) as u32 {
            crate::services::asset_management::service::validate_source(&self.source)?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), crate::ErrorReason> {
        crate::services::asset_management::service::validate_source(&self.source)
    }

    fn resource_demand(
        &self,
        demand: &mut std::collections::BTreeSet<
            crate::services::asset_management::service::AssetDemandSelection,
        >,
    ) {
        if !self.source.is_empty() {
            demand.insert(
                crate::services::asset_management::service::AssetDemandSelection::new(
                    crate::AssetResourceKind::Texture,
                    &self.source,
                    self.variant,
                ),
            );
        }
    }
}

/// Optional immutable base-color image for either PBR or unlit shading.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct BaseColorTexture {
    /// Owned source URI; empty means no selection.
    pub source: String,
    /// Per-entity texture variant.
    pub variant: u32,
}

impl crate::components::schema::ComponentLifecycle for BaseColorTexture {
    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[crate::components::schema::ComponentAssetReference {
            kind: 2,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
    }

    fn validate_field(&self, offset: u32) -> Result<(), crate::ErrorReason> {
        if offset == std::mem::offset_of!(Self, source) as u32 {
            crate::services::asset_management::service::validate_source(&self.source)?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), crate::ErrorReason> {
        crate::services::asset_management::service::validate_source(&self.source)
    }

    fn resource_demand(
        &self,
        demand: &mut std::collections::BTreeSet<
            crate::services::asset_management::service::AssetDemandSelection,
        >,
    ) {
        if !self.source.is_empty() {
            demand.insert(
                crate::services::asset_management::service::AssetDemandSelection::new(
                    crate::AssetResourceKind::Texture,
                    &self.source,
                    self.variant,
                ),
            );
        }
    }
}
