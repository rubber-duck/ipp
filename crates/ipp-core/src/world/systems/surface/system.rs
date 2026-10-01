use super::Surface;
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
                crate::ComponentValue::SURFACE,
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
    fn update(&mut self, _context: &mut crate::systems::SystemUpdateContext<'_, '_>) {}
}

impl crate::WorldContext<'_> {
    /// Inspect the current completed effective Surface value.
    pub fn surface(&self, entity: crate::EntityId) -> Option<&Surface> {
        self.world.state.entities.get(&entity)?;
        self.world.components.surface(entity.index() as usize)
    }

    /// Conservative centred local rectangle for culling or plane interaction.
    pub fn surface_bounding_geometry(
        &self,
        entity: crate::EntityId,
    ) -> Option<crate::systems::geometry::GeometryShape> {
        Some(self.surface(entity)?.local_bounding_geometry())
    }

    /// Map an entity-local XY plane hit to Surface content coordinates if within bounds.
    pub fn surface_plane_hit(
        &self,
        entity: crate::EntityId,
        entity_x: f32,
        entity_y: f32,
    ) -> Option<[f32; 2]> {
        self.surface(entity)?
            .plane_hit_to_content(entity_x, entity_y)
    }

    /// Map a Surface content point ([0, width] x [0, height], +X right, +Y down) to centred entity-local coordinates.
    pub fn surface_content_to_entity_local(
        &self,
        entity: crate::EntityId,
        x: f32,
        y: f32,
    ) -> Option<[f32; 3]> {
        Some(self.surface(entity)?.content_to_entity_local(x, y))
    }
}
