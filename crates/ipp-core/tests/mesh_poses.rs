//! Pose compatibility, ordered mutation, immutable sharing and headless bounds.
#![cfg(feature = "mesh-poses")]

mod support;
use support::{HostWorldTestDriver, WorldTestDriver};

use ipp_core::systems::geometry::GeometryBounds;
use ipp_core::{
    components::{MeshInstance, MeshPose, Transform},
    *,
};

fn payload(shift: f32, reversed: bool) -> Vec<u8> {
    let mut bytes = b"IPPM".to_vec();
    for value in [3u32, 3, 3, 1] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0, 1, 0, 0]);
    bytes.extend(36u32.to_le_bytes());
    for point in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for value in [point[0] + shift, point[1], point[2]] {
            bytes.extend(value.to_le_bytes());
        }
    }
    for index in if reversed {
        [0u16, 2, 1]
    } else {
        [0, 1, 2]
    } {
        bytes.extend(index.to_le_bytes());
    }
    bytes
}

fn upload(world: &mut WorldContext<'_>, asset: u64, shift: f32, reversed: bool) {
    world
        .enqueue_mesh(MeshUpload {
            id: asset,
            key: MeshKey {
                asset,
                variant: 0,
            },
            bytes: payload(shift, reversed),
        })
        .unwrap();
    let report = world.await_upload_for_test();
    assert!(
        report.assets[0].result.is_ok(),
        "upload {asset}: {report:?}"
    );
}

fn field(offset: usize, value: FieldValue) -> FieldWrite {
    FieldWrite {
        offset: offset as u32,
        value,
    }
}

fn apply(world: &mut WorldContext<'_>, operations: Vec<Command>) -> WorldUpdateReport {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap()
}

fn entity(world: &mut WorldContext<'_>, weight: f32, target: &str) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
    }];
    for (component, fields) in [
        (ComponentValue::TRANSFORM, vec![]),
        (ComponentValue::UNLIT_MATERIAL, vec![]),
        (ComponentValue::BOUNDING_GEOMETRY, vec![]),
        (
            ComponentValue::MESH_INSTANCE,
            vec![field(
                std::mem::offset_of!(MeshInstance, source),
                FieldValue::String("asset://1/1".into()),
            )],
        ),
        (
            ComponentValue::MESH_POSE,
            vec![
                field(
                    std::mem::offset_of!(MeshPose, source),
                    FieldValue::String(target.into()),
                ),
                field(
                    std::mem::offset_of!(MeshPose, weight),
                    FieldValue::F32(weight),
                ),
            ],
        ),
    ] {
        operations.push(Command::InsertComponent {
            entity: EntityRef::Alias(0),
            component,
            fields,
        });
    }
    apply(world, operations).outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1
}

#[test]
fn shared_endpoints_have_independent_weights_and_conservative_interpolated_bounds() {
    let mut host = HostRuntime::new();
    let world_id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    upload(&mut world, 1, 0.0, false);
    upload(&mut world, 2, 8.0, false);
    let pointer = world
        .mesh(MeshKey {
            asset: 1,
            variant: 0,
        })
        .unwrap()
        .positions()
        .as_ptr();
    for weight in [0.0, 0.5, 1.0] {
        let id = entity(&mut world, weight, "asset://1/2");
        assert_eq!(world.mesh_pose(id).unwrap().unwrap().1, weight);
        let bounds = world.bounding_geometry(id).unwrap().bounds().unwrap();
        assert_eq!(
            bounds,
            [
                [f64::from(weight) * 8.0, 0.0, 0.0],
                [f64::from(weight) * 8.0 + 1.0, 1.0, 0.0]
            ]
        );
    }
    assert_eq!(
        world
            .render_items()
            .iter()
            .map(|item| item.pose.unwrap().1)
            .collect::<Vec<_>>(),
        [0.0, 0.5, 1.0]
    );
    assert_eq!(
        world
            .mesh(MeshKey {
                asset: 1,
                variant: 0
            })
            .unwrap()
            .positions()
            .as_ptr(),
        pointer
    );
}

#[test]
fn invalid_changes_preserve_prior_operations_and_allow_explicit_repair() {
    let mut host = HostRuntime::new();
    let world_id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    upload(&mut world, 1, 0.0, false);
    upload(&mut world, 2, 8.0, false);
    upload(&mut world, 3, 8.0, true);
    let id = entity(&mut world, 0.5, "asset://1/2");
    for value in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
        let report = apply(
            &mut world,
            vec![
                Command::SetField {
                    entity: EntityRef::Handle(id),
                    component: ComponentValue::TRANSFORM,
                    field: field(std::mem::offset_of!(Transform, x), FieldValue::F32(9.0)),
                },
                Command::SetField {
                    entity: EntityRef::Handle(id),
                    component: ComponentValue::MESH_POSE,
                    field: field(
                        std::mem::offset_of!(MeshPose, weight),
                        FieldValue::F32(value),
                    ),
                },
                Command::Delete {
                    entity: EntityRef::Handle(id),
                },
            ],
        );
        assert_eq!(
            report.outcomes[0].result.as_ref().unwrap_err().reason,
            ErrorReason::InvalidValue
        );
        // Failure stops the batch without rolling back the earlier transform.
        assert_eq!(
            world.entities()[0]
                .effective
                .iter()
                .find_map(|value| match value {
                    ComponentValue::Transform(value) => Some(value.x),
                    _ => None,
                }),
            Some(9.0)
        );
        let repair = apply(
            &mut world,
            vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(id),
                value: ComponentValue::MeshPose(MeshPose {
                    source: "asset://1/2".into(),
                    variant: 0,
                    weight: 0.5,
                }),
            }],
        );
        assert!(
            repair.outcomes[0].result.is_ok(),
            "value={value}: {repair:?}"
        );
        assert_eq!(world.mesh_pose(id).unwrap().unwrap().1, 0.5);
    }
    for component in [ComponentValue::MESH_POSE, ComponentValue::MESH_INSTANCE] {
        let report = apply(
            &mut world,
            vec![Command::SetField {
                entity: EntityRef::Handle(id),
                component,
                field: field(0, FieldValue::String("asset://1/3".into())),
            }],
        );
        assert_eq!(
            report.outcomes[0].result.as_ref().unwrap_err().reason,
            ErrorReason::InvalidAsset
        );
        assert_eq!(world.mesh_pose(id), Err(ErrorReason::InvalidAsset));
        assert!(world.render_items().is_empty());
        assert_eq!(
            world.render_diagnostics(),
            [RenderDiagnostic {
                entity: id,
                reason: ErrorReason::InvalidAsset
            }]
        );
        let source = if component == ComponentValue::MESH_POSE {
            "asset://1/2"
        } else {
            "asset://1/1"
        };
        assert!(
            apply(
                &mut world,
                vec![Command::SetField {
                    entity: EntityRef::Handle(id),
                    component,
                    field: field(0, FieldValue::String(source.into())),
                }]
            )
            .outcomes[0]
                .result
                .is_ok()
        );
        assert_eq!(world.render_items().len(), 1);
    }
}

#[test]
fn pending_target_recovers_without_resubmission_and_late_incompatibility_is_per_use() {
    let mut host = HostRuntime::new();
    let world_id = host.create_world(Default::default()).unwrap();
    let upload = |host: &mut HostRuntime, asset, shift, reversed| {
        host.world_mut(world_id)
            .unwrap()
            .enqueue_mesh(MeshUpload {
                id: asset,
                key: MeshKey {
                    asset,
                    variant: 0,
                },
                bytes: payload(shift, reversed),
            })
            .unwrap();
        let report = host.await_world_upload_for_test(world_id);
        assert!(
            report.assets[0].result.is_ok(),
            "upload {asset}: {report:?}"
        );
    };
    upload(&mut host, 1, 0.0, false);
    let (a, b) = {
        let mut world = host.world_mut(world_id).unwrap();
        let a = entity(&mut world, 0.5, "asset://1/2");
        let b = entity(&mut world, 0.5, "asset://1/3");
        assert!(world.render_items().is_empty());
        (a, b)
    };
    upload(&mut host, 2, 8.0, false);
    upload(&mut host, 3, 8.0, true);
    let mut world = host.world_mut(world_id).unwrap();
    assert_eq!(world.render_items().len(), 1);
    assert_eq!(world.render_items()[0].entity, a);
    assert_eq!(
        world.render_diagnostics(),
        [RenderDiagnostic {
            entity: b,
            reason: ErrorReason::InvalidAsset
        }]
    );
    assert!(
        world
            .mesh(MeshKey {
                asset: 3,
                variant: 0
            })
            .is_some()
    );
    let deleted = apply(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(a),
        }],
    );
    assert!(deleted.outcomes[0].result.is_ok());
    assert_eq!(world.mesh_pose(a), Err(ErrorReason::InvalidEntity));
}

#[test]
fn pose_sources_remain_scoped_to_their_world_after_peer_teardown() {
    let mut host = HostRuntime::new();
    let first = host.create_world(Default::default()).unwrap();
    let second = host.create_world(Default::default()).unwrap();
    let mut targets = Vec::new();
    for (world_id, shift) in [(first, 8.0), (second, -4.0)] {
        let mut world = host.world_mut(world_id).unwrap();
        upload(&mut world, 1, 0.0, false);
        upload(&mut world, 2, shift, false);
        let entity = entity(&mut world, 0.5, "asset://1/2");
        let target = world.mesh_pose(entity).unwrap().unwrap().0;
        let bounds = world.bounding_geometry(entity).unwrap().bounds().unwrap();
        assert_eq!(bounds[0][0], f64::from(shift) * 0.5);
        targets.push((entity, target));
    }
    assert_ne!(targets[0].1, targets[1].1);

    assert!(host.destroy_world(first));
    let mut world = host.world_mut(second).unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(world.mesh_pose(targets[1].0), Ok(Some((targets[1].1, 0.5))));
    assert_eq!(world.render_items().len(), 1);
    assert_eq!(
        world
            .bounding_geometry(targets[1].0)
            .unwrap()
            .bounds()
            .unwrap()[0][0],
        -2.0
    );
}
