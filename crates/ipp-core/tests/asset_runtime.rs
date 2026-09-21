//! Asset-runtime lifecycle checks. Real host I/O and frame capture live
//! in the maintained native/browser harnesses; this suite controls completions.

mod support;
use support::HostWorldTestDriver;

use ipp_core::{
    AssetResourceKind, AssetResourceStatus, Batch, Command, ComponentValue, EntityId, EntityRef,
    ErrorReason, FieldValue, FieldWrite, MeshKey, MeshUpload, WorldLimits,
    components::MeshInstance,
};
use std::mem::offset_of;

fn source_world(host: &mut ipp_core::HostRuntime) -> ipp_core::WorldId {
    let world_id = host.create_world(ipp_core::WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    for scheme in ["http", "https", "file", "ipc", "ipp"] {
        world.register_stream_resource_provider(scheme).unwrap();
    }
    world_id
}

fn source(value: &str) -> FieldWrite {
    FieldWrite {
        offset: offset_of!(MeshInstance, source) as u32,
        value: FieldValue::String(value.into()),
    }
}

fn apply(
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
    operations: Vec<Command>,
) -> ipp_core::WorldUpdateReport {
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations,
            })
            .unwrap();
    }
    host.update_world_for_test(world, 0.0).unwrap()
}

fn resource_requests(
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
) -> Vec<ipp_core::AssetAcquisitionRequest> {
    host.progress_assets();
    host.world_mut(world).unwrap().take_resource_requests()
}

fn create(host: &mut ipp_core::HostRuntime, world: ipp_core::WorldId, uri: &str) -> EntityId {
    let report = apply(
        host,
        world,
        vec![
            Command::Create {
                alias: 0,
                metadata: Default::default(),
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(0),
                component: ComponentValue::TRANSFORM,
                fields: vec![],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(0),
                component: ComponentValue::UNLIT_MATERIAL,
                fields: vec![],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(0),
                component: ComponentValue::MESH_INSTANCE,
                fields: vec![source(uri)],
            },
        ],
    );
    report.outcomes[0].result.as_ref().unwrap()[0].1
}

fn set(entity: EntityId, uri: &str) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::MESH_INSTANCE,
        field: source(uri),
    }
}

fn triangle() -> Vec<u8> {
    let mut bytes = b"IPPM".to_vec();
    for n in [1u32, 3, 3] {
        bytes.extend(n.to_le_bytes());
    }
    for position in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for value in position.into_iter().chain([1.0, 1.0, 1.0]) {
            bytes.extend(value.to_le_bytes());
        }
    }
    for n in [0u16, 1, 2] {
        bytes.extend(n.to_le_bytes());
    }
    bytes
}

#[test]
fn committed_pending_demand_shares_work_and_becomes_drawable_only_at_boundary() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let world = source_world(&mut fixture_host);
    let first = create(&mut fixture_host, world, "https://assets.test/a.mesh");
    let second = create(&mut fixture_host, world, "https://assets.test/a.mesh");
    assert_eq!(fixture_host.world_mut(world).unwrap().entities().len(), 2);
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .render_items()
            .is_empty()
    );
    let snapshots = fixture_host.world_mut(world).unwrap().resource_snapshots();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].status, AssetResourceStatus::Start);
    let requests = resource_requests(&mut fixture_host, world);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].kind, AssetResourceKind::Mesh);
    assert_eq!(requests[0].source, snapshots[0].source);
    assert_eq!(requests[0].variant, 0);
    assert!(resource_requests(&mut fixture_host, world).is_empty());
    fixture_host
        .world_mut(world)
        .unwrap()
        .complete_resource(requests[0].id, Ok(triangle()))
        .unwrap();
    assert_eq!(
        fixture_host.world_mut(world).unwrap().resource_snapshots(),
        snapshots
    );
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .render_items()
            .is_empty()
    );
    assert_eq!(
        fixture_host.update_world_for_test(world, -1.0),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(
        fixture_host.world_mut(world).unwrap().resource_snapshots(),
        snapshots
    );
    fixture_host.update_world_for_test(world, 0.0).unwrap();
    let items = fixture_host
        .world_mut(world)
        .unwrap()
        .render_items()
        .to_vec();
    assert_eq!(
        items.iter().map(|i| i.entity).collect::<Vec<_>>(),
        vec![first, second]
    );
    assert_eq!(items[0].mesh, items[1].mesh);
    assert_eq!(
        fixture_host.world_mut(world).unwrap().resource_snapshots()[0].status,
        AssetResourceStatus::Loaded
    );
    let key = items[0].mesh;
    assert!(fixture_host.world_mut(world).unwrap().mesh(key).is_some());
    assert_eq!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .enqueue_mesh(MeshUpload {
                id: 1,
                key,
                bytes: triangle()
            }),
        Err(ErrorReason::InvalidAsset)
    );
    apply(
        &mut fixture_host,
        world,
        vec![Command::Delete {
            entity: EntityRef::Handle(first),
        }],
    );
    assert_eq!(
        fixture_host.world_mut(world).unwrap().render_items().len(),
        1
    );
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .take_resource_cancellations()
            .is_empty()
    );
    apply(
        &mut fixture_host,
        world,
        vec![Command::Delete {
            entity: EntityRef::Handle(second),
        }],
    );
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .resource_snapshots()
            .is_empty()
    );
    assert!(
        fixture_host.world_mut(world).unwrap().mesh(key).is_none(),
        "source retention ends with final demand"
    );
    fixture_host
        .asset_resources_mut()
        .set_idle_resident_bytes_target(0);
    fixture_host.flush_resource_lifecycle();
    assert!(fixture_host.world_mut(world).unwrap().mesh(key).is_none());
}

#[cfg(feature = "builtin-assets")]
#[test]
fn builtin_query_sources_keep_exact_identity_and_publish_through_resource_manager() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let world = source_world(&mut fixture_host);
    let reordered = "ipp://mesh/cube?length=6&width=2&height=4";
    let canonical = "ipp://mesh/cube?width=2&height=4&length=6";
    let first = create(&mut fixture_host, world, reordered);
    let second = create(&mut fixture_host, world, reordered);
    let third = create(&mut fixture_host, world, canonical);

    let requests = resource_requests(&mut fixture_host, world);
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().any(|request| request.source == reordered));
    assert!(requests.iter().any(|request| request.source == canonical));
    for request in requests {
        fixture_host
            .world_mut(world)
            .unwrap()
            .complete_resource(
                request.id,
                ipp_core::services::asset_management::builtin::mesh(&request.source)
                    .map_err(|reason| format!("{reason:?}")),
            )
            .unwrap();
    }
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .render_items()
            .is_empty()
    );

    fixture_host.update_world_for_test(world, 0.0).unwrap();
    let items = fixture_host
        .world_mut(world)
        .unwrap()
        .render_items()
        .to_vec();
    assert_eq!(
        items.iter().map(|item| item.entity).collect::<Vec<_>>(),
        vec![first, second, third]
    );
    assert_eq!(items[0].mesh, items[1].mesh);
    assert_ne!(items[0].mesh, items[2].mesh);
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .resource_snapshots()
            .iter()
            .all(|snapshot| snapshot.status == AssetResourceStatus::Loaded)
    );
}

#[cfg(feature = "builtin-assets")]
#[test]
fn structurally_valid_encoded_nul_reaches_builtin_provider_failure() {
    let uri = "ipp://mesh/sphere?radius=%00";
    let mut fixture_host = ipp_core::HostRuntime::new();
    let world = source_world(&mut fixture_host);
    let entity = create(&mut fixture_host, world, uri);
    let request = resource_requests(&mut fixture_host, world).pop().unwrap();
    assert_eq!(request.source, uri);
    let started = fixture_host
        .world_mut(world)
        .unwrap()
        .take_asset_events()
        .unwrap();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].status, AssetResourceStatus::Start);
    assert_eq!(
        ipp_core::services::asset_management::builtin::mesh(uri),
        Err(ErrorReason::InvalidAsset)
    );
    fixture_host
        .world_mut(world)
        .unwrap()
        .complete_resource(request.id, Err("InvalidAsset".into()))
        .unwrap();

    let report = fixture_host.update_world_for_test(world, 0.0).unwrap();
    assert_eq!(
        report.resource_changes,
        fixture_host.world_mut(world).unwrap().resource_snapshots()
    );
    assert_eq!(report.resource_changes[0].source, uri);
    assert!(
        fixture_host
            .update_world_for_test(world, 0.0)
            .unwrap()
            .resource_changes
            .is_empty()
    );
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .inspect(entity)
            .is_some()
    );
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .render_items()
            .is_empty()
    );
    assert!(matches!(
        &fixture_host.world_mut(world).unwrap().resource_snapshots()[0].status,
        AssetResourceStatus::Failed(error) if error == "InvalidAsset"
    ));
}

#[test]
fn replacement_cancels_old_work_and_queued_or_late_completions_cannot_resurrect_it() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let world = source_world(&mut fixture_host);
    let entity = create(&mut fixture_host, world, "file:///a.mesh");
    let old = resource_requests(&mut fixture_host, world)[0].id;
    // Keep the accepted input stream incomplete across the shared Host poll.
    // Replacement must cancel it before another chunk or EOF can be accepted.
    let mut pending = triangle();
    pending.resize(ipp_core::services::asset_management::STREAM_CAPACITY + 1, 0);
    fixture_host
        .world_mut(world)
        .unwrap()
        .complete_resource(old, Ok(pending))
        .unwrap();
    let report = apply(
        &mut fixture_host,
        world,
        vec![set(entity, "ipc://replacement")],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert_eq!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .take_resource_cancellations(),
        vec![old]
    );
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .take_resource_cancellations()
            .is_empty()
    );
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .render_items()
            .is_empty()
    );
    let next = resource_requests(&mut fixture_host, world)[0].id;
    assert_ne!(next, old);
    fixture_host
        .world_mut(world)
        .unwrap()
        .complete_resource(old, Ok(triangle()))
        .unwrap();
    fixture_host.update_world_for_test(world, 0.0).unwrap();
    assert_eq!(
        fixture_host.world_mut(world).unwrap().resource_snapshots()[0].status,
        AssetResourceStatus::Start
    );
    apply(
        &mut fixture_host,
        world,
        vec![set(entity, "file:///a.mesh")],
    );
    assert_eq!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .take_resource_cancellations(),
        vec![next]
    );
    let newest = resource_requests(&mut fixture_host, world)[0].id;
    assert_ne!(newest, old);
    fixture_host
        .world_mut(world)
        .unwrap()
        .complete_resource(next, Err("obsolete".into()))
        .unwrap();
    fixture_host
        .world_mut(world)
        .unwrap()
        .complete_resource(newest, Ok(triangle()))
        .unwrap();
    fixture_host
        .world_mut(world)
        .unwrap()
        .complete_resource(
            newest,
            Err("duplicate must not replace owned completion".into()),
        )
        .unwrap();
    fixture_host.update_world_for_test(world, 0.0).unwrap();
    assert_eq!(
        fixture_host.world_mut(world).unwrap().render_items().len(),
        1
    );
    assert_eq!(
        fixture_host.world_mut(world).unwrap().resource_snapshots()[0].status,
        AssetResourceStatus::Loaded
    );
}

#[test]
fn failed_batches_keep_new_demand_and_ignore_cancelled_source_completions() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let world = source_world(&mut fixture_host);
    let entity = create(
        &mut fixture_host,
        world,
        "ipp://mesh/cube?width=1&height=1&length=1",
    );
    let request = resource_requests(&mut fixture_host, world)[0].clone();
    let report = apply(
        &mut fixture_host,
        world,
        vec![
            set(entity, "https://replacement.test/mesh"),
            Command::Delete {
                entity: EntityRef::Alias(99),
            },
        ],
    );
    assert!(report.outcomes[0].result.is_err());
    assert!(fixture_host.world_mut(world).unwrap().inspect(entity).unwrap().base.iter().any(|value| {
        matches!(value, ComponentValue::MeshInstance(value) if value.source == "https://replacement.test/mesh")
    }));
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .take_resource_cancellations()
            .contains(&request.id)
    );
    let replacement = resource_requests(&mut fixture_host, world)[0].clone();
    assert_eq!(replacement.source, "https://replacement.test/mesh");
    fixture_host
        .world_mut(world)
        .unwrap()
        .complete_resource(request.id, Ok(triangle()))
        .unwrap();
    fixture_host.update_world_for_test(world, 0.0).unwrap();
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .render_items()
            .is_empty()
    );
    fixture_host
        .world_mut(world)
        .unwrap()
        .complete_resource(replacement.id, Ok(triangle()))
        .unwrap();
    fixture_host.update_world_for_test(world, 0.0).unwrap();
    assert_eq!(
        fixture_host.world_mut(world).unwrap().render_items().len(),
        1
    );
}

#[test]
fn malformed_and_failed_payloads_preserve_scene_and_do_not_block_ready_items() {
    for failure in [
        Err("Unsupported provider".into()),
        Ok(b"not a mesh".to_vec()),
    ] {
        let mut fixture_host = ipp_core::HostRuntime::new();
        let world = source_world(&mut fixture_host);
        let ready = create(&mut fixture_host, world, "ipp://ready");
        let request = resource_requests(&mut fixture_host, world)[0].id;
        fixture_host
            .world_mut(world)
            .unwrap()
            .complete_resource(request, Ok(triangle()))
            .unwrap();
        fixture_host.update_world_for_test(world, 0.0).unwrap();
        let failed = create(&mut fixture_host, world, "http://source");
        let request = resource_requests(&mut fixture_host, world)[0].id;
        fixture_host
            .world_mut(world)
            .unwrap()
            .complete_resource(request, failure)
            .unwrap();
        fixture_host.update_world_for_test(world, 0.0).unwrap();
        assert_eq!(fixture_host.world_mut(world).unwrap().entities().len(), 2);
        assert_eq!(
            fixture_host
                .world_mut(world)
                .unwrap()
                .render_items()
                .iter()
                .map(|i| i.entity)
                .collect::<Vec<_>>(),
            vec![ready]
        );
        assert!(
            fixture_host
                .world_mut(world)
                .unwrap()
                .resource_snapshots()
                .iter()
                .any(
                    |s| matches!(&s.status, AssetResourceStatus::Failed(error) if !error.is_empty())
                )
        );
        assert!(
            fixture_host
                .world_mut(world)
                .unwrap()
                .inspect(failed)
                .is_some()
        );
        assert!(
            resource_requests(&mut fixture_host, world).is_empty(),
            "no unbounded retry loop"
        );
    }
}

#[test]
fn source_routing_preserves_opaque_names_and_owned_allocations() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let world = source_world(&mut fixture_host);
    let entity = create(&mut fixture_host, world, "");
    assert!(resource_requests(&mut fixture_host, world).is_empty());
    // Routing forwards opaque identifiers; concrete sources validate their syntax.
    for source in [
        "relative.mesh",
        "1bad://mesh",
        "https://bad host/mesh",
        "ipp://bad%",
        "ipp://bad%XX",
        "https://[unclosed/mesh",
        "https://host:bad/mesh",
        "https://host/a[b]",
        "https://host/a#fragment#second",
        "ipp://\nmesh",
    ] {
        assert!(
            apply(&mut fixture_host, world, vec![set(entity, source)]).outcomes[0]
                .result
                .is_ok()
        );
    }
    for valid in [
        "https://[::1]:8080/a%20b?q=a/b?c#fragment",
        "file:///mesh",
        "other:opaque(value)",
    ] {
        assert!(
            apply(&mut fixture_host, world, vec![set(entity, valid)]).outcomes[0]
                .result
                .is_ok()
        );
    }
    let report = apply(
        &mut fixture_host,
        world,
        vec![set(entity, &format!("ipp://{}", "a".repeat(2048)))],
    );
    assert!(report.outcomes[0].result.is_ok());
    let mut reserved = String::with_capacity(65537);
    reserved.push_str("ipp://mesh");
    assert_eq!(
        fixture_host.world_mut(world).unwrap().enqueue(Batch {
            id: 1,
            operations: vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::MESH_INSTANCE,
                field: FieldWrite {
                    offset: 0,
                    value: FieldValue::String(reserved)
                }
            }]
        }),
        Ok(())
    );
    apply(&mut fixture_host, world, vec![set(entity, "ipp://mesh")]);
    let ticket = resource_requests(&mut fixture_host, world)[0].id;
    let mut bytes = Vec::with_capacity((1 << 20) + 1);
    bytes.extend(triangle());
    assert_eq!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .complete_resource(ticket, Ok(bytes)),
        Ok(())
    );
    assert_eq!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .complete_resource(ticket, Err("x".repeat(2049))),
        Ok(())
    );
    fixture_host
        .world_mut(world)
        .unwrap()
        .complete_resource(ticket, Ok(triangle()))
        .unwrap();
    fixture_host.update_world_for_test(world, 0.0).unwrap();
    assert_eq!(
        fixture_host.world_mut(world).unwrap().render_items().len(),
        1
    );
}

#[test]
fn source_cache_releases_capacity_across_more_than_one_cacheful_of_replacements() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let world = source_world(&mut fixture_host);
    let entity = create(&mut fixture_host, world, "ipp://first");
    for n in 0..300 {
        apply(
            &mut fixture_host,
            world,
            vec![set(entity, &format!("ipp://mesh/{n}"))],
        );
        let ticket = resource_requests(&mut fixture_host, world)[0].id;
        fixture_host
            .world_mut(world)
            .unwrap()
            .complete_resource(ticket, Ok(triangle()))
            .unwrap();
        fixture_host.update_world_for_test(world, 0.0).unwrap();
        assert_eq!(
            fixture_host.world_mut(world).unwrap().render_items().len(),
            1
        );
    }
    let producer = MeshKey {
        asset: 1,

        variant: 0,
    };
    fixture_host
        .world_mut(world)
        .unwrap()
        .enqueue_mesh(MeshUpload {
            id: 1,
            key: producer,
            bytes: triangle(),
        })
        .unwrap();
    fixture_host.update_world_for_test(world, 0.0).unwrap();
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .mesh(producer)
            .is_some()
    );
    assert_ne!(
        fixture_host.world_mut(world).unwrap().render_items()[0].mesh,
        producer
    );
}

#[test]
fn queued_batches_reconcile_only_the_final_boundary_demand() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let world = source_world(&mut fixture_host);
    let entity = create(&mut fixture_host, world, "https://assets.test/initial.mesh");
    let initial = resource_requests(&mut fixture_host, world)[0].id;

    for n in 0..WorldLimits::default().max_queued_batches {
        fixture_host
            .world_mut(world)
            .unwrap()
            .enqueue(Batch {
                id: 100 + n as u64,
                operations: vec![set(
                    entity,
                    &format!("https://assets.test/replacement-{n}.mesh"),
                )],
            })
            .unwrap();
    }

    let report = fixture_host.update_world_for_test(world, 0.0).unwrap();
    assert!(report.outcomes.iter().all(|outcome| outcome.result.is_ok()));
    assert!(report.resource_changes.len() <= 8);
    assert_eq!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .take_resource_cancellations(),
        vec![initial]
    );
    let requests = resource_requests(&mut fixture_host, world);
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].source,
        "https://assets.test/replacement-63.mesh"
    );
    assert_eq!(
        fixture_host.world_mut(world).unwrap().resource_snapshots()[0].source,
        "https://assets.test/replacement-63.mesh"
    );
}

#[test]
fn undrained_cancellations_do_not_limit_new_host_work() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let world = source_world(&mut fixture_host);
    let entity = create(&mut fixture_host, world, "ipp://first");
    for n in 0..256 {
        assert_eq!(resource_requests(&mut fixture_host, world).len(), 1);
        apply(
            &mut fixture_host,
            world,
            vec![set(entity, &format!("ipp://mesh/{n}"))],
        );
    }
    assert_eq!(resource_requests(&mut fixture_host, world).len(), 1);
    let key = fixture_host
        .world_mut(world)
        .unwrap()
        .asset_resources()
        .iter()
        .next()
        .unwrap()
        .key();
    assert_eq!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .take_resource_cancellations()
            .len(),
        256
    );
    fixture_host
        .world_mut(world)
        .unwrap()
        .asset_resources_mut()
        .unload(key);
    fixture_host.update_world_for_test(world, 0.0).unwrap();
    assert_eq!(resource_requests(&mut fixture_host, world).len(), 1);
}

#[test]
fn replacing_the_only_resource_waits_for_its_host_release_barrier() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let world = source_world(&mut fixture_host);
    let entity = create(&mut fixture_host, world, "ipc://original");
    let original = resource_requests(&mut fixture_host, world)[0].id;
    let original_key = fixture_host.asset_resources().iter().next().unwrap().key();

    fixture_host
        .world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 2,
            operations: vec![set(entity, "ipc://replacement")],
        })
        .unwrap();
    fixture_host
        .world_mut(world)
        .unwrap()
        .prepare_update(0.0)
        .unwrap();
    fixture_host.progress_assets();
    let report = fixture_host.world_mut(world).unwrap().step(0.0).unwrap();
    assert!(report.outcomes[0].result.is_ok());
    assert_eq!(fixture_host.asset_resources().iter().count(), 2);
    assert!(
        fixture_host.asset_resources().get(original_key).is_some(),
        "the occupied slot cannot be reused before all Host lifecycle handlers"
    );
    assert!(fixture_host.world_mut(world).unwrap().inspect(entity).unwrap().base.iter().any(|value| {
            matches!(value, ComponentValue::MeshInstance(mesh) if mesh.source == "ipc://replacement")
        }));

    fixture_host.flush_resource_lifecycle();
    assert!(fixture_host.asset_resources().get(original_key).is_none());
    assert_eq!(fixture_host.take_resource_cancellations(), vec![original]);
    fixture_host.update_world_for_test(world, 0.0).unwrap();
    let replacement = resource_requests(&mut fixture_host, world);
    assert_eq!(replacement.len(), 1);
    assert_eq!(replacement[0].source, "ipc://replacement");
    assert_ne!(replacement[0].id, original);
    assert_eq!(fixture_host.asset_resources().iter().count(), 1);
    let replacement_key = fixture_host.asset_resources().iter().next().unwrap().key();
    assert_ne!(replacement_key, original_key);
}

#[test]
fn completion_queue_and_world_demand_grow_past_former_quotas() {
    let mut host = ipp_core::HostRuntime::new();
    let world = source_world(&mut host);
    for n in 0..300 {
        create(&mut host, world, &format!("ipp://mesh/{n}"));
    }
    let requests = resource_requests(&mut host, world);
    assert_eq!(requests.len(), 300);
    for request in requests {
        host.complete_resource(request.id, Ok(triangle())).unwrap();
    }
    host.update_world_for_test(world, 0.0).unwrap();
    assert_eq!(host.world_mut(world).unwrap().render_items().len(), 300);
}

#[test]
fn typed_demand_is_distinct_and_uv_incompatibility_does_not_poison_shared_mesh() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let world = source_world(&mut fixture_host);
    let solid = create(&mut fixture_host, world, "https://assets.test/shared");
    let textured = create(&mut fixture_host, world, "https://assets.test/shared");
    let report = apply(
        &mut fixture_host,
        world,
        vec![Command::InsertComponent {
            entity: EntityRef::Handle(textured),
            component: ComponentValue::UNLIT_TEXTURE,
            fields: vec![FieldWrite {
                offset: offset_of!(ipp_core::components::UnlitTexture, source) as u32,
                value: FieldValue::String("https://assets.test/shared".into()),
            }],
        }],
    );
    assert!(report.outcomes[0].result.is_ok());
    let requests = resource_requests(&mut fixture_host, world);
    assert_eq!(requests.len(), 2, "same URI has distinct typed acquisition");
    let mut texture = b"IPPT".to_vec();
    for n in [3u32, 1, 1] {
        texture.extend(n.to_le_bytes());
    }
    texture.extend([255u8; 4]);
    for request in requests {
        fixture_host
            .world_mut(world)
            .unwrap()
            .complete_resource(
                request.id,
                Ok(match request.kind {
                    AssetResourceKind::Mesh => triangle(),
                    AssetResourceKind::Texture => texture.clone(),
                    _ => panic!("unexpected fixture type"),
                }),
            )
            .unwrap();
    }
    fixture_host.update_world_for_test(world, 0.0).unwrap();
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .resource_snapshots()
            .iter()
            .all(|s| s.status == AssetResourceStatus::Loaded)
    );
    assert_eq!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .render_items()
            .iter()
            .map(|i| i.entity)
            .collect::<Vec<_>>(),
        vec![solid]
    );
    assert_eq!(
        fixture_host.world_mut(world).unwrap().render_diagnostics(),
        vec![ipp_core::RenderDiagnostic {
            entity: textured,
            reason: ErrorReason::InvalidAsset
        }]
    );
    let report = apply(
        &mut fixture_host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(textured),
            component: ComponentValue::UNLIT_TEXTURE,
        }],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert_eq!(
        fixture_host.world_mut(world).unwrap().render_items().len(),
        2
    );
    assert!(
        fixture_host
            .world_mut(world)
            .unwrap()
            .render_diagnostics()
            .is_empty()
    );
}
