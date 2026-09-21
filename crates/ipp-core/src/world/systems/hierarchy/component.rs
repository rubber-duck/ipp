use crate::{EntityId, components::schema::ComponentLifecycle};
use ipp_schema_derive::SchemaComponent;

/// Explicit non-owning parent relationship; zero means a World-space root.
#[repr(C)]
#[derive(Debug, SchemaComponent)]
pub struct Hierarchy {
    /// Parent's generational identity in this World.
    pub parent: EntityId,
    /// Optional joint index in the parent Skeleton; u32::MAX selects the object.
    pub parent_bone: u32,
    /// Evaluated affine placement, excluded from authoring and persistence.
    #[schema(ignore)]
    pub runtime: Box<HierarchyRuntimeState>,
}

/// Per-instance propagation result. No authored Transform is overwritten.
#[derive(Debug, Default)]
pub struct HierarchyRuntimeState {
    pub(crate) world: Option<crate::systems::geometry::GeometryShapeTransform>,
}

impl Default for Hierarchy {
    fn default() -> Self {
        Self {
            parent: EntityId::from_bits(0),
            parent_bone: u32::MAX,
            runtime: Default::default(),
        }
    }
}

impl Clone for Hierarchy {
    fn clone(&self) -> Self {
        Self {
            parent: self.parent,
            parent_bone: self.parent_bone,
            runtime: Default::default(),
        }
    }
}

impl PartialEq for Hierarchy {
    fn eq(&self, other: &Self) -> bool {
        self.parent == other.parent && self.parent_bone == other.parent_bone
    }
}

impl ComponentLifecycle for Hierarchy {
    fn accepts_null_entity(offset: u32) -> bool {
        offset == std::mem::offset_of!(Self, parent) as u32
    }
}
