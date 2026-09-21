//! Rig decoding, instance lifetime, palette math and partial update failure.
#![cfg(all(feature = "skeletal-animation", feature = "builtin-assets"))]

mod support;
use support::WorldTestDriver;

use ipp_core::{
    components::{MeshInstance, Skeleton, Skin, Transform},
    services::asset_management::{self as assets, AssetUpload, AssetUploadIdentity, builtin},
    systems::camera,
    *,
};

use ipp_core::systems::geometry;

fn field(offset: usize, value: FieldValue) -> FieldWrite {
    FieldWrite {
        offset: offset as u32,
        value,
    }
}

fn create(alias: u32) -> Command {
    Command::Create {
        alias,
        metadata: Default::default(),
    }
}

fn insert(alias: u32, component: u16, fields: Vec<FieldWrite>) -> Command {
    Command::InsertComponent {
        entity: EntityRef::Alias(alias),
        component,
        fields,
    }
}

fn apply(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) -> WorldUpdateReport {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap()
}

fn upload(
    world: &mut ipp_core::WorldContext<'_>,
    kind: assets::AssetTypeId,
    asset: u64,
    bytes: Vec<u8>,
) {
    world
        .enqueue_asset(AssetUpload {
            id: asset,
            key: AssetUploadIdentity {
                kind,
                asset,
                variant: 0,
            },
            bytes,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap();
}

fn fixture(host: &mut ipp_core::HostRuntime) -> (ipp_core::WorldContext<'_>, EntityId, EntityId) {
    let world_id = host.create_world(ipp_core::WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    for (kind, asset, uri) in [
        (SKELETON_TYPE, 1, "ipp://skeleton/rig-strip"),
        (SKIN_TYPE, 2, "ipp://skin/rig-strip"),
        (MESH_TYPE, 3, "ipp://mesh/rig-strip"),
        (POSE_TYPE, 4, "ipp://pose/rig-strip-bent"),
    ] {
        upload(&mut world, kind, asset, builtin::rig(kind, uri).unwrap());
    }
    let mut ops = vec![];
    for alias in [0, 1] {
        ops.extend([
            create(alias),
            insert(alias, ComponentValue::TRANSFORM, vec![]),
            insert(
                alias,
                ComponentValue::SKELETON,
                vec![field(
                    std::mem::offset_of!(Skeleton, source),
                    FieldValue::String("asset://3/1".into()),
                )],
            ),
            insert(
                alias,
                ComponentValue::SKIN,
                vec![
                    field(
                        std::mem::offset_of!(Skin, skeleton),
                        FieldValue::Entity(EntityRef::Alias(alias)),
                    ),
                    field(
                        std::mem::offset_of!(Skin, source),
                        FieldValue::String("asset://5/2".into()),
                    ),
                ],
            ),
            insert(
                alias,
                ComponentValue::MESH_INSTANCE,
                vec![field(
                    std::mem::offset_of!(MeshInstance, source),
                    FieldValue::String("asset://1/3".into()),
                )],
            ),
            insert(alias, ComponentValue::UNLIT_MATERIAL, vec![]),
        ]);
    }
    let report = apply(&mut world, ops);
    let aliases = report.outcomes[0].result.as_ref().unwrap();
    let (a, b) = (aliases[0].1, aliases[1].1);
    world.update_for_test(0.0).unwrap();
    (world, a, b)
}

fn set(entity: EntityId, component: u16, offset: usize, value: FieldValue) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component,
        field: field(offset, value),
    }
}

#[test]
fn rest_inverse_binds_mapping_and_independent_poses_have_stable_buffers() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, a, b) = fixture(&mut fixture_host);
    let identity = camera::model_matrix(&Transform::default()).unwrap();
    assert_eq!(world.skin_palette(a).unwrap(), &[identity, identity]);
    let address = world.skeleton_pose(a).unwrap().as_ptr();
    let palette_address = world.skin_palette(a).unwrap().as_ptr();
    let report = apply(
        &mut world,
        vec![set(
            a,
            ComponentValue::SKELETON,
            std::mem::offset_of!(Skeleton, pose_source),
            FieldValue::String("asset://4/4".into()),
        )],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert_eq!(world.skeleton_pose(a).unwrap().as_ptr(), address);
    let palette = world.skin_palette(a).unwrap();
    assert_eq!(palette.as_ptr(), palette_address);
    // Upper joint rotates around (0,1), palette entry 0 maps to joint 1.
    let tip = [0.0, 2.0, 0.0, 1.0];
    let x: f32 = (0..4).map(|k| palette[0][k * 4] * tip[k]).sum();
    let y: f32 = (0..4).map(|k| palette[0][k * 4 + 1] * tip[k]).sum();
    assert!((x + 1.0).abs() < 1e-5 && (y - 1.0).abs() < 1e-5);
    assert_eq!(world.skin_palette(b).unwrap(), &[identity, identity]);
    assert_eq!(world.skin_palette(a).unwrap()[1], identity);

    // Runtime evaluation never replaces retained authored rest/source state.
    world.update_for_test(0.0).unwrap();
    assert_eq!(world.skeleton_pose(a).unwrap().as_ptr(), address);
    let report = apply(
        &mut world,
        vec![set(
            a,
            ComponentValue::SKELETON,
            std::mem::offset_of!(Skeleton, pose_source),
            FieldValue::String(String::new()),
        )],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert_eq!(world.skin_palette(a).unwrap(), &[identity, identity]);
    assert_eq!(world.skin_palette(a).unwrap().as_ptr(), palette_address);
}

#[test]
fn repeated_numeric_commits_hide_palettes_until_each_skinning_pass() {
    use ipp_core::components::CustomMaterial;
    use ipp_core::systems::animation::AnimationSystem;

    let mut host = ipp_core::HostRuntime::new();
    let (mut world, a, b) = fixture(&mut host);
    let report = apply(
        &mut world,
        vec![Command::InsertComponent {
            entity: EntityRef::Handle(a),
            component: ComponentValue::CUSTOM_MATERIAL,
            fields: vec![],
        }],
    );
    assert!(report.outcomes[0].result.is_ok());
    let address = world.skin_palette(a).unwrap().as_ptr();
    for pass in 0..2 {
        assert!(world.skin_palette(a).is_some());
        assert!(world.skin_palette(b).is_some());
        for step in 0..3 {
            world
                .with_system::<AnimationSystem, _>(AnimationSystem::ID, |system, access| {
                    access.apply_evaluated_properties(
                        system,
                        a,
                        ComponentValue::CUSTOM_MATERIAL,
                        [(
                            std::mem::offset_of!(CustomMaterial, alpha_cutoff) as u32,
                            ipp_core::components::schema::FieldValue::F32(
                                (pass * 3 + step) as f32 / 10.0,
                            ),
                        )],
                    )
                })
                .unwrap()
                .unwrap();
            // The synchronous barrier also hides dependent/unchanged palettes.
            assert!(world.skin_palette(a).is_none());
            assert!(world.skin_palette(b).is_none());
        }
        world.update_for_test(0.0).unwrap();
        assert_eq!(world.skin_palette(a).unwrap().as_ptr(), address);
    }
}

#[test]
fn failed_pose_propagation_hides_outputs_and_reuses_buffers_after_recovery() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, entity, _) = fixture(&mut fixture_host);
    let pose_address = world.skeleton_pose(entity).unwrap().as_ptr();
    let palette_address = world.skin_palette(entity).unwrap().as_ptr();
    let huge = Transform {
        sx: 1.0e30,
        ..Default::default()
    };
    let report = apply(
        &mut world,
        vec![set(
            entity,
            ComponentValue::SKELETON,
            std::mem::offset_of!(Skeleton, joints),
            FieldValue::Bytes(joint_overrides(&[(0, huge), (1, huge)])),
        )],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert!(world.skeleton_pose(entity).is_none());
    assert!(world.skin_palette(entity).is_none());

    let report = apply(
        &mut world,
        vec![set(
            entity,
            ComponentValue::SKELETON,
            std::mem::offset_of!(Skeleton, joints),
            FieldValue::Bytes(Vec::new()),
        )],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert_eq!(world.skeleton_pose(entity).unwrap().as_ptr(), pose_address);
    assert_eq!(
        world.skin_palette(entity).unwrap().as_ptr(),
        palette_address
    );
}

#[test]
fn mesh_and_skeleton_spaces_are_converted_and_deleted_targets_do_not_rebind() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, a, b) = fixture(&mut fixture_host);
    let report = apply(
        &mut world,
        vec![
            set(
                b,
                ComponentValue::SKIN,
                std::mem::offset_of!(Skin, skeleton),
                FieldValue::Entity(EntityRef::Handle(a)),
            ),
            set(
                a,
                ComponentValue::TRANSFORM,
                std::mem::offset_of!(Transform, x),
                FieldValue::F32(3.0),
            ),
            set(
                b,
                ComponentValue::TRANSFORM,
                std::mem::offset_of!(Transform, x),
                FieldValue::F32(1.0),
            ),
        ],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert!((world.skin_palette(b).unwrap()[0][12] - 2.0).abs() < 1e-5);
    let report = apply(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(a),
        }],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert!(world.skin_palette(b).is_none());
    let report = apply(
        &mut world,
        vec![create(0), insert(0, ComponentValue::SKELETON, vec![])],
    );
    let next = report.outcomes[0].result.as_ref().unwrap()[0].1;
    assert_eq!(next.index(), a.index());
    assert_ne!(next, a);
    assert!(world.skin_palette(b).is_none());
}

#[test]
fn invalid_override_keeps_neighbor_edits_and_malformed_assets_reject() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, a, _) = fixture(&mut fixture_host);

    let mut joints = 31u32.to_le_bytes().to_vec();
    for v in [0.0f32, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0] {
        joints.extend(v.to_le_bytes());
    }
    let report = apply(
        &mut world,
        vec![
            set(
                a,
                ComponentValue::TRANSFORM,
                std::mem::offset_of!(Transform, x),
                FieldValue::F32(5.0),
            ),
            set(
                a,
                ComponentValue::SKELETON,
                std::mem::offset_of!(Skeleton, joints),
                FieldValue::Bytes(joints),
            ),
        ],
    );
    assert!(report.outcomes[0].result.is_err());
    assert!(world.skin_palette(a).is_none());
    assert!(
        world
            .inspect(a)
            .unwrap()
            .base
            .iter()
            .any(|value| { matches!(value, ComponentValue::Transform(value) if value.x == 5.0) })
    );
    apply(
        &mut world,
        vec![set(
            a,
            ComponentValue::SKELETON,
            std::mem::offset_of!(Skeleton, joints),
            FieldValue::Bytes(Vec::new()),
        )],
    );
    assert!(world.skin_palette(a).is_some());
    let rig = builtin::rig(SKELETON_TYPE, "ipp://skeleton/rig-strip").unwrap();
    for offset in [12, 56] {
        let mut bad = rig.clone();
        bad[offset..offset + 4].copy_from_slice(&1u32.to_le_bytes());
        assert!(SkeletonAsset::decode(&bad).is_err());
    }
    for size in 0..rig.len() {
        assert!(SkeletonAsset::decode(&rig[..size]).is_err());
    }
    let mut skin = builtin::rig(SKIN_TYPE, "ipp://skin/rig-strip").unwrap();
    skin[16..80].fill(0);
    assert!(SkinAsset::decode(&skin).is_err());
}

#[test]
fn optional_joint_streams_validate_all_slots_and_preserve_rigid_byte_cost() {
    let bytes = builtin::rig(MESH_TYPE, "ipp://mesh/rig-strip").unwrap();
    let (mesh, stats) = MeshAsset::decode(&bytes).unwrap();
    assert_eq!(stats.vertex_bytes, 18 * (12 + 12 + 4 + 16));
    assert!(
        mesh.joint_weights()
            .unwrap()
            .iter()
            .all(|weights| (weights.iter().sum::<f32>() - 1.0).abs() < 1e-6)
    );
    // Descriptor bytes (52), positions (216), colors (216), joint bytes (72).
    let mut bad = bytes.clone();
    bad[52 + 432] = 32;
    assert!(MeshAsset::decode(&bad).is_err());
    let weight_start = 52 + 432 + 72;
    let mut bad = bytes.clone();
    bad[weight_start..weight_start + 16].fill(0);
    assert!(MeshAsset::decode(&bad).is_err());
    for value in [f32::NAN, f32::INFINITY, -0.1] {
        let mut bad = bytes.clone();
        bad[weight_start..weight_start + 4].copy_from_slice(&value.to_le_bytes());
        assert!(MeshAsset::decode(&bad).is_err());
    }
    let cube = builtin::mesh("ipp://mesh/cube?width=1&height=1&length=1").unwrap();
    let (mesh, stats) = MeshAsset::decode(&cube).unwrap();
    assert!(mesh.joint_indices().is_none());
    assert!(mesh.normals().is_some());
    assert_eq!(stats.vertex_bytes, mesh.vertex_count() as u32 * 44);
}

fn mapped_geometry() -> Vec<u8> {
    use geometry::{GeometryDefinition, GeometryShape, GeometryShapePart};
    GeometryDefinition {
        parts: vec![GeometryShapePart {
            joints: Some([0, 1]),
            ..GeometryShape::Pill {
                start: [0.0; 3],
                end: [0.0; 3],
                radius: 0.1,
            }
            .into()
        }],
    }
    .encode()
    .unwrap()
}

fn joint_overrides(values: &[(u32, Transform)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for &(joint, t) in values {
        bytes.extend(joint.to_le_bytes());
        for value in [t.x, t.y, t.z, t.qx, t.qy, t.qz, t.qw, t.sx, t.sy, t.sz] {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn shared_geometry_maps_two_final_joint_origins_and_scales_radius_for_both_components() {
    use components::{BoundingGeometry, PickingGeometry};
    use geometry::{GEOMETRY_TYPE, GeometryBounds, GeometryRay, GeometryShape};
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, a, b) = fixture(&mut fixture_host);
    upload(&mut world, GEOMETRY_TYPE, 50, mapped_geometry());
    let mut commands = Vec::new();
    for entity in [a, b] {
        commands.extend([
            Command::InsertComponentValue {
                entity: EntityRef::Handle(entity),
                value: ComponentValue::BoundingGeometry(BoundingGeometry {
                    source: "asset://6/50".into(),
                    is_rendered: true,
                    ..Default::default()
                }),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Handle(entity),
                value: ComponentValue::PickingGeometry(PickingGeometry {
                    source: "asset://6/50".into(),
                    is_rendered: true,
                    ..Default::default()
                }),
            },
        ]);
    }
    assert!(apply(&mut world, commands).outcomes[0].result.is_ok());
    let rest = world.picking_geometry(b).unwrap().clone();
    let root = Transform {
        qz: std::f32::consts::FRAC_1_SQRT_2,
        qw: std::f32::consts::FRAC_1_SQRT_2,
        sx: 2.0,
        sy: 3.0,
        ..Default::default()
    };
    let child = Transform {
        y: 1.0,
        sy: 2.0,
        ..Default::default()
    };
    let report = apply(
        &mut world,
        vec![
            Command::InsertComponentValue {
                entity: EntityRef::Handle(a),
                value: ComponentValue::Transform(Transform {
                    x: 5.0,
                    sx: 2.0,
                    sz: 0.5,
                    ..Default::default()
                }),
            },
            set(
                a,
                ComponentValue::SKELETON,
                std::mem::offset_of!(Skeleton, joints),
                FieldValue::Bytes(joint_overrides(&[(0, root), (1, child)])),
            ),
        ],
    );
    assert!(report.outcomes[0].result.is_ok());
    let evaluated = world.picking_geometry(a).unwrap();
    assert_eq!(evaluated, world.bounding_geometry(a).unwrap());
    assert_eq!(world.picking_geometry(b).unwrap(), &rest);
    let GeometryShape::Pill {
        start,
        end,
        radius,
    } = evaluated.parts[0].shape
    else {
        panic!("expected mapped pill")
    };
    for (actual, expected) in start
        .into_iter()
        .chain(end)
        .zip([5.0, 0.0, 0.0, -1.0, 0.0, 0.0])
    {
        assert!(
            (actual - expected).abs() < 1e-5,
            "joint origins include hierarchy and entity placement"
        );
    }
    assert!(
        (radius - 1.2).abs() < 1e-5,
        "larger endpoint stretch scales the radius"
    );
    let hit = evaluated
        .ray_intersection(
            &GeometryRay {
                origin: [2.0, 0.0, 5.0],
                direction: [0.0, 0.0, -1.0],
            },
            0.0,
            10.0,
        )
        .unwrap();
    assert!((hit.distance - 3.8).abs() < 1e-5);
    let items: Vec<_> = world
        .debug_render_items()
        .iter()
        .filter(|item| item.entity == a)
        .collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].model, items[1].model);
    assert_eq!(items[0].geometry, items[1].geometry);
}

#[test]
fn mapped_geometry_invalidates_on_skeleton_replacement_until_explicitly_rebound() {
    use components::PickingGeometry;
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, a, _) = fixture(&mut fixture_host);
    let geometry = PickingGeometry {
        geometry: mapped_geometry(),
        ..Default::default()
    };
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(a),
            value: ComponentValue::PickingGeometry(geometry.clone()),
        }],
    );
    assert!(world.picking_geometry(a).is_ok());
    apply(
        &mut world,
        vec![set(
            a,
            ComponentValue::SKELETON,
            std::mem::offset_of!(Skeleton, source),
            FieldValue::String(String::new()),
        )],
    );
    apply(
        &mut world,
        vec![set(
            a,
            ComponentValue::SKELETON,
            std::mem::offset_of!(Skeleton, source),
            FieldValue::String("asset://3/1".into()),
        )],
    );
    assert_eq!(world.picking_geometry(a), Err(ErrorReason::InvalidGeometry));
    apply(
        &mut world,
        vec![set(
            a,
            ComponentValue::PICKING_GEOMETRY,
            std::mem::offset_of!(PickingGeometry, r),
            FieldValue::F32(0.5),
        )],
    );
    assert_eq!(
        world.picking_geometry(a),
        Err(ErrorReason::InvalidGeometry),
        "color does not rebind stale joint ordinals"
    );
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(a),
            value: ComponentValue::PickingGeometry(geometry),
        }],
    );
    assert!(world.picking_geometry(a).is_ok());
}

#[test]
fn generated_bounds_enclose_every_blended_vertex_after_pose_and_nonuniform_scale() {
    use components::BoundingGeometry;
    use geometry::{GeometryBounds, GeometryShapeTransform};
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, a, _) = fixture(&mut fixture_host);
    apply(
        &mut world,
        vec![
            Command::InsertComponentValue {
                entity: EntityRef::Handle(a),
                value: ComponentValue::BoundingGeometry(BoundingGeometry::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Handle(a),
                value: ComponentValue::Transform(Transform {
                    x: 2.0,
                    qz: 0.3,
                    qw: 0.9539392,
                    sx: 2.0,
                    sy: 0.7,
                    sz: 3.0,
                    ..Default::default()
                }),
            },
            set(
                a,
                ComponentValue::SKELETON,
                std::mem::offset_of!(Skeleton, pose_source),
                FieldValue::String("asset://4/4".into()),
            ),
        ],
    );
    let bounds = world.bounding_geometry(a).unwrap().bounds().unwrap();
    assert_eq!(
        world.render_geometry(a).mesh_bounds,
        world.mesh_bounds(a).unwrap()
    );
    assert_eq!(world.render_geometry(a).mesh_bounds, Some(bounds));
    let mesh = world
        .mesh(MeshKey {
            asset: 3,
            variant: 0,
        })
        .unwrap();
    let model = GeometryShapeTransform::from_matrix(
        camera::model_matrix(&Transform {
            x: 2.0,
            qz: 0.3,
            qw: 0.9539392,
            sx: 2.0,
            sy: 0.7,
            sz: 3.0,
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    let palette = world.skin_palette(a).unwrap();
    // Independent vertex-by-vertex LBS oracle; production only transforms cached influence boxes.
    for vertex in 0..mesh.vertex_count() {
        let position = mesh.positions()[vertex].map(f64::from);
        let mut blended = [0.0; 3];
        for influence in 0..4 {
            let joint = mesh.joint_indices().unwrap()[vertex][influence] as usize;
            let weight = f64::from(mesh.joint_weights().unwrap()[vertex][influence]);
            let p = GeometryShapeTransform::from_matrix(palette[joint])
                .unwrap()
                .point(position);
            for axis in 0..3 {
                blended[axis] += p[axis] * weight;
            }
        }
        let world_position = model.point(blended);
        for axis in 0..3 {
            assert!(
                world_position[axis] >= bounds[0][axis] - 1e-5
                    && world_position[axis] <= bounds[1][axis] + 1e-5
            );
        }
    }
    let planes = geometry::frustum_planes(
        camera::prepare(
            a,
            &components::Camera {
                projection: 1,
                ortho_height: 4.0,
                ..Default::default()
            },
            &Transform {
                z: 6.0,
                ..Default::default()
            },
            100,
            100,
        )
        .unwrap()
        .view_projection,
    );
    assert!(world.geometry_visible(a, &planes));
    apply(
        &mut world,
        vec![set(
            a,
            ComponentValue::TRANSFORM,
            std::mem::offset_of!(Transform, x),
            FieldValue::F32(100.0),
        )],
    );
    assert!(!world.geometry_visible(a, &planes));
    assert!(
        !world
            .culling_geometry(a)
            .unwrap()
            .intersects_frustum(&planes)
    );
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(a),
            value: ComponentValue::BoundingGeometry(BoundingGeometry {
                geometry: geometry::GeometryDefinition::from(geometry::GeometryShape::default())
                    .encode()
                    .unwrap(),
                ..Default::default()
            }),
        }],
    );
    assert!(
        world.geometry_visible(a, &planes),
        "unproven enclosure never hides the visual mesh"
    );
    assert!(world.render_geometry(a).culling.is_none());
    assert_eq!(
        world.render_geometry(a).mesh_bounds,
        world.mesh_bounds(a).unwrap()
    );
}
