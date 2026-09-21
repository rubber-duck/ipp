//! Native behavioral evidence through the public production API.

mod support;
use support::WorldTestDriver;

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, ErrorReason, FieldValue,
    FieldWrite, WorldLimits, components::Scalar,
};

fn scalar_id() -> u16 {
    ComponentValue::SCALAR
}

fn create(alias: u32, name: &str) -> Command {
    Command::Create {
        alias,
        metadata: EntityMetadata {
            symbolic_id: Some(name.into()),
            classes: vec!["spatial".into(), "spatial".into()],
        },
    }
}

fn scalar(entity: EntityRef, value: f32) -> Command {
    Command::InsertComponent {
        entity,
        component: scalar_id(),
        fields: vec![write(value)],
    }
}

fn write(value: f32) -> FieldWrite {
    FieldWrite {
        offset: std::mem::offset_of!(Scalar, value) as u32,
        value: FieldValue::F32(value),
    }
}

fn run(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) -> ipp_core::BatchOutcome {
    world
        .enqueue(Batch {
            id: 42,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap().outcomes.remove(0)
}

fn setup(world: &mut ipp_core::WorldContext<'_>, name: &str, value: f32) -> EntityId {
    run(
        world,
        vec![create(1, name), scalar(EntityRef::Alias(1), value)],
    )
    .result
    .unwrap()[0]
        .1
}

fn values(world: &ipp_core::WorldContext<'_>, id: EntityId) -> (f32, f32) {
    let snapshot = world.inspect(id).unwrap();
    let get = |values: Vec<ComponentValue>| match values.first() {
        Some(ComponentValue::Scalar(scalar)) => scalar.value,
        _ => panic!("expected scalar first in registry order"),
    };
    (get(snapshot.base), get(snapshot.effective))
}

#[test]
fn failed_batch_keeps_values_metadata_and_allocations_and_stops_execution() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let id = setup(&mut world, "original", 7.0);

    let failed = run(
        &mut world,
        vec![
            Command::SetMetadata {
                entity: EntityRef::Handle(id),
                metadata: EntityMetadata {
                    symbolic_id: Some("changed".into()),
                    classes: vec!["other".into()],
                },
            },
            Command::SetField {
                entity: EntityRef::Handle(id),
                component: scalar_id(),
                field: write(99.0),
            },
            create(2, "partial"),
            Command::Delete {
                entity: EntityRef::Handle(EntityId::from_bits(u64::MAX)),
            },
            create(3, "unreached"),
        ],
    )
    .result
    .unwrap_err();

    assert_eq!(
        (failed.operation, failed.reason),
        (Some(3), ErrorReason::InvalidEntity)
    );
    assert_eq!(values(&world, id), (99.0, 99.0));
    assert_eq!(world.lookup_id("original"), None);
    assert_eq!(world.lookup_id("changed"), Some(id));
    assert_eq!(world.lookup_id("partial"), Some(failed.aliases[0].1));
    assert_eq!(failed.aliases[0].0, 2);
    assert_eq!(world.lookup_id("unreached"), None);
    assert_eq!(world.lookup_class("spatial"), vec![failed.aliases[0].1]);
    assert_eq!(world.lookup_class("other"), vec![id]);
    let next = setup(&mut world, "next", 8.0);
    assert_eq!(next.index(), 2);
    assert_eq!(next.generation(), 1);
}

#[test]
fn delete_and_reuse_fence_stale_handles_and_remove_indexes() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let id = setup(&mut world, "one", 1.0);

    run(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(id),
        }],
    )
    .result
    .unwrap();

    assert_eq!(world.inspect(id), None);
    assert_eq!(world.lookup_id("one"), None);
    assert!(world.lookup_class("spatial").is_empty());
    let replacement = setup(&mut world, "two", 2.0);
    assert_eq!(replacement.index(), id.index());
    assert_ne!(replacement.generation(), id.generation());
    assert_eq!(
        run(&mut world, vec![scalar(EntityRef::Handle(id), 9.0)])
            .result
            .unwrap_err()
            .reason,
        ErrorReason::InvalidEntity
    );
    assert_eq!(values(&world, replacement), (2.0, 2.0));
}

#[test]
fn aliases_are_ordered_unique_and_batch_local() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    assert_eq!(
        run(
            &mut world,
            vec![scalar(EntityRef::Alias(1), 1.0), create(1, "late")]
        )
        .result
        .unwrap_err()
        .reason,
        ErrorReason::UnknownAlias
    );

    assert_eq!(
        run(&mut world, vec![create(1, "a"), create(1, "b")])
            .result
            .unwrap_err()
            .reason,
        ErrorReason::DuplicateAlias
    );
    assert!(world.lookup_id("a").is_some());
    assert_eq!(world.lookup_id("b"), None);

    setup(&mut world, "ok", 1.0);

    assert_eq!(
        run(&mut world, vec![scalar(EntityRef::Alias(1), 2.0)])
            .result
            .unwrap_err()
            .reason,
        ErrorReason::UnknownAlias
    );
}

#[test]
fn metadata_uniqueness_and_class_order_survive_staged_deletion() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let one = setup(&mut world, "one", 1.0);
    let two = setup(&mut world, "two", 2.0);

    assert_eq!(world.lookup_class("spatial"), vec![one, two]);
    assert_eq!(
        run(&mut world, vec![create(1, "one")])
            .result
            .unwrap_err()
            .reason,
        ErrorReason::DuplicateSymbolicId
    );
    let created = run(
        &mut world,
        vec![
            Command::Delete {
                entity: EntityRef::Handle(one),
            },
            create(3, "one"),
        ],
    )
    .result
    .unwrap()[0]
        .1;

    assert_eq!(world.lookup_id("one"), Some(created));
    assert_eq!(world.lookup_class("spatial"), vec![two, created]);
    assert_eq!(
        world.inspect(created).unwrap().metadata.classes,
        vec!["spatial"]
    );
}

#[test]
fn invalid_exact_fields_and_values_reject_atomically() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let id = setup(&mut world, "value", 3.0);

    for field in [
        FieldWrite {
            offset: 1,
            value: FieldValue::F32(4.0),
        },
        FieldWrite {
            offset: 0,
            value: FieldValue::Entity(EntityRef::Handle(id)),
        },
        write(f32::NAN),
        write(f32::INFINITY),
    ] {
        assert!(
            run(
                &mut world,
                vec![Command::SetField {
                    entity: EntityRef::Handle(id),
                    component: scalar_id(),
                    field
                }]
            )
            .result
            .is_err()
        );
        assert_eq!(values(&world, id), (3.0, 3.0));
    }
    assert_eq!(
        run(
            &mut world,
            vec![Command::InsertComponent {
                entity: EntityRef::Handle(id),
                component: u16::MAX,
                fields: vec![]
            }]
        )
        .result
        .unwrap_err()
        .reason,
        ErrorReason::UnknownComponent
    );
}

#[test]
fn default_ingress_preserves_large_indivisible_batches() {
    let mut host = ipp_core::HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    // Exceed both historical defaults: 256 operations and 64 KiB estimated bytes.
    let operations = (0..600)
        .map(|alias| create(alias, &format!("entity-{alias}-{}", "x".repeat(128))))
        .collect();
    let outcome = run(&mut world, operations);
    assert_eq!(outcome.result.unwrap().len(), 600);
}

#[test]
fn ingress_budgets_reject_complete_batches_and_allow_world_growth() {
    let limits = WorldLimits {
        max_operations: 2,
        max_queued_batches: 1,
        ..WorldLimits::default()
    };
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host.create_world(limits).unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    assert_eq!(
        world.enqueue(Batch {
            id: 0,
            operations: vec![create(0, "a"), create(1, "b"), create(2, "c")]
        }),
        Err(ErrorReason::Capacity)
    );
    world
        .enqueue(Batch {
            id: 5,
            operations: vec![create(0, "a")],
        })
        .unwrap();
    assert_eq!(
        world.enqueue(Batch {
            id: 6,
            operations: vec![]
        }),
        Err(ErrorReason::Capacity)
    );
    let report = world.update_for_test(0.25).unwrap();

    assert_eq!(report.outcomes[0].batch_id, 5);
    let id = report.outcomes[0].result.as_ref().unwrap()[0].1;
    assert!(run(&mut world, vec![create(0, "b")]).result.is_ok());
    assert!(world.inspect(id).is_some());
    let tiny_activation = WorldLimits {
        max_staging_bytes: 1,
        ..WorldLimits::default()
    };
    let mut host = ipp_core::HostRuntime::new();
    let id = host.create_world(tiny_activation).unwrap();
    let mut world = host.world_mut(id).unwrap();
    world
        .set_capacity_hints(ipp_core::WorldCapacityHints {
            entities: 1024,
            ..Default::default()
        })
        .unwrap();
    assert!(run(&mut world, vec![create(0, "reserved")]).result.is_ok());
}

#[test]
fn invalid_time_preserves_queue_and_clock() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    world
        .enqueue(Batch {
            id: 9,
            operations: vec![create(1, "queued")],
        })
        .unwrap();
    for dt in [-1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(world.update_for_test(dt), Err(ErrorReason::InvalidValue));
        assert_eq!((world.tick(), world.time()), (0, 0.0));
        assert_eq!(world.lookup_id("queued"), None);
    }
    let report = world.update_for_test(0.5).unwrap();

    assert_eq!(
        (report.tick, report.time, report.outcomes.len()),
        (1, 0.5, 1)
    );
    assert_eq!(report.outcomes[0].tick, 1);
}

#[test]
fn outcomes_keep_submission_order_across_failure_and_later_success() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    for (id, operations) in [
        (1, vec![create(1, "one")]),
        (2, vec![create(2, "one")]),
        (3, vec![create(3, "three")]),
    ] {
        world
            .enqueue(Batch {
                id,
                operations,
            })
            .unwrap();
    }
    let report = world.update_for_test(0.0).unwrap();

    assert_eq!(
        report
            .outcomes
            .iter()
            .map(|o| o.batch_id)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(report.outcomes[0].result.is_ok());
    assert!(report.outcomes[1].result.is_err());
    assert!(report.outcomes[2].result.is_ok());
    assert!(report.outcomes.iter().all(|o| o.tick == 1));
}

mod drivers {
    use super::*;
    use ipp_core::components::LinearDriver;

    fn driver_id() -> u16 {
        ComponentValue::LINEAR_DRIVER
    }

    fn source(source: EntityRef) -> FieldWrite {
        FieldWrite {
            offset: std::mem::offset_of!(LinearDriver, source) as u32,
            value: FieldValue::Entity(source),
        }
    }

    fn driver(target: EntityRef, from: EntityRef, scale: f32, bias: f32) -> Command {
        Command::InsertComponent {
            entity: target,
            component: driver_id(),
            fields: vec![
                source(from),
                FieldWrite {
                    offset: std::mem::offset_of!(LinearDriver, scale) as u32,
                    value: FieldValue::F32(scale),
                },
                FieldWrite {
                    offset: std::mem::offset_of!(LinearDriver, bias) as u32,
                    value: FieldValue::F32(bias),
                },
            ],
        }
    }

    #[test]
    fn aliases_resolve_nested_driver_sources_and_base_is_retained() {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let ids = run(
            &mut world,
            vec![
                create(1, "source"),
                scalar(EntityRef::Alias(1), 3.0),
                create(2, "target"),
                scalar(EntityRef::Alias(2), 10.0),
                driver(EntityRef::Alias(2), EntityRef::Alias(1), 2.0, 1.0),
            ],
        )
        .result
        .unwrap();
        let (from, target) = (ids[0].1, ids[1].1);
        assert_eq!(values(&world, target), (10.0, 7.0));
        for _ in 0..3 {
            world.update_for_test(0.25).unwrap();
            assert_eq!(values(&world, target), (10.0, 7.0));
        }
        run(
            &mut world,
            vec![Command::SetField {
                entity: EntityRef::Handle(from),
                component: scalar_id(),
                field: write(4.0),
            }],
        )
        .result
        .unwrap();
        assert_eq!(values(&world, target), (10.0, 9.0));
    }

    #[test]
    fn source_replacement_invalidates_and_explicit_reference_write_rebinds() {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let from = setup(&mut world, "source", 3.0);
        let target = setup(&mut world, "target", 10.0);
        run(
            &mut world,
            vec![driver(
                EntityRef::Handle(target),
                EntityRef::Handle(from),
                2.0,
                1.0,
            )],
        )
        .result
        .unwrap();
        run(&mut world, vec![scalar(EntityRef::Handle(from), 5.0)])
            .result
            .unwrap();
        assert_eq!(values(&world, target), (10.0, 10.0));
        assert_eq!(world.driver_bound(target), Some(false));
        run(
            &mut world,
            vec![Command::SetField {
                entity: EntityRef::Handle(target),
                component: driver_id(),
                field: source(EntityRef::Handle(from)),
            }],
        )
        .result
        .unwrap();
        assert_eq!(values(&world, target), (10.0, 11.0));
        assert_eq!(world.driver_bound(target), Some(true));
        run(
            &mut world,
            vec![
                Command::Delete {
                    entity: EntityRef::Handle(from),
                },
                create(3, "replacement"),
                scalar(EntityRef::Alias(3), 99.0),
            ],
        )
        .result
        .unwrap();
        assert_eq!(values(&world, target), (10.0, 10.0));
        assert_eq!(world.driver_bound(target), Some(false));
        let replacement = world.lookup_id("replacement").unwrap();
        assert_eq!(replacement.index(), from.index());
        assert_ne!(replacement, from);
        assert_eq!(
            run(
                &mut world,
                vec![Command::SetField {
                    entity: EntityRef::Handle(target),
                    component: driver_id(),
                    field: source(EntityRef::Handle(from))
                }]
            )
            .result
            .unwrap_err()
            .reason,
            ErrorReason::InvalidEntity
        );
    }

    #[test]
    fn failed_batch_keeps_source_removal_and_binding_invalidation() {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let from = setup(&mut world, "source", 3.0);
        let target = setup(&mut world, "target", 10.0);
        run(
            &mut world,
            vec![driver(
                EntityRef::Handle(target),
                EntityRef::Handle(from),
                2.0,
                1.0,
            )],
        )
        .result
        .unwrap();
        let failure = run(
            &mut world,
            vec![
                Command::RemoveComponent {
                    entity: EntityRef::Handle(from),
                    component: scalar_id(),
                },
                create(7, "target"),
            ],
        );
        assert!(failure.result.is_err());
        assert!(world.inspect(from).unwrap().effective.is_empty());
        assert_eq!(values(&world, target), (10.0, 10.0));
        assert_eq!(world.driver_bound(target), Some(false));
    }

    #[test]
    #[cfg(debug_assertions)]
    fn fixed_forward_chains_work_and_debug_checks_report_partial_invalid_updates() {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let a = setup(&mut world, "a", 2.0);
        let b = setup(&mut world, "b", 20.0);
        let c = setup(&mut world, "c", 30.0);
        run(
            &mut world,
            vec![
                driver(EntityRef::Handle(b), EntityRef::Handle(a), 2.0, 1.0),
                driver(EntityRef::Handle(c), EntityRef::Handle(b), 3.0, 0.0),
            ],
        )
        .result
        .unwrap();
        assert_eq!(values(&world, c), (30.0, 15.0));
        assert_eq!(
            run(
                &mut world,
                vec![driver(EntityRef::Handle(a), EntityRef::Handle(c), 1.0, 0.0)]
            )
            .result
            .unwrap_err()
            .reason,
            ErrorReason::UnsupportedDependency
        );
        assert_eq!(
            run(
                &mut world,
                vec![driver(EntityRef::Handle(c), EntityRef::Handle(c), 1.0, 0.0)]
            )
            .result
            .unwrap_err()
            .reason,
            ErrorReason::UnsupportedDependency
        );
        // Correct the invalid declarations explicitly before testing overflow.
        run(
            &mut world,
            vec![Command::RemoveComponent {
                entity: EntityRef::Handle(a),
                component: driver_id(),
            }],
        );
        run(
            &mut world,
            vec![driver(EntityRef::Handle(c), EntityRef::Handle(b), 3.0, 0.0)],
        )
        .result
        .unwrap();
        assert_eq!(
            run(
                &mut world,
                vec![Command::SetField {
                    entity: EntityRef::Handle(a),
                    component: scalar_id(),
                    field: write(f32::MAX)
                }]
            )
            .result
            .unwrap_err()
            .reason,
            ErrorReason::InvalidValue
        );
        assert_eq!(values(&world, a), (f32::MAX, f32::MAX));
        assert!(values(&world, c).1.is_infinite());
        run(
            &mut world,
            vec![Command::SetField {
                entity: EntityRef::Handle(a),
                component: scalar_id(),
                field: write(2.0),
            }],
        )
        .result
        .unwrap();
        assert_eq!(values(&world, c), (30.0, 15.0));
    }
}

#[test]
fn retained_metadata_grows_while_batch_allocation_bounds_include_spare_capacity() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(WorldLimits {
            max_batch_bytes: 65536,
            ..WorldLimits::default()
        })
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let class = "x".repeat(16 * 1024);
    for index in 0..80 {
        run(
            &mut world,
            vec![Command::Create {
                alias: 0,
                metadata: EntityMetadata {
                    symbolic_id: Some(format!("metadata-{index}")),
                    classes: vec![class.clone()],
                },
            }],
        )
        .result
        .unwrap();
    }
    let entities = world.entities();
    assert_eq!(entities.len(), 80);
    assert!(
        entities
            .iter()
            .all(|entity| entity.metadata.classes == [class.clone()])
    );
    run(&mut world, vec![create(0, "after-growth")])
        .result
        .unwrap();
    let mut operations = Vec::with_capacity(10000);
    operations.push(create(1, "a"));
    assert_eq!(
        world.enqueue(Batch {
            id: 7,
            operations
        }),
        Err(ErrorReason::Capacity)
    );
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let first = setup(&mut world, "first", 1.0);
    let second = setup(&mut world, "second", 2.0);
    assert_eq!(
        world.entities().iter().map(|s| s.id).collect::<Vec<_>>(),
        vec![first, second]
    );
}
