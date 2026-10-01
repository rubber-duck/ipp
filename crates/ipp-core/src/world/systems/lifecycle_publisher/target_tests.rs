use super::*;
use crate::services::reliable_output::*;
use crate::{Batch, Command, ComponentValue, EntityMetadata, EntityRef, HostRuntime, WorldId};

/// Watched Worlds hold Scalars, drivers, Transforms and, with GUI, controls.
#[cfg(feature = "gui")]
const FIXTURE_SYSTEMS: &[crate::systems::SystemId] = &[
    LifecyclePublisherSystem::ID,
    crate::systems::constraints::ConstraintSystem::ID,
    crate::systems::hierarchy::HierarchySystem::ID,
    crate::systems::canvas::CanvasSystem::ID,
    crate::systems::gui::GuiSystem::ID,
];

/// Watched Worlds hold Scalars, drivers and Transforms.
#[cfg(not(feature = "gui"))]
const FIXTURE_SYSTEMS: &[crate::systems::SystemId] = &[
    LifecyclePublisherSystem::ID,
    crate::systems::constraints::ConstraintSystem::ID,
    crate::systems::hierarchy::HierarchySystem::ID,
];

fn fixture(session: u64) -> (HostRuntime, WorldId, LifecycleWatchOutput) {
    let mut host = HostRuntime::new();
    let world = host
        .create_world(Default::default(), FIXTURE_SYSTEMS)
        .unwrap();
    let account = ReliableOutputAccount::new(OutputLimits {
        bytes: 16 * 1024 * 1024,
        reply_reserve: 0,
    });
    let output = LifecycleWatchOutput::new(
        account,
        host.world_ref(world).unwrap(),
        session,
        LifecycleWatchEncoding {
            acknowledgement_bytes: 64,
            baseline_bytes: 48,
            event_bytes: 96,
            baseline_field_bytes: 4,
            value_bytes: 96,
            value_field_bytes: 40,
        },
    )
    .unwrap();
    (host, world, output)
}

fn prepare(
    output: &LifecycleWatchOutput,
    action: LifecycleMembershipAction,
    members: &[LifecycleWatchMember],
    request: u64,
) -> LifecycleMembershipCommand {
    let lease = output
        .account()
        .reserve(OutputCharge {
            entries: 1,
            bytes: 0,
        })
        .unwrap();
    LifecycleMembershipCommand::prepare(output, action, members.to_vec(), request, lease).unwrap()
}

fn cut(
    system: &mut LifecyclePublisherSystem,
    host: &mut HostRuntime,
    world: WorldId,
    command: &LifecycleMembershipCommand,
) {
    let world = host.world_mut(world).unwrap();
    system.membership(
        crate::systems::SystemWorldView {
            world: world.world,
            authored: &world.world.state,
        },
        command.session(),
        command,
    );
}

fn records(output: &LifecycleWatchOutput) -> Vec<LifecycleWatchRecord> {
    let mut result = Vec::new();
    while let Some(delivery) = output.pop_front() {
        let (record, lease) = delivery.into_parts();
        result.push(record);
        drop(lease);
    }
    result
}

fn member(output: &LifecycleWatchOutput, entity: u64) -> LifecycleWatchMember {
    output
        .new_member(
            LifecycleWatchTarget::Entity(EntityId::from_bits(entity)),
            LifecycleWatchKinds::ENTITY_DELETED,
        )
        .unwrap()
}

fn deleted(entity: u64) -> LifecycleObservation {
    LifecycleObservation::Entity {
        entity: EntityId::from_bits(entity),
        kind: EntityLifecycleKind::Deleted,
    }
}

#[test]
fn large_sets_ignore_unrelated_churn_without_visiting_members_or_queuing_bytes() {
    for count in [65, 512, 2048] {
        let (mut host, world, output) = fixture(7);
        let mut system = LifecyclePublisherSystem::default();
        let members: Vec<_> = (1..=count).map(|entity| member(&output, entity)).collect();
        for (page, members) in members.chunks(47).enumerate() {
            cut(
                &mut system,
                &mut host,
                world,
                &prepare(
                    &output,
                    LifecycleMembershipAction::Add,
                    members,
                    page as u64 + 1,
                ),
            );
            assert!(
                matches!(&records(&output)[0].body, LifecycleWatchRecordBody::Acknowledgement { result: LifecycleMembershipResult::Applied(baselines), .. } if baselines.len() == members.len())
            );
        }
        assert_eq!(system.targets.counts(), (count as usize, 1, count as usize));
        let usage = output.account().usage();
        for ordinal in 0..1000 {
            system.observe(1, &deleted(count + ordinal + 1));
        }
        assert_eq!(
            (
                system.target_work().lookups,
                system.target_work().recipient_visits
            ),
            (1000, 0)
        );
        assert!(!system.target_work().saturated);
        assert_eq!(output.traffic(), Some(LifecycleWatchTraffic::default()));
        assert_eq!(output.account().usage(), usage);
        assert!(records(&output).is_empty());
        system.observe(
            2,
            &LifecycleObservation::Entity {
                entity: EntityId::from_bits(1),
                kind: EntityLifecycleKind::MetadataChanged,
            },
        );
        assert_eq!(output.account().usage(), usage);
        system.observe(3, &deleted(1));
        assert_eq!(system.target_work().recipient_visits, 2);
        assert_eq!(output.traffic().unwrap().queued_events, 1);
        assert_eq!(records(&output).len(), 1);
        system.release_session(7);
        assert_eq!(system.targets.counts(), (0, 0, 0));
        drop(members);
        output.close();
        let account = output.account().clone();
        drop(output);
        assert_eq!(account.usage(), OutputCharge::default());
    }
}

#[test]
fn removal_preserves_handed_off_leases_and_acks_and_cannot_remove_readded_generation() {
    let (mut host, world, output) = fixture(7);
    let mut system = LifecyclePublisherSystem::default();
    let first = member(&output, 1);
    cut(
        &mut system,
        &mut host,
        world,
        &prepare(
            &output,
            LifecycleMembershipAction::Add,
            std::slice::from_ref(&first),
            1,
        ),
    );
    let ack = output.pop_front().unwrap();
    system.observe(1, &deleted(1));
    let event = output.pop_front().unwrap();
    let held = ack.charge().bytes + event.charge().bytes;
    system.observe(2, &deleted(1));
    cut(
        &mut system,
        &mut host,
        world,
        &prepare(
            &output,
            LifecycleMembershipAction::Remove,
            std::slice::from_ref(&first),
            2,
        ),
    );
    assert!(matches!(
        records(&output).as_slice(),
        [LifecycleWatchRecord {
            body: LifecycleWatchRecordBody::Acknowledgement {
                request: 2,
                ..
            },
            ..
        }]
    ));
    assert!(output.account().usage().bytes >= held);
    let second = member(&output, 1);
    cut(
        &mut system,
        &mut host,
        world,
        &prepare(
            &output,
            LifecycleMembershipAction::Add,
            std::slice::from_ref(&second),
            3,
        ),
    );
    cut(
        &mut system,
        &mut host,
        world,
        &prepare(
            &output,
            LifecycleMembershipAction::Remove,
            std::slice::from_ref(&first),
            4,
        ),
    );
    system.observe(3, &deleted(1));
    let result = records(&output);
    assert!(matches!(result.as_slice(), [
        LifecycleWatchRecord { body: LifecycleWatchRecordBody::Acknowledgement { request: 3, .. }, .. },
        LifecycleWatchRecord { body: LifecycleWatchRecordBody::Acknowledgement { request: 4, .. }, .. },
        LifecycleWatchRecord { body: LifecycleWatchRecordBody::Event { member, .. }, .. },
    ] if *member == second.id()));
    assert!(!first.is_active());
    assert!(second.is_active());
    output.close();
    assert!(output.account().usage().bytes >= held);
    assert!(
        matches!(event.record().body, LifecycleWatchRecordBody::Event { member, tick: 1, .. } if member == first.id())
    );
    drop(event);
    drop(ack);
    system.release_session(7);
    drop(first);
    drop(second);
    let account = output.account().clone();
    drop(output);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn shared_target_index_lives_until_its_last_member_leaves_without_tombstones() {
    let (mut host, world, output) = fixture(7);
    let mut system = LifecyclePublisherSystem::default();
    for iteration in 0..256 {
        let first = member(&output, 1);
        let second = member(&output, 1);
        cut(
            &mut system,
            &mut host,
            world,
            &prepare(
                &output,
                LifecycleMembershipAction::Add,
                &[first.clone(), second.clone()],
                1,
            ),
        );
        assert_eq!(system.targets.counts(), (1, 1, 2));
        cut(
            &mut system,
            &mut host,
            world,
            &prepare(&output, LifecycleMembershipAction::Remove, &[first], 2),
        );
        assert_eq!(system.targets.counts(), (1, 1, 1));
        system.observe(iteration, &deleted(1));
        let retained = records(&output);
        assert_eq!(retained.len(), 3);
        assert!(
            matches!(retained.last().unwrap().body, LifecycleWatchRecordBody::Event { member, .. } if member == second.id())
        );
        cut(
            &mut system,
            &mut host,
            world,
            &prepare(&output, LifecycleMembershipAction::Remove, &[second], 3),
        );
        records(&output);
        system.drain_events(7);
        assert_eq!(system.targets.counts(), (0, 0, 0));
        assert!(system.sessions.is_empty());
    }
    let account = output.account().clone();
    output.close();
    drop(output);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn more_than_128_tracked_entities_deleted_in_one_frame_deliver_every_record_in_order() {
    const COUNT: usize = 300;
    let (mut host, world_id, output) = fixture(7);
    let mut world = host.world_mut(world_id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: (1..=COUNT as u32)
                .map(|alias| Command::Create {
                    alias,
                    metadata: EntityMetadata::default(),
                    adopt: false,
                })
                .collect(),
        })
        .unwrap();
    let created = world.step(0.0).unwrap().outcomes.remove(0).result.unwrap();
    let entities: Vec<_> = created.iter().map(|(_, entity)| *entity).collect();
    assert_eq!(entities.len(), COUNT);

    let members: Vec<_> = entities
        .iter()
        .map(|entity| member(&output, entity.to_bits()))
        .collect();
    for (ordinal, page) in members.chunks(47).enumerate() {
        world
            .enqueue_system_command(
                LifecyclePublisherSystem::ID,
                7,
                prepare(
                    &output,
                    LifecycleMembershipAction::Add,
                    page,
                    ordinal as u64 + 1,
                ),
            )
            .unwrap();
    }
    world
        .enqueue_system_command(
            LifecyclePublisherSystem::ID,
            7,
            LifecyclePublisherCommand::Subscribe {
                subscription: 9,
                filter: LifecycleFilter {
                    components: false,
                    assets: false,
                    ..Default::default()
                },
            },
        )
        .unwrap();
    world.step(0.0).unwrap();
    assert_eq!(records(&output).len(), COUNT.div_ceil(47));
    world.drain_system_events::<LifecyclePublisherOutput>(LifecyclePublisherSystem::ID, 7);

    world
        .enqueue(Batch {
            id: 2,
            operations: entities
                .iter()
                .map(|&entity| Command::Delete {
                    entity: EntityRef::Handle(entity),
                })
                .collect(),
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());

    let deliveries = records(&output);
    assert_eq!(deliveries.len(), COUNT);
    let mut last = 0;
    for ((delivery, member), entity) in deliveries.iter().zip(&members).zip(&entities) {
        let LifecycleWatchRecordBody::Event {
            member: id,
            sequence,
            observation,
            ..
        } = &delivery.body
        else {
            panic!("expected a lifecycle event, got {:?}", delivery.body);
        };
        assert_eq!(*id, member.id());
        assert_eq!(
            observation,
            &LifecycleObservation::Entity {
                entity: *entity,
                kind: EntityLifecycleKind::Deleted,
            }
        );
        assert!(*sequence > last);
        last = *sequence;
    }
    assert!(members.iter().all(LifecycleWatchMember::is_active));
    assert_eq!(output.account().status(), OutputStatus::Open);

    let published: Vec<_> = world
        .drain_system_events::<LifecyclePublisherOutput>(LifecyclePublisherSystem::ID, 7)
        .into_iter()
        .flat_map(|LifecyclePublisherOutput(events)| events)
        .map(|event| event.observation)
        .collect();
    assert_eq!(
        published,
        entities
            .iter()
            .map(|&entity| LifecycleObservation::Entity {
                entity,
                kind: EntityLifecycleKind::Deleted,
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn prepare_accounts_request_and_reply_bytes_and_returns_the_reserved_lease_on_failure() {
    let (host, world, _) = fixture(7);
    let account = ReliableOutputAccount::new(OutputLimits {
        bytes: 4096,
        reply_reserve: 0,
    });
    let output = LifecycleWatchOutput::new(
        account.clone(),
        host.world_ref(world).unwrap(),
        7,
        LifecycleWatchEncoding {
            baseline_bytes: 8192,
            baseline_field_bytes: 4,
            ..Default::default()
        },
    )
    .unwrap();
    let target = member(&output, 1);
    let before = account.usage();
    let lease = account
        .reserve(OutputCharge {
            entries: 1,
            bytes: 13,
        })
        .unwrap();
    let error = LifecycleMembershipCommand::prepare(
        &output,
        LifecycleMembershipAction::Add,
        vec![target],
        1,
        lease,
    )
    .err()
    .unwrap();
    assert_eq!(
        error.reason,
        LifecycleMembershipPrepareFailure::Output(OutputReserveError::Capacity)
    );
    assert_eq!(
        error.lease.charge(),
        OutputCharge {
            entries: 1,
            bytes: 13
        }
    );
    drop(error);
    assert!(account.usage().bytes < before.bytes);
    assert_eq!(account.status(), OutputStatus::Open);
    assert!(output.acknowledgement_charge(usize::MAX, 0).is_none());
    assert!(output.acknowledgement_charge(0, usize::MAX).is_none());
}

#[test]
fn ordered_pending_remove_add_and_cancellation_do_not_resurrect_generations() {
    let (mut host, world, output) = fixture(7);
    let mut system = LifecyclePublisherSystem::default();
    let target = member(&output, 1);
    let add = prepare(
        &output,
        LifecycleMembershipAction::Add,
        std::slice::from_ref(&target),
        1,
    );
    cut(
        &mut system,
        &mut host,
        world,
        &prepare(
            &output,
            LifecycleMembershipAction::Remove,
            std::slice::from_ref(&target),
            2,
        ),
    );
    cut(&mut system, &mut host, world, &add);
    assert!(matches!(
        &records(&output)[1].body,
        LifecycleWatchRecordBody::Acknowledgement {
            result: LifecycleMembershipResult::Rejected(LifecycleMembershipRejection::StaleMember),
            ..
        }
    ));
    let next = member(&output, 1);
    let pending = prepare(
        &output,
        LifecycleMembershipAction::Add,
        std::slice::from_ref(&next),
        3,
    );
    drop(pending);
    assert!(matches!(
        records(&output).as_slice(),
        [LifecycleWatchRecord {
            body: LifecycleWatchRecordBody::Acknowledgement {
                request: 3,
                cut: None,
                result: LifecycleMembershipResult::Cancelled,
                ..
            },
            ..
        }]
    ));
    assert!(!next.is_active());
    let last = member(&output, 1);
    let pending = prepare(&output, LifecycleMembershipAction::Add, &[last], 4);
    output.close();
    cut(&mut system, &mut host, world, &pending);
    assert!(records(&output).is_empty());
    assert_eq!(system.targets.counts(), (0, 0, 0));
}

#[test]
fn real_ingress_freezes_baseline_before_later_replacement_and_fences_world_and_session() {
    let (mut host, world_id, output) = fixture(7);
    let other = host
        .create_world(Default::default(), FIXTURE_SYSTEMS)
        .unwrap();
    #[cfg(feature = "gui")]
    let value = ComponentValue::GuiButton(Default::default());
    #[cfg(not(feature = "gui"))]
    let value = ComponentValue::Transform(Default::default());
    let component = value.type_id();

    let mut world = host.world_mut(world_id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: EntityMetadata {
                        symbolic_id: Some("strict".into()),
                        classes: Vec::new(),
                    },
                    adopt: false,
                },
                Command::insert_value(EntityRef::Alias(1), value.clone()),
            ],
        })
        .unwrap();
    let attachment = world.step(0.0).unwrap().outcomes.remove(0);
    let entity = attachment.result.as_ref().unwrap()[0].1;
    let initial = world.world.state.entities[&entity]
        .input(component)
        .unwrap()
        .incarnation;
    let target = output
        .new_member(
            LifecycleWatchTarget::Component(entity, component),
            LifecycleWatchKinds::COMPONENT_RETIRED,
        )
        .unwrap();
    world
        .enqueue_system_command(
            LifecyclePublisherSystem::ID,
            7,
            prepare(
                &output,
                LifecycleMembershipAction::Add,
                std::slice::from_ref(&target),
                11,
            ),
        )
        .unwrap();
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::insert_value(EntityRef::Handle(entity), value)],
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    assert!(report.system_command_outcomes.is_empty());
    let replacement = world.world.state.entities[&entity]
        .input(component)
        .unwrap()
        .incarnation;
    assert_ne!(initial, replacement);
    #[cfg(feature = "gui")]
    assert!(
        world.inspect(entity).unwrap().components.iter().any(
            |value| matches!(value, ComponentValue::GuiBehavior(behavior) if behavior.available)
        )
    );

    let ack = output.pop_front().unwrap();
    assert!(
        matches!(&ack.record().body, LifecycleWatchRecordBody::Acknowledgement { request: 11, result: LifecycleMembershipResult::Applied(baselines), .. } if baselines[0].lifetime == LifecycleTargetLifetime::Component { entity_live: true, incarnation: Some(initial) })
    );
    assert!(
        matches!(records(&output)[0].body, LifecycleWatchRecordBody::Event { observation: LifecycleObservation::Component { previous_incarnation: Some(previous), incarnation: Some(next), .. }, .. } if previous == initial && next == replacement)
    );
    drop(world);
    for (receiver, session, expected) in [
        (other, 7, LifecycleMembershipRejection::StaleWorld),
        (world_id, 8, LifecycleMembershipRejection::StaleSession),
    ] {
        let command = prepare(
            &output,
            LifecycleMembershipAction::Remove,
            std::slice::from_ref(&target),
            12,
        );
        let mut world = host.world_mut(receiver).unwrap();
        world
            .enqueue_system_command(LifecyclePublisherSystem::ID, session, command)
            .unwrap();
        world.step(0.0).unwrap();
        assert!(
            matches!(&records(&output)[0].body, LifecycleWatchRecordBody::Acknowledgement { result: LifecycleMembershipResult::Rejected(reason), .. } if *reason == expected)
        );
        assert!(target.is_active());
    }
    let mut world = host.world_mut(world_id).unwrap();
    world
        .enqueue_system_command(
            LifecyclePublisherSystem::ID,
            7,
            prepare(
                &output,
                LifecycleMembershipAction::Remove,
                std::slice::from_ref(&target),
                13,
            ),
        )
        .unwrap();
    world.release_system_session(7);
    assert!(!target.is_active());
    assert!(
        output
            .new_member(
                LifecycleWatchTarget::Entity(entity),
                LifecycleWatchKinds::ENTITY_DELETED
            )
            .is_err()
    );
    assert!(matches!(
        &records(&output)[0].body,
        LifecycleWatchRecordBody::Acknowledgement {
            request: 13,
            result: LifecycleMembershipResult::Cancelled,
            ..
        }
    ));
}

#[test]
fn more_than_129_entities_and_64_targets_mount_without_observing_creation_traffic() {
    let (mut host, world_id, output) = fixture(7);
    let mut world = host.world_mut(world_id).unwrap();
    let mut operations = Vec::new();
    for alias in 1..=200 {
        operations.push(Command::Create {
            alias,
            metadata: EntityMetadata::default(),
            adopt: false,
        });
        operations.push(Command::insert_value(
            EntityRef::Alias(alias),
            ComponentValue::Transform(Default::default()),
        ));
    }
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    let created = world.step(0.0).unwrap().outcomes.remove(0).result.unwrap();
    assert_eq!(created.len(), 200);
    let members: Vec<_> = created
        .iter()
        .map(|(_, entity)| {
            output
                .new_member(
                    LifecycleWatchTarget::Component(*entity, ComponentValue::TRANSFORM),
                    LifecycleWatchKinds::COMPONENT_RETIRED,
                )
                .unwrap()
        })
        .collect();
    for (ordinal, page) in members.chunks(47).enumerate() {
        world
            .enqueue_system_command(
                LifecyclePublisherSystem::ID,
                7,
                prepare(
                    &output,
                    LifecycleMembershipAction::Add,
                    page,
                    ordinal as u64 + 1,
                ),
            )
            .unwrap();
    }
    world.step(0.0).unwrap();
    assert_eq!(records(&output).len(), 5);
    assert!(members.iter().all(LifecycleWatchMember::is_active));
    let before = output.account().usage();
    let mut operations = Vec::new();
    for alias in 1..=200 {
        operations.push(Command::Create {
            alias,
            metadata: EntityMetadata::default(),
            adopt: false,
        });
        operations.push(Command::insert_value(
            EntityRef::Alias(alias),
            ComponentValue::Transform(Default::default()),
        ));
        operations.push(Command::insert_value(
            EntityRef::Alias(alias),
            ComponentValue::Transform(Default::default()),
        ));
        operations.push(Command::Delete {
            entity: EntityRef::Alias(alias),
        });
    }
    world
        .enqueue(Batch {
            id: 2,
            operations,
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    assert_eq!(output.account().usage(), before);
    assert!(records(&output).is_empty());
    assert!(members.iter().all(LifecycleWatchMember::is_active));
}

#[test]
fn deleted_generation_never_follows_allocator_reuse_and_baselines_distinguish_absence() {
    let (mut host, world_id, output) = fixture(7);
    let mut world = host.world_mut(world_id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![Command::Create {
                alias: 1,
                metadata: EntityMetadata::default(),
                adopt: false,
            }],
        })
        .unwrap();
    let first = world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    let entity_member = output
        .new_member(
            LifecycleWatchTarget::Entity(first),
            LifecycleWatchKinds::ENTITY_DELETED,
        )
        .unwrap();
    let component_member = output
        .new_member(
            LifecycleWatchTarget::Component(first, ComponentValue::TRANSFORM),
            LifecycleWatchKinds::COMPONENT_RETIRED,
        )
        .unwrap();
    world
        .enqueue_system_command(
            LifecyclePublisherSystem::ID,
            7,
            prepare(
                &output,
                LifecycleMembershipAction::Add,
                &[entity_member.clone(), component_member.clone()],
                1,
            ),
        )
        .unwrap();
    world.step(0.0).unwrap();
    assert!(
        matches!(&records(&output)[0].body, LifecycleWatchRecordBody::Acknowledgement { result: LifecycleMembershipResult::Applied(baselines), .. } if baselines[1].lifetime == LifecycleTargetLifetime::Component { entity_live: true, incarnation: None })
    );
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::Delete {
                entity: EntityRef::Handle(first),
            }],
        })
        .unwrap();
    world.step(0.0).unwrap();
    assert!(
        matches!(&records(&output)[0].body, LifecycleWatchRecordBody::Event { member, .. } if *member == entity_member.id())
    );
    world
        .enqueue(Batch {
            id: 3,
            operations: vec![Command::Create {
                alias: 1,
                metadata: EntityMetadata::default(),
                adopt: false,
            }],
        })
        .unwrap();
    let second = world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    assert_eq!(first.index(), second.index());
    assert_ne!(first, second);
    world
        .enqueue(Batch {
            id: 4,
            operations: vec![Command::Delete {
                entity: EntityRef::Handle(second),
            }],
        })
        .unwrap();
    world.step(0.0).unwrap();
    assert!(records(&output).is_empty());
    let absent = output
        .new_member(
            LifecycleWatchTarget::Component(first, ComponentValue::TRANSFORM),
            LifecycleWatchKinds::COMPONENT_RETIRED,
        )
        .unwrap();
    world
        .enqueue_system_command(
            LifecyclePublisherSystem::ID,
            7,
            prepare(&output, LifecycleMembershipAction::Add, &[absent], 2),
        )
        .unwrap();
    world.step(0.0).unwrap();
    assert!(
        matches!(&records(&output)[0].body, LifecycleWatchRecordBody::Acknowledgement { result: LifecycleMembershipResult::Applied(baselines), .. } if baselines[0].lifetime == LifecycleTargetLifetime::Component { entity_live: false, incarnation: None })
    );
}

#[test]
fn reliable_output_exhaustion_preserves_reserved_ack_and_only_fails_its_account() {
    let (mut host, world, healthy) = fixture(8);
    let account = ReliableOutputAccount::new(OutputLimits {
        bytes: 1024 * 1024,
        reply_reserve: 0,
    });
    let stalled = LifecycleWatchOutput::new(
        account.clone(),
        host.world_ref(world).unwrap(),
        7,
        LifecycleWatchEncoding::default(),
    )
    .unwrap();
    let mut system = LifecyclePublisherSystem::default();
    let stalled_member = member(&stalled, 1);
    let healthy_member = member(&healthy, 1);
    cut(
        &mut system,
        &mut host,
        world,
        &prepare(
            &stalled,
            LifecycleMembershipAction::Add,
            &[stalled_member],
            1,
        ),
    );
    cut(
        &mut system,
        &mut host,
        world,
        &prepare(
            &healthy,
            LifecycleMembershipAction::Add,
            &[healthy_member],
            2,
        ),
    );
    let filler = account
        .reserve(OutputCharge {
            entries: 0,
            bytes: 1024 * 1024 - account.usage().bytes,
        })
        .unwrap();
    system.observe(1, &deleted(1));
    assert_eq!(
        account.status(),
        OutputStatus::Failed(OutputFailure::Capacity)
    );
    assert!(matches!(
        records(&stalled).as_slice(),
        [LifecycleWatchRecord {
            body: LifecycleWatchRecordBody::Acknowledgement {
                request: 1,
                ..
            },
            ..
        }]
    ));
    assert_eq!(healthy.account().status(), OutputStatus::Open);
    assert!(matches!(
        records(&healthy).last().unwrap().body,
        LifecycleWatchRecordBody::Event { .. }
    ));
    system.release_session(7);
    stalled.close();
    drop(stalled);
    drop(filler);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn membership_pages_validate_atomically_and_acknowledgements_are_not_observations() {
    let (mut host, world, output) = fixture(7);
    let mut system = LifecyclePublisherSystem::default();
    let first = member(&output, 1);
    cut(
        &mut system,
        &mut host,
        world,
        &prepare(
            &output,
            LifecycleMembershipAction::Add,
            std::slice::from_ref(&first),
            1,
        ),
    );
    let next = member(&output, 2);
    cut(
        &mut system,
        &mut host,
        world,
        &prepare(
            &output,
            LifecycleMembershipAction::Add,
            &[first.clone(), next.clone()],
            2,
        ),
    );
    assert!(first.is_active());
    assert!(!next.is_active());
    assert_eq!(system.targets.counts(), (1, 1, 1));
    assert!(matches!(
        &records(&output)[1].body,
        LifecycleWatchRecordBody::Acknowledgement {
            result: LifecycleMembershipResult::Rejected(
                LifecycleMembershipRejection::AlreadyActive
            ),
            ..
        }
    ));

    for request in 3..=132 {
        cut(
            &mut system,
            &mut host,
            world,
            &prepare(
                &output,
                LifecycleMembershipAction::Remove,
                std::slice::from_ref(&next),
                request,
            ),
        );
    }
    system.observe(1, &deleted(1));
    assert!(first.is_active());
    let retained = records(&output);
    assert_eq!(retained.len(), 131);
    assert!(matches!(
        retained[130].body,
        LifecycleWatchRecordBody::Event { .. }
    ));

    let wrong_account = ReliableOutputAccount::new(OutputLimits {
        bytes: 1024,
        reply_reserve: 0,
    });
    let lease = wrong_account
        .reserve(OutputCharge {
            entries: 1,
            bytes: 7,
        })
        .unwrap();
    let error = LifecycleMembershipCommand::prepare(
        &output,
        LifecycleMembershipAction::Remove,
        vec![first],
        133,
        lease,
    )
    .err()
    .unwrap();
    assert_eq!(
        error.reason,
        LifecycleMembershipPrepareFailure::InvalidLease
    );
    assert!(error.lease.belongs_to(&wrong_account));
    assert_eq!(
        error.lease.charge(),
        OutputCharge {
            entries: 1,
            bytes: 7
        }
    );
}

#[test]
fn world_incarnations_fence_reused_host_local_ids_and_drop_retires_tracking() {
    let (mut host, world_id, output) = fixture(7);
    let target = member(&output, 1);
    host.world_mut(world_id)
        .unwrap()
        .enqueue_system_command(
            LifecyclePublisherSystem::ID,
            7,
            prepare(
                &output,
                LifecycleMembershipAction::Add,
                std::slice::from_ref(&target),
                1,
            ),
        )
        .unwrap();
    host.world_mut(world_id).unwrap().step(0.0).unwrap();
    assert!(target.is_active());

    let mut replacement_host = HostRuntime::new();
    let replacement_world = replacement_host
        .create_world(Default::default(), FIXTURE_SYSTEMS)
        .unwrap();
    assert_eq!(replacement_world, world_id);
    assert_ne!(
        replacement_host.world_ref(replacement_world).unwrap(),
        output.0.world
    );
    replacement_host
        .world_mut(replacement_world)
        .unwrap()
        .enqueue_system_command(
            LifecyclePublisherSystem::ID,
            7,
            prepare(
                &output,
                LifecycleMembershipAction::Remove,
                std::slice::from_ref(&target),
                2,
            ),
        )
        .unwrap();
    replacement_host
        .world_mut(replacement_world)
        .unwrap()
        .step(0.0)
        .unwrap();
    assert!(matches!(
        &records(&output)[1].body,
        LifecycleWatchRecordBody::Acknowledgement {
            result: LifecycleMembershipResult::Rejected(LifecycleMembershipRejection::StaleWorld),
            ..
        }
    ));
    assert!(target.is_active());

    host.destroy_world(world_id);
    assert!(!target.is_active());
    let account = output.account().clone();
    drop(target);
    drop(output);
    assert_eq!(account.usage(), OutputCharge::default());
}

fn value_member(output: &LifecycleWatchOutput, entity: EntityId) -> LifecycleWatchMember {
    output
        .new_member(
            LifecycleWatchTarget::Value(
                entity,
                ComponentValue::SCALAR,
                std::sync::Arc::from([
                    std::mem::offset_of!(crate::components::Scalar, value) as u32
                ]),
            ),
            LifecycleWatchKinds::VALUE_CHANGED,
        )
        .unwrap()
}

/// `(member generation, tick, reported Scalar value)` of each value record.
fn value_records(records: &[LifecycleWatchRecord]) -> Vec<(u64, u64, Option<f32>)> {
    records
        .iter()
        .filter_map(|record| match &record.body {
            LifecycleWatchRecordBody::Value {
                member,
                tick,
                values,
            } => Some((
                member.generation,
                *tick,
                values.as_ref().map(|values| match values.as_slice() {
                    [(_, crate::components::schema::FieldValue::F32(value))] => *value,
                    other => panic!("unexpected values {other:?}"),
                }),
            )),
            _ => None,
        })
        .collect()
}

fn set_scalar(entity: EntityId, value: f32) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::SCALAR,
        field: crate::FieldWrite {
            offset: std::mem::offset_of!(crate::components::Scalar, value) as u32,
            value: crate::FieldValue::F32(value),
        },
    }
}

fn scalar_entity(world: &mut crate::WorldContext<'_>, value: f32) -> EntityId {
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::Scalar(crate::components::Scalar {
                        value,
                    }),
                ),
            ],
        })
        .unwrap();
    world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1
}

fn add_members(
    world: &mut crate::WorldContext<'_>,
    output: &LifecycleWatchOutput,
    members: &[LifecycleWatchMember],
    request: u64,
) {
    world
        .enqueue_system_command(
            LifecyclePublisherSystem::ID,
            7,
            prepare(output, LifecycleMembershipAction::Add, members, request),
        )
        .unwrap();
}

#[test]
fn value_members_report_current_values_after_the_ack_then_once_per_changed_frame() {
    let (mut host, world_id, output) = fixture(7);
    let mut world = host.world_mut(world_id).unwrap();
    let entity = scalar_entity(&mut world, 1.0);
    let member = value_member(&output, entity);
    let generation = member.id().generation;
    add_members(&mut world, &output, std::slice::from_ref(&member), 21);
    let first = world.step(0.0).unwrap().tick;

    // The ACK comes first, then the current value of the frame that applied it.
    let delivered = records(&output);
    assert!(matches!(
        &delivered[0].body,
        LifecycleWatchRecordBody::Acknowledgement {
            request: 21,
            result: LifecycleMembershipResult::Applied(_),
            ..
        }
    ));
    assert_eq!(value_records(&delivered), [(generation, first, Some(1.0))]);
    assert_eq!(delivered.len(), 2);

    // Two writes in one frame report the frame's final value once.
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![set_scalar(entity, 2.0), set_scalar(entity, 3.0)],
        })
        .unwrap();
    let changed = world.step(0.0).unwrap().tick;
    assert!(changed > first);
    assert_eq!(
        value_records(&records(&output)),
        [(generation, changed, Some(3.0))]
    );

    // Unchanged frames and rewrites of the same value report nothing.
    world.step(0.0).unwrap();
    world
        .enqueue(Batch {
            id: 3,
            operations: vec![set_scalar(entity, 3.0)],
        })
        .unwrap();
    world.step(0.0).unwrap();
    assert!(records(&output).is_empty());

    // Absence is reported once.
    world
        .enqueue(Batch {
            id: 4,
            operations: vec![Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::SCALAR,
            }],
        })
        .unwrap();
    let removed = world.step(0.0).unwrap().tick;
    assert_eq!(
        value_records(&records(&output)),
        [(generation, removed, None)]
    );
    world.step(0.0).unwrap();
    assert!(records(&output).is_empty());
}

#[test]
fn value_records_observe_the_values_systems_evaluated_in_the_same_frame() {
    let (mut host, world_id, output) = fixture(7);
    let mut world = host.world_mut(world_id).unwrap();
    let source = scalar_entity(&mut world, 3.0);
    let target = scalar_entity(&mut world, 1.0);
    let member = value_member(&output, target);
    let generation = member.id().generation;
    add_members(&mut world, &output, std::slice::from_ref(&member), 22);
    world.step(0.0).unwrap();
    records(&output);

    // The constraint evaluates during the frame; the Observe phase after every
    // System's Finish reports its result, not the value before evaluation.
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::insert_value(
                EntityRef::Handle(target),
                ComponentValue::LinearDriver(crate::systems::constraints::LinearDriver {
                    source,
                    scale: 2.0,
                    bias: 1.0,
                }),
            )],
        })
        .unwrap();
    let tick = world.step(0.0).unwrap().tick;
    assert_eq!(
        value_records(&records(&output)),
        [(generation, tick, Some(7.0))]
    );
}

#[test]
fn an_undelivered_value_record_is_superseded_and_moves_behind_earlier_records() {
    let (mut host, world_id, output) = fixture(7);
    let mut world = host.world_mut(world_id).unwrap();
    let a = scalar_entity(&mut world, 1.0);
    let b = scalar_entity(&mut world, 10.0);
    let members = [value_member(&output, a), value_member(&output, b)];
    let [first, second] = members.each_ref().map(|member| member.id().generation);
    add_members(&mut world, &output, &members, 23);
    world.step(0.0).unwrap();
    records(&output);

    let change = |world: &mut crate::WorldContext<'_>, entity, value| {
        world
            .enqueue(Batch {
                id: 2,
                operations: vec![set_scalar(entity, value)],
            })
            .unwrap();
        world.step(0.0).unwrap().tick
    };
    change(&mut world, a, 2.0);
    let b_tick = change(&mut world, b, 20.0);
    let a_tick = change(&mut world, a, 3.0);

    // A's first undelivered record is replaced by its newer value, which now
    // follows B's record; nothing reports the intermediate value 2.
    assert_eq!(
        value_records(&records(&output)),
        [(second, b_tick, Some(20.0)), (first, a_tick, Some(3.0))]
    );
}
