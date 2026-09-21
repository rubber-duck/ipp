//! Sparse declaration identities, strict bindings and scalar restoration values.

/// State belongs to ConstraintSystem; components retain their sole effective values.
#[derive(Default)]
pub struct ConstraintSystemState {
    pub(super) numeric: Vec<ScalarNumericBinding>,
    pub(super) restores_active: bool,
    pub(super) numeric_dirty: bool,
    pub(in crate::world) declarations:
        std::collections::BTreeMap<crate::EntityId, super::ConstraintDriverIdentity>,
    pub(in crate::world) bindings:
        std::collections::BTreeMap<crate::EntityId, super::ScalarConstraintBinding>,
    pub(in crate::world) restores: std::collections::BTreeMap<
        crate::EntityId,
        crate::world::ComponentStateInstance<crate::components::Scalar>,
    >,
}

/// Prepared in entity-slot order. Scalar and driver slots stay live until hooks
/// clear this vector before the corresponding incarnation changes.
pub(super) struct ScalarNumericBinding {
    pub(super) entity: crate::EntityId,
    pub(super) incarnation: u64,
    pub(super) source: crate::world::component_binding::ComponentBinding<crate::components::Scalar>,
    pub(super) target: crate::world::component_binding::ComponentBinding<crate::components::Scalar>,
    pub(super) driver: crate::world::component_binding::ComponentBinding<super::LinearDriver>,
}
