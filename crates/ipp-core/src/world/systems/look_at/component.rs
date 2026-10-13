use crate::{EntityId, components::schema::ComponentLifecycle};
use ipp_schema_derive::SchemaComponent;

/// Terminal object constraint: local -Z aims at an entity or a World-space
/// point, with World +Y up.
#[repr(C)]
#[derive(Debug, SchemaComponent)]
pub struct LookAt {
    /// Target's generational identity; zero withdraws the entity aim.
    pub target: EntityId,
    /// Whether this declaration contributes an evaluated orientation.
    pub enabled: bool,
    /// Aim at the World-space point below instead of the target entity.
    pub has_point: bool,
    /// World-space aim point X, used when `has_point` is set.
    pub point_x: f32,
    /// World-space aim point Y, used when `has_point` is set.
    pub point_y: f32,
    /// World-space aim point Z, used when `has_point` is set.
    pub point_z: f32,
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
            has_point: false,
            point_x: 0.0,
            point_y: 0.0,
            point_z: 0.0,
            runtime: Default::default(),
        }
    }
}

impl Clone for LookAt {
    fn clone(&self) -> Self {
        Self {
            target: self.target,
            enabled: self.enabled,
            has_point: self.has_point,
            point_x: self.point_x,
            point_y: self.point_y,
            point_z: self.point_z,
            runtime: Default::default(),
        }
    }
}

impl PartialEq for LookAt {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target
            && self.enabled == other.enabled
            && self.has_point == other.has_point
            && self.point_x == other.point_x
            && self.point_y == other.point_y
            && self.point_z == other.point_z
    }
}

impl ComponentLifecycle for LookAt {
    fn accepts_null_entity(offset: u32) -> bool {
        offset == std::mem::offset_of!(Self, target) as u32
    }
}
