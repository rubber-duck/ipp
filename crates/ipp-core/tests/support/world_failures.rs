//! Fixture System producing real World failures through the public System factory boundary.
//!
//! A failed publication keeps the World's previous completed publication, so tests can
//! observe a retained contribution after the World has already applied and evaluated later
//! edits. A diverging commit faults the World that changed a Scalar, without affecting others.

use ipp_core::components::Scalar;
use ipp_core::systems::{
    System, SystemCommitContext, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemUpdateContext, compiled_system_factories,
};
use ipp_core::{
    ComponentValue, ErrorReason, HostRuntime, WorldContext, WorldId, WorldOutputBuilder,
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Targets {
    publication: Option<WorldId>,
    diverging_scalars: bool,
}

/// Selects which failures the fixture System produces from now on.
#[derive(Clone, Default)]
pub struct WorldFailures(Arc<Mutex<Targets>>);

impl WorldFailures {
    /// Fail every later publication of `world`; `None` lets every World publish.
    pub fn fail_publication(&self, world: Option<WorldId>) {
        self.0.lock().unwrap().publication = world;
    }

    /// Make any commit that changes a Scalar never converge, faulting its World.
    pub fn diverge_scalar_commits(&self, enabled: bool) {
        self.0.lock().unwrap().diverging_scalars = enabled;
    }
}

/// The fixture System's identity. Only a World that selects it produces failures.
pub const FAILURES: SystemId = SystemId("fixture.world-failures");

/// The named parts plus the failure fixture, registered after every compiled System.
pub fn select_with_failures(parts: &[&[SystemId]]) -> Vec<SystemId> {
    let mut selected = super::selection::select(parts);
    selected.push(FAILURES);
    selected
}

struct Factory(WorldFailures);

struct FailingSystem {
    failures: WorldFailures,
    rounds: usize,
}

impl SystemFactory for Factory {
    fn id(&self) -> SystemId {
        FAILURES
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(FailingSystem {
            failures: self.0.clone(),
            rounds: 0,
        }))
    }
}

impl System for FailingSystem {
    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        if !self.failures.0.lock().unwrap().diverging_scalars {
            return;
        }

        let changed: Vec<_> = context
            .changed_components()
            .filter(|(_, component)| *component == ComponentValue::SCALAR)
            .map(|(entity, _)| entity)
            .collect();
        for entity in changed {
            self.rounds += 1;
            context.restore_evaluated_component(
                entity,
                ComponentValue::Scalar(Scalar {
                    value: self.rounds as f32,
                }),
            );
        }
    }

    fn publish_output(
        &self,
        world: &WorldContext<'_>,
        _: &mut WorldOutputBuilder<'_>,
    ) -> Result<(), ErrorReason> {
        if self.failures.0.lock().unwrap().publication == Some(world.id()) {
            Err(ErrorReason::InvalidValue)
        } else {
            Ok(())
        }
    }
}

/// A Host registering every compiled System plus the failure fixture.
pub fn host_with_world_failures() -> (HostRuntime, WorldFailures) {
    let failures = WorldFailures::default();
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Factory(failures.clone())));
    (
        HostRuntime::with_system_factories(factories).unwrap(),
        failures,
    )
}
