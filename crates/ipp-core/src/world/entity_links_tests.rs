use super::*;
use crate::components::Transform;
use crate::{Batch, Command, ComponentValue, EntityMetadata, HostRuntime};

/// Transforms and final World-space propagation.
const SPATIAL_SYSTEMS: &[crate::systems::SystemId] = &[
    crate::systems::hierarchy::HierarchySystem::ID,
    crate::systems::look_at::LookAtSystem::ID,
    crate::systems::hierarchy::FinalPropagationSystem::ID,
];

/// Skeleton joints under propagated transforms.
#[cfg(all(feature = "skeletal-animation", feature = "builtin-assets"))]
const SKELETON_AND_SPATIAL_SYSTEMS: &[crate::systems::SystemId] = &[
    crate::systems::animation::AnimationSystem::ID,
    crate::systems::asset_dependencies::AssetDependencySystem::ID,
    crate::systems::skeleton::SkeletonSystem::ID,
    crate::systems::hierarchy::HierarchySystem::ID,
    crate::systems::look_at::LookAtSystem::ID,
    crate::systems::hierarchy::FinalPropagationSystem::ID,
];

fn insert(store: &mut EntityLinkStore, ordinal: u32) -> EntityId {
    let entity = EntityId::new(ordinal, 1);
    store.insert(entity, EntityPersistentId(u64::from(ordinal) + 1));
    entity
}

fn place(
    store: &mut EntityLinkStore,
    entity: EntityId,
    parent: Option<EntityId>,
    before: Option<EntityId>,
) {
    let value = store.resolve(entity, parent, before).unwrap();
    store.set(entity, value).unwrap();
}

fn assert_indexes(store: &EntityLinkStore) {
    let mut expected: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for (&entity, record) in &store.records {
        if record.live {
            expected.entry(record.link.parent).or_default().insert((
                record.link.order,
                record.persistent_id,
                entity,
            ));
        }
    }
    assert_eq!(store.children, expected);
}

#[test]
fn next_sibling_uses_current_order_and_parent() {
    let mut store = EntityLinkStore::default();
    let parent = insert(&mut store, 0);
    let other = insert(&mut store, 1);
    let first = insert(&mut store, 2);
    let second = insert(&mut store, 3);
    let third = insert(&mut store, 4);
    for entity in [first, second, third] {
        place(&mut store, entity, Some(parent), None);
    }
    assert_eq!(store.next_sibling(first), Some(second));
    assert_eq!(store.next_sibling(second), Some(third));
    assert_eq!(store.next_sibling(third), None);

    place(&mut store, second, Some(other), None);
    assert_eq!(store.next_sibling(first), Some(third));
    assert_eq!(store.next_sibling(second), None);
    place(&mut store, second, Some(parent), Some(third));
    assert_eq!(store.next_sibling(first), Some(second));

    place(&mut store, third, Some(parent), Some(first));
    assert_eq!(store.next_sibling(third), Some(first));
    assert_eq!(store.next_sibling(first), Some(second));
    assert_eq!(store.next_sibling(second), None);
    assert_indexes(&store);
}

#[test]
fn next_sibling_uses_full_order_key_for_equal_order_roots() {
    let mut store = EntityLinkStore::default();
    let parents: Vec<_> = (0..3).map(|ordinal| insert(&mut store, ordinal)).collect();
    let siblings: Vec<_> = (3..6).map(|ordinal| insert(&mut store, ordinal)).collect();
    for (&parent, &entity) in parents.iter().zip(&siblings) {
        place(&mut store, entity, Some(parent), None);
    }
    store.restore_identity(siblings[0], EntityPersistentId(99));
    store.restore_identity(siblings[1], EntityPersistentId(99));
    store.restore_identity(siblings[2], EntityPersistentId(98));
    for parent in parents {
        store.retire(parent);
        store.release_retired(parent);
    }
    assert_eq!(
        store.effective(siblings[0]).unwrap().order,
        store.effective(siblings[1]).unwrap().order
    );
    assert_eq!(
        store.effective(siblings[1]).unwrap().order,
        store.effective(siblings[2]).unwrap().order
    );
    assert_eq!(store.next_sibling(siblings[2]), Some(siblings[0]));
    assert_eq!(store.next_sibling(siblings[0]), Some(siblings[1]));
    assert_eq!(store.next_sibling(siblings[1]), None);
    assert_indexes(&store);
}

#[test]
fn next_sibling_rejects_retired_and_reused_generations() {
    let mut store = EntityLinkStore::default();
    let first = insert(&mut store, 0);
    let second = insert(&mut store, 1);
    assert_eq!(store.next_sibling(first), Some(second));
    assert_eq!(store.next_sibling(EntityId::new(99, 1)), None);
    store.retire(first);
    assert!(store.records.contains_key(&first));
    assert_eq!(store.next_sibling(first), None);
    store.release_retired(first);
    let replacement = EntityId::new(first.index(), first.generation() + 1);
    store.insert(replacement, EntityPersistentId(100));
    place(&mut store, replacement, None, Some(second));
    assert_eq!(store.next_sibling(replacement), Some(second));
    assert_eq!(store.next_sibling(first), None);
    assert_eq!(store.next_sibling(second), None);
    assert_indexes(&store);
}

#[test]
fn next_sibling_wide_ranges_visit_only_returned_candidates() {
    for width in [128, 4096, 32_768] {
        for nested in [false, true] {
            let mut store = EntityLinkStore::default();
            let parent = nested.then(|| insert(&mut store, 0));
            let siblings: Vec<_> = (1..=width)
                .map(|ordinal| insert(&mut store, ordinal))
                .collect();
            if let Some(parent) = parent {
                for &entity in &siblings {
                    place(&mut store, entity, Some(parent), None);
                }
            }
            VISITS.set(0);
            assert_eq!(
                store.next_sibling(siblings[siblings.len() - 2]),
                siblings.last().copied()
            );
            assert_eq!(VISITS.get(), 1);
            assert_eq!(store.next_sibling(*siblings.last().unwrap()), None);
            assert_eq!(VISITS.get(), 1);

            VISITS.set(0);
            for pair in siblings.windows(2) {
                assert_eq!(store.next_sibling(pair[0]), Some(pair[1]));
            }
            assert_eq!(VISITS.get(), siblings.len() - 1);
            assert_indexes(&store);
        }
    }
}

#[test]
fn large_append_keeps_fixed_width_orders_and_uses_the_derived_tail() {
    let mut store = EntityLinkStore::default();
    let parent = insert(&mut store, 0);
    for ordinal in 1..=30_000 {
        let entity = insert(&mut store, ordinal);
        place(&mut store, entity, Some(parent), None);
    }
    for ordinal in 30_001..=60_000 {
        insert(&mut store, ordinal);
    }
    assert_eq!(store.children(Some(parent)).count(), 30_000);
    assert_eq!(store.children(None).count(), 30_001);
    assert_eq!(std::mem::size_of::<EntityOrder>(), 16);
    assert_eq!(store.relabels, 0);
    assert_indexes(&store);
}

#[test]
fn repeated_gap_insertions_relabel_without_growing_keys_or_losing_exact_order() {
    let mut store = EntityLinkStore::default();
    let parent = insert(&mut store, 0);
    let anchor = insert(&mut store, 1);
    place(&mut store, anchor, Some(parent), None);
    let mut expected = Vec::new();
    for ordinal in 2..=4097 {
        let entity = insert(&mut store, ordinal);
        place(&mut store, entity, Some(parent), Some(anchor));
        expected.push(entity);
    }
    expected.push(anchor);
    assert_eq!(store.children(Some(parent)).collect::<Vec<_>>(), expected);
    assert!(store.relabels > 0 && store.relabels < 128);
    assert!(
        store.relabeled_values < 4096 * 64,
        "gap edits touched {} labels",
        store.relabeled_values
    );
    assert_eq!(std::mem::size_of::<EntityOrder>(), 16);
    assert_indexes(&store);
}

#[test]
fn precise_before_sibling_survives_equal_orders_from_deleted_parent_lists() {
    let mut store = EntityLinkStore::default();
    let left_parent = insert(&mut store, 0);
    let right_parent = insert(&mut store, 1);
    let left = insert(&mut store, 2);
    let right = insert(&mut store, 3);
    place(&mut store, left, Some(left_parent), None);
    place(&mut store, right, Some(right_parent), None);
    assert_eq!(
        store.effective(left).unwrap().order,
        store.effective(right).unwrap().order
    );
    store.retire(left_parent);
    store.retire(right_parent);
    let middle = insert(&mut store, 4);
    place(&mut store, middle, None, Some(right));
    assert_eq!(
        store.children(None).collect::<Vec<_>>(),
        vec![left, middle, right]
    );
    assert_eq!(store.relabels, 1);
    assert_indexes(&store);
}

fn apply(
    world: &mut super::super::WorldContext<'_>,
    operations: Vec<Command>,
) -> crate::BatchOutcome {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    world.step(0.0).unwrap().outcomes.remove(0)
}

fn create(world: &mut super::super::WorldContext<'_>, names: &[&str]) -> Vec<EntityId> {
    apply(
        world,
        names
            .iter()
            .enumerate()
            .map(|(alias, name)| Command::Create {
                alias: alias as u32,
                metadata: EntityMetadata {
                    symbolic_id: Some((*name).into()),
                    classes: Vec::new(),
                },
                adopt: false,
            })
            .collect(),
    )
    .result
    .unwrap()
    .into_iter()
    .map(|(_, entity)| entity)
    .collect()
}

fn placement(parent: Option<EntityId>, before: Option<EntityId>) -> EntityPlacementRef {
    EntityPlacementRef {
        parent: parent.map(EntityRef::Handle),
        before: before.map(EntityRef::Handle),
    }
}

#[test]
fn world_next_sibling_follows_root_and_child_reordering() {
    let mut host = HostRuntime::default();
    let id = host.create_world(Default::default(), &[]).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let entities = create(&mut world, &["parent", "other", "first", "second"]);
    let [parent, other, first, second] = entities[..] else {
        unreachable!()
    };
    assert_eq!(world.entity_next_sibling(parent), Some(other));
    assert_eq!(world.entity_next_sibling(other), Some(first));
    assert_eq!(world.entity_next_sibling(second), None);
    apply(
        &mut world,
        vec![
            Command::PlaceEntity {
                entity: EntityRef::Handle(first),
                placement: placement(Some(parent), None),
            },
            Command::PlaceEntity {
                entity: EntityRef::Handle(second),
                placement: placement(Some(parent), Some(first)),
            },
            Command::PlaceEntity {
                entity: EntityRef::Handle(other),
                placement: placement(None, Some(parent)),
            },
        ],
    )
    .result
    .unwrap();
    assert_eq!(world.entity_next_sibling(other), Some(parent));
    assert_eq!(world.entity_next_sibling(parent), None);
    assert_eq!(world.entity_next_sibling(second), Some(first));
    assert_eq!(world.entity_next_sibling(first), None);
    assert_eq!(world.entity_next_sibling(EntityId::new(99, 1)), None);
}

#[test]
fn invalid_branch_recovers_and_final_transform_storage_survives_growth() {
    let mut host = HostRuntime::default();
    let id = host
        .create_world(Default::default(), SPATIAL_SYSTEMS)
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let entities = create(&mut world, &["parent", "child", "unrelated"]);
    let [parent, child, unrelated] = entities[..] else {
        unreachable!()
    };
    apply(
        &mut world,
        vec![
            Command::insert_value(
                EntityRef::Handle(parent),
                ComponentValue::Transform(Transform {
                    x: 3.0,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(child),
                ComponentValue::Transform(Transform {
                    y: 4.0,
                    ..Default::default()
                }),
            ),
            Command::PlaceEntity {
                entity: EntityRef::Handle(child),
                placement: placement(Some(parent), None),
            },
        ],
    )
    .result
    .unwrap();
    let address = world.world.state.links.transform(child).unwrap() as *const _;
    apply(
        &mut world,
        (0..2000)
            .map(|alias| Command::Create {
                alias,
                metadata: Default::default(),
                adopt: false,
            })
            .collect(),
    )
    .result
    .unwrap();
    assert_eq!(
        world.world.state.links.transform(child).unwrap() as *const _,
        address
    );
    assert_eq!(
        &world.world_matrix(child).unwrap()[12..15],
        &[3.0, 4.0, 0.0]
    );
    assert!(
        apply(
            &mut world,
            vec![Command::PlaceEntity {
                entity: EntityRef::Handle(parent),
                placement: placement(Some(child), None)
            }]
        )
        .result
        .is_err()
    );
    assert!(world.world_matrix(child).is_err());
    assert!(world.world_matrix(unrelated).is_ok());
    apply(
        &mut world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(parent),
            placement: placement(None, None),
        }],
    )
    .result
    .unwrap();
    assert_eq!(
        &world.world_matrix(child).unwrap()[12..15],
        &[3.0, 4.0, 0.0]
    );
    apply(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(parent),
        }],
    )
    .result
    .unwrap();
    assert_eq!(
        &world.world_matrix(child).unwrap()[12..15],
        &[0.0, 4.0, 0.0]
    );
}

#[test]
fn snapshot_preserves_links_and_durable_tie_order() {
    let mut host = HostRuntime::default();
    let original = host.create_world(Default::default(), &[]).unwrap();
    let snapshot = {
        let mut world = host.world_mut(original).unwrap();
        let entities = create(&mut world, &["parent", "first", "second"]);
        for &entity in &entities[1..] {
            apply(
                &mut world,
                vec![Command::PlaceEntity {
                    entity: EntityRef::Handle(entity),
                    placement: placement(Some(entities[0]), None),
                }],
            )
            .result
            .unwrap();
        }
        world.capture_world(Default::default()).unwrap()
    };
    assert_eq!(
        snapshot.entities[1].link.parent,
        Some(snapshot.entities[0].persistent_id)
    );
    let bytes = host.save_world(original, 44, Default::default()).unwrap();
    let restored = host
        .load_world(
            &bytes,
            44,
            crate::services::world_serialization::WorldLoadOptions {
                symbolic_id: Some("copy".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        )
        .unwrap()
        .root
        .id();
    let mut world = host.world_mut(restored).unwrap();
    world.step(0.0).unwrap();
    let captured = world.capture_world(Default::default()).unwrap();
    assert_eq!(captured.entities, snapshot.entities);
    let parent = world
        .entities()
        .into_iter()
        .find(|entity| entity.metadata.symbolic_id.as_deref() == Some("parent"))
        .unwrap()
        .id;
    assert_eq!(world.entity_children(Some(parent)).count(), 2);
}

#[test]
fn snapshot_rejects_missing_parents_and_cycles_before_restore() {
    let mut host = HostRuntime::default();
    let original = host.create_world(Default::default(), &[]).unwrap();
    let snapshot = {
        let mut world = host.world_mut(original).unwrap();
        create(&mut world, &["first", "second"]);
        world.capture_world(Default::default()).unwrap()
    };
    let mut snapshot = crate::services::world_serialization::WorldGraphSnapshot {
        root: crate::services::world_serialization::WorldGraphNodeId(0),
        nodes: vec![crate::services::world_serialization::WorldGraphNode {
            id: crate::services::world_serialization::WorldGraphNodeId(0),
            world: snapshot,
            references: Vec::new(),
        }],
    };
    snapshot.nodes[0].world.entities[0].link.parent = Some(EntityPersistentId(999));
    assert!(
        snapshot
            .encode(44, Default::default())
            .unwrap_err()
            .contains("parent")
    );
    snapshot.nodes[0].world.entities[0].link.parent =
        Some(snapshot.nodes[0].world.entities[1].persistent_id);
    snapshot.nodes[0].world.entities[1].link.parent =
        Some(snapshot.nodes[0].world.entities[0].persistent_id);
    assert!(
        snapshot
            .encode(44, Default::default())
            .unwrap_err()
            .contains("Cyclic")
    );
    assert_eq!(host.world_mut(original).unwrap().entities().len(), 2);
}

#[cfg(all(feature = "skeletal-animation", feature = "builtin-assets"))]
#[test]
fn joint_selection_and_look_at_share_the_effective_parent_frame() {
    use crate::components::{LookAt, ParentJoint, Skeleton};
    use crate::services::asset_management::{AssetUpload, AssetUploadIdentity, builtin};

    let mut host = HostRuntime::default();
    let id = host
        .create_world(Default::default(), SKELETON_AND_SPATIAL_SYSTEMS)
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let entities = create(&mut world, &["rig", "child", "tip", "target"]);
    let [rig, child, tip, target] = entities[..] else {
        unreachable!()
    };
    apply(
        &mut world,
        vec![
            Command::insert_value(
                EntityRef::Handle(rig),
                ComponentValue::Transform(Transform {
                    x: 3.0,
                    sx: 2.0,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(child),
                ComponentValue::Transform(Transform {
                    y: 0.5,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(tip),
                ComponentValue::Transform(Transform {
                    y: 0.25,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(target),
                ComponentValue::Transform(Transform {
                    x: 5.0,
                    y: 2.0,
                    z: -3.0,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(rig),
                ComponentValue::Skeleton(Skeleton {
                    source: "asset://3/971".into(),
                    ..Default::default()
                }),
            ),
            Command::PlaceEntity {
                entity: EntityRef::Handle(child),
                placement: placement(Some(rig), None),
            },
            Command::insert_value(
                EntityRef::Handle(child),
                ComponentValue::ParentJoint(ParentJoint {
                    ordinal: 1,
                }),
            ),
            Command::PlaceEntity {
                entity: EntityRef::Handle(tip),
                placement: placement(Some(child), None),
            },
        ],
    )
    .result
    .unwrap();
    assert!(world.world_matrix(child).is_err());
    assert!(world.world_matrix(tip).is_err());
    world
        .enqueue_asset(AssetUpload {
            id: 971,
            key: AssetUploadIdentity {
                kind: crate::SKELETON_TYPE,
                asset: 971,
                variant: 0,
            },
            bytes: builtin::rig(crate::SKELETON_TYPE, "ipp://skeleton/rig-strip").unwrap(),
        })
        .unwrap();
    for _ in 0..64 {
        world.prepare_update(0.0).unwrap();
        world.poll_assets();
        world.step(0.0).unwrap();
        if world.world_matrix(child).is_ok() {
            break;
        }
    }
    assert_eq!(
        &world.world_matrix(child).unwrap()[12..15],
        &[3.0, 1.5, 0.0]
    );
    assert_eq!(&world.world_matrix(tip).unwrap()[12..15], &[3.0, 1.75, 0.0]);
    apply(
        &mut world,
        vec![Command::insert_value(
            EntityRef::Handle(child),
            ComponentValue::ParentJoint(ParentJoint {
                ordinal: 30,
            }),
        )],
    )
    .result
    .unwrap();
    assert!(world.world_matrix(child).is_err());
    assert!(world.world_matrix(tip).is_err());
    apply(
        &mut world,
        vec![
            Command::insert_value(
                EntityRef::Handle(child),
                ComponentValue::ParentJoint(ParentJoint {
                    ordinal: 1,
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(child),
                ComponentValue::LookAt(LookAt {
                    target,
                    ..Default::default()
                }),
            ),
        ],
    )
    .result
    .unwrap();
    let matrix = world.world_matrix(child).unwrap();
    let direction = [5.0 - matrix[12], 2.0 - matrix[13], -3.0 - matrix[14]];
    let forward = [-matrix[8], -matrix[9], -matrix[10]];
    let length = |vector: [f32; 3]| vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    for (actual, expected) in forward
        .map(|value| value / length(forward))
        .into_iter()
        .zip(direction.map(|value| value / length(direction)))
    {
        assert!((actual - expected).abs() < 2e-5);
    }
    apply(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(rig),
        }],
    )
    .result
    .unwrap();
    assert_eq!(
        &world.world_matrix(child).unwrap()[12..15],
        &[0.0, 0.5, 0.0]
    );
    assert!(world.world_matrix(tip).is_ok());
}
