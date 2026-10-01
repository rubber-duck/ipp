//! CameraSystem: factory configuration and exclusively owned per-world state.

use super::CameraSystemState;
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemTeardownContext, SystemUpdateContext,
};

/// Fresh runtime state owned by one World.
#[derive(Default)]
pub struct CameraSystem {
    pub(in crate::world) state: CameraSystemState,
}

impl CameraSystem {
    pub(in crate::world) fn read<'a>(
        &'a self,
        world: &'a crate::world::WorldSimulationState,
    ) -> super::CameraReadAccess<'a> {
        super::CameraReadAccess::new(world, &self.state)
    }

    /// Stable factory and instance identity.
    pub const ID: SystemId = SystemId("ipp.camera");
}

/// Reusable factory; it retains no mutable world state.
#[derive(Default)]
pub struct CameraSystemFactory;

impl SystemFactory for CameraSystemFactory {
    fn id(&self) -> SystemId {
        CameraSystem::ID
    }

    fn capabilities(&self) -> crate::systems::SystemCapabilities {
        crate::systems::SystemCapabilities::new(
            [crate::ComponentValue::CAMERA],
            [crate::systems::WorldOperation::Camera],
        )
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[SystemDependency::Required(SystemId("ipp.geometry"))]
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        context.dependency::<crate::systems::geometry::GeometrySystem>(
            crate::systems::geometry::GeometrySystem::ID,
        )?;
        Ok(Box::new(CameraSystem::default()))
    }
}

impl System for CameraSystem {
    fn command_world_references(
        &self,
        command: &dyn std::any::Any,
        visit: &mut dyn FnMut(crate::WorldRef),
    ) {
        if let Some(command) = command.downcast_ref::<super::CameraNavigationCommand>() {
            command.world_references(visit);
        }
    }

    fn output_component(&self, kind: crate::OutputKind) -> Option<u16> {
        (kind == crate::OutputKind::Camera).then_some(crate::ComponentValue::CAMERA)
    }

    fn publish_output(
        &self,
        world: &crate::WorldContext<'_>,
        output: &mut crate::host::WorldOutputBuilder<'_>,
    ) -> Result<(), crate::ErrorReason> {
        self.publish(world, output)
    }

    fn command(
        &mut self,
        _context: &mut crate::systems::SystemCommandContext<'_>,
        _session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), crate::ErrorReason> {
        if let Some(command) = command.downcast_ref::<super::CameraNavigationCommand>() {
            return command.apply(self, _context);
        }
        if let Some(query) =
            command.downcast_ref::<crate::systems::geometry::GeometryQueryCommand>()
        {
            use crate::systems::geometry::GeometryQueryCommand;
            self.state.pending_queries.push(match *query {
                GeometryQueryCommand::Project {
                    request_id,
                    query,
                } => GeometryQueryCommand::Project {
                    request_id,
                    query,
                },
                GeometryQueryCommand::Pick {
                    request_id,
                    query,
                } => GeometryQueryCommand::Pick {
                    request_id,
                    query,
                },
            });
            return Ok(());
        }
        match command
            .downcast_ref::<super::update::CameraCommand>()
            .ok_or(crate::ErrorReason::InvalidValue)?
        {
            super::update::CameraCommand::Activate(entity) => {
                let result = self.activate(_context.world.world, *entity);
                if let Err(_reason) = &result {
                    crate::diagnostic!(
                        Warn,
                        "[IPP core] camera.activate.reject entity={} reason={_reason}",
                        entity.to_bits()
                    );
                }
                result
            }
            super::update::CameraCommand::Navigate(motion) => {
                let result = self.navigate_active(&mut _context.world, *motion);
                if let Err(_reason) = &result {
                    crate::diagnostic!(Warn, "[IPP core] camera.navigate.reject reason={_reason}");
                }
                result
            }
        }
    }

    fn finish_update(
        &mut self,
        _context: &mut SystemUpdateContext<'_, '_>,
        _report: &mut crate::WorldUpdateReport,
    ) {
        _report
            .camera_state_changes
            .extend(
                self.state
                    .state_changes
                    .drain(..)
                    .map(|changes| crate::CameraStateChange {
                        tick: _report.tick,
                        changes,
                    }),
            );
        for query in std::mem::take(&mut self.state.pending_queries) {
            use crate::systems::geometry::{GeometryQueryAccess, GeometryQueryCommand};
            let queries =
                GeometryQueryAccess::new(_context.world.world, self.read(_context.world.world));
            match query {
                GeometryQueryCommand::Project {
                    request_id,
                    query,
                } => _report
                    .camera_projections
                    .push(crate::CameraProjectOutcome {
                        request_id,
                        tick: _report.tick,
                        camera: self.state.active_camera,
                        result: queries.camera_project(query),
                    }),
                GeometryQueryCommand::Pick {
                    request_id,
                    query,
                } => _report.geometry_picks.push(crate::GeometryPickOutcome {
                    request_id,
                    tick: _report.tick,
                    camera: self.state.active_camera,
                    result: queries.geometry_pick(query),
                }),
            }
        }
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state.active_camera = None;
    }

    fn update(&mut self, _context: &mut SystemUpdateContext<'_, '_>) {}
}
