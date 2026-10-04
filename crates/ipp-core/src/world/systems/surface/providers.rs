//! Single registration boundary for typed Surface implementations.

use super::SurfaceGeometry;
use crate::{ComponentValue, components::registry::ComponentStorage};

/// Registered concrete provider component identities.
pub const SURFACE_PROVIDERS: &[u16] = &[
    ComponentValue::FLAT_SURFACE,
    ComponentValue::CYLINDER_SURFACE,
    ComponentValue::SPHERE_SURFACE,
];

/// Whether a component supplies Surface geometry.
pub fn is_provider(component: u16) -> bool {
    SURFACE_PROVIDERS.contains(&component)
}

pub(crate) fn provider(storage: &ComponentStorage, index: usize) -> Option<SurfaceGeometry> {
    storage
        .flat_surface(index)
        .cloned()
        .map(|value| SurfaceGeometry::new(ComponentValue::FLAT_SURFACE, value))
        .or_else(|| {
            storage
                .cylinder_surface(index)
                .cloned()
                .map(|value| SurfaceGeometry::new(ComponentValue::CYLINDER_SURFACE, value))
        })
        .or_else(|| {
            storage
                .sphere_surface(index)
                .cloned()
                .map(|value| SurfaceGeometry::new(ComponentValue::SPHERE_SURFACE, value))
        })
}

/// Prepare immutable geometry from one registered component value.
pub fn from_component(value: &ComponentValue) -> Option<SurfaceGeometry> {
    match value {
        ComponentValue::FlatSurface(surface) => {
            Some(SurfaceGeometry::new(value.type_id(), surface.clone()))
        }
        ComponentValue::CylinderSurface(surface) => {
            Some(SurfaceGeometry::new(value.type_id(), surface.clone()))
        }
        ComponentValue::SphereSurface(surface) => {
            Some(SurfaceGeometry::new(value.type_id(), surface.clone()))
        }
        _ => None,
    }
}

/// Validate a completed provider against live component membership and lifetime.
pub fn publication_is_current(
    world: crate::systems::SystemWorldView<'_>,
    entity: crate::EntityId,
    geometry: &SurfaceGeometry,
    incarnation: Option<u64>,
) -> bool {
    world.component_is_active(entity, geometry.component())
        && world.component_incarnation(entity, geometry.component()) == incarnation
}
