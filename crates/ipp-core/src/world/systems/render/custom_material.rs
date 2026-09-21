//! Per-instance custom shading inputs; shader code belongs to immutable assets.

use crate::{
    DynamicProperties, ErrorReason,
    components::schema::{ComponentLifecycle, SchemaComponent},
};

/// Instance properties and render policy for an immutable shader-definition asset.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct CustomMaterial {
    /// Immutable shader-definition asset source.
    pub source: String,
    /// Selected definition variant.
    pub variant: u32,
    /// 0 = opaque, 1 = cutout, 2 = straight alpha blend.
    pub alpha_mode: u32,
    /// Discard threshold for cutout mode, defaulting to 0.5.
    pub alpha_cutoff: f32,
    /// Expose standard camera, ambient and punctual lighting inputs.
    pub receives_light: bool,
    /// Expose supported shadow reception when lighting is enabled.
    pub receives_shadows: bool,
    /// Participate in shadow rendering; blended materials never cast.
    pub casts_shadows: bool,
    /// Assert that authored bounds contain all custom vertex deformation.
    pub conservative_bounds: bool,
    /// Independently editable numeric and asset properties.
    #[schema(ignore)]
    pub properties: DynamicProperties,
}

impl Default for CustomMaterial {
    fn default() -> Self {
        Self {
            source: String::new(),
            variant: 0,
            alpha_mode: 0,
            alpha_cutoff: 0.5,
            receives_light: false,
            receives_shadows: false,
            casts_shadows: false,
            conservative_bounds: false,
            properties: DynamicProperties::default(),
        }
    }
}

impl ComponentLifecycle for CustomMaterial {
    fn supports_numeric_property(offset: u32) -> bool {
        (crate::components::dynamic_properties::is_dynamic_field(offset)
            && offset != crate::components::dynamic_properties::DYNAMIC_METADATA)
            || [
                std::mem::offset_of!(Self, alpha_mode) as u32,
                std::mem::offset_of!(Self, alpha_cutoff) as u32,
                std::mem::offset_of!(Self, receives_light) as u32,
                std::mem::offset_of!(Self, receives_shadows) as u32,
                std::mem::offset_of!(Self, casts_shadows) as u32,
                std::mem::offset_of!(Self, conservative_bounds) as u32,
            ]
            .contains(&offset)
    }

    fn validate_numeric_properties(
        &self,
        fields: &[(u32, crate::components::schema::FieldValue)],
    ) -> Result<(), ErrorReason> {
        self.validate()?;
        // Validate the scalar candidate on the stack. Resource/descriptor fields
        // cannot change through this API and are validated on the original above.
        let mut scalar = Self {
            source: String::new(),
            variant: self.variant,
            properties: Default::default(),
            alpha_mode: self.alpha_mode,
            alpha_cutoff: self.alpha_cutoff,
            receives_light: self.receives_light,
            receives_shadows: self.receives_shadows,
            casts_shadows: self.casts_shadows,
            conservative_bounds: self.conservative_bounds,
        };
        for (offset, field) in fields {
            if !Self::supports_numeric_property(*offset) {
                return Err(ErrorReason::InvalidField);
            }
            if let crate::components::schema::FieldValue::Dynamic(value) = field {
                if value.kind() == crate::DynamicPropertyKind::Asset {
                    return Err(ErrorReason::InvalidField);
                }
                value.validate().map_err(|_| ErrorReason::InvalidValue)?;
                if self
                    .properties
                    .get_key(*offset)
                    .is_none_or(|previous| previous.kind() != value.kind())
                {
                    return Err(ErrorReason::InvalidField);
                }
            } else {
                scalar
                    .set_field(*offset, field.clone())
                    .map_err(|_| ErrorReason::InvalidField)?;
            }
        }
        scalar.validate()
    }

    fn supports_dynamic_properties() -> bool {
        true
    }

    fn dynamic_properties(&self) -> Option<&DynamicProperties> {
        Some(&self.properties)
    }

    fn dynamic_properties_mut(&mut self) -> Option<&mut DynamicProperties> {
        Some(&mut self.properties)
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        crate::services::asset_management::service::validate_source(&self.source)?;
        if self.alpha_mode > 2
            || !self.alpha_cutoff.is_finite()
            || !(0.0..=1.0).contains(&self.alpha_cutoff)
        {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }

    fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
        self.validate()
    }

    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[crate::components::schema::ComponentAssetReference {
            kind: crate::services::asset_management::shader::SHADER_TYPE.0,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
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
                    crate::services::asset_management::shader::SHADER_TYPE,
                    &self.source,
                    self.variant,
                ),
            );
        }
        self.properties.resource_demand(demand);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DynamicValue, components::schema::FieldValue};

    #[test]
    fn numeric_patches_validate_complete_policy_and_exclude_resource_changes() {
        let mut material = CustomMaterial::default();
        let key = material
            .properties
            .set("color", DynamicValue::Vec3([0.2; 3]))
            .unwrap();
        let alpha = std::mem::offset_of!(CustomMaterial, alpha_cutoff) as u32;
        let mode = std::mem::offset_of!(CustomMaterial, alpha_mode) as u32;
        assert!(
            material
                .validate_numeric_properties(&[
                    (alpha, FieldValue::F32(0.2)),
                    (mode, FieldValue::U32(2)),
                    (key, FieldValue::Dynamic(DynamicValue::Vec3([0.5; 3])))
                ])
                .is_ok()
        );
        for fields in [
            vec![(alpha, FieldValue::F32(1.1))],
            vec![(mode, FieldValue::U32(3))],
            vec![(
                std::mem::offset_of!(CustomMaterial, variant) as u32,
                FieldValue::U32(2),
            )],
            vec![(key, FieldValue::Dynamic(DynamicValue::Bool(false)))],
        ] {
            assert!(material.validate_numeric_properties(&fields).is_err());
        }
        assert_eq!(material.alpha_cutoff, 0.5);
        assert_eq!(
            material.properties.get("color"),
            Some(DynamicValue::Vec3([0.2; 3]))
        );
    }
}
