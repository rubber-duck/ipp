//! Release timing and graph-work assertions over the real core mutation paths.
use crate::components::LookAt;
use crate::systems::look_at::LOOK_AT_CHECKS;
use crate::world::entity_links::VISITS;
use crate::{
    Batch, Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef, ErrorReason,
    HostRuntime,
};
use std::time::Instant;

/// Transforms, terminal aiming and final propagation, which maintain the graph.
const SPATIAL_SYSTEMS: &[crate::systems::SystemId] = &[
    crate::systems::hierarchy::HierarchySystem::ID,
    crate::systems::look_at::LookAtSystem::ID,
    crate::systems::hierarchy::FinalPropagationSystem::ID,
];

const SCALING_SIZES: &[usize] = &[1_000, 4_000, 16_000];

fn measure<T>(size: usize, shape: &str, operation: &str, f: impl FnOnce() -> T) -> T {
    VISITS.set(0);
    let start = Instant::now();
    let result = f();
    let visits = VISITS.get();
    println!(
        "scaling entities={size} shape={shape} operation={operation} elapsed_us={} graph_visits={visits}",
        start.elapsed().as_micros()
    );
    assert!(
        visits <= size * 16,
        "{operation} revisited the graph {visits} times for {size} entities"
    );
    result
}

fn apply(host: &mut HostRuntime, world: crate::WorldId, operations: Vec<Command>) -> Vec<EntityId> {
    let mut world = host.world_mut(world).unwrap();
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    world
        .step(0.0)
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()
        .into_iter()
        .map(|(_, id)| id)
        .collect()
}

#[test]
fn maintained_scaling_flat_and_deep_creation_restore_and_removal() {
    for &size in SCALING_SIZES {
        for deep in [false, true] {
            let shape = if deep {
                "deep"
            } else {
                "flat"
            };
            let mut host = HostRuntime::default();
            let world = host
                .create_world(Default::default(), SPATIAL_SYSTEMS)
                .unwrap();
            let entities = measure(size, shape, "create", || {
                apply(
                    &mut host,
                    world,
                    (0..size)
                        .map(|alias| Command::Create {
                            alias: alias as u32,
                            metadata: Default::default(),
                            adopt: false,
                        })
                        .collect(),
                )
            });
            measure(size, shape, "parent", || {
                apply(
                    &mut host,
                    world,
                    entities
                        .iter()
                        .enumerate()
                        .map(|(i, &entity)| Command::PlaceEntity {
                            entity: EntityRef::Handle(entity),
                            placement: EntityPlacementRef {
                                parent: if deep && i > 0 {
                                    Some(EntityRef::Handle(entities[i - 1]))
                                } else {
                                    None
                                },
                                before: None,
                            },
                        })
                        .collect(),
                )
            });
            let limits = Default::default();
            let bytes = measure(size, shape, "save", || {
                host.save_world(world, 42, limits).unwrap()
            });
            let restored = measure(size, shape, "restore", || {
                host.load_world(
                    &bytes,
                    42,
                    crate::services::world_serialization::WorldLoadOptions {
                        symbolic_id: Some("restored".into()),
                        ..Default::default()
                    },
                    Default::default(),
                    limits,
                )
                .unwrap()
                .root
                .id()
            });
            let restored_entities = host.world_mut(restored).unwrap().entities();
            assert_eq!(restored_entities.len(), size);
            assert_eq!(
                restored_entities
                    .iter()
                    .filter(|entity| entity.link.parent.is_some())
                    .count(),
                if deep {
                    size - 1
                } else {
                    0
                }
            );
            host.world_mut(restored).unwrap().step(0.0).unwrap();
            measure(size, shape, "delete-root-first", || {
                apply(
                    &mut host,
                    world,
                    entities
                        .iter()
                        .map(|&entity| Command::Delete {
                            entity: EntityRef::Handle(entity),
                        })
                        .collect(),
                )
            });
            assert!(host.world_mut(world).unwrap().entities().is_empty());
        }
    }
}

#[test]
fn maintained_scaling_sparse_look_at_edit_keeps_unrelated_graph_work_constant() {
    for &size in SCALING_SIZES {
        let mut host = HostRuntime::default();
        let world = host
            .create_world(Default::default(), SPATIAL_SYSTEMS)
            .unwrap();
        let entities = apply(
            &mut host,
            world,
            (0..size as u32)
                .map(|alias| Command::Create {
                    alias,
                    metadata: Default::default(),
                    adopt: false,
                })
                .collect(),
        );
        apply(
            &mut host,
            world,
            entities[1..]
                .iter()
                .map(|&entity| {
                    Command::insert_value(
                        EntityRef::Handle(entity),
                        ComponentValue::LookAt(LookAt {
                            target: entities[0],
                            ..Default::default()
                        }),
                    )
                })
                .collect(),
        );
        VISITS.set(0);
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(entities[1]),
                ComponentValue::LookAt(LookAt {
                    target: entities[0],
                    enabled: false,
                    ..Default::default()
                }),
            )],
        );
        assert_eq!(
            VISITS.get(),
            0,
            "a LookAt edit must not rebuild the hierarchy"
        );
    }
}

#[test]
fn maintained_scaling_dependency_placement_and_correction_touch_only_their_readers() {
    for &size in SCALING_SIZES {
        let mut host = HostRuntime::default();
        let world = host
            .create_world(Default::default(), SPATIAL_SYSTEMS)
            .unwrap();
        let entities = apply(
            &mut host,
            world,
            (0..size as u32)
                .map(|alias| Command::Create {
                    alias,
                    metadata: Default::default(),
                    adopt: false,
                })
                .collect(),
        );
        let tracker = entities[0];
        let target = entities[1];
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(tracker),
                ComponentValue::LookAt(LookAt {
                    target,
                    ..Default::default()
                }),
            )],
        );

        // Placing the aimed-at target beneath its own tracker is reported as
        // an unsupported dependency; it checks only the LookAt that reads it.
        let mut world = host.world_mut(world).unwrap();
        let place = |parent: Option<EntityId>| Command::PlaceEntity {
            entity: EntityRef::Handle(target),
            placement: EntityPlacementRef {
                parent: parent.map(EntityRef::Handle),
                before: None,
            },
        };
        LOOK_AT_CHECKS.set(0);
        VISITS.set(0);
        world
            .enqueue(Batch {
                id: world.tick() + 1,
                operations: vec![place(Some(tracker))],
            })
            .unwrap();
        let outcome = world.step(0.0).unwrap().outcomes.remove(0);
        assert_eq!(
            outcome.result.unwrap_err().reason,
            ErrorReason::UnsupportedDependency
        );
        assert!(LOOK_AT_CHECKS.get() <= 16, "checked unrelated declarations");
        assert!(VISITS.get() <= 16, "traversed unrelated links");
        assert!(world.world_matrix(tracker).is_err());

        // The correction is just as local and restores evaluation.
        LOOK_AT_CHECKS.set(0);
        VISITS.set(0);
        world
            .enqueue(Batch {
                id: world.tick() + 1,
                operations: vec![place(None)],
            })
            .unwrap();
        assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
        assert!(
            LOOK_AT_CHECKS.get() <= 16,
            "correction checked unrelated declarations"
        );
        assert!(VISITS.get() <= 16, "correction traversed unrelated links");
        assert!(world.world_matrix(tracker).is_ok());
    }
}

/// Keep empty extension dispatch visible separately from relationship traversal.
#[test]
fn maintained_scaling_empty_extension_dispatch() {
    use crate::systems::{
        System, SystemFactory, SystemId, SystemInitContext, SystemInitError,
        SystemOperationContext, compiled_system_factories,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Probe(Arc<AtomicUsize>);
    struct Factory(&'static str, Arc<AtomicUsize>);

    impl System for Probe {
        fn update(&mut self, _: &mut crate::systems::SystemUpdateContext<'_, '_>) {}

        fn after_operation(
            &mut self,
            _: &mut SystemOperationContext<'_>,
        ) -> Result<(), crate::ErrorReason> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    impl SystemFactory for Factory {
        fn id(&self) -> SystemId {
            SystemId(self.0)
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(Probe(self.1.clone())))
        }
    }

    const IDS: [&str; 16] = [
        "probe.0", "probe.1", "probe.2", "probe.3", "probe.4", "probe.5", "probe.6", "probe.7",
        "probe.8", "probe.9", "probe.10", "probe.11", "probe.12", "probe.13", "probe.14",
        "probe.15",
    ];
    for &size in SCALING_SIZES {
        for width in [0, 16] {
            let calls = Arc::new(AtomicUsize::new(0));
            let mut factories = compiled_system_factories();
            for id in &IDS[..width] {
                factories.push(Arc::new(Factory(id, calls.clone())));
            }
            let mut host = HostRuntime::with_system_factories(factories).unwrap();
            let world = host
                .create_world(
                    Default::default(),
                    &[
                        SPATIAL_SYSTEMS.to_vec(),
                        IDS[..width].iter().map(|&id| SystemId(id)).collect(),
                    ]
                    .concat(),
                )
                .unwrap();
            let start = Instant::now();
            VISITS.set(0);
            apply(
                &mut host,
                world,
                (0..size)
                    .map(|alias| Command::Create {
                        alias: alias as u32,
                        metadata: Default::default(),
                        adopt: false,
                    })
                    .collect(),
            );
            let count = calls.load(Ordering::Relaxed);
            assert_eq!(count, size * width);
            assert_eq!(
                VISITS.get(),
                size,
                "extension width does not add graph traversal"
            );
            println!(
                "scaling entities={size} empty_extensions={width} dispatch_calls={count} create_elapsed_us={}",
                start.elapsed().as_micros()
            );
        }
    }
}
