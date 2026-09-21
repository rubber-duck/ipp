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

    /// Prepare a private effective value, including its owned activation resources.
    /// Failure or superseded preparation drops only private resources. The world
    /// installs the result after invalidating bindings; replacing/removing effective
    /// storage releases its old resources through ordinary Rust ownership.
    fn prepare_effective(&self, _max_activation_bytes: usize) -> Result<Self, crate::ErrorReason> {
        Ok(self.clone())
    }

    /// Owned internal allocations in a prepared effective value. Exposed fields
    /// are bounded at ingress. Asset-bearing types check the supplied transient
    /// preparation allowance before allocating.
    fn activation_bytes(&self) -> usize {
        0
    }

    /// Validate field-local semantics after typed replacement.
    fn validate_field(&self, _offset: u32) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Validate a complete authored/effective value before commit.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
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
