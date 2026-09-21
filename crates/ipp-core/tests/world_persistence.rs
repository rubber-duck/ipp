//! Durable World metadata, authored capture and asynchronous output invariants.

use ipp_core::services::data_source::{DataWriteJob, DataWriter, MemoryDataWriter};
use ipp_core::services::world_serialization::{
    WorldLoadOptions, WorldPersistenceLimits, WorldSnapshot,
};
use ipp_core::{
    Batch, Command, ComponentValue, EntityMetadata, EntityRef, FieldValue, FieldWrite, HostRuntime,
    WorldCapacityHints, WorldCreateOptions, WorldLimits, WorldSelector,
};
use std::task::{Context, Poll, Waker};

fn populated() -> (HostRuntime, ipp_core::WorldId) {
    let mut host = HostRuntime::new();
    let id = host
        .create_world_with_options(
            WorldLimits::default(),
            WorldCreateOptions {
                symbolic_id: "authored".into(),
                capacity_hints: WorldCapacityHints {
                    entities: 1,
                    ..Default::default()
                },
            },
        )
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    for index in 0..3 {
        world
            .enqueue(Batch {
                id: index as u64,
                operations: vec![
                    Command::Create {
                        alias: 0,
                        metadata: EntityMetadata {
                            symbolic_id: Some(format!("scalar-{index}")),
                            classes: vec!["authored".into()],
                        },
                    },
                    Command::InsertComponent {
                        entity: EntityRef::Alias(0),
                        component: ComponentValue::SCALAR,
                        fields: vec![FieldWrite {
                            offset: 0,
                            value: FieldValue::F32(index as f32 + 0.5),
                        }],
                    },
                ],
            })
            .unwrap();
        assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    }
    drop(world);
    (host, id)
}

fn save(host: &mut HostRuntime, id: ipp_core::WorldId) -> Vec<u8> {
    host.save_world(id, 123, WorldPersistenceLimits::default())
        .unwrap()
}

#[test]
fn hints_grow_and_named_worlds_round_trip_with_fresh_handles() {
    let (mut host, id) = populated();
    let persistent = host.world_mut(id).unwrap().metadata().persistent_id;
    let before = host.world_mut(id).unwrap().entities();
    let bytes = save(&mut host, id);
    let limits = WorldPersistenceLimits::default();
    let snapshot = WorldSnapshot::decode(&bytes, 123, limits).unwrap();
    assert_eq!(snapshot.capacity_hints.entities, 1);
    assert_eq!(snapshot.entities.len(), 3);
    assert!(
        host.load_world(
            &bytes,
            123,
            WorldLoadOptions::default(),
            WorldLimits::default(),
            limits
        )
        .unwrap_err()
        .contains("already exists")
    );
    assert_eq!(host.list_worlds().len(), 1);
    let restored = host
        .load_world(
            &bytes,
            123,
            WorldLoadOptions {
                symbolic_id: Some("copy".into()),
                ..Default::default()
            },
            WorldLimits::default(),
            limits,
        )
        .unwrap();
    assert_ne!(restored, id);
    assert_eq!(
        host.world_mut(restored).unwrap().metadata().persistent_id,
        persistent
    );
    let after = host.world_mut(restored).unwrap().entities();
    assert_eq!(
        before
            .iter()
            .map(|entity| (&entity.metadata, &entity.base))
            .collect::<Vec<_>>(),
        after
            .iter()
            .map(|entity| (&entity.metadata, &entity.base))
            .collect::<Vec<_>>()
    );
    host.rename_world(restored, "renamed".into()).unwrap();
    assert_eq!(
        host.resolve_world(&WorldSelector::SymbolicId("renamed".into())),
        Some(restored)
    );
    let mut world = host.world_mut(restored).unwrap();
    world
        .set_capacity_hints(WorldCapacityHints {
            entities: 1024,
            ..Default::default()
        })
        .unwrap();
    world
        .set_capacity_hints(WorldCapacityHints {
            entities: 0,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(world.entities().len(), 3);
    assert_eq!(world.metadata().persistent_id, persistent);
}

#[test]
fn corruption_and_contract_mismatch_preserve_all_published_worlds() {
    let (mut host, id) = populated();
    let bytes = save(&mut host, id);
    let before = host.list_worlds();
    for index in [0, 8, 16, 24, bytes.len() - 1] {
        let mut bad = bytes.clone();
        bad[index] ^= 1;
        assert!(
            host.load_world(
                &bad,
                123,
                WorldLoadOptions::default(),
                WorldLimits::default(),
                WorldPersistenceLimits::default()
            )
            .is_err()
        );
        assert_eq!(host.list_worlds(), before);
    }
    for length in 0..32 {
        assert!(
            WorldSnapshot::decode(&bytes[..length], 123, WorldPersistenceLimits::default())
                .is_err()
        );
    }
}

#[test]
fn capture_is_detached_from_later_authoring_and_world_destruction() {
    let (mut host, id) = populated();
    let bytes = save(&mut host, id);
    assert!(host.destroy_world(id));
    let snapshot = WorldSnapshot::decode(&bytes, 123, WorldPersistenceLimits::default()).unwrap();
    assert_eq!(snapshot.entities.len(), 3);
    let id = host
        .load_world(
            &bytes,
            123,
            WorldLoadOptions::default(),
            WorldLimits::default(),
            WorldPersistenceLimits::default(),
        )
        .unwrap();
    assert_eq!(host.world_mut(id).unwrap().entities().len(), 3);
}

struct ShortWriter {
    memory: MemoryDataWriter,
    pending: bool,
}

impl DataWriter for ShortWriter {
    fn poll_write(&mut self, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<Result<usize, String>> {
        self.pending = !self.pending;
        if self.pending {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            self.memory.poll_write(cx, &bytes[..bytes.len().min(3)])
        }
    }

    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        self.memory.poll_flush(cx)
    }

    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        self.memory.poll_finish(cx)
    }

    fn abort(&mut self) {
        self.memory.abort();
    }
}

#[test]
fn async_writer_handles_partial_progress_and_publication() {
    let bytes: Vec<_> = (0..255).collect();
    let mut job = DataWriteJob::new(
        bytes.clone(),
        ShortWriter {
            memory: MemoryDataWriter::new(1024),
            pending: false,
        },
    );
    for _ in 0..300 {
        match job.poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Pending => {}
            Poll::Ready(result) => {
                result.unwrap();
                assert_eq!(
                    job.into_writer().unwrap().memory.into_bytes().unwrap(),
                    bytes
                );
                return;
            }
        }
    }
    panic!("writer never completed");
}

#[test]
fn world_save_preserves_unavailable_resource_references_without_fetching_assets() {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let source = "https://unavailable.invalid/unchanged.mesh?variant=authored";
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: Default::default(),
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(0),
                    component: ComponentValue::MESH_INSTANCE,
                    fields: vec![FieldWrite {
                        offset: std::mem::offset_of!(ipp_core::components::MeshInstance, source)
                            as u32,
                        value: FieldValue::String(source.into()),
                    }],
                },
            ],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    drop(world);

    // No provider is registered for this URL and no service progress is needed.
    let bytes = save(&mut host, id);
    assert!(String::from_utf8_lossy(&bytes).contains(source));
    assert!(!String::from_utf8_lossy(&bytes).contains("bundle://"));
    let snapshot = WorldSnapshot::decode(&bytes, 123, Default::default()).unwrap();
    let component = &snapshot.entities[0].components[0];
    assert!(component.fields().iter().any(|(_, value)| {
        matches!(value, ipp_core::components::schema::FieldValue::String(value) if value == source)
    }));

    assert!(host.destroy_world(id));
    let restored = host
        .load_world(
            &bytes,
            123,
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let restored_snapshot = host
        .world_mut(restored)
        .unwrap()
        .capture_world(Default::default())
        .unwrap();
    assert_eq!(restored_snapshot, snapshot);
}

#[cfg(feature = "skeletal-animation")]
#[test]
fn typed_rig_writers_preserve_source_payloads_and_reject_small_budgets() {
    use ipp_core::services::asset_management::{
        skeleton::{PoseAsset, SkeletonAsset},
        writer::AssetEncoder,
    };
    let transform: [f32; 10] = [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let header = |magic: &[u8]| {
        let mut bytes = magic.to_vec();
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes
    };
    let mut skeleton = header(b"IPPS");
    skeleton.extend(u32::MAX.to_le_bytes());
    for value in transform {
        skeleton.extend(value.to_le_bytes());
    }
    let asset = SkeletonAsset::decode(&skeleton).unwrap();
    assert_eq!(asset.encode_asset(1024).unwrap(), skeleton);
    assert!(asset.encode_asset(8).is_err());
    let mut pose = header(b"IPPP");
    for value in transform {
        pose.extend(value.to_le_bytes());
    }
    assert_eq!(
        PoseAsset::decode(&pose)
            .unwrap()
            .encode_asset(1024)
            .unwrap(),
        pose
    );
    #[cfg(feature = "skeletal-animation")]
    {
        let mut skin = header(b"IPPB");
        skin.extend(0u32.to_le_bytes());
        for i in 0..16 {
            skin.extend(
                (if i % 5 == 0 {
                    1.0f32
                } else {
                    0.0
                })
                .to_le_bytes(),
            );
        }
        let asset =
            ipp_core::services::asset_management::skin_binding::SkinAsset::decode(&skin).unwrap();
        assert_eq!(asset.encode_asset(1024).unwrap(), skin);
    }
}

#[test]
fn authored_capture_excludes_owned_components_and_auto_fallbacks() {
    use ipp_core::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};
    let (mut host, id) = populated();
    let mut world = host.world_mut(id).unwrap();
    let entities = world.entities();
    let mut operations = vec![Command::CreateStateOverlayOwner {
        alias: 0,
    }];
    for (index, mode) in [ComponentOverlayMode::Owned, ComponentOverlayMode::Auto]
        .into_iter()
        .enumerate()
    {
        operations.push(Command::RemoveComponent {
            entity: EntityRef::Handle(entities[index + 1].id),
            component: ComponentValue::SCALAR,
        });
        operations.push(Command::AttachEntityOverlayBinding {
            owner: StateOverlayRef::Alias(0),
            alias: index as u32 * 2 + 1,
            symbolic_id: format!("scalar-{}", index + 1),
            mode: EntityOverlayMode::Bound,
        });
        operations.push(Command::AttachComponentStateOverlay {
            owner: StateOverlayRef::Alias(0),
            binding: StateOverlayRef::Alias(index as u32 * 2 + 1),
            alias: index as u32 * 2 + 2,
            component: ComponentValue::SCALAR,
            mode,
            fields: vec![FieldWrite {
                offset: 0,
                value: FieldValue::F32(99.0),
            }],
        });
    }
    world
        .enqueue(Batch {
            id: 10,
            operations,
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    let capture = world.capture_world(Default::default()).unwrap();
    assert_eq!(capture.entities.len(), 3);
    assert_eq!(capture.entities[0].components.len(), 1);
    assert!(
        capture.entities[1..]
            .iter()
            .all(|entity| entity.components.is_empty())
    );
    drop(world);
    let bytes = save(&mut host, id);
    let restored = host
        .load_world(
            &bytes,
            123,
            WorldLoadOptions {
                symbolic_id: Some("without-ui".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let restored = host
        .world_mut(restored)
        .unwrap()
        .capture_world(Default::default())
        .unwrap();
    assert_eq!(restored.entities, capture.entities);
}

#[test]
fn references_and_durable_entity_ids_survive_allocator_reuse() {
    use ipp_core::components::LinearDriver;
    let (mut host, id) = populated();
    let mut world = host.world_mut(id).unwrap();
    let entities = world.entities();
    world
        .enqueue(Batch {
            id: 10,
            operations: vec![
                Command::Delete {
                    entity: EntityRef::Handle(entities[0].id),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Handle(entities[2].id),
                    value: ComponentValue::LinearDriver(LinearDriver {
                        source: entities[1].id,
                        ..Default::default()
                    }),
                },
            ],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    let captured = world.capture_world(Default::default()).unwrap();
    drop(world);
    let bytes = save(&mut host, id);
    let restored_id = host
        .load_world(
            &bytes,
            123,
            WorldLoadOptions {
                symbolic_id: Some("references".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let mut restored = host.world_mut(restored_id).unwrap();
    assert_eq!(
        restored.capture_world(Default::default()).unwrap().entities,
        captured.entities
    );
    let new_entities = restored.entities();
    assert_ne!(new_entities[0].id, entities[1].id);
    restored
        .enqueue(Batch {
            id: 11,
            operations: vec![Command::Create {
                alias: 0,
                metadata: Default::default(),
            }],
        })
        .unwrap();
    let report = restored.step(0.0).unwrap();
    let created = report.outcomes[0].result.as_ref().unwrap()[0].1;
    assert!(restored.entity_persistent_id(created).unwrap().0 > captured.next_entity_id);
}

#[test]
fn controller_state_and_durable_bindings_round_trip_without_loaded_clips() {
    use ipp_core::systems::animation::*;
    let (mut host, id) = populated();
    let limits = WorldPersistenceLimits::default();
    let controller;
    let next_id;
    {
        let mut world = host.world_mut(id).unwrap();
        let targets = world.entities();
        let description = AnimationControllerDescription {
            drivers: targets[..2]
                .iter()
                .map(|entity| AnimationDriverDescription {
                    source: "unavailable:clip?unchanged=yes".into(),
                    variant: 7,
                    track: 0,
                    target: entity.id,
                    property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                        component: ComponentValue::SCALAR,
                        offsets: vec![0],
                    }),
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                })
                .collect(),
            ..Default::default()
        };
        controller = world
            .create_animation_controller(description.clone())
            .unwrap();
        let deleted = world.create_animation_controller(description).unwrap();
        world.remove_animation_controller(deleted).unwrap();
        world
            .control_animation_controller(controller, AnimationPlaybackControl::Play)
            .unwrap();
        world
            .control_animation_controller(controller, AnimationPlaybackControl::Pause)
            .unwrap();
        world
            .control_animation_controller(controller, AnimationPlaybackControl::Seek(0.75))
            .unwrap();
        next_id = world.animation_persistent_state().next_id;
    }
    let bytes = save(&mut host, id);
    assert_eq!(bytes, save(&mut host, id));
    let snapshot = WorldSnapshot::decode(&bytes, 123, limits).unwrap();
    let mut animation = AnimationPersistentState::decode(
        &snapshot.systems[AnimationSystem::ID.0],
        limits.max_bytes,
    )
    .unwrap();
    assert_eq!(animation.next_id, next_id);
    assert_eq!(animation.controllers.len(), 1);
    let mut restored_host = HostRuntime::new();
    let restored = restored_host
        .load_world(
            &bytes,
            123,
            WorldLoadOptions::default(),
            WorldLimits::default(),
            limits,
        )
        .unwrap();
    let world = restored_host.world_mut(restored).unwrap();
    let state = world.animation_persistent_state();
    assert_eq!(state.next_id, next_id);
    assert_eq!(state.controllers[0].id, controller);
    assert_eq!(state.controllers[0].state, AnimationPlaybackStatus::Paused);
    assert_eq!(state.controllers[0].time, 0.75);
    for driver in &state.controllers[0].description.drivers {
        assert!(world.inspect(driver.target).is_some());
        assert_eq!(driver.source, "unavailable:clip?unchanged=yes");
        assert_eq!(driver.variant, 7);
    }
    assert_eq!(world.capture_world(limits).unwrap(), snapshot);
    let mut invalid = snapshot;
    animation.controllers[0].description.drivers[0].target =
        ipp_core::EntityId::from_bits(u64::MAX);
    invalid.systems.insert(
        AnimationSystem::ID.0.to_owned(),
        animation.encode(limits.max_bytes).unwrap(),
    );
    let bytes = invalid.encode(123, limits).unwrap();
    assert!(
        HostRuntime::new()
            .load_world(&bytes, 123, Default::default(), Default::default(), limits)
            .is_err()
    );
}
