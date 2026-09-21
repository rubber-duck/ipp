//! Factory initialization, owned state, dependency borrowing and lifecycle on real Worlds.

use ipp_core::systems::{
    System, SystemCommitContext, SystemDependency, SystemDependencyBinding, SystemFactories,
    SystemFactory, SystemId, SystemInitContext, SystemInitError, SystemScheduleError,
    SystemTeardownContext, SystemUpdateContext, compiled_system_factories,
};
use ipp_core::{Batch, Command, HostRuntime, WorldConstructionError, WorldLimits};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

type SystemTrace = Arc<Mutex<Vec<(&'static str, &'static str, u64)>>>;
type RetainedBinding = Arc<Mutex<Option<SystemDependencyBinding<RecordingSystem>>>>;

struct RecordingSystemFactory {
    id: SystemId,
    dependencies: Vec<SystemDependency>,
    events: SystemTrace,
    fail: Arc<AtomicBool>,
    retained: RetainedBinding,
}

struct RecordingSystem {
    id: SystemId,
    events: SystemTrace,
    counter: u64,
    predecessor: Option<SystemDependencyBinding<Self>>,
    previous_world_binding: Option<SystemDependencyBinding<Self>>,
}

impl SystemFactory for RecordingSystemFactory {
    fn id(&self) -> SystemId {
        self.id
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &self.dependencies
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        self.events
            .lock()
            .unwrap()
            .push((self.id.0, "init", context.world.id().0));
        if self.fail.load(Ordering::Relaxed) {
            return Err(SystemInitError::Message(
                "fixture initialization failed".into(),
            ));
        }
        let predecessor = self
            .dependencies
            .iter()
            .find_map(|dependency| match dependency {
                SystemDependency::Required(id) => Some(context.dependency::<RecordingSystem>(*id)),
                _ => None,
            })
            .transpose()?;
        if let Some(binding) = predecessor {
            assert_eq!(
                context.get(binding).unwrap().counter,
                10,
                "predecessor initialized before dependent"
            );
        }
        let previous_world_binding =
            std::mem::replace(&mut *self.retained.lock().unwrap(), predecessor);
        Ok(Box::new(RecordingSystem {
            id: self.id,
            events: Arc::clone(&self.events),
            counter: 10,
            predecessor,
            previous_world_binding,
        }))
    }
}

impl System for RecordingSystem {
    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.events.lock().unwrap().push((
            self.id.0,
            "before",
            context.world().entities_effective().len() as u64,
        ));
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        assert_eq!(context.dt(), 0.25);
        // SystemRuntimeAccess exposes no time-advancement operation.
        self.counter += 1;
        if let Some(binding) = self.predecessor {
            assert_eq!(context.dependency(binding).unwrap().counter, self.counter);
        }
        if let Some(binding) = self.previous_world_binding {
            assert!(
                context.dependency(binding).is_none(),
                "foreign-world binding must not resolve"
            );
        }
        self.events
            .lock()
            .unwrap()
            .push((self.id.0, "update", self.counter));
    }

    fn teardown(&mut self, context: &mut SystemTeardownContext<'_>) {
        if let Some(binding) = self.predecessor {
            assert!(
                context.dependency(binding).is_some(),
                "predecessor still exists during reverse teardown"
            );
        }
        self.events.lock().unwrap().push((
            self.id.0,
            "teardown",
            context.world.entities_effective().len() as u64,
        ));
    }
}

impl Drop for RecordingSystem {
    fn drop(&mut self) {
        self.events
            .lock()
            .unwrap()
            .push((self.id.0, "drop", self.counter));
    }
}

fn factory(
    id: &'static str,
    dependencies: Vec<SystemDependency>,
    events: &SystemTrace,
) -> Arc<RecordingSystemFactory> {
    Arc::new(RecordingSystemFactory {
        id: SystemId(id),
        dependencies,
        events: Arc::clone(events),
        fail: Default::default(),
        retained: Default::default(),
    })
}

fn host(factories: Vec<Arc<dyn SystemFactory>>) -> HostRuntime {
    let mut selected = compiled_system_factories();
    selected.extend(factories);
    HostRuntime::with_system_factories(selected).unwrap()
}

#[test]
fn ordered_initialization_mutable_updates_invalidation_and_reverse_destruction() {
    let events = SystemTrace::default();
    let mut host = host(vec![
        factory(
            "last",
            vec![SystemDependency::Required(SystemId("first"))],
            &events,
        ),
        factory(
            "first",
            vec![SystemDependency::After(SystemId("absent"))],
            &events,
        ),
    ]);
    let id = host.create_world(Default::default()).unwrap();
    {
        let mut world = host.world_mut(id).unwrap();
        world
            .enqueue(Batch {
                id: 1,
                operations: vec![Command::Create {
                    alias: 0,
                    metadata: Default::default(),
                }],
            })
            .unwrap();
        world.step(0.25).unwrap();
        world.step(0.25).unwrap();
    }
    assert!(host.destroy_world(id));
    assert!(!host.destroy_world(id));
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            ("first", "init", id.0),
            ("last", "init", id.0),
            ("first", "before", 1),
            ("last", "before", 1),
            ("first", "update", 11),
            ("last", "update", 11),
            ("first", "update", 12),
            ("last", "update", 12),
            ("last", "teardown", 1),
            ("last", "drop", 12),
            ("first", "teardown", 1),
            ("first", "drop", 12),
        ]
    );
}

#[test]
fn reusable_factories_create_independent_state_and_bindings_cannot_cross_worlds_or_hosts() {
    let events = SystemTrace::default();
    let first = factory("first", vec![], &events);
    let last = factory(
        "last",
        vec![SystemDependency::Required(SystemId("first"))],
        &events,
    );
    let factories = vec![first as Arc<dyn SystemFactory>, last];
    let mut a = host(factories.clone());
    let mut b = host(factories);
    let one = a.create_world(Default::default()).unwrap();
    let two = a.create_world(Default::default()).unwrap();
    let other_host = b.create_world(Default::default()).unwrap();
    assert_eq!(
        one, other_host,
        "Host-local IDs intentionally collide; binding identities cannot"
    );
    a.world_mut(one).unwrap().step(0.25).unwrap();
    a.world_mut(two).unwrap().step(0.25).unwrap();
    b.world_mut(other_host).unwrap().step(0.25).unwrap();
    a.destroy_world(one);
    a.world_mut(two).unwrap().step(0.25).unwrap();
    drop(a);
    drop(b);
    let trace = events.lock().unwrap();
    assert_eq!(
        trace
            .iter()
            .filter(|(_, phase, count)| *phase == "update" && *count == 11)
            .count(),
        6
    );
    assert_eq!(
        trace
            .iter()
            .filter(|(_, phase, _)| *phase == "drop")
            .count(),
        6
    );
}

#[test]
fn initialization_failure_unwinds_before_publication_and_factories_can_retry() {
    let events = SystemTrace::default();
    let first = factory("first", vec![], &events);
    let last = factory(
        "last",
        vec![SystemDependency::Required(SystemId("first"))],
        &events,
    );
    last.fail.store(true, Ordering::Relaxed);
    let mut host = host(vec![last.clone(), first]);
    assert!(matches!(
        host.create_world(Default::default()),
        Err(WorldConstructionError::Initialization {
            system: SystemId("last"),
            ..
        })
    ));
    assert_eq!(host.world_ids().len(), 0);
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            ("first", "init", 1),
            ("last", "init", 1),
            ("first", "teardown", 0),
            ("first", "drop", 10)
        ]
    );
    last.fail.store(false, Ordering::Relaxed);
    host.create_world(Default::default()).unwrap();
    assert_eq!(host.world_ids().len(), 1);
}

#[test]
fn invalid_limits_or_missing_authoring_factories_do_not_initialize_anything() {
    let events = SystemTrace::default();
    let mut host = host(vec![factory("custom", vec![], &events)]);
    assert!(matches!(
        host.create_world(WorldLimits {
            max_operations: 0,
            ..Default::default()
        }),
        Err(WorldConstructionError::Limits(_))
    ));
    assert!(matches!(
        host.create_world_with_systems(Default::default(), &[SystemId("custom")]),
        Err(WorldConstructionError::Systems(
            SystemScheduleError::MissingRequired { .. }
        ))
    ));
    assert!(matches!(
        host.create_world_with_systems(Default::default(), &[SystemId("unknown")]),
        Err(WorldConstructionError::Systems(
            SystemScheduleError::Unknown(_)
        ))
    ));
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(host.world_ids().len(), 0);
}

#[test]
fn complete_factory_graph_validation_precedes_initialization() {
    use SystemDependency::{After, Required};
    let events = SystemTrace::default();
    let build = |entries: Vec<Arc<RecordingSystemFactory>>| {
        SystemFactories::new(
            entries
                .into_iter()
                .map(|entry| entry as Arc<dyn SystemFactory>)
                .collect(),
        )
    };
    assert!(matches!(
        build(vec![
            factory("a", vec![], &events),
            factory("a", vec![], &events)
        ]),
        Err(SystemScheduleError::Duplicate(_))
    ));
    assert!(matches!(
        build(vec![factory("a", vec![Required(SystemId("b"))], &events)]),
        Err(SystemScheduleError::MissingRequired { .. })
    ));
    for edge in [After(SystemId("a")), Required(SystemId("a"))] {
        assert!(matches!(
            build(vec![factory("a", vec![edge], &events)]),
            Err(SystemScheduleError::SelfDependency(_))
        ));
    }
    assert!(matches!(
        build(vec![
            factory("a", vec![After(SystemId("b"))], &events),
            factory("b", vec![Required(SystemId("a"))], &events)
        ]),
        Err(SystemScheduleError::Cycle(_))
    ));
    let valid = build(vec![
        factory(
            "last",
            vec![After(SystemId("first")), After(SystemId("absent"))],
            &events,
        ),
        factory("independent", vec![], &events),
        factory("first", vec![Required(SystemId("root"))], &events),
        factory("root", vec![], &events),
    ])
    .unwrap();
    assert_eq!(
        valid.ids().collect::<Vec<_>>(),
        ["independent", "root", "first", "last"].map(SystemId)
    );
    assert!(events.lock().unwrap().is_empty());
}
#[test]
fn bindings_are_scoped_to_the_declaring_dependent_even_inside_one_world() {
    let events = SystemTrace::default();
    let first = factory("first", vec![], &events);
    let a = factory(
        "a",
        vec![SystemDependency::Required(SystemId("first"))],
        &events,
    );
    let mut b = factory(
        "b",
        vec![
            SystemDependency::Required(SystemId("first")),
            SystemDependency::After(SystemId("a")),
        ],
        &events,
    );
    Arc::get_mut(&mut b).unwrap().retained = Arc::clone(&a.retained);
    let mut host = host(vec![b, a, first]);
    let id = host.create_world(Default::default()).unwrap();
    host.world_mut(id).unwrap().step(0.25).unwrap();
}

#[test]
fn incorrect_dependency_type_rejects_initialization_and_preserves_existing_worlds() {
    use ipp_core::systems::hierarchy::HierarchySystem;
    let events = SystemTrace::default();
    let wrong = factory(
        "wrong",
        vec![SystemDependency::Required(HierarchySystem::ID)],
        &events,
    );
    let mut host = host(vec![wrong]);
    let builtins: Vec<_> = compiled_system_factories()
        .iter()
        .map(|factory| factory.id())
        .collect();
    let existing = host
        .create_world_with_systems(Default::default(), &builtins)
        .unwrap();
    host.world_mut(existing).unwrap().step(0.0).unwrap();
    assert!(matches!(
        host.create_world(Default::default()),
        Err(WorldConstructionError::Initialization {
            error: SystemInitError::DependencyType(HierarchySystem::ID),
            ..
        })
    ));
    assert_eq!(host.world_ids().collect::<Vec<_>>(), [existing]);
    assert_eq!(host.world_mut(existing).unwrap().tick(), 1);
}

struct InvalidBindingFactory;

impl SystemFactory for InvalidBindingFactory {
    fn id(&self) -> SystemId {
        SystemId("invalid-binding")
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        context.dependency::<ipp_core::systems::hierarchy::HierarchySystem>(
            ipp_core::systems::hierarchy::HierarchySystem::ID,
        )?;
        unreachable!("an undeclared system must not become an implicit dependency")
    }
}

#[test]
fn presence_alone_does_not_grant_dependency_access() {
    let mut host = host(vec![Arc::new(InvalidBindingFactory)]);
    assert!(matches!(
        host.create_world(Default::default()),
        Err(WorldConstructionError::Initialization {
            error: SystemInitError::UnavailableDependency(_),
            ..
        })
    ));
    assert_eq!(host.world_ids().len(), 0);
}

struct IncorrectBuiltinFactory(SystemId);

impl SystemFactory for IncorrectBuiltinFactory {
    fn id(&self) -> SystemId {
        self.0
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(
            ipp_core::systems::hierarchy::HierarchySystem::default(),
        ))
    }
}

#[test]
fn builtin_instances_cannot_be_hidden_under_an_extension_identity() {
    let mut host = host(vec![Arc::new(IncorrectBuiltinFactory(SystemId(
        "aliased-builtin",
    )))]);
    assert!(matches!(
        host.create_world(Default::default()),
        Err(WorldConstructionError::Initialization {
            error: SystemInitError::AuthoringSystemType(SystemId("aliased-builtin")),
            ..
        })
    ));
    assert_eq!(host.world_ids().len(), 0);
}

#[test]
fn producer_assets_grow_and_world_teardown_preserves_other_consumers() {
    use ipp_core::{MeshKey, MeshUpload};
    let mut host = HostRuntime::new();
    let limits = WorldLimits {
        ..Default::default()
    };
    let first = host.create_world(limits).unwrap();
    let second = host.create_world(limits).unwrap();
    let triangle = || {
        let mut bytes = b"IPPM".to_vec();
        for value in [1u32, 3, 3] {
            bytes.extend(value.to_le_bytes());
        }
        for position in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for value in position.into_iter().chain([1.0; 3]) {
                bytes.extend(value.to_le_bytes());
            }
        }
        for index in [0u16, 1, 2] {
            bytes.extend(index.to_le_bytes());
        }
        bytes
    };
    for (world_id, assets) in [(first, vec![1, 2]), (second, vec![1])] {
        let mut world = host.world_mut(world_id).unwrap();
        for asset in assets {
            world
                .enqueue_mesh(MeshUpload {
                    id: asset,
                    key: MeshKey {
                        asset,
                        variant: 0,
                    },
                    bytes: triangle(),
                })
                .unwrap();
        }
        world.prepare_update(0.0).unwrap();
    }
    host.progress_assets();
    let first_report = host.world_mut(first).unwrap().step(0.0).unwrap();
    assert_eq!(first_report.assets.len(), 2);
    assert!(
        first_report.assets[0].result.is_ok(),
        "{:?}",
        first_report.assets
    );
    assert!(first_report.assets[1].result.is_ok());
    assert!(
        host.world_mut(second).unwrap().step(0.0).unwrap().assets[0]
            .result
            .is_ok()
    );
    assert_eq!(host.asset_resources().iter().count(), 3);
    host.destroy_world(first);
    assert_eq!(host.asset_resources().iter().count(), 1);
    assert!(
        host.world_mut(second)
            .unwrap()
            .mesh(MeshKey {
                asset: 1,
                variant: 0
            })
            .is_some()
    );
}

#[test]
fn destroying_one_consumer_preserves_another_worlds_in_flight_reader() {
    use ipp_core::{AssetResourceStatus, ComponentValue, EntityRef, components::MeshInstance};
    let mut host = HostRuntime::new();
    host.register_stream_resource_provider("fixture").unwrap();
    let first = host.create_world(Default::default()).unwrap();
    let second = host.create_world(Default::default()).unwrap();
    for id in [first, second] {
        let mut world = host.world_mut(id).unwrap();
        world
            .enqueue(Batch {
                id: 1,
                operations: vec![
                    Command::Create {
                        alias: 0,
                        metadata: Default::default(),
                    },
                    Command::InsertComponentValue {
                        entity: EntityRef::Alias(0),
                        value: ComponentValue::MeshInstance(MeshInstance {
                            source: "fixture:///shared.mesh".into(),
                            variant: 0,
                        }),
                    },
                ],
            })
            .unwrap();
        assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    }
    host.progress_assets();
    let requests = host.take_resource_requests();
    assert_eq!(requests.len(), 1);
    let resource = host.world_mut(second).unwrap().resource_snapshots()[0].id;
    host.destroy_world(first);
    assert!(host.take_resource_cancellations().is_empty());
    host.complete_resource(requests[0].id, Err("fixture terminal result".into()))
        .unwrap();
    host.progress_assets();
    let mut surviving = host.world_mut(second).unwrap();
    surviving.step(0.0).unwrap();
    let observation = &surviving.resource_snapshots()[0];
    assert_eq!(observation.id, resource);
    assert_eq!(
        observation.status,
        AssetResourceStatus::Failed("fixture terminal result".into())
    );
}
