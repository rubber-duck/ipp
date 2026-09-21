//! A misbehaving extension must not trap the Host or bypass mandatory cleanup.
use ipp_core::systems::{
    System, SystemCommitContext, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemUpdateContext, compiled_system_factories,
};
use ipp_core::{
    Batch, BatchErrorScope, Command, ComponentValue, EntityMetadata, EntityRef, ErrorReason,
    HostRuntime,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

struct OscillatingFactory {
    enabled: Arc<AtomicBool>,
    rounds: Arc<AtomicUsize>,
    updates: Arc<AtomicUsize>,
}

struct OscillatingSystem(Arc<AtomicBool>, Arc<AtomicUsize>, Arc<AtomicUsize>);

impl SystemFactory for OscillatingFactory {
    fn id(&self) -> SystemId {
        SystemId("test.oscillating-commit")
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(OscillatingSystem(
            self.enabled.clone(),
            self.rounds.clone(),
            self.updates.clone(),
        )))
    }
}

impl System for OscillatingSystem {
    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        if !self.0.load(Ordering::Relaxed) {
            return;
        }
        let affected: Vec<_> = context
            .changed_components()
            .filter(|(_, kind)| *kind == ComponentValue::SCALAR)
            .collect();
        for (entity, _) in affected {
            let value = (self.1.fetch_add(1, Ordering::Relaxed) + 1) as f32;
            context.restore_evaluated_component(
                entity,
                ComponentValue::Scalar(ipp_core::components::Scalar {
                    value,
                }),
            );
        }
    }

    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {
        self.2.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn nonconvergent_commit_releases_deleted_entities_and_faults_only_its_world() {
    let enabled = Arc::new(AtomicBool::new(false));
    let rounds = Arc::new(AtomicUsize::new(0));
    let updates = Arc::new(AtomicUsize::new(0));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(OscillatingFactory {
        enabled: enabled.clone(),
        rounds: rounds.clone(),
        updates: updates.clone(),
    }));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let id = host.create_world(Default::default()).unwrap();
    let healthy = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: EntityMetadata::default(),
                },
                Command::Create {
                    alias: 1,
                    metadata: EntityMetadata::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::Scalar(Default::default()),
                },
            ],
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    let ids = report.outcomes[0].result.as_ref().unwrap();
    let deleted = ids[0].1;
    let scalar = ids[1].1;
    enabled.store(true, Ordering::Relaxed);
    updates.store(0, Ordering::Relaxed);
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![
                Command::Delete {
                    entity: EntityRef::Handle(deleted),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Handle(scalar),
                    value: ComponentValue::Scalar(Default::default()),
                },
            ],
        })
        .unwrap();
    world
        .enqueue(Batch {
            id: 3,
            operations: vec![Command::Create {
                alias: 0,
                metadata: EntityMetadata::default(),
            }],
        })
        .unwrap();
    let report = world.step(0.1).unwrap();
    assert_eq!(rounds.load(Ordering::Relaxed), 65);
    assert_eq!(updates.load(Ordering::Relaxed), 0);
    assert_eq!(world.fault(), Some(ErrorReason::NonConvergentCommit));
    assert!(world.inspect(deleted).is_none());
    assert_eq!(world.entities().len(), 1);
    assert_eq!(report.outcomes.len(), 2);
    for outcome in report.outcomes {
        let error = outcome.result.unwrap_err();
        assert_eq!(error.scope, BatchErrorScope::Commit);
        assert_eq!(error.operation, None);
        assert_eq!(error.reason, ErrorReason::NonConvergentCommit);
    }
    assert_eq!(
        world.enqueue(Batch {
            id: 4,
            operations: vec![]
        }),
        Err(ErrorReason::NonConvergentCommit)
    );
    drop(world);
    host.world_mut(healthy).unwrap().step(0.1).unwrap();
    assert_eq!(updates.load(Ordering::Relaxed), 1);
}
