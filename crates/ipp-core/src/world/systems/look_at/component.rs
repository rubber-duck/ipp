use crate::{EntityId, components::schema::ComponentLifecycle};
use ipp_schema_derive::SchemaComponent;

/// Terminal object constraint: local -Z aims at an entity, with World +Y up.
#[repr(C)]
#[derive(Debug, SchemaComponent)]
pub struct LookAt {
    /// Target's generational identity; zero withdraws the constraint.
    pub target: EntityId,
    /// Whether this declaration contributes an evaluated orientation.
    pub enabled: bool,
    /// Private local orientation, reconstructed after mutation and load.
    #[schema(ignore)]
    pub runtime: LookAtRuntimeState,
}

/// Only constrained instances retain an evaluated orientation.
#[derive(Debug, Default)]
pub struct LookAtRuntimeState {
    pub(crate) rotation: Option<[f32; 4]>,
    pub(crate) invalid: bool,
}

impl Default for LookAt {
    fn default() -> Self {
        Self {
            target: EntityId::from_bits(0),
            enabled: true,
            runtime: Default::default(),
        }
    }
}

impl Clone for LookAt {
    fn clone(&self) -> Self {
        Self {
            target: self.target,
            enabled: self.enabled,
            runtime: Default::default(),
        }
    }
}

impl PartialEq for LookAt {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target && self.enabled == other.enabled
    }
}

impl ComponentLifecycle for LookAt {
    fn accepts_null_entity(offset: u32) -> bool {
        offset == std::mem::offset_of!(Self, target) as u32
    }
}
