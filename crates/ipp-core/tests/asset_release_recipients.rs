//! Public Host subscriptions retain release recipients across independently stepped Worlds.

mod support;

use support::selection::{LIFECYCLE, RENDER, select};

use ipp_core::{
    AssetResourceStatus, Batch, Command, ComponentValue, EntityRef, HostRuntime, MESH_TYPE,
    WorldId,
    components::MeshInstance,
    services::asset_management::{AssetLifecycleKind, AssetLoadStatus, AssetSource},
    systems::lifecycle_publisher::{
        LifecycleFilter, LifecycleObservation, LifecyclePublisherCommand, LifecyclePublisherOutput,
        LifecyclePublisherSystem,
    },
};

const SOURCE: &str = "fixture://release-recipients.mesh";
const SESSION: u64 = 1;

#[test]
fn later_world_reconciliation_preserves_completed_release_observations() {
    let mut host = crate::support::task_scheduler::host();
    host.io_mut().register_stream("fixture://").unwrap();
    let first = host
        .create_world(Default::default(), &select(&[LIFECYCLE, RENDER]))
        .unwrap();
    let second = host
        .create_world(Default::default(), &select(&[LIFECYCLE, RENDER]))
        .unwrap();
    for world in [first, second] {
        host.world_mut(world)
            .unwrap()
            .enqueue_system_command(
                LifecyclePublisherSystem::ID,
                SESSION,
                LifecyclePublisherCommand::Subscribe {
                    subscription: 1,
                    filter: LifecycleFilter {
                        entities: false,
                        components: false,
                        assets: true,
                        ..Default::default()
                    },
                },
            )
            .unwrap();
    }
    host.world_mut(first)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(0),
                    ComponentValue::MeshInstance(MeshInstance {
                        source: SOURCE.into(),
                        variant: 0,
                    }),
                ),
            ],
        })
        .unwrap();
    let source = AssetSource {
        kind: MESH_TYPE,
        uri: SOURCE.into(),
        variant: 0,
    };
    for _ in 0..16 {
        frame(&mut host, [first, second]);
        for request in host.take_resource_requests() {
            host.complete_resource(request.id, Ok(triangle())).unwrap();
        }
        if host
            .asset_resources()
            .find(&source)
            .and_then(|key| host.asset_resources().get(key))
            .is_some_and(|resource| *resource.status() == AssetLoadStatus::Loaded)
        {
            break;
        }
    }
    let key = host.asset_resources().find(&source).unwrap();
    host.asset_resources_mut().set_idle_resident_bytes_target(0);
    assert_eq!(
        host.asset_resources().get(key).unwrap().status(),
        &AssetLoadStatus::Loaded,
        "the actual mesh provider must finish before testing its release"
    );
    assert!(drain(&mut host, first).iter().any(|event| matches!(event,
        LifecycleObservation::Asset { resource, kind: AssetLifecycleKind::StatusChanged }
            if resource.id == key.to_u64() && resource.status == AssetResourceStatus::Loaded
    )));
    assert!(drain(&mut host, second).is_empty());

    let entity = host.world_mut(first).unwrap().entities()[0].id;
    host.world_mut(first)
        .unwrap()
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::Delete {
                entity: EntityRef::Handle(entity),
            }],
        })
        .unwrap();
    for world in [first, second] {
        host.world_mut(world).unwrap().prepare_update(0.0).unwrap();
    }
    support::task_scheduler::progress(&mut host);

    // The first World drops its last reference and drains its ordinary resource
    // report. Its subscription still needs the eventual shared release event.
    let report = host.world_mut(first).unwrap().step(0.0).unwrap();
    assert!(report.outcomes[0].result.is_ok());
    assert!(
        host.world_mut(first)
            .unwrap()
            .resource_snapshots()
            .is_empty()
    );
    host.flush_resource_lifecycle();
    assert!(host.asset_resources().get(key).is_none());
    assert!(host.has_pending_world_updates());

    host.world_mut(second).unwrap().step(0.0).unwrap();
    host.flush_resource_lifecycle();
    assert!(host.asset_resources().get(key).is_none());
    let events = drain(&mut host, first);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event,
                LifecycleObservation::Asset { resource, kind: AssetLifecycleKind::Removed }
                    if resource.id == key.to_u64() && resource.source == std::sync::Arc::<str>::from(SOURCE)
            ))
            .count(),
        1,
        "the departing consumer receives exactly one applied identity removal: {events:?}"
    );
    assert!(
        drain(&mut host, second).is_empty(),
        "an unrelated World must not observe the resource"
    );
    host.flush_resource_lifecycle();
    assert!(drain(&mut host, first).is_empty());
}

#[test]
fn release_deferred_by_a_publication_lease_still_reaches_the_departed_consumer() {
    let mut host = crate::support::task_scheduler::host();
    // This test proves eviction at final demand, so the Host keeps no idle cache.
    host.asset_resources_mut().set_idle_resident_bytes_target(0);
    host.io_mut().register_stream("fixture://").unwrap();
    let world = host
        .create_world(Default::default(), &select(&[LIFECYCLE, RENDER]))
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command(
            LifecyclePublisherSystem::ID,
            SESSION,
            LifecyclePublisherCommand::Subscribe {
                subscription: 1,
                filter: LifecycleFilter {
                    entities: false,
                    components: false,
                    assets: true,
                    ..Default::default()
                },
            },
        )
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(0),
                    ComponentValue::MeshInstance(MeshInstance {
                        source: SOURCE.into(),
                        variant: 0,
                    }),
                ),
            ],
        })
        .unwrap();
    let source = AssetSource {
        kind: MESH_TYPE,
        uri: SOURCE.into(),
        variant: 0,
    };
    let mut events = Vec::new();
    for _ in 0..16 {
        support::task_scheduler::progress(&mut host);
        host.frame(0.0).unwrap();
        events.extend(drain(&mut host, world));
        for request in host.take_resource_requests() {
            host.complete_resource(request.id, Ok(triangle())).unwrap();
        }
        if host
            .asset_resources()
            .find(&source)
            .and_then(|key| host.asset_resources().get(key))
            .is_some_and(|resource| *resource.status() == AssetLoadStatus::Loaded)
        {
            break;
        }
    }
    let key = host.asset_resources().find(&source).unwrap();
    support::task_scheduler::progress(&mut host);
    host.frame(0.0).unwrap();
    events.extend(drain(&mut host, world));
    assert!(events.iter().any(|event| matches!(event,
        LifecycleObservation::Asset { resource, kind: AssetLifecycleKind::StatusChanged }
            if resource.id == key.to_u64() && resource.status == AssetResourceStatus::Loaded
    )));

    let entity = host.world_mut(world).unwrap().entities()[0].id;
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::Delete {
                entity: EntityRef::Handle(entity),
            }],
        })
        .unwrap();

    // The World reconciles its observed sources during evaluation, while the
    // completed publication still leases the mesh; the release follows later.
    let mut events = Vec::new();
    for _ in 0..4 {
        support::task_scheduler::progress(&mut host);
        host.frame(0.0).unwrap();
        events.extend(drain(&mut host, world));
    }
    assert!(host.asset_resources().get(key).is_none());
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event,
                LifecycleObservation::Asset { resource, kind: AssetLifecycleKind::Removed }
                    if resource.id == key.to_u64() && resource.source == std::sync::Arc::<str>::from(SOURCE)
            ))
            .count(),
        1,
        "the departed consumer receives exactly one identity removal: {events:?}"
    );
}

fn frame(host: &mut HostRuntime, worlds: [WorldId; 2]) {
    for world in worlds {
        host.world_mut(world).unwrap().prepare_update(0.0).unwrap();
    }
    support::task_scheduler::progress(host);
    for world in worlds {
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }
    host.flush_resource_lifecycle();
}

fn drain(host: &mut HostRuntime, world: WorldId) -> Vec<LifecycleObservation> {
    host.world_mut(world)
        .unwrap()
        .drain_system_events::<LifecyclePublisherOutput>(LifecyclePublisherSystem::ID, SESSION)
        .into_iter()
        .flat_map(|LifecyclePublisherOutput(events)| events)
        .map(|event| event.observation)
        .collect()
}

fn triangle() -> Vec<u8> {
    let mut bytes = b"IPPM".to_vec();
    for value in [1u32, 3, 3] {
        bytes.extend(value.to_le_bytes());
    }
    for position in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for value in position.into_iter().chain([1.0, 1.0, 1.0]) {
            bytes.extend(value.to_le_bytes());
        }
    }
    for index in [0u16, 1, 2] {
        bytes.extend(index.to_le_bytes());
    }
    bytes
}
