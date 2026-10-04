//! Generic placement dispatch with real output lifetimes and Host scheduling, not GUI coverage.

mod support;

use support::selection::{ATTACHMENTS, CAMERA, GEOMETRY, select};

use ipp_core::{
    AttachmentPlacement, Batch, Command, ComponentValue, EntityId, EntityRef, HostRuntime,
    OutputKind, OutputRef, WorldAttachment, WorldAttachmentRetirement, WorldContext,
    WorldFrameContext, WorldId,
    components::{Camera, Transform},
    systems::*,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct PlacementInputs {
    target: Option<(WorldId, EntityId)>,
    claim: AttachmentPlacement,
    duplicate: bool,
    contexts: BTreeMap<WorldId, WorldFrameContext>,
}

type SharedInputs = Arc<Mutex<PlacementInputs>>;

struct PlacementFactory {
    inputs: SharedInputs,
    primary: bool,
}

struct PlacementSystem {
    inputs: SharedInputs,
    primary: bool,
    prepared: Option<(WorldId, EntityId, AttachmentPlacement)>,
}

impl SystemFactory for PlacementFactory {
    fn id(&self) -> SystemId {
        SystemId(if self.primary {
            "fixture.placement-primary"
        } else {
            "fixture.placement-secondary"
        })
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(PlacementSystem {
            inputs: self.inputs.clone(),
            primary: self.primary,
            prepared: None,
        }))
    }
}

impl System for PlacementSystem {
    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        let mut inputs = self.inputs.lock().unwrap();
        if let Some(frame) = context.world.frame_context() {
            inputs.contexts.insert(context.world.id(), frame.clone());
        }
        self.prepared = inputs
            .target
            .filter(|(world, _)| *world == context.world.id())
            .filter(|_| self.primary || inputs.duplicate)
            .map(|(world, entity)| (world, entity, inputs.claim));
    }

    fn attachment_placement(
        &self,
        world: &WorldContext<'_>,
        anchor: EntityId,
    ) -> AttachmentPlacement {
        self.prepared
            .filter(|(parent, entity, _)| *parent == world.id() && *entity == anchor)
            .map_or(AttachmentPlacement::Unmanaged, |(_, _, claim)| claim)
    }
}

fn submit(
    host: &mut HostRuntime,
    world: WorldId,
    operations: Vec<Command>,
) -> ipp_core::BatchOutcome {
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
}

fn apply(
    host: &mut HostRuntime,
    world: WorldId,
    operations: Vec<Command>,
) -> ipp_core::BatchOutcome {
    let outcome = submit(host, world, operations);
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    outcome
}

fn create(host: &mut HostRuntime, world: WorldId, values: Vec<ComponentValue>) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(0), value)),
    );
    apply(host, world, operations).result.unwrap()[0].1
}

fn camera(host: &mut HostRuntime, world: WorldId) -> OutputRef {
    let entity = create(host, world, vec![ComponentValue::Camera(Camera::default())]);
    host.bind_output(host.world_ref(world).unwrap(), entity, OutputKind::Camera)
        .unwrap()
}

fn affine(translation: f64) -> [f64; 16] {
    [
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        translation,
        0.0,
        0.0,
        1.0,
    ]
}

fn fixture() -> (
    HostRuntime,
    SharedInputs,
    WorldId,
    WorldId,
    EntityId,
    OutputRef,
) {
    let inputs = SharedInputs::default();
    let mut factories = compiled_system_factories();
    for primary in [true, false] {
        factories.push(Arc::new(PlacementFactory {
            inputs: inputs.clone(),
            primary,
        }));
    }
    let mut host = crate::support::task_scheduler::with_factories(factories).unwrap();
    // The parent also anchors Surfaces and joint-parented placements.
    let mut parts = vec![ATTACHMENTS, CAMERA];
    parts.push(support::selection::SURFACE);
    parts.push(support::selection::SKELETON);
    let parent = host
        .create_world(
            Default::default(),
            &[
                select(&parts),
                vec![
                    SystemId("fixture.placement-primary"),
                    SystemId("fixture.placement-secondary"),
                ],
            ]
            .concat(),
        )
        .unwrap();
    let child = host
        .create_world(
            Default::default(),
            &[
                select(&[ATTACHMENTS, CAMERA]),
                vec![
                    SystemId("fixture.placement-primary"),
                    SystemId("fixture.placement-secondary"),
                ],
            ]
            .concat(),
        )
        .unwrap();
    let owner = camera(&mut host, parent);
    let value = WorldAttachment::spatial(host.world_ref(child).unwrap());
    let anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Transform(Transform {
                x: 100.0,
                ..Default::default()
            }),
            ComponentValue::WorldAttachment(value),
        ],
    );
    inputs.lock().unwrap().target = Some((parent, anchor));
    (host, inputs, parent, child, anchor, owner)
}

fn edge(host: &HostRuntime, parent: WorldId) -> &ipp_core::PublishedWorldAttachment {
    &host
        .publication(host.latest_publication(parent).unwrap())
        .unwrap()
        .attachments[0]
}

fn camera_far(owner: OutputRef, far: f32) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(owner.camera_entity().unwrap()),
        component: ComponentValue::CAMERA,
        field: ipp_core::FieldWrite {
            offset: std::mem::offset_of!(Camera, far) as u32,
            value: ipp_core::FieldValue::F32(far),
        },
    }
}

#[test]
fn output_aware_spatial_traversal_matches_owned_placements_and_picking() {
    use ipp_core::components::PickingGeometry;
    use ipp_core::systems::geometry::{GeometryDefinition, GeometryRay, GeometryShape};

    let (mut host, inputs, parent, child, anchor, owner) = fixture();
    let other = camera(&mut host, parent);
    host.frame(0.0).unwrap();
    let publication = host.latest_publication(parent).unwrap();
    assert_eq!(host.spatial_contributions(publication).len(), 2);
    assert_eq!(
        host.spatial_contributions_for_output(publication, owner)
            .unwrap()
            .len(),
        2
    );

    let grandchild = host.create_world(Default::default(), GEOMETRY).unwrap();
    let attachment = WorldAttachment::spatial(host.world_ref(grandchild).unwrap());
    let child_anchor = create(
        &mut host,
        child,
        vec![
            ComponentValue::Transform(Transform {
                x: 2.0,
                ..Default::default()
            }),
            ComponentValue::WorldAttachment(attachment),
        ],
    );
    let picked = create(
        &mut host,
        grandchild,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::PickingGeometry(PickingGeometry {
                geometry: GeometryDefinition::from(GeometryShape::Box {
                    min: [-0.5; 3],
                    max: [0.5; 3],
                })
                .encode()
                .unwrap(),
                ..Default::default()
            }),
        ],
    );
    inputs.lock().unwrap().claim = AttachmentPlacement::Ready {
        owner,
        affine: affine(7.0),
    };
    host.frame(0.0).unwrap();
    let publication = host.latest_publication(parent).unwrap();
    let contributions = host
        .spatial_contributions_for_output(publication, owner)
        .unwrap();
    assert_eq!(
        contributions
            .iter()
            .map(|entry| entry.publication.world.id())
            .collect::<Vec<_>>(),
        [parent, child, grandchild]
    );
    assert_eq!(contributions[2].placement.matrix(), affine(9.0));
    assert_eq!(host.spatial_contributions(publication).len(), 1);
    assert_eq!(
        host.spatial_contributions_for_output(publication, other)
            .unwrap()
            .len(),
        1
    );

    let ray = GeometryRay {
        origin: [9.0, 0.0, 5.0],
        direction: [0.0, 0.0, -1.0],
    };
    let hit = host
        .pick_publication(publication, owner, &ray, 0.0, 100.0)
        .unwrap()
        .unwrap();
    assert_eq!(
        (hit.world.id(), hit.entity, hit.hit.distance),
        (grandchild, picked, 4.5)
    );
    assert_eq!(
        hit.path,
        [
            (host.world_ref(parent).unwrap(), anchor),
            (host.world_ref(child).unwrap(), child_anchor),
        ]
    );
    assert_eq!(
        host.pick_publication(publication, other, &ray, 0.0, 100.0),
        Ok(None)
    );
    assert!(host.root_output(parent).is_none());
}

#[test]
fn output_aware_spatial_queries_preserve_unavailable_geometry_and_output_errors() {
    use ipp_core::{ErrorReason, components::PickingGeometry, systems::geometry::GeometryRay};

    let (mut host, inputs, parent, child, _, owner) = fixture();
    let other = camera(&mut host, parent);
    host.io_mut().register_stream("fixture://").unwrap();
    create(
        &mut host,
        child,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::PickingGeometry(PickingGeometry {
                source: "fixture:///pending-geometry".into(),
                ..Default::default()
            }),
        ],
    );
    inputs.lock().unwrap().claim = AttachmentPlacement::Ready {
        owner,
        affine: affine(0.0),
    };
    host.frame(0.0).unwrap();
    let publication = host.latest_publication(parent).unwrap();
    let ray = GeometryRay {
        origin: [0.0, 0.0, 5.0],
        direction: [0.0, 0.0, -1.0],
    };
    assert_eq!(
        host.spatial_contributions_for_output(publication, owner)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        host.pick_publication(publication, owner, &ray, 0.0, 100.0),
        Err(ErrorReason::GeometryUnavailable)
    );
    assert_eq!(
        host.pick_publication(publication, other, &ray, 0.0, 100.0),
        Ok(None)
    );

    let wrong_publication = host.latest_publication(child).unwrap();
    assert!(matches!(
        host.spatial_contributions_for_output(wrong_publication, owner),
        Err(ErrorReason::InvalidEntity)
    ));
    assert_eq!(
        host.pick_publication(wrong_publication, owner, &ray, 0.0, 100.0),
        Err(ErrorReason::InvalidEntity)
    );
    apply(
        &mut host,
        parent,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(owner.camera_entity().unwrap()),
            component: ComponentValue::CAMERA,
        }],
    );
    assert!(matches!(
        host.spatial_contributions_for_output(publication, owner),
        Err(ErrorReason::InvalidEntity)
    ));
    assert_eq!(
        host.pick_publication(publication, owner, &ray, 0.0, 100.0),
        Err(ErrorReason::InvalidEntity)
    );
}

#[test]
fn prepared_claim_replaces_hierarchy_before_child_context_without_selecting_presentation() {
    let (mut host, inputs, parent, child, anchor, owner) = fixture();
    host.frame(0.0).unwrap();
    assert_eq!(edge(&host, parent).placement, affine(100.0));
    assert_eq!(edge(&host, parent).placement_output, None);
    assert_eq!(
        host.spatial_contributions(host.latest_publication(parent).unwrap())
            .len(),
        2
    );
    let original_token = edge(&host, parent).token.clone();
    inputs.lock().unwrap().claim = AttachmentPlacement::Ready {
        owner,
        affine: affine(7.0),
    };
    let report = host.frame(0.1).unwrap();
    assert!(report.publication_errors.is_empty());
    assert_eq!(edge(&host, parent).placement, affine(7.0));
    assert_eq!(edge(&host, parent).placement_output, Some(owner));
    assert_eq!(edge(&host, parent).token, original_token);
    assert_eq!(
        inputs.lock().unwrap().contexts[&child].placement,
        affine(7.0)
    );
    assert!(host.attached_publication(edge(&host, parent)).is_some());
    assert_eq!(
        host.spatial_contributions(host.latest_publication(parent).unwrap())
            .len(),
        1
    );
    assert!(host.root_output(parent).is_none());

    apply(
        &mut host,
        parent,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(anchor),
            component: ComponentValue::TRANSFORM,
        }],
    );
    inputs.lock().unwrap().claim = AttachmentPlacement::Ready {
        owner,
        affine: affine(11.0),
    };
    host.frame(0.1).unwrap();
    assert_eq!(edge(&host, parent).placement, affine(11.0));
    assert_eq!(
        inputs.lock().unwrap().contexts[&child].placement,
        affine(11.0)
    );
    assert_eq!(edge(&host, parent).token, original_token);
}

#[test]
fn unavailable_duplicate_foreign_and_invalid_affine_claims_never_fall_back() {
    let (mut host, inputs, parent, child, _, owner) = fixture();
    let foreign = camera(&mut host, child);
    let mut not_affine = affine(7.0);
    not_affine[3] = 1.0;
    let mut nonfinite = affine(7.0);
    nonfinite[12] = f64::NAN;
    let mut singular = affine(7.0);
    singular[0] = 0.0;
    for claim in [
        AttachmentPlacement::Unavailable {
            owner,
        },
        AttachmentPlacement::Ready {
            owner: foreign,
            affine: affine(7.0),
        },
        AttachmentPlacement::Ready {
            owner,
            affine: not_affine,
        },
        AttachmentPlacement::Ready {
            owner,
            affine: nonfinite,
        },
        AttachmentPlacement::Ready {
            owner,
            affine: singular,
        },
    ] {
        inputs.lock().unwrap().claim = claim;
        host.frame(0.0).unwrap();
        assert!(
            host.publication(host.latest_publication(parent).unwrap())
                .unwrap()
                .attachments
                .is_empty()
        );
        assert_eq!(
            inputs.lock().unwrap().contexts[&child].placement,
            affine(0.0)
        );
    }
    {
        let mut inputs = inputs.lock().unwrap();
        inputs.claim = AttachmentPlacement::Ready {
            owner,
            affine: affine(7.0),
        };
        inputs.duplicate = true;
    }
    host.frame(0.0).unwrap();
    assert!(
        host.publication(host.latest_publication(parent).unwrap())
            .unwrap()
            .attachments
            .is_empty()
    );
    inputs.lock().unwrap().duplicate = false;
    host.frame(0.0).unwrap();
    assert_eq!(edge(&host, parent).placement_output, Some(owner));

    apply(
        &mut host,
        parent,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(owner.camera_entity().unwrap()),
                component: ComponentValue::CAMERA,
            },
            Command::insert_value(
                EntityRef::Handle(owner.camera_entity().unwrap()),
                ComponentValue::Camera(Camera::default()),
            ),
        ],
    );
    host.frame(0.0).unwrap();
    assert!(
        host.publication(host.latest_publication(parent).unwrap())
            .unwrap()
            .attachments
            .is_empty()
    );
    let replacement = host
        .bind_output(
            host.world_ref(parent).unwrap(),
            owner.camera_entity().unwrap(),
            OutputKind::Camera,
        )
        .unwrap();
    assert_ne!(replacement, owner);
    inputs.lock().unwrap().claim = AttachmentPlacement::Ready {
        owner: replacement,
        affine: affine(7.0),
    };
    host.frame(0.0).unwrap();
    assert_eq!(edge(&host, parent).placement_output, Some(replacement));
}

#[test]
fn stale_owner_invalidates_retained_edge_and_retires_its_token() {
    let (mut host, inputs, parent, _, _, owner) = fixture();
    inputs.lock().unwrap().claim = AttachmentPlacement::Ready {
        owner,
        affine: affine(7.0),
    };
    host.frame(0.0).unwrap();
    let retained = edge(&host, parent).clone();
    apply(
        &mut host,
        parent,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(owner.camera_entity().unwrap()),
                component: ComponentValue::CAMERA,
            },
            Command::DetachWorldAttachmentIf {
                expected: retained.token.clone(),
            },
        ],
    );
    assert!(host.attached_publication(&retained).is_none());
    assert_eq!(
        host.attachment_retirement(&retained.token),
        Ok(WorldAttachmentRetirement::Retired)
    );

    apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(owner.camera_entity().unwrap()),
            ComponentValue::Camera(Camera::default()),
        )],
    );
    let replacement = host
        .bind_output(
            host.world_ref(parent).unwrap(),
            owner.camera_entity().unwrap(),
            OutputKind::Camera,
        )
        .unwrap();
    assert_ne!(replacement, owner);
    assert!(host.attached_publication(&retained).is_none());
    assert_eq!(
        host.attachment_retirement(&retained.token),
        Ok(WorldAttachmentRetirement::Retired)
    );
}

#[test]
fn retired_detach_cannot_recover_with_same_output_incarnation_or_compete_with_new_parent() {
    let (mut host, inputs, parent, child, anchor, owner) = fixture();
    let peer = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let peer_anchor = create(&mut host, peer, Vec::new());
    let child_ref = host.world_ref(child).unwrap();
    inputs.lock().unwrap().claim = AttachmentPlacement::Ready {
        owner,
        affine: affine(7.0),
    };
    host.frame(0.0).unwrap();
    let publication = host.latest_publication(parent).unwrap();
    let retained = edge(&host, parent).clone();
    assert!(host.attached_publication(&retained).is_some());
    assert_eq!(
        host.spatial_contributions_for_output(publication, owner)
            .unwrap()
            .len(),
        2
    );

    let outcome = submit(
        &mut host,
        parent,
        vec![
            Command::DetachWorldAttachmentIf {
                expected: retained.token.clone(),
            },
            camera_far(owner, 0.0),
        ],
    );
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ipp_core::ErrorReason::InvalidValue
    );
    assert_eq!(outcome.effects.len(), 1);
    assert_eq!(outcome.effects[0].operation, 0);
    assert_eq!(
        outcome.effects[0].effect,
        ipp_core::OperationEffect::WorldAttachment(ipp_core::WorldAttachmentEffect::Detached(
            retained.token.clone()
        ))
    );
    assert!(host.attached_publication(&retained).is_none());
    assert_eq!(
        host.attachment_retirement(&retained.token),
        Ok(WorldAttachmentRetirement::Retired)
    );
    apply(
        &mut host,
        peer,
        vec![Command::insert_value(
            EntityRef::Handle(peer_anchor),
            ComponentValue::WorldAttachment(WorldAttachment::spatial(child_ref)),
        )],
    );

    apply(&mut host, parent, vec![camera_far(owner, 100.0)]);
    assert_eq!(
        host.bind_output(owner.world(), owner.camera_entity().unwrap(), owner.kind()),
        Ok(owner)
    );
    assert!(host.attached_publication(&retained).is_none());

    for _ in 0..3 {
        host.frame(0.0).unwrap();
        assert!(
            host.publication(host.latest_publication(parent).unwrap())
                .unwrap()
                .attachments
                .is_empty()
        );
        assert_eq!(
            host.attachment_retirement(&retained.token),
            Ok(WorldAttachmentRetirement::Retired)
        );
        assert!(host.attached_publication(&retained).is_none());
        assert!(host.attached_publication(edge(&host, peer)).is_some());
    }

    let rejected = submit(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::WorldAttachment(WorldAttachment::spatial(child_ref)),
        )],
    );
    assert_eq!(
        rejected.result.unwrap_err().reason,
        ipp_core::ErrorReason::InvalidValue
    );
    assert!(host.attached_publication(&retained).is_none());
    assert!(host.attached_publication(edge(&host, peer)).is_some());
}

#[test]
fn active_attachment_recovers_from_invalid_placement_owner_without_retiring_its_token() {
    let (mut host, inputs, parent, _, _, owner) = fixture();
    inputs.lock().unwrap().claim = AttachmentPlacement::Ready {
        owner,
        affine: affine(7.0),
    };
    host.frame(0.0).unwrap();
    let retained = edge(&host, parent).clone();
    let failed = submit(&mut host, parent, vec![camera_far(owner, 0.0)]);
    assert_eq!(
        failed.result.unwrap_err().reason,
        ipp_core::ErrorReason::InvalidValue
    );
    assert!(host.attached_publication(&retained).is_none());
    assert_eq!(
        host.attachment_retirement(&retained.token),
        Ok(WorldAttachmentRetirement::Pending)
    );
    apply(&mut host, parent, vec![camera_far(owner, 100.0)]);
    assert_eq!(
        host.bind_output(owner.world(), owner.camera_entity().unwrap(), owner.kind()),
        Ok(owner)
    );
    let recovered = edge(&host, parent);
    assert_eq!(recovered.token, retained.token);
    assert!(host.attached_publication(recovered).is_some());
    assert_eq!(
        host.attachment_retirement(&retained.token),
        Ok(WorldAttachmentRetirement::Pending)
    );
}

#[test]
fn surface_child_selection_and_physical_extent_remain_separate_from_placement_owner() {
    let (mut host, inputs, parent, child, anchor, owner) = fixture();
    let child_output = camera(&mut host, child);
    apply(
        &mut host,
        parent,
        vec![
            Command::InsertComponent {
                entity: EntityRef::Handle(anchor),
                component: ComponentValue::FLAT_SURFACE,
                fields: vec![
                    ipp_core::FieldWrite {
                        offset: std::mem::offset_of!(ipp_core::FlatSurface, width) as u32,
                        value: ipp_core::FieldValue::F32(3.0),
                    },
                    ipp_core::FieldWrite {
                        offset: std::mem::offset_of!(ipp_core::FlatSurface, height) as u32,
                        value: ipp_core::FieldValue::F32(2.0),
                    },
                ],
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::WorldAttachment(WorldAttachment::surface(child_output)),
            ),
        ],
    );
    let mut placement = affine(9.0);
    placement[0] = 2.0;
    placement[5] = 3.0;
    inputs.lock().unwrap().claim = AttachmentPlacement::Ready {
        owner,
        affine: placement,
    };
    host.frame(0.0).unwrap();
    let published = edge(&host, parent);
    assert_eq!(published.placement_output, Some(owner));
    assert_eq!(published.output, Some(child_output));
    assert_eq!(published.surface_extent, Some([3.0, 2.0]));
    assert_eq!(published.placement, placement);
    let context = &inputs.lock().unwrap().contexts[&child];
    assert_eq!(context.selected_output, Some(child_output));
    assert_eq!(context.surface_extent, Some([3.0, 2.0]));
    assert_eq!(context.placement, placement);
}

#[test]
fn ready_placement_is_joined_before_unavailable_joint_hierarchy() {
    let (mut host, inputs, parent, _, anchor, owner) = fixture();
    let object = create(&mut host, parent, Vec::new());
    apply(
        &mut host,
        parent,
        vec![
            Command::PlaceEntity {
                entity: EntityRef::Handle(anchor),
                placement: ipp_core::EntityPlacementRef {
                    parent: Some(EntityRef::Handle(object)),
                    before: None,
                },
            },
            Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::ParentJoint(ipp_core::components::ParentJoint {
                    ordinal: 0,
                }),
            ),
        ],
    );
    host.frame(0.0).unwrap();
    assert!(
        host.world_mut(parent)
            .unwrap()
            .world_matrix(anchor)
            .is_err()
    );
    assert!(
        host.publication(host.latest_publication(parent).unwrap())
            .unwrap()
            .attachments
            .is_empty()
    );
    inputs.lock().unwrap().claim = AttachmentPlacement::Ready {
        owner,
        affine: affine(7.0),
    };
    host.frame(0.0).unwrap();
    assert!(
        host.world_mut(parent)
            .unwrap()
            .world_matrix(anchor)
            .is_err()
    );
    assert_eq!(edge(&host, parent).placement, affine(7.0));
}
