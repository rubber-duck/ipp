use super::{SurfaceGeometry, provider};
use crate::systems::{System, SystemFactory, SystemId, SystemInitContext, SystemInitError};

/// Owner of the Surface and SurfaceCache component capabilities. Surface
/// content comes from ordinary entities, such as an attached Canvas World.
#[derive(Default)]
pub struct SurfaceSystem;

impl SurfaceSystem {
    /// Stable composition identity.
    pub const ID: SystemId = SystemId("ipp.surface");
}

/// Reusable factory retaining no World state.
#[derive(Default)]
pub struct SurfaceSystemFactory;

impl SystemFactory for SurfaceSystemFactory {
    fn id(&self) -> SystemId {
        SurfaceSystem::ID
    }

    fn capabilities(&self) -> crate::systems::SystemCapabilities {
        crate::systems::SystemCapabilities::new(
            [
                crate::ComponentValue::FLAT_SURFACE,
                crate::ComponentValue::CYLINDER_SURFACE,
                crate::ComponentValue::SPHERE_SURFACE,
                crate::ComponentValue::SURFACE_CACHE,
            ],
            [crate::systems::WorldOperation::Surface],
        )
    }

    fn dependencies(&self) -> &[crate::systems::SystemDependency] {
        &[]
    }

    fn create(
        &self,
        _context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(SurfaceSystem))
    }
}

impl System for SurfaceSystem {
    fn before_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        let (entity, component) = match context.command {
            crate::Command::InsertComponent {
                entity,
                component,
                ..
            } if super::is_provider(*component) => (context.resolve_entity(entity)?, *component),
            crate::Command::InsertComponentValue {
                entity,
                value,
            } if super::is_provider(value.type_id()) => {
                (context.resolve_entity(entity)?, value.type_id())
            }
            _ => return Ok(()),
        };
        if super::SURFACE_PROVIDERS.iter().any(|&other| {
            other != component
                && context
                    .staged
                    .input_value(&context.world_data.components, entity, other)
                    .is_some()
        }) {
            return Err(crate::ErrorReason::InvalidValue);
        }
        Ok(())
    }

    fn update(&mut self, _context: &mut crate::systems::SystemUpdateContext<'_, '_>) {}
}

impl crate::WorldContext<'_> {
    /// Inspect the current completed effective Surface value.
    pub fn surface(&self, entity: crate::EntityId) -> Option<SurfaceGeometry> {
        self.world.state.entities.get(&entity)?;
        provider(&self.world.components, entity.index() as usize)
    }

    /// Conservative local enclosure of the base Surface.
    pub fn surface_bounding_geometry(
        &self,
        entity: crate::EntityId,
    ) -> Option<crate::systems::geometry::GeometryShape> {
        self.surface(entity)?.bounds([0.0, 0.0]).ok()
    }

    /// Sample the base Surface in entity-local coordinates from content metres (+X right, +Y down).
    pub fn surface_content_to_entity_local(
        &self,
        entity: crate::EntityId,
        x: f32,
        y: f32,
    ) -> Option<[f32; 3]> {
        Some(
            self.surface(entity)?
                .sample([f64::from(x), f64::from(y)], 0.0)
                .ok()?
                .position
                .map(|value| value as f32),
        )
    }
}
