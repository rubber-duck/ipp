use ipp_schema_derive::SchemaComponent;

/// Linear constraint authored inputs. Runtime bindings remain outside this value.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct LinearDriver {
    /// Source scalar's generational entity identity.
    pub source: crate::EntityId,
    /// Multiplicative factor.
    pub scale: f32,
    /// Additive bias.
    pub bias: f32,
}

impl Default for LinearDriver {
    fn default() -> Self {
        Self {
            source: crate::EntityId::from_bits(0),
            scale: 1.0,
            bias: 0.0,
        }
    }
}

impl crate::components::schema::ComponentLifecycle for LinearDriver {
    fn validate_field(&self, offset: u32) -> Result<(), crate::ErrorReason> {
        let finite = if offset == std::mem::offset_of!(Self, scale) as u32 {
            self.scale.is_finite()
        } else if offset == std::mem::offset_of!(Self, bias) as u32 {
            self.bias.is_finite()
        } else {
            true
        };
        if finite {
            Ok(())
        } else {
            Err(crate::ErrorReason::InvalidValue)
        }
    }

    /// Scale and bias are finite. Evaluated results are not validated: a finite
    /// driver may still overflow, and dependency cycles are diagnosed by the
    /// ConstraintSystem instead of rejecting the batch.
    fn validate(&self) -> Result<(), crate::ErrorReason> {
        if self.scale.is_finite() && self.bias.is_finite() {
            Ok(())
        } else {
            Err(crate::ErrorReason::InvalidValue)
        }
    }
}

/// Pure expression authored inputs. Target is always this component's entity;
/// selectors use generated schema/metadata addresses, never native pointers.
/// Plan, input access, scratch and status are reconstructible System state.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct ExpressionDriver {
    /// Same-World source entity; typed so persistence remaps the handle.
    pub source: crate::EntityId,
    /// Ordinary immutable ExpressionAsset (kind 19) source reference.
    pub expression_source: std::sync::Arc<str>,
    /// Expression asset variant.
    pub expression_variant: u32,
    /// Registered destination component ID, checked to fit u16.
    pub target_component: u32,
    /// Exact field/row address or dynamic metadata key on this entity.
    pub target_offset: u32,
    /// Canonical consumer selectors; see [`super::encode_expression_driver_inputs`].
    pub inputs: Vec<u8>,
}

impl Default for ExpressionDriver {
    fn default() -> Self {
        Self {
            source: crate::EntityId::from_bits(0),
            expression_source: Default::default(),
            expression_variant: 0,
            target_component: u32::from(crate::ComponentValue::SCALAR),
            target_offset: std::mem::offset_of!(crate::components::Scalar, value) as u32,
            inputs: Vec::new(),
        }
    }
}

impl crate::components::schema::ComponentLifecycle for ExpressionDriver {
    fn animatable_field(_offset: u32) -> bool {
        false
    }

    fn accepts_null_entity(offset: u32) -> bool {
        offset == std::mem::offset_of!(Self, source) as u32
    }

    fn validate_field(&self, offset: u32) -> Result<(), crate::ErrorReason> {
        if offset == std::mem::offset_of!(Self, inputs) as u32 {
            super::decode_expression_driver_inputs(&self.inputs)?;
        }
        if offset == std::mem::offset_of!(Self, target_component) as u32 {
            u16::try_from(self.target_component).map_err(|_| crate::ErrorReason::InvalidValue)?;
        }
        if offset == std::mem::offset_of!(Self, expression_source) as u32 {
            crate::services::asset_management::validate_source(&self.expression_source)?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), crate::ErrorReason> {
        super::decode_expression_driver_inputs(&self.inputs)?;
        u16::try_from(self.target_component).map_err(|_| crate::ErrorReason::InvalidValue)?;
        crate::services::asset_management::validate_source(&self.expression_source)
    }

    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[crate::components::schema::ComponentAssetReference {
            kind: crate::services::asset_management::formats::expression::EXPRESSION_TYPE.0,
            source_offset: std::mem::offset_of!(Self, expression_source) as u32,
            variant_offset: std::mem::offset_of!(Self, expression_variant) as u32,
        }]
    }

    fn resource_demand(
        &self,
        demand: &mut std::collections::BTreeSet<
            crate::services::asset_management::AssetDemandSelection,
        >,
    ) {
        if !self.expression_source.is_empty() {
            demand.insert(
                crate::services::asset_management::AssetDemandSelection::new(
                    crate::services::asset_management::formats::expression::EXPRESSION_TYPE,
                    &self.expression_source,
                    self.expression_variant,
                ),
            );
        }
    }
}
