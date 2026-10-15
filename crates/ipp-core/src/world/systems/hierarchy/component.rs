use crate::components::schema::ComponentLifecycle;
use ipp_schema_derive::SchemaComponent;

/// Joint selection interpreted against the entity's effective same-World parent.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct ParentJoint {
    /// Skeleton-space joint ordinal; u32::MAX uses the parent object frame.
    pub ordinal: u32,
}

impl Default for ParentJoint {
    fn default() -> Self {
        Self {
            ordinal: u32::MAX,
        }
    }
}

impl ComponentLifecycle for ParentJoint {}

/// Ordinary presentation eligibility inherited through effective entity links.
/// Closing a gate preserves simulation and component lifetimes.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct PresentationGate {
    /// Whether this entity branch participates in presentation and input.
    pub open: bool,
}

impl Default for PresentationGate {
    fn default() -> Self {
        Self {
            open: true,
        }
    }
}

impl ComponentLifecycle for PresentationGate {}
