//! Component-specific activation and ownership behavior used by generated dispatch.
//!
//! The lifecycle trait stays crate-private; the asset reference declaration is
//! public so generated dispatch can name it through
//! [`super::schema`](crate::components::schema).

/// Typed asset reference fields used for durable remapping. String fields without
/// this declaration remain ordinary authored text, even if they resemble a URI.
#[derive(Clone, Copy, Debug)]
pub struct ComponentAssetReference {
    /// Compiled asset payload type.
    pub kind: u16,
    /// Exposed URI field.
    pub source_offset: u32,
    /// Exposed variant field.
    pub variant_offset: u32,
}

/// Component-specific activation and ownership behavior used by generated dispatch.
pub(crate) trait ComponentLifecycle: Clone {
    /// Shared default components retained while this component is present.
    fn required_components() -> &'static [u16] {
        &[]
    }

    /// Whether animation of any kind, numeric or discrete, may target an exposed
    /// field or property. Components return false for fields with another sole
    /// writer, such as committed control values or rows an evaluator owns.
    fn animatable_field(_offset: u32) -> bool {
        true
    }

    /// Numeric fields eligible for in-place writes without changing resources or runtime identity.
    fn supports_numeric_property(_offset: u32) -> bool {
        false
    }

    /// Opt into existing numeric property updates after validating their complete
    /// candidate. Implementations may inspect the patch without cloning resources.
    fn validate_numeric_properties(
        &self,
        _fields: &[(u32, super::schema::FieldValue)],
    ) -> Result<(), crate::ErrorReason> {
        Err(crate::ErrorReason::InvalidField)
    }

    fn supports_dynamic_properties() -> bool {
        false
    }

    fn dynamic_properties(&self) -> Option<&super::DynamicProperties> {
        None
    }

    fn dynamic_properties_mut(&mut self) -> Option<&mut super::DynamicProperties> {
        None
    }

    /// Typed source/variant pairs. Persistence checks these against resource demand.
    fn asset_references() -> &'static [ComponentAssetReference] {
        &[]
    }

    /// Whether zero represents an omitted reference at this exact entity field.
    fn accepts_null_entity(_offset: u32) -> bool {
        false
    }

    /// Carry phase-owned instance state across an update of the same incarnation.
    /// The previous value is still live after synchronous invalidation. Implementations
    /// may move allocations into this value; they must not clone evaluated buffers.
    fn preserve_runtime(&mut self, _previous: &mut Self) {}

    /// Keep derived storage consistent after a field was replaced, before
    /// field-local validation. Every field write through the registry runs it.
    fn after_field_write(&mut self, _offset: u32) {}

    /// Validate field-local semantics after `offset` was written, independently
    /// of other fields. Ingress applies it to every field write, so it must
    /// accept any other valid field combination.
    fn validate_field(&self, _offset: u32) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Whether ingress validates the complete value after every operation that
    /// changes it. The default suits small components whose invariants span
    /// several fields, such as a camera's clip planes: an operation's writes may
    /// pass through intermediate combinations, and the checked result does not
    /// depend on their order. Components with large or indexed state return
    /// false; [`Self::validate_field`] then checks each write proportionally and
    /// whole insertions still run [`Self::validate`].
    fn validates_after_operation() -> bool {
        true
    }

    /// Validate a complete value: whole insertions, evaluated replacements,
    /// restored persistence and, per [`Self::validates_after_operation`], each
    /// operation's result.
    fn validate(&self) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Add resource selections referenced by this value.
    fn resource_demand(
        &self,
        _demand: &mut std::collections::BTreeSet<
            crate::services::asset_management::service::AssetDemandSelection,
        >,
    ) {
    }
}
