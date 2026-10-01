//! Real headless Host graph and immutable output evidence; no transport or GPU claim.

mod support;

#[cfg(feature = "surfaces")]
use support::selection::SURFACE;
use support::selection::{ATTACHMENTS, CAMERA, CONSTRAINTS, GEOMETRY, RENDER, SPATIAL, select};
use support::world_failures::select_with_failures;

use ipp_core::components::schema::SchemaComponent;
use ipp_core::components::{Camera, MeshInstance, PickingGeometry, Transform, UnlitMaterial};
use ipp_core::services::asset_management::{AssetKey, AssetSource};
use ipp_core::systems::{
    camera::CameraPublication,
    geometry::{
        GeometryDefinition, GeometryPublication, GeometryRay, GeometryShape, GeometrySystem,
    },
    render::{RenderPublication, RenderSystem},
};
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, ErrorReason, FieldValue, FieldWrite,
    HostRuntime, OutputKind, WorldAttachment, WorldId, WorldViewport,
};

fn viewport() -> WorldViewport {
    WorldViewport {
        width: 640,
        height: 480,
        device_pixel_ratio: 1.0,
    }
}

fn apply(
    host: &mut HostRuntime,
    world: WorldId,
    operations: Vec<Command>,
) -> Result<Vec<(u32, EntityId)>, ipp_core::BatchError> {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
}

fn create(host: &mut HostRuntime, world: WorldId, components: Vec<ComponentValue>) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        components
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(0), value)),
    );
    apply(host, world, operations).unwrap()[0].1
}

fn attach(host: &mut HostRuntime, parent: WorldId, child: WorldId, x: f32) -> EntityId {
    let value = WorldAttachment::spatial(host.world_ref(child).unwrap());
    create(
        host,
        parent,
        vec![
            ComponentValue::Transform(Transform {
                x,
                ..Default::default()
            }),
            ComponentValue::WorldAttachment(value),
        ],
    )
}

fn camera(host: &mut HostRuntime, world: WorldId) -> ipp_core::OutputRef {
    let entity = create(
        host,
        world,
        vec![
            ComponentValue::Transform(Transform {
                z: 5.0,
                ..Default::default()
            }),
            ComponentValue::Camera(Camera::default()),
        ],
    );
    host.bind_output(host.world_ref(world).unwrap(), entity, OutputKind::Camera)
        .unwrap()
}

fn mesh_bytes() -> Vec<u8> {
    let mut bytes = b"IPPM".to_vec();
    for value in [3_u32, 3, 3, 1] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0, 1, 0, 0]);
    bytes.extend(36_u32.to_le_bytes());
    for value in [-1.0_f32, -1.0, 0.0, 1.0, -1.0, 0.0, 0.0, 1.0, 0.0] {
        bytes.extend(value.to_le_bytes());
    }
    for index in [0_u16, 1, 2] {
        bytes.extend(index.to_le_bytes());
    }
    bytes
}

fn mesh(host: &mut HostRuntime, world: WorldId) -> (EntityId, AssetKey, AssetSource) {
    let source = AssetSource {
        kind: ipp_core::MESH_TYPE,
        uri: std::sync::Arc::<str>::from(format!("producer://{}/1/10", world.0)),
        variant: 0,
    };
    host.asset_resources_mut()
        .register_client_source(world, source.clone(), mesh_bytes())
        .unwrap();
    host.progress_assets();
    let key = host.asset_resources().find(&source).unwrap();
    assert!(
        host.asset_resources()
            .get_typed::<ipp_core::MeshAsset>(key)
            .is_some()
    );
    let picking = GeometryDefinition::from(GeometryShape::Sphere {
        center: [0.0; 3],
        radius: 0.5,
    })
    .encode()
    .unwrap();
    let entity = create(
        host,
        world,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::MeshInstance(MeshInstance {
                source: source.uri.clone(),
                variant: 0,
            }),
            ComponentValue::UnlitMaterial(UnlitMaterial::default()),
            ComponentValue::PickingGeometry(PickingGeometry {
                geometry: picking,
                ..Default::default()
            }),
        ],
    );
    (entity, key, source)
}

fn frame(host: &mut HostRuntime) -> ipp_core::HostFrameReport {
    let report = host.frame(0.25).unwrap();
    assert!(
        report.worlds.values().all(Result::is_ok),
        "{:?}",
        report.worlds
    );
    assert!(
        report.publication_errors.is_empty(),
        "{:?}",
        report.publication_errors
    );
    report
}

#[test]
fn faulted_deletion_drains_outcomes_without_republishing_stale_render_items() {
    use ipp_core::{components::Scalar, systems::*};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    struct Factory(Arc<AtomicBool>, Arc<AtomicUsize>);
    struct CleanupFault(Arc<AtomicBool>, Arc<AtomicUsize>);

    const COMMIT_FAULT: SystemId = SystemId("fixture.publication-commit-fault");

    impl SystemFactory for Factory {
        fn id(&self) -> SystemId {
            COMMIT_FAULT
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(CleanupFault(self.0.clone(), self.1.clone())))
        }
    }

    impl System for CleanupFault {
        fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

        fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
            if !self.0.load(Ordering::Relaxed) {
                return;
            }
            let targets: Vec<_> = context
                .changed_components()
                .filter(|(_, component)| *component == ComponentValue::SCALAR)
                .map(|(entity, _)| entity)
                .collect();
            for entity in targets {
                let round = self.1.fetch_add(1, Ordering::Relaxed);
                context.restore_evaluated_component(
                    entity,
                    ComponentValue::Scalar(Scalar {
                        value: round as f32 + 100.0,
                    }),
                );
            }
        }
    }

    let enabled = Arc::new(AtomicBool::new(false));
    let restorations = Arc::new(AtomicUsize::new(0));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Factory(enabled.clone(), restorations.clone())));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA]))
        .unwrap();
    let child = host
        .create_world(
            Default::default(),
            &[
                select(&[ATTACHMENTS, CONSTRAINTS, RENDER]),
                vec![COMMIT_FAULT],
            ]
            .concat(),
        )
        .unwrap();
    let sibling = host.create_world(Default::default(), &[]).unwrap();
    let root = camera(&mut host, parent);
    host.set_root_output(root, viewport()).unwrap();
    attach(&mut host, parent, child, 2.0);
    attach(&mut host, parent, sibling, 4.0);
    let (entity, key, source) = mesh(&mut host, child);
    let oscillator = create(
        &mut host,
        child,
        vec![ComponentValue::Scalar(Scalar::default())],
    );
    let grandchild = host.create_world(Default::default(), &[]).unwrap();
    let grandchild_ref = host.world_ref(grandchild).unwrap();
    frame(&mut host);
    let previous = host.latest_publication(child).unwrap();
    let previous_time = host.publication(previous).unwrap().time;
    let previous_tick = host.publication(previous).unwrap().tick;
    let retained_render = host
        .publication(previous)
        .unwrap()
        .chunk(RenderSystem::ID)
        .unwrap()
        .data::<RenderPublication>()
        .unwrap()
        .clone();
    assert_eq!(retained_render.items[0].item.entity, entity);
    enabled.store(true, Ordering::Relaxed);
    host.world_mut(child)
        .unwrap()
        .enqueue(Batch {
            id: 61,
            operations: vec![
                Command::insert_value(
                    EntityRef::Handle(oscillator),
                    ComponentValue::WorldAttachment(WorldAttachment::spatial(grandchild_ref)),
                ),
                Command::SetField {
                    entity: EntityRef::Handle(oscillator),
                    component: ComponentValue::SCALAR,
                    field: FieldWrite {
                        offset: std::mem::offset_of!(Scalar, value) as u32,
                        value: FieldValue::F32(1.0),
                    },
                },
                Command::Delete {
                    entity: EntityRef::Handle(entity),
                },
            ],
        })
        .unwrap();
    host.world_mut(child)
        .unwrap()
        .enqueue(Batch {
            id: 62,
            operations: vec![Command::Create {
                alias: 0,
                metadata: Default::default(),
                adopt: false,
            }],
        })
        .unwrap();
    let report = host.frame(0.25).unwrap();
    let child_report = report.worlds[&child].as_ref().unwrap();
    assert_eq!(
        child_report
            .outcomes
            .iter()
            .map(|outcome| outcome.batch_id)
            .collect::<Vec<_>>(),
        [61, 62]
    );
    assert!(
        child_report
            .outcomes
            .iter()
            .all(|outcome| outcome.result.as_ref().unwrap_err().reason
                == ErrorReason::NonConvergentCommit)
    );
    assert!(restorations.load(Ordering::Relaxed) >= 64);
    let [
        ipp_core::AppliedOperationEffect {
            operation: 0,
            effect:
                ipp_core::OperationEffect::WorldAttachment(ipp_core::WorldAttachmentEffect::Written(
                    applied,
                )),
        },
    ] = child_report.outcomes[0].effects.as_slice()
    else {
        panic!("attachment receipt survives commit-level failure");
    };
    let applied = applied.clone();
    assert_eq!(applied.child(), Some(grandchild_ref));
    assert_eq!(
        host.attachment_retirement(&applied),
        Ok(ipp_core::WorldAttachmentRetirement::Pending)
    );
    assert!(child_report.outcomes[1].effects.is_empty());
    let world = host.world_mut(child).unwrap();
    assert_eq!(world.fault(), Some(ErrorReason::NonConvergentCommit));
    assert!(world.inspect(entity).is_none());
    assert_eq!(world.time(), previous_time);
    drop(world);
    assert!(!report.evaluation_order.contains(&child));
    assert!(!report.publication_order.contains(&child));
    assert_eq!(
        report
            .publication_errors
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        [child]
    );
    assert!(
        report.evaluation_order.contains(&parent) && report.evaluation_order.contains(&sibling)
    );
    assert!(
        report.publication_order.contains(&parent) && report.publication_order.contains(&sibling)
    );
    assert_eq!(host.latest_publication(child), Some(previous));
    let retained = host.publication(previous).unwrap();
    assert_eq!(retained.tick, previous_tick);
    assert_eq!(
        retained
            .chunk(RenderSystem::ID)
            .unwrap()
            .data::<RenderPublication>()
            .unwrap(),
        &retained_render
    );
    assert!(host.publication_resource(previous, key).is_some());
    assert!(
        host.spatial_contributions(host.root_output(parent).unwrap().2)
            .iter()
            .any(|entry| entry.publication.id == previous)
    );
    let next = host.frame(0.25).unwrap();
    assert!(next.worlds[&child].as_ref().unwrap().outcomes.is_empty());
    assert!(!next.evaluation_order.contains(&child) && !next.publication_order.contains(&child));
    assert_eq!(host.latest_publication(child), Some(previous));
    host.asset_resources_mut()
        .release_client_source(child, &source);
    host.asset_resources_mut().revoke_resource(key);
    host.flush_resource_lifecycle();
    assert!(host.publication(previous).is_none());
    assert!(host.publication_resource(previous, key).is_none());
    assert!(host.asset_resources().get(key).is_none());
    let next = host.frame(0.25).unwrap();
    assert_eq!(host.latest_publication(child), None);
    assert!(!next.evaluation_order.contains(&child) && !next.publication_order.contains(&child));
    assert!(next.evaluation_order.contains(&parent) && next.evaluation_order.contains(&sibling));
    assert_eq!(
        host.world_mut(child).unwrap().enqueue(Batch {
            id: 63,
            operations: Vec::new(),
        }),
        Err(ErrorReason::NonConvergentCommit)
    );
    assert!(host.destroy_world(child));
    assert_eq!(
        host.attachment_retirement(&applied),
        Ok(ipp_core::WorldAttachmentRetirement::Retired)
    );
}

#[test]
fn stale_ancestor_keeps_retiring_child_reserved_until_every_reachable_edge_retires() {
    stale_ancestor_detach(false);
}

#[test]
fn published_picking_errors_freeze_until_ready_without_click_through_or_visual_failure() {
    let mut host = HostRuntime::new();
    host.register_stream_resource_provider("https").unwrap();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA, RENDER]))
        .unwrap();
    let child = host
        .create_world(Default::default(), &select(&[GEOMETRY]))
        .unwrap();
    let root = camera(&mut host, parent);
    host.set_root_output(root, viewport()).unwrap();
    attach(&mut host, parent, child, 0.0);
    let (behind, _, _) = mesh(&mut host, parent);
    apply(
        &mut host,
        parent,
        vec![Command::SetField {
            entity: EntityRef::Handle(behind),
            component: ComponentValue::TRANSFORM,
            field: FieldWrite {
                offset: std::mem::offset_of!(Transform, z) as u32,
                value: FieldValue::F32(-2.0),
            },
        }],
    )
    .unwrap();
    let pending = create(
        &mut host,
        child,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::PickingGeometry(PickingGeometry {
                source: "https://fixture/published-picking".into(),
                ..Default::default()
            }),
        ],
    );
    let ray = GeometryRay {
        origin: [0.0, 0.0, 5.0],
        direction: [0.0, 0.0, -1.0],
    };
    let miss = GeometryRay {
        origin: [100.0, 0.0, 5.0],
        direction: [0.0, 0.0, -1.0],
    };
    frame(&mut host);
    let initial = host.root_output(parent).unwrap().2;
    let retained = host.latest_publication(child).unwrap();
    let retained_geometry = host
        .publication(retained)
        .unwrap()
        .chunk(GeometrySystem::ID)
        .unwrap()
        .data::<GeometryPublication>()
        .unwrap()
        .clone();
    for query in [&ray, &miss] {
        assert_eq!(
            host.pick_publication(initial, root, query, 0.0, 1000.0),
            Err(ErrorReason::GeometryUnavailable)
        );
    }
    assert_eq!(
        host.publication(initial)
            .unwrap()
            .chunk(RenderSystem::ID)
            .unwrap()
            .data::<RenderPublication>()
            .unwrap()
            .items
            .len(),
        1
    );
    host.progress_assets();
    let requests = host.take_resource_requests();
    let request = requests
        .iter()
        .find(|request| {
            request.source == std::sync::Arc::<str>::from("https://fixture/published-picking")
        })
        .unwrap();
    let definition = GeometryDefinition::from(GeometryShape::Sphere {
        center: [0.0; 3],
        radius: 1.0,
    })
    .encode()
    .unwrap();
    host.complete_resource(request.id, Ok(definition)).unwrap();
    host.progress_assets();
    frame(&mut host);
    let current = host.root_output(parent).unwrap().2;
    let hit = host
        .pick_publication(current, root, &ray, 0.0, 1000.0)
        .unwrap()
        .unwrap();
    assert_eq!(
        (hit.world.id(), hit.entity, hit.hit.distance),
        (child, pending, 4.0)
    );
    assert_eq!(
        host.pick_publication(current, root, &miss, 0.0, 1000.0),
        Ok(None)
    );
    assert_eq!(
        retained_geometry.pick(&ray, 0.0, 1000.0),
        Err(ErrorReason::GeometryUnavailable)
    );

    apply(
        &mut host,
        child,
        vec![Command::SetField {
            entity: EntityRef::Handle(pending),
            component: ComponentValue::PICKING_GEOMETRY,
            field: FieldWrite {
                offset: std::mem::offset_of!(PickingGeometry, source) as u32,
                value: FieldValue::String("https://fixture/published-picking-failed".into()),
            },
        }],
    )
    .unwrap();
    frame(&mut host);
    host.progress_assets();
    let requests = host.take_resource_requests();
    let request = requests
        .iter()
        .find(|request| {
            request.source
                == std::sync::Arc::<str>::from("https://fixture/published-picking-failed")
        })
        .unwrap();
    host.complete_resource(request.id, Ok(vec![1, 2, 3]))
        .unwrap();
    host.progress_assets();
    frame(&mut host);
    for query in [&ray, &miss] {
        assert_eq!(
            host.pick_publication(
                host.root_output(parent).unwrap().2,
                root,
                query,
                0.0,
                1000.0
            ),
            Err(ErrorReason::GeometryUnavailable)
        );
    }
    apply(
        &mut host,
        child,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(pending),
            component: ComponentValue::PICKING_GEOMETRY,
        }],
    )
    .unwrap();
    frame(&mut host);
    assert_eq!(
        host.pick_publication(host.root_output(parent).unwrap().2, root, &ray, 0.0, 1000.0)
            .unwrap()
            .unwrap()
            .entity,
        behind
    );
    apply(
        &mut host,
        parent,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(behind),
            component: ComponentValue::PICKING_GEOMETRY,
        }],
    )
    .unwrap();
    frame(&mut host);
    let current = host.root_output(parent).unwrap().2;
    assert_eq!(
        host.pick_publication(current, root, &ray, 0.0, 1000.0),
        Ok(None)
    );
    assert_eq!(
        host.publication(current)
            .unwrap()
            .chunk(RenderSystem::ID)
            .unwrap()
            .data::<RenderPublication>()
            .unwrap()
            .items
            .len(),
        1
    );
}

#[test]
fn immutable_picking_distinguishes_no_evaluation_and_retained_invalid_geometry() {
    use ipp_core::systems::geometry::PublishedGeometry;
    let entity = EntityId::from_bits(1);
    let mut publication = GeometryPublication {
        entities: vec![PublishedGeometry {
            entity,
            bounding_incarnation: Some(1),
            picking_incarnation: Some(1),
            visual_bounds: Some([[-1.0; 3], [1.0; 3]]),
            culling: None,
            picking: None,
        }],
    };
    let ray = GeometryRay {
        origin: [0.0, 0.0, 5.0],
        direction: [0.0, 0.0, -1.0],
    };
    assert_eq!(publication.pick(&ray, 0.0, 100.0), Ok(None));
    publication.entities[0].picking = Some(Err(ErrorReason::InvalidGeometry));
    assert_eq!(
        publication.pick(&ray, 0.0, 100.0),
        Err(ErrorReason::InvalidGeometry)
    );
    assert!(publication.entities[0].visual_bounds.is_some());
}

#[test]
fn branch_without_a_completed_publication_contributes_nothing() {
    let (mut host, failures) = support::world_failures::host_with_world_failures();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA]))
        .unwrap();
    let child = host
        .create_world(Default::default(), &select_with_failures(&[RENDER]))
        .unwrap();
    failures.fail_publication(Some(child));
    let root = camera(&mut host, parent);
    host.set_root_output(root, viewport()).unwrap();
    mesh(&mut host, child);
    attach(&mut host, parent, child, 2.0);
    let report = host.frame(0.25).unwrap();
    assert_eq!(
        report
            .publication_errors
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        [child]
    );
    assert_eq!(host.latest_publication(child), None);
    let parent_output = host.latest_publication(parent).unwrap();
    assert!(
        !host
            .spatial_contributions(parent_output)
            .iter()
            .any(|entry| entry.publication.world.id() == child)
    );

    failures.fail_publication(None);
    frame(&mut host);
    let parent_output = host.latest_publication(parent).unwrap();
    assert_eq!(
        host.spatial_contributions(parent_output)
            .iter()
            .filter(|entry| entry.publication.world.id() == child)
            .count(),
        1
    );
}

#[test]
fn revoked_stale_ancestor_retires_unreachable_edges_and_leases_without_evaluation() {
    stale_ancestor_detach(true);
}

fn stale_ancestor_detach(revoke: bool) {
    let (mut host, failure) = support::world_failures::host_with_world_failures();
    let ancestor = host
        .create_world(
            Default::default(),
            &select_with_failures(&[ATTACHMENTS, CAMERA, RENDER]),
        )
        .unwrap();
    let middle = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, SPATIAL]))
        .unwrap();
    let child = host
        .create_world(Default::default(), &select(&[CAMERA, RENDER]))
        .unwrap();
    let peer = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let root = camera(&mut host, ancestor);
    let child_output = camera(&mut host, child);
    host.set_root_output(root, viewport()).unwrap();
    attach(&mut host, ancestor, middle, 2.0);
    let old_anchor = attach(&mut host, middle, child, 3.0);
    let (_, ancestor_key, _) = mesh(&mut host, ancestor);
    let (child_mesh, child_key, child_source) = mesh(&mut host, child);
    let peer_anchor = create(&mut host, peer, Vec::new());
    let child_ref = host.world_ref(child).unwrap();
    let reattach = || {
        Command::insert_value(
            EntityRef::Handle(peer_anchor),
            ComponentValue::WorldAttachment(WorldAttachment::spatial(child_ref)),
        )
    };
    frame(&mut host);
    let old_ancestor = host.root_output(ancestor).unwrap().2;
    let old_middle = host.latest_publication(middle).unwrap();
    let old_edge = host.publication(old_middle).unwrap().attachments[0]
        .token
        .clone();
    let old_child = host.latest_publication(child).unwrap();
    failure.fail_publication(Some(ancestor));
    apply(
        &mut host,
        middle,
        vec![Command::Delete {
            entity: EntityRef::Handle(old_anchor),
        }],
    )
    .unwrap();
    apply(
        &mut host,
        child,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(child_mesh),
            component: ComponentValue::MESH_INSTANCE,
        }],
    )
    .unwrap();
    host.asset_resources_mut()
        .release_client_source(child, &child_source);
    let report = host.frame(0.25).unwrap();
    assert_eq!(
        report
            .publication_errors
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        [ancestor]
    );
    assert!(report.publication_order.contains(&middle));
    assert_ne!(host.latest_publication(middle), Some(old_middle));
    assert_eq!(host.root_output(ancestor).unwrap().2, old_ancestor);
    assert_eq!(
        host.spatial_contributions(old_ancestor)
            .iter()
            .filter(|entry| entry.publication.world.id() == child)
            .count(),
        1
    );
    assert!(apply(&mut host, peer, vec![reattach()]).is_err());
    assert_eq!(
        host.attachment_retirement(&old_edge),
        Ok(ipp_core::WorldAttachmentRetirement::Pending)
    );
    assert_eq!(
        host.set_root_output(child_output, viewport()),
        Err(ErrorReason::InvalidValue)
    );
    host.clear_root_output(ancestor);
    assert!(host.root_output(ancestor).is_none());
    assert_eq!(
        host.attachment_retirement(&old_edge),
        Ok(ipp_core::WorldAttachmentRetirement::Pending)
    );
    assert!(apply(&mut host, peer, vec![reattach()]).is_err());
    host.set_root_output(root, viewport()).unwrap();
    assert_eq!(host.root_output(ancestor).unwrap().2, old_ancestor);
    assert!(host.publication_resource(old_child, child_key).is_some());
    if revoke {
        host.asset_resources_mut().revoke_resource(ancestor_key);
        host.flush_resource_lifecycle();
        assert!(host.root_output(ancestor).is_none());
    } else {
        failure.fail_publication(None);
        frame(&mut host);
    }
    assert!(host.publication(old_ancestor).is_none());
    assert!(host.publication(old_middle).is_none());
    assert!(host.publication(old_child).is_none());
    assert!(host.publication_resource(old_child, child_key).is_none());
    assert_eq!(
        host.attachment_retirement(&old_edge),
        Ok(ipp_core::WorldAttachmentRetirement::Retired)
    );
    apply(&mut host, peer, vec![reattach()]).unwrap();
    if revoke {
        host.frame(0.25).unwrap();
        assert!(host.root_output(ancestor).is_none());
    } else {
        frame(&mut host);
        let new_ancestor = host.root_output(ancestor).unwrap().2;
        assert!(
            !host
                .spatial_contributions(new_ancestor)
                .iter()
                .any(|entry| entry.publication.world.id() == child)
        );
    }
    let peer_publication = host.latest_publication(peer).unwrap();
    assert_eq!(
        host.attachment_retirement(&old_edge),
        Ok(ipp_core::WorldAttachmentRetirement::Retired)
    );
    assert_eq!(
        host.spatial_contributions(peer_publication)
            .iter()
            .filter(|entry| entry.publication.world.id() == child)
            .count(),
        1
    );
    assert_eq!(
        host.set_root_output(child_output, viewport()),
        Err(ErrorReason::InvalidValue)
    );
}

#[test]
fn root_withdrawal_is_explicit_and_historical_output_is_not_a_presentation_path() {
    let mut host = HostRuntime::new();
    let parent = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let child = host.create_world(Default::default(), CAMERA).unwrap();
    let root = camera(&mut host, child);
    host.set_root_output(root, viewport()).unwrap();
    frame(&mut host);
    let anchor = create(&mut host, parent, Vec::new());
    let value = WorldAttachment::spatial(host.world_ref(child).unwrap());
    let attach = || {
        Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::WorldAttachment(value.clone()),
        )
    };
    assert!(apply(&mut host, parent, vec![attach()]).is_err());
    let historical = host.root_output(child).unwrap().2;
    host.clear_root_output(child);
    assert!(host.root_output(child).is_none());
    assert!(host.output(historical, root).is_some());
    apply(&mut host, parent, vec![attach()]).unwrap();
    assert_eq!(
        host.set_root_output(root, viewport()),
        Err(ErrorReason::InvalidValue)
    );
    frame(&mut host);
    assert!(host.root_output(child).is_none());
    assert!(host.publication(historical).is_none());
    assert_eq!(
        host.spatial_contributions(host.latest_publication(parent).unwrap())
            .len(),
        2
    );
}

#[test]
fn publication_runtime_identity_is_not_a_durable_host_namespace() {
    let mut first = HostRuntime::new();
    let mut second = HostRuntime::new();
    first.set_identity_namespace(42).unwrap();
    second.set_identity_namespace(42).unwrap();
    let first_world = first.create_world(Default::default(), &[]).unwrap();
    let second_world = second.create_world(Default::default(), &[]).unwrap();
    frame(&mut first);
    frame(&mut second);
    let first_output = first.latest_publication(first_world).unwrap();
    let second_output = second.latest_publication(second_world).unwrap();
    assert_ne!(first_output, second_output);
    assert!(first.publication(second_output).is_none());
    assert!(second.publication(first_output).is_none());
}

#[test]
fn generic_attachment_fields_have_dedicated_reference_kinds_and_graph_admission() {
    let mut host = HostRuntime::new();
    let parent = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let child = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let competing = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let anchor = create(&mut host, parent, Vec::new());
    let child_ref = host.world_ref(child).unwrap();
    apply(
        &mut host,
        parent,
        vec![Command::InsertComponent {
            entity: EntityRef::Handle(anchor),
            component: ComponentValue::WORLD_ATTACHMENT,
            fields: vec![FieldWrite {
                offset: std::mem::offset_of!(WorldAttachment, child) as u32,
                value: FieldValue::World(Some(child_ref)),
            }],
            adopt: false,
        }],
    )
    .unwrap();
    let snapshot = host.world_mut(parent).unwrap().inspect(anchor).unwrap();
    assert!(snapshot.components.iter().any(|value| matches!(value, ComponentValue::WorldAttachment(value) if value.child() == Some(child_ref))));
    assert_eq!(
        WorldAttachment::spatial(child_ref)
            .field(std::mem::offset_of!(WorldAttachment, child) as u32)
            .unwrap()
            .kind(),
        ipp_core::components::schema::FieldKind::World
    );
    let other = create(&mut host, competing, Vec::new());
    assert!(
        apply(
            &mut host,
            competing,
            vec![Command::insert_value(
                EntityRef::Handle(other),
                ComponentValue::WorldAttachment(WorldAttachment::spatial(child_ref))
            )]
        )
        .is_err()
    );
    let parent_ref = host.world_ref(parent).unwrap();
    let child_anchor = create(&mut host, child, Vec::new());
    assert!(
        apply(
            &mut host,
            child,
            vec![Command::insert_value(
                EntityRef::Handle(child_anchor),
                ComponentValue::WorldAttachment(WorldAttachment::spatial(parent_ref))
            )]
        )
        .is_err()
    );
    assert!(
        apply(
            &mut host,
            parent,
            vec![Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::WorldAttachment(WorldAttachment::spatial(parent_ref))
            )]
        )
        .is_err()
    );
    assert!(
        apply(
            &mut host,
            parent,
            vec![Command::SetField {
                entity: EntityRef::Handle(anchor),
                component: ComponentValue::WORLD_ATTACHMENT,
                field: FieldWrite {
                    offset: std::mem::offset_of!(WorldAttachment, child) as u32,
                    value: FieldValue::Entity(EntityRef::Handle(anchor))
                }
            }]
        )
        .is_err()
    );
}

#[test]
fn retained_branch_keeps_owned_mesh_geometry_and_advances_parent_placement_and_sibling() {
    let (mut host, failures) = support::world_failures::host_with_world_failures();
    let child = host
        .create_world(
            Default::default(),
            &select_with_failures(&[ATTACHMENTS, RENDER]),
        )
        .unwrap();
    let grandchild = host.create_world(Default::default(), &[]).unwrap();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA]))
        .unwrap();
    let sibling = host.create_world(Default::default(), &[]).unwrap();
    let root = camera(&mut host, parent);
    host.set_root_output(root, viewport()).unwrap();
    let (entity, _, _) = mesh(&mut host, child);
    let anchor = attach(&mut host, parent, child, 2.0);
    attach(&mut host, child, grandchild, 1.0);
    attach(&mut host, parent, sibling, -2.0);
    let first = frame(&mut host);
    assert!(
        first
            .evaluation_order
            .iter()
            .position(|world| *world == parent)
            < first
                .evaluation_order
                .iter()
                .position(|world| *world == child)
    );
    assert!(
        first
            .publication_order
            .iter()
            .position(|world| *world == child)
            < first
                .publication_order
                .iter()
                .position(|world| *world == parent)
    );
    let old_child = host.latest_publication(child).unwrap();
    let old_geometry = host
        .publication(old_child)
        .unwrap()
        .chunk(GeometrySystem::ID)
        .unwrap()
        .data::<GeometryPublication>()
        .unwrap()
        .clone();
    assert_eq!(
        old_geometry
            .entities
            .iter()
            .find(|value| value.entity == entity)
            .unwrap()
            .visual_bounds,
        Some([[-1.0, -1.0, 0.0], [1.0, 1.0, 0.0]])
    );
    let old_version = host
        .publication(old_child)
        .unwrap()
        .chunk(RenderSystem::ID)
        .unwrap()
        .version();

    // The child's own edit applies, but its failed publication keeps the completed branch.
    failures.fail_publication(Some(child));
    apply(
        &mut host,
        child,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::TRANSFORM,
            field: FieldWrite {
                offset: std::mem::offset_of!(Transform, x) as u32,
                value: FieldValue::F32(100.0),
            },
        }],
    )
    .unwrap();
    apply(
        &mut host,
        parent,
        vec![Command::SetField {
            entity: EntityRef::Handle(anchor),
            component: ComponentValue::TRANSFORM,
            field: FieldWrite {
                offset: std::mem::offset_of!(Transform, x) as u32,
                value: FieldValue::F32(7.0),
            },
        }],
    )
    .unwrap();
    let second = host.frame(0.25).unwrap();
    assert!(
        second.evaluation_order.contains(&parent) && second.evaluation_order.contains(&sibling)
    );
    assert_eq!(
        second
            .publication_errors
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        [child]
    );
    assert_eq!(host.latest_publication(child), Some(old_child));
    let parent_output = host.latest_publication(parent).unwrap();
    let contributions = host.spatial_contributions(parent_output);
    let child_output = contributions
        .iter()
        .find(|value| value.publication.world.id() == child)
        .unwrap();
    assert_eq!(child_output.placement.matrix()[12], 7.0);
    assert_eq!(
        child_output
            .publication
            .chunk(RenderSystem::ID)
            .unwrap()
            .version(),
        old_version
    );
    assert_eq!(
        child_output
            .publication
            .chunk(GeometrySystem::ID)
            .unwrap()
            .data::<GeometryPublication>()
            .unwrap(),
        &old_geometry
    );
    let ray = GeometryRay {
        origin: [7.0, 0.0, 5.0],
        direction: [0.0, 0.0, -1.0],
    };
    let hit = host
        .pick_publication(parent_output, root, &ray, 0.0, 100.0)
        .unwrap()
        .unwrap();
    assert_eq!(
        (hit.world.id(), hit.entity, hit.hit.distance),
        (child, entity, 4.5)
    );

    failures.fail_publication(None);
    frame(&mut host);
    let current = host.latest_publication(child).unwrap();
    assert_ne!(current, old_child);
    let geometry = host
        .publication(current)
        .unwrap()
        .chunk(GeometrySystem::ID)
        .unwrap()
        .data::<GeometryPublication>()
        .unwrap();
    assert_eq!(
        geometry
            .entities
            .iter()
            .find(|value| value.entity == entity)
            .unwrap()
            .visual_bounds
            .unwrap()[0][0],
        99.0
    );
    assert_eq!(
        old_geometry
            .entities
            .iter()
            .find(|value| value.entity == entity)
            .unwrap()
            .visual_bounds
            .unwrap()[0][0],
        -1.0
    );
}

#[test]
fn output_selection_is_explicit_view_independent_and_incarnation_fenced() {
    let (mut host, failure) = support::world_failures::host_with_world_failures();
    let world = host
        .create_world(Default::default(), &select_with_failures(&[CAMERA]))
        .unwrap();
    let first = camera(&mut host, world);
    let second = camera(&mut host, world);
    host.world_mut(world)
        .unwrap()
        .enqueue_camera_activate(second.camera_entity().unwrap())
        .unwrap();
    host.set_root_output(first, viewport()).unwrap();
    frame(&mut host);
    let publication = host.latest_publication(world).unwrap();
    let camera = host
        .output(publication, first)
        .unwrap()
        .data::<CameraPublication>()
        .unwrap()
        .clone();
    assert_eq!(camera.selection, first);
    assert_ne!(
        camera.prepare(640, 480).unwrap().view_projection,
        camera.prepare(1280, 480).unwrap().view_projection
    );

    // Keep the replacement unpublished: selection alone never makes it presentable.
    failure.fail_publication(Some(world));
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(first.camera_entity().unwrap()),
                component: ComponentValue::CAMERA,
            },
            Command::insert_value(
                EntityRef::Handle(first.camera_entity().unwrap()),
                ComponentValue::Camera(Camera {
                    fov_y: 1.0,
                    ..Default::default()
                }),
            ),
        ],
    )
    .unwrap();
    assert!(host.output(publication, first).is_none());
    assert!(host.root_output(world).is_none());
    let replacement = host
        .bind_output(
            host.world_ref(world).unwrap(),
            first.camera_entity().unwrap(),
            OutputKind::Camera,
        )
        .unwrap();
    assert_ne!(replacement, first);
    assert_eq!(
        host.resolve_world_ref(first.world().id(), first.world().incarnation()),
        Some(first.world())
    );
    assert_eq!(
        host.resolve_world_ref(first.world().id(), first.world().incarnation() + 1),
        None
    );
    assert_eq!(
        host.resolve_output_ref(first.world(), first.target()),
        Err(ErrorReason::InvalidEntity)
    );
    assert_eq!(
        host.resolve_output_ref(replacement.world(), replacement.target()),
        Ok(replacement)
    );
    host.set_root_output(replacement, viewport()).unwrap();
    assert!(host.root_output(world).is_none());
    failure.fail_publication(None);
    frame(&mut host);
    assert_eq!(host.root_output(world).unwrap().0, replacement);
    assert_eq!(camera.projection.fov_y, Camera::default().fov_y);
}

#[test]
fn published_anchor_deletion_releases_child_and_world_destruction_does_not_wait() {
    let mut host = HostRuntime::new();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, SPATIAL]))
        .unwrap();
    let child = host.create_world(Default::default(), &[]).unwrap();
    let peer = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let anchor = attach(&mut host, parent, child, 0.0);
    let peer_anchor = create(&mut host, peer, Vec::new());
    frame(&mut host);
    let child_ref = host.world_ref(child).unwrap();
    apply(
        &mut host,
        parent,
        vec![Command::Delete {
            entity: EntityRef::Handle(anchor),
        }],
    )
    .unwrap();
    apply(
        &mut host,
        peer,
        vec![Command::insert_value(
            EntityRef::Handle(peer_anchor),
            ComponentValue::WorldAttachment(WorldAttachment::spatial(child_ref)),
        )],
    )
    .unwrap();
    frame(&mut host);
    assert!(host.destroy_world(peer));
    assert_eq!(host.world_ref(child), Some(child_ref));
    attach(&mut host, parent, child, 0.0);
    frame(&mut host);
}

#[test]
fn retained_publication_keeps_exact_mesh_then_explicit_revoke_invalidates_it_without_waiting() {
    let (mut host, failure) = support::world_failures::host_with_world_failures();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, SPATIAL]))
        .unwrap();
    let child = host
        .create_world(Default::default(), &select_with_failures(&[RENDER]))
        .unwrap();
    let peer = host.create_world(Default::default(), &[]).unwrap();
    attach(&mut host, parent, child, 0.0);
    let (entity, key, source) = mesh(&mut host, child);
    frame(&mut host);
    let publication = host.latest_publication(child).unwrap();
    assert_eq!(
        host.publication(publication)
            .unwrap()
            .chunk(RenderSystem::ID)
            .unwrap()
            .data::<RenderPublication>()
            .unwrap()
            .items
            .len(),
        1
    );
    assert!(host.publication_resource(publication, key).is_some());

    // The child's failed publication retains the completed one after the World drops its mesh.
    failure.fail_publication(Some(child));
    apply(
        &mut host,
        child,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::MESH_INSTANCE,
        }],
    )
    .unwrap();
    assert_eq!(host.latest_publication(child), Some(publication));
    host.asset_resources_mut()
        .release_client_source(child, &source);
    host.flush_resource_lifecycle();
    assert!(host.publication_resource(publication, key).is_some());
    host.asset_resources_mut().invalidate_graphics(key);
    host.flush_resource_lifecycle();
    assert!(host.publication(publication).is_some());
    assert!(host.publication_resource(publication, key).is_some());
    assert!(
        host.asset_resources()
            .get_typed::<ipp_core::MeshAsset>(key)
            .is_some()
    );
    host.asset_resources_mut().revoke_resource(key);
    host.flush_resource_lifecycle();
    assert!(host.publication(publication).is_none());
    assert!(host.publication_resource(publication, key).is_none());
    assert!(host.asset_resources().get(key).is_none());
    let next = host.frame(0.0).unwrap();
    assert!(next.publication_errors.contains_key(&child));
    assert!(next.evaluation_order.contains(&parent) && next.evaluation_order.contains(&peer));
    assert_eq!(host.latest_publication(child), None);

    failure.fail_publication(None);
    frame(&mut host);
    let current = host.latest_publication(child).unwrap();
    assert_ne!(current, publication);
    assert!(
        host.publication(current)
            .unwrap()
            .chunk(RenderSystem::ID)
            .unwrap()
            .data::<RenderPublication>()
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn unselected_evaluators_have_explicit_absence_and_no_output_fallback() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), &[]).unwrap();
    let entity = create(&mut host, world, Vec::new());
    let access = host.world_mut(world).unwrap();
    assert_eq!(access.active_camera(), None);
    assert!(access.active_camera_component().is_none());
    assert_eq!(
        access.prepare_camera(640, 480),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert!(access.geometry_spatial_index().is_none() && access.picking_spatial_index().is_none());
    assert_eq!(
        access.mesh_bounds(entity),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert_eq!(
        access.picking_geometry(entity),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert_eq!(
        access.render_state(),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert!(access.render_items().is_empty() && access.light_items().next().is_none());
    // A canvas names no entity; this World supplies none.
    assert_eq!(
        access.bind_output(entity, OutputKind::Canvas),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(
        access.bind_output(entity, OutputKind::Camera),
        Err(ErrorReason::UnsupportedDependency)
    );
    let world_ref = access.world_ref();
    drop(access);
    assert_eq!(
        host.resolve_output_ref(world_ref, ipp_core::OutputTarget::Canvas),
        Err(ErrorReason::UnsupportedDependency)
    );
    let child = host.create_world(Default::default(), &[]).unwrap();
    let attachment = WorldAttachment::spatial(host.world_ref(child).unwrap());
    assert_eq!(
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::WorldAttachment(attachment)
            )]
        )
        .unwrap_err()
        .reason,
        ErrorReason::UnsupportedDependency
    );
}

#[test]
fn independent_saved_world_copies_share_durable_metadata_not_runtime_attachment_identity() {
    use ipp_core::services::world_serialization::{WorldLoadOptions, WorldPersistenceLimits};
    let mut host = HostRuntime::new();
    let original = host.create_world(Default::default(), &[]).unwrap();
    let bytes = host
        .save_world(original, 17, WorldPersistenceLimits::default())
        .unwrap();
    let copy = host
        .load_world(
            &bytes,
            17,
            WorldLoadOptions {
                symbolic_id: Some("copy".into()),
                ..Default::default()
            },
            Default::default(),
            WorldPersistenceLimits::default(),
        )
        .unwrap()
        .root
        .id();
    let descriptors = host.list_worlds();
    assert_eq!(
        descriptors[0].metadata.persistent_id,
        descriptors[1].metadata.persistent_id
    );
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, SPATIAL]))
        .unwrap();
    attach(&mut host, parent, original, 1.0);
    attach(&mut host, parent, copy, 2.0);
    frame(&mut host);
    let publication = host
        .publication(host.latest_publication(parent).unwrap())
        .unwrap();
    assert_eq!(publication.attachments.len(), 2);
    assert_ne!(
        publication.attachments[0].child,
        publication.attachments[1].child
    );
    let mut other_host = HostRuntime::new();
    let same_id = other_host
        .create_world(Default::default(), ATTACHMENTS)
        .unwrap();
    let anchor = create(&mut other_host, same_id, Vec::new());
    assert!(
        apply(
            &mut other_host,
            same_id,
            vec![Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::WorldAttachment(WorldAttachment::spatial(
                    host.world_ref(original).unwrap()
                ))
            )]
        )
        .is_err()
    );
}

type FrameObservations =
    std::sync::Arc<std::sync::Mutex<Vec<(WorldId, ipp_core::WorldFrameContext)>>>;

const FRAME_CONTEXT: ipp_core::systems::SystemId =
    ipp_core::systems::SystemId("fixture.frame-context");

/// The named parts plus the frame-context observer, registered after every compiled System.
fn observed(parts: &[&[ipp_core::systems::SystemId]]) -> Vec<ipp_core::systems::SystemId> {
    let mut selected = select(parts);
    selected.push(FRAME_CONTEXT);
    selected
}

fn observed_host() -> (HostRuntime, FrameObservations) {
    use ipp_core::systems::*;
    use std::sync::Arc;

    struct Observer(FrameObservations);

    struct Factory(FrameObservations);

    impl SystemFactory for Factory {
        fn id(&self) -> SystemId {
            FRAME_CONTEXT
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(Observer(self.0.clone())))
        }
    }

    impl System for Observer {
        fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
            self.0.lock().unwrap().push((
                context.world.id(),
                context.world.frame_context().unwrap().clone(),
            ));
        }
    }

    let observations = FrameObservations::default();
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Factory(observations.clone())));
    (
        HostRuntime::with_system_factories(factories).unwrap(),
        observations,
    )
}

fn observed_context(
    observations: &FrameObservations,
    world: WorldId,
) -> ipp_core::WorldFrameContext {
    observations
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|(observed, _)| *observed == world)
        .unwrap()
        .1
        .clone()
}

#[test]
fn selected_root_context_requires_an_explicit_live_output_incarnation() {
    let (mut host, observations) = observed_host();
    let world = host
        .create_world(Default::default(), &observed(&[CAMERA]))
        .unwrap();
    let other = camera(&mut host, world);
    let selected = camera(&mut host, world);
    let viewport = WorldViewport {
        width: 1600,
        height: 900,
        device_pixel_ratio: 2.0,
    };
    frame(&mut host);
    assert_eq!(observed_context(&observations, world).selected_output, None);

    host.set_root_output(selected, viewport).unwrap();
    frame(&mut host);
    let context = observed_context(&observations, world);
    assert_eq!(context.selected_output, Some(selected));
    assert_ne!(context.selected_output, Some(other));
    assert_eq!(context.viewport, Some(viewport));
    assert_eq!(context.surface_extent, None);

    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(selected.camera_entity().unwrap()),
                component: ComponentValue::CAMERA,
            },
            Command::insert_value(
                EntityRef::Handle(selected.camera_entity().unwrap()),
                ComponentValue::Camera(Camera::default()),
            ),
        ],
    )
    .unwrap();
    frame(&mut host);
    assert_eq!(observed_context(&observations, world).selected_output, None);
    assert!(host.root_output(world).is_none());

    let replacement = host
        .bind_output(
            host.world_ref(world).unwrap(),
            selected.camera_entity().unwrap(),
            OutputKind::Camera,
        )
        .unwrap();
    assert_ne!(replacement, selected);
    host.set_root_output(replacement, viewport).unwrap();
    frame(&mut host);
    assert_eq!(
        observed_context(&observations, world).selected_output,
        Some(replacement)
    );

    host.clear_root_output(world);
    frame(&mut host);
    let context = observed_context(&observations, world);
    assert_eq!(context.selected_output, None);
    assert_eq!(context.viewport, None);
}

#[test]
fn parent_context_is_current_and_detachment_clears_it() {
    let (mut host, observations) = observed_host();
    #[cfg(feature = "surfaces")]
    let parent_parts = [ATTACHMENTS, CAMERA, SURFACE];
    #[cfg(not(feature = "surfaces"))]
    let parent_parts = [ATTACHMENTS, CAMERA];
    let child = host
        .create_world(Default::default(), &observed(&[]))
        .unwrap();
    let parent = host
        .create_world(Default::default(), &observed(&parent_parts))
        .unwrap();
    let root = camera(&mut host, parent);
    host.set_root_output(root, viewport()).unwrap();
    let anchor = attach(&mut host, parent, child, 13.0);
    #[cfg(feature = "surfaces")]
    apply(
        &mut host,
        parent,
        vec![
            Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::Surface(ipp_core::components::Surface::default()),
            ),
            Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::SurfaceCache(ipp_core::components::SurfaceCache::default()),
            ),
        ],
    )
    .unwrap();
    frame(&mut host);
    assert_eq!(
        observed_context(&observations, parent).selected_output,
        Some(root)
    );

    observations.lock().unwrap().clear();
    let report = frame(&mut host);
    let values = observations.lock().unwrap();
    let child_context = &values.iter().find(|(world, _)| *world == child).unwrap().1;
    assert_eq!(child_context.frame, report.frame);
    assert_eq!(child_context.delta, 0.25);
    assert_eq!(child_context.placement[12], 13.0);
    assert_eq!(child_context.viewport, Some(viewport()));
    assert_eq!(child_context.surface_extent, None);
    assert_eq!(child_context.selected_output, None);
    drop(values);

    apply(
        &mut host,
        parent,
        vec![Command::Delete {
            entity: EntityRef::Handle(anchor),
        }],
    )
    .unwrap();
    observations.lock().unwrap().clear();
    host.world_mut(child)
        .unwrap()
        .enqueue(Batch {
            id: 9,
            operations: vec![Command::Create {
                alias: 42,
                metadata: Default::default(),
                adopt: false,
            }],
        })
        .unwrap();
    let detached = frame(&mut host);
    assert_eq!(
        detached.worlds[&child].as_ref().unwrap().outcomes[0].batch_id,
        9
    );
    let values = observations.lock().unwrap();
    let child_context = &values.iter().find(|(world, _)| *world == child).unwrap().1;
    assert_eq!(child_context.placement[12], 0.0);
    assert_eq!(child_context.viewport, None);
    assert_eq!(child_context.selected_output, None);
    drop(values);

    let peer = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA]))
        .unwrap();
    let peer_output = camera(&mut host, peer);
    let peer_viewport = WorldViewport {
        width: 1024,
        height: 768,
        device_pixel_ratio: 2.0,
    };
    host.set_root_output(peer_output, peer_viewport).unwrap();
    let anchor = create(
        &mut host,
        peer,
        vec![ComponentValue::Transform(Transform {
            x: 29.0,
            ..Default::default()
        })],
    );
    let attachment = WorldAttachment::spatial(host.world_ref(child).unwrap());
    observations.lock().unwrap().clear();
    host.world_mut(peer)
        .unwrap()
        .enqueue(Batch {
            id: 10,
            operations: vec![Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::WorldAttachment(attachment),
            )],
        })
        .unwrap();
    let reattached = frame(&mut host);
    assert!(
        reattached
            .evaluation_order
            .iter()
            .position(|world| *world == peer)
            .unwrap()
            < reattached
                .evaluation_order
                .iter()
                .position(|world| *world == child)
                .unwrap()
    );
    assert_eq!(
        reattached
            .evaluation_order
            .iter()
            .filter(|world| **world == child)
            .count(),
        1
    );
    let values = observations.lock().unwrap();
    let child_context = &values.iter().find(|(world, _)| *world == child).unwrap().1;
    assert_eq!(child_context.placement[12], 29.0);
    assert_eq!(child_context.viewport, Some(peer_viewport));
    assert_eq!(child_context.selected_output, None);
    assert_eq!(child_context.frame, reattached.frame);
}

#[test]
fn custom_material_lights_unknown_bounds_and_unchanged_chunks_are_owned() {
    use ipp_core::{
        DynamicValue,
        components::{CustomMaterial, Light},
    };
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), RENDER).unwrap();
    let (entity, _, _) = mesh(&mut host, world);
    let mut custom = CustomMaterial::default();
    custom
        .properties
        .set("tint", DynamicValue::Vec3([0.2, 0.4, 0.6]))
        .unwrap();
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::CustomMaterial(custom),
        )],
    )
    .unwrap();
    let light = create(
        &mut host,
        world,
        vec![
            ComponentValue::Transform(Transform {
                x: 3.0,
                ..Default::default()
            }),
            ComponentValue::Light(Light::default()),
        ],
    );
    let unknown = create(
        &mut host,
        world,
        vec![ComponentValue::BoundingGeometry(
            ipp_core::components::BoundingGeometry::default(),
        )],
    );
    frame(&mut host);
    let publication = host.latest_publication(world).unwrap();
    let render = host
        .publication(publication)
        .unwrap()
        .chunk(RenderSystem::ID)
        .unwrap();
    let version = render.version();
    let old = render.data::<RenderPublication>().unwrap().clone();
    assert_eq!(old.lights[0].entity, light);
    assert_eq!(old.lights[0].model[12], 3.0);
    let geometry = host
        .publication(publication)
        .unwrap()
        .chunk(GeometrySystem::ID)
        .unwrap()
        .data::<GeometryPublication>()
        .unwrap();
    assert!(
        geometry
            .entities
            .iter()
            .find(|value| value.entity == unknown)
            .unwrap()
            .culling
            .is_none()
    );
    let excluded = [ipp_core::systems::geometry::GeometryPlane {
        normal: [1.0, 0.0, 0.0],
        offset: -100.0,
    }; 6];
    assert!(
        geometry
            .visible(&excluded)
            .any(|value| value.entity == unknown)
    );
    frame(&mut host);
    let unchanged = host.latest_publication(world).unwrap();
    assert_eq!(
        host.publication(unchanged)
            .unwrap()
            .chunk(RenderSystem::ID)
            .unwrap()
            .version(),
        version
    );
    apply(
        &mut host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: "tint".into(),
            value: DynamicValue::Vec3([1.0; 3]),
        }],
    )
    .unwrap();
    frame(&mut host);
    let current = host
        .publication(host.latest_publication(world).unwrap())
        .unwrap()
        .chunk(RenderSystem::ID)
        .unwrap();
    assert_ne!(current.version(), version);
    assert_ne!(
        old.items[0].custom,
        current.data::<RenderPublication>().unwrap().items[0].custom
    );
}

#[cfg(feature = "surfaces")]
#[test]
fn parent_surface_cache_policy_is_owned_validated_and_current() {
    use ipp_core::components::{Surface, SurfaceCache};
    use ipp_core::systems::surface::SurfaceCachePolicy;

    let mut host = HostRuntime::new();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, SURFACE]))
        .unwrap();
    let child = host.create_world(Default::default(), CAMERA).unwrap();
    let selected = camera(&mut host, child);
    let cache = SurfaceCache::default();
    let anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Surface(Surface::default()),
            ComponentValue::SurfaceCache(cache),
            ComponentValue::WorldAttachment(WorldAttachment::surface(selected)),
        ],
    );
    let published = |host: &HostRuntime| {
        host.publication(host.latest_publication(parent).unwrap())
            .unwrap()
            .attachments[0]
            .clone()
    };
    frame(&mut host);
    let original = published(&host);
    assert_eq!(
        original.surface_cache_policy,
        Some(SurfaceCachePolicy::new(&cache).unwrap())
    );
    assert!(original.publication.is_some());

    let replacement = SurfaceCache {
        direct_distance: 12.0,
        texels_per_metre: 512.0,
        max_refresh_hz: 15.0,
    };
    apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::SurfaceCache(replacement),
        )],
    )
    .unwrap();
    frame(&mut host);
    let updated = published(&host);
    assert_eq!(updated.publication, host.latest_publication(child));
    assert_eq!(updated.output, Some(selected));
    assert_eq!(
        updated.surface_cache_policy,
        Some(SurfaceCachePolicy::new(&replacement).unwrap())
    );
    assert_eq!(
        original.surface_cache_policy,
        Some(SurfaceCachePolicy::new(&cache).unwrap())
    );

    assert!(
        apply(
            &mut host,
            parent,
            vec![Command::SetField {
                entity: EntityRef::Handle(anchor),
                component: ComponentValue::SURFACE_CACHE,
                field: FieldWrite {
                    offset: std::mem::offset_of!(SurfaceCache, max_refresh_hz) as u32,
                    value: FieldValue::F32(f32::NAN),
                },
            }],
        )
        .is_err()
    );
    frame(&mut host);
    assert_eq!(
        published(&host).surface_cache_policy,
        updated.surface_cache_policy
    );

    apply(
        &mut host,
        parent,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(anchor),
            component: ComponentValue::SURFACE_CACHE,
        }],
    )
    .unwrap();
    frame(&mut host);
    let without_cache = published(&host);
    assert_eq!(without_cache.surface_cache_policy, None);
    assert_eq!(without_cache.publication, host.latest_publication(child));

    apply(
        &mut host,
        parent,
        vec![
            Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::SurfaceCache(cache),
            ),
            Command::RemoveComponent {
                entity: EntityRef::Handle(anchor),
                component: ComponentValue::SURFACE,
            },
        ],
    )
    .unwrap();
    frame(&mut host);
    let unavailable = published(&host);
    assert_eq!(unavailable.surface_extent, None);
    assert_eq!(unavailable.surface_cache_policy, None);
    assert_eq!(unavailable.publication, None);
}

#[cfg(feature = "surfaces")]
#[test]
fn surface_camera_mode_never_becomes_spatial_and_explicit_rebind_restores_availability() {
    let (mut host, observations) = observed_host();
    let child = host
        .create_world(Default::default(), &observed(&[CAMERA]))
        .unwrap();
    let parent = host
        .create_world(
            Default::default(),
            &observed(&[ATTACHMENTS, CAMERA, SURFACE]),
        )
        .unwrap();
    let selected = camera(&mut host, child);
    let parent_output = camera(&mut host, parent);
    host.set_root_output(parent_output, viewport()).unwrap();
    let anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::Surface(ipp_core::components::Surface::default()),
            ComponentValue::WorldAttachment(WorldAttachment::surface(selected)),
        ],
    );
    assert_eq!(
        host.set_root_output(selected, viewport()),
        Err(ErrorReason::InvalidValue)
    );
    frame(&mut host);
    let published = |host: &HostRuntime| {
        host.publication(host.latest_publication(parent).unwrap())
            .unwrap()
            .attachments[0]
            .clone()
    };
    assert!(published(&host).publication.is_some());
    let child_context = observed_context(&observations, child);
    assert_eq!(child_context.selected_output, Some(selected));
    assert_eq!(child_context.viewport, Some(viewport()));
    assert_eq!(child_context.surface_extent, Some([1.0; 2]));
    assert_eq!(
        observed_context(&observations, parent).selected_output,
        Some(parent_output)
    );
    apply(
        &mut host,
        child,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(selected.camera_entity().unwrap()),
                component: ComponentValue::CAMERA,
            },
            Command::insert_value(
                EntityRef::Handle(selected.camera_entity().unwrap()),
                ComponentValue::Camera(Camera::default()),
            ),
        ],
    )
    .unwrap();
    frame(&mut host);
    assert!(published(&host).publication.is_none());
    assert_eq!(observed_context(&observations, child).selected_output, None);
    assert_eq!(
        published(&host).mode,
        ipp_core::WorldAttachmentMode::SurfaceCamera
    );
    let replacement = host
        .bind_output(
            host.world_ref(child).unwrap(),
            selected.camera_entity().unwrap(),
            OutputKind::Camera,
        )
        .unwrap();
    apply(
        &mut host,
        parent,
        vec![Command::SetField {
            entity: EntityRef::Handle(anchor),
            component: ComponentValue::WORLD_ATTACHMENT,
            field: FieldWrite {
                offset: std::mem::offset_of!(WorldAttachment, output) as u32,
                value: FieldValue::Output(Some(replacement)),
            },
        }],
    )
    .unwrap();
    frame(&mut host);
    assert!(published(&host).publication.is_some());
    assert_eq!(
        observed_context(&observations, child).selected_output,
        Some(replacement)
    );
    apply(
        &mut host,
        parent,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(anchor),
            component: ComponentValue::SURFACE,
        }],
    )
    .unwrap();
    frame(&mut host);
    assert!(published(&host).publication.is_none());
    assert_eq!(observed_context(&observations, child).selected_output, None);
    assert_eq!(observed_context(&observations, child).surface_extent, None);
    assert_eq!(
        published(&host).mode,
        ipp_core::WorldAttachmentMode::SurfaceCamera
    );
}
