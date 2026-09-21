//! Core debug resources, typed fields, and atomic global render settings.

mod support;
use support::WorldTestDriver;

use std::mem::offset_of;

use ipp_core::{
    Batch, Command, ComponentValue, EntityMetadata, EntityRef, ErrorReason, FieldValue, FieldWrite,
    RenderState, RenderStatePatch, WorldLimits,
    components::schema::{FieldError, FieldKind, SchemaComponent, SchemaField},
    components::{BoundingGeometry, Transform},
    systems::geometry::{GeometryDefinition, GeometryShape},
};

fn apply(
    world: &mut ipp_core::WorldContext<'_>,
    operations: Vec<Command>,
) -> ipp_core::WorldUpdateReport {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap()
}

fn create(world: &mut ipp_core::WorldContext<'_>, debug: BoundingGeometry) -> ipp_core::EntityId {
    let report = apply(
        world,
        vec![
            Command::Create {
                alias: 0,
                metadata: EntityMetadata {
                    symbolic_id: Some("debug".into()),
                    ..EntityMetadata::default()
                },
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value: ComponentValue::Transform(Transform::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value: ComponentValue::BoundingGeometry(debug),
            },
        ],
    );
    report.outcomes[0].result.as_ref().unwrap()[0].1
}

fn shape_bytes(shape: u32) -> Vec<u8> {
    let shape = match shape {
        0 => GeometryShape::Box {
            min: [-1.0; 3],
            max: [1.0; 3],
        },
        1 => GeometryShape::Sphere {
            center: [0.0; 3],
            radius: 1.0,
        },
        _ => GeometryShape::Pill {
            start: [0.0, -0.5, 0.0],
            end: [0.0, 0.5, 0.0],
            radius: 0.5,
        },
    };
    GeometryDefinition::from(shape).encode().unwrap()
}

fn unit_geometry() -> BoundingGeometry {
    BoundingGeometry {
        geometry: shape_bytes(0),
        ..BoundingGeometry::default()
    }
}

#[test]
fn sparse_updates_preserve_omission_reject_atomically_and_keep_submission_order() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let patches = [
        RenderStatePatch {
            show_all_debug_geometries: Some(true),
            ..RenderStatePatch::default()
        },
        RenderStatePatch {
            debug_geometry_color: Some([0.2, 0.3, 0.4]),
            ..RenderStatePatch::default()
        },
        RenderStatePatch {
            show_all_debug_geometries: Some(false),
            debug_geometry_color: Some([f32::NAN, 0.0, 1.0]),
            ambient_light: None,
        },
        RenderStatePatch::default(),
    ];
    for (index, patch) in patches.into_iter().enumerate() {
        world.enqueue_render_state_update(patch).unwrap();
        world
            .enqueue(Batch {
                id: index as u64,
                operations: vec![],
            })
            .unwrap();
    }
    assert_eq!(world.render_state(), RenderState::default());
    assert_eq!(
        world.update_for_test(f64::NAN),
        Err(ErrorReason::InvalidValue)
    );
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(report.outcomes.len(), 4);
    assert_eq!(report.render_state_changes.len(), 2);
    for (index, change) in report.render_state_changes.iter().enumerate() {
        assert_eq!(change.tick, 1);
        assert_eq!(change.changes, patches[index]);
    }
    assert_eq!(
        world.render_state(),
        RenderState {
            show_all_debug_geometries: true,
            debug_geometry_color: [0.2, 0.3, 0.4],
            ambient_light: [0.0; 3],
        }
    );
    for invalid in [
        [-0.1, 0.0, 0.0],
        [1.01, 0.0, 0.0],
        [f32::INFINITY, 0.0, 0.0],
    ] {
        world
            .enqueue_render_state_update(RenderStatePatch {
                show_all_debug_geometries: Some(false),
                debug_geometry_color: Some(invalid),
                ambient_light: None,
            })
            .unwrap();
        assert!(
            world
                .update_for_test(0.0)
                .unwrap()
                .render_state_changes
                .is_empty()
        );
        assert!(world.render_state().show_all_debug_geometries);
    }
    world
        .enqueue_render_state_update(RenderStatePatch {
            show_all_debug_geometries: Some(false),
            ..RenderStatePatch::default()
        })
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert!(!world.render_state().show_all_debug_geometries);
    assert_eq!(world.render_state().debug_geometry_color, [0.2, 0.3, 0.4]);
    let mut fresh_host = ipp_core::HostRuntime::new();
    let fresh = fresh_host.create_world(Default::default()).unwrap();
    assert_eq!(
        fresh_host.world_mut(fresh).unwrap().render_state(),
        RenderState::default()
    );
}

#[test]
fn state_notifications_omit_unchanged_fields_and_retain_each_changed_transition() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let color = world.render_state().debug_geometry_color;
    for is_rendered in [false, true, true, false, true] {
        world
            .enqueue_render_state_update(RenderStatePatch {
                show_all_debug_geometries: Some(is_rendered),
                debug_geometry_color: Some(color),
                ambient_light: None,
            })
            .unwrap();
    }
    world
        .enqueue_render_state_update(RenderStatePatch::default())
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert!(report.outcomes.is_empty());
    assert_eq!(
        report
            .render_state_changes
            .iter()
            .map(|event| event.changes)
            .collect::<Vec<_>>(),
        [true, false, true].map(|is_rendered| RenderStatePatch {
            show_all_debug_geometries: Some(is_rendered),
            ..RenderStatePatch::default()
        })
    );
    assert!(
        world
            .update_for_test(0.0)
            .unwrap()
            .render_state_changes
            .is_empty()
    );
}

#[test]
fn render_state_and_batches_share_the_bounded_control_queue() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(WorldLimits {
            max_queued_batches: 1,
            ..WorldLimits::default()
        })
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    world
        .enqueue_render_state_update(RenderStatePatch::default())
        .unwrap();
    assert_eq!(
        world.enqueue(Batch {
            id: 1,
            operations: vec![]
        }),
        Err(ErrorReason::Capacity)
    );
    assert_eq!(
        world.enqueue_render_state_update(RenderStatePatch::default()),
        Err(ErrorReason::Capacity)
    );
    world.update_for_test(0.0).unwrap();
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![],
        })
        .unwrap();
    assert_eq!(
        world.enqueue_render_state_update(RenderStatePatch::default()),
        Err(ErrorReason::Capacity)
    );
}

#[test]
fn boolean_schema_uses_exact_types_offsets_and_canonical_defaults() {
    let mut value = BoundingGeometry::default();
    let offset = offset_of!(BoundingGeometry, is_rendered) as u32;
    assert_eq!(FieldKind::Bool as u8, 7);
    assert_eq!(BoundingGeometry::FIELD_COUNT, 11);
    assert_eq!(
        BoundingGeometry::validate_field(offset, FieldKind::Bool),
        Ok(())
    );
    assert_eq!(
        BoundingGeometry::validate_field(offset, FieldKind::U32),
        Err(FieldError::WrongType)
    );
    assert_eq!(
        value.set_field(offset, ipp_core::components::schema::FieldValue::U32(1)),
        Err(FieldError::WrongType)
    );
    value
        .set_field(offset, ipp_core::components::schema::FieldValue::Bool(true))
        .unwrap();
    assert!(value.is_rendered);
    assert!(
        value
            .fields()
            .contains(&(offset, ipp_core::components::schema::FieldValue::Bool(true)))
    );
    let mut bytes = Vec::new();
    false.write_default(&mut bytes);
    true.write_default(&mut bytes);
    assert_eq!(bytes, [0, 1]);

    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, value);
    let before = world.inspect(entity).unwrap();
    let result = apply(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::BOUNDING_GEOMETRY,
            field: FieldWrite {
                offset,
                value: FieldValue::U32(0),
            },
        }],
    );
    assert_eq!(
        result.outcomes[0].result.as_ref().unwrap_err().reason,
        ErrorReason::InvalidField
    );
    assert_eq!(world.inspect(entity).unwrap(), before);
    assert!(before.base.iter().any(
        |value| matches!(value, ComponentValue::BoundingGeometry(debug) if debug.is_rendered)
    ));
}

#[test]
fn shapes_validate_without_acquiring_assets_and_global_state_preserves_authored_values() {
    for shape in 0..3 {
        for outline in [false, true] {
            let mut world_host = ipp_core::HostRuntime::new();
            let world_id = world_host
                .create_world(ipp_core::WorldLimits::default())
                .unwrap();
            let mut world = world_host.world_mut(world_id).unwrap();
            let entity = create(
                &mut world,
                BoundingGeometry {
                    geometry: shape_bytes(shape),
                    outline,
                    ..BoundingGeometry::default()
                },
            );
            assert!(world.render_items().is_empty());
            assert!(world.debug_render_items().is_empty());
            assert!(world.resource_snapshots().is_empty());
            let snapshot = world.inspect(entity).unwrap();
            world
                .enqueue_render_state_update(RenderStatePatch {
                    show_all_debug_geometries: Some(true),
                    ..RenderStatePatch::default()
                })
                .unwrap();
            let report = world.update_for_test(0.0).unwrap();
            assert!(report.resource_changes.is_empty());
            assert!(report.assets.is_empty());
            assert!(world.resource_requests_for_test().is_empty());
            let items = world.debug_render_items();
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].color, [1.0, 0.8, 0.0]);
            assert_eq!(world.inspect(entity).unwrap(), snapshot);
            assert!(world.resource_snapshots().is_empty());
        }
    }
    for debug in [
        BoundingGeometry {
            stroke: 0.0,
            ..unit_geometry()
        },
        BoundingGeometry {
            r: 2.0,
            ..unit_geometry()
        },
        BoundingGeometry {
            geometry: vec![1, 2, 3],
            ..unit_geometry()
        },
    ] {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let report = apply(
            &mut world,
            vec![
                Command::Create {
                    alias: 0,
                    metadata: EntityMetadata::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(0),
                    value: ComponentValue::BoundingGeometry(debug),
                },
            ],
        );
        assert!(report.outcomes[0].result.is_err());
        assert_eq!(world.entities().len(), 1);
        assert!(world.resource_snapshots().is_empty());
    }
}

#[test]
fn debug_items_are_independent_of_visual_materials_meshes_and_picking() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(
        &mut world,
        BoundingGeometry {
            is_rendered: true,
            geometry: shape_bytes(2),
            has_color_override: true,
            r: 0.0,
            g: 0.0,
            b: 1.0,
            ..BoundingGeometry::default()
        },
    );
    assert!(world.render_items().is_empty());
    let items = world.debug_render_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].color, [0.0, 0.0, 1.0]);
    assert_eq!(items[0].entity, entity);
    assert!(world.resource_snapshots().is_empty());
    assert!(
        world
            .inspect(entity)
            .unwrap()
            .effective
            .iter()
            .all(|value| !matches!(value, ComponentValue::PickingGeometry(_)))
    );
}

#[test]
fn geometry_overlay_reveals_latest_hidden_base_and_never_acquires_client_assets() {
    use ipp_core::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};

    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(
        &mut world,
        BoundingGeometry {
            is_rendered: true,
            ..unit_geometry()
        },
    );
    let report = apply(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: "debug".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(0),
                binding: StateOverlayRef::Alias(1),
                alias: 2,
                component: ComponentValue::BOUNDING_GEOMETRY,
                mode: ComponentOverlayMode::Auto,
                fields: vec![FieldWrite {
                    offset: offset_of!(BoundingGeometry, geometry) as u32,
                    value: FieldValue::Bytes(shape_bytes(1)),
                }],
            },
        ],
    );
    let outcome = &report.outcomes[0];
    assert!(outcome.result.is_ok());
    let owner = outcome.state_overlays[0].id;
    let overlay = outcome.state_overlays[2].id;
    assert_eq!(world.debug_render_items()[0].geometry.shape, 1);
    apply(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::BOUNDING_GEOMETRY,
            field: FieldWrite {
                offset: offset_of!(BoundingGeometry, geometry) as u32,
                value: FieldValue::Bytes(
                    GeometryDefinition::from(GeometryShape::Box {
                        min: [-1.5, -1.0, -1.0],
                        max: [1.5, 1.0, 1.0],
                    })
                    .encode()
                    .unwrap(),
                ),
            },
        }],
    );
    assert_eq!(world.debug_render_items()[0].geometry.shape, 1);
    assert!(world.resource_snapshots().is_empty());
    assert!(world.resource_requests_for_test().is_empty());
    apply(
        &mut world,
        vec![Command::ReleaseComponentStateOverlay {
            owner: StateOverlayRef::Handle(owner),
            overlay: StateOverlayRef::Handle(overlay),
        }],
    );
    assert!(world.resource_snapshots().is_empty());
    let debug = &world.debug_render_items()[0];
    assert_eq!((debug.geometry.shape, debug.model[0]), (0, 1.5));
    apply(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(entity),
        }],
    );
    assert!(world.debug_render_items().is_empty());
}

#[test]
fn private_generator_reuses_all_shapes_but_retains_only_positions_and_indices() {
    for shape in 0..4 {
        for outline in [false, true] {
            let value = ipp_core::systems::geometry::GeometryPrimitiveVisual {
                shape,
                outline,
                ..ipp_core::systems::geometry::GeometryPrimitiveVisual::default()
            };
            let mesh = ipp_core::services::asset_management::builtin::debug_mesh(&value).unwrap();
            assert!(mesh.vertex_count() > 0);
            assert!(mesh.colors().is_none());
            assert_eq!(mesh.vertex_bytes(), mesh.vertex_count() * 12);
            {
                assert!(mesh.uvs().is_none());
                assert!(mesh.texture_weights().is_none());
            }
        }
    }
}

#[test]
fn private_generator_rejects_unrepresentable_arrowheads_and_collapsed_dimensions() {
    use ipp_core::systems::geometry::GeometryPrimitiveVisual;
    for outline in [false, true] {
        let invalid = GeometryPrimitiveVisual {
            shape: 3,
            outline,
            stroke: 1.0e-10,
            ..GeometryPrimitiveVisual::default()
        };
        assert_eq!(
            invalid.validate_dimensions(),
            Err(ErrorReason::InvalidValue)
        );
        let valid = GeometryPrimitiveVisual {
            stroke: 1.0e-5,
            ..invalid
        };
        assert_eq!(valid.validate_dimensions(), Ok(()));
        assert!(ipp_core::services::asset_management::builtin::debug_mesh(&valid).is_ok());
    }
    let collapsed = GeometryPrimitiveVisual {
        width: f32::from_bits(1),
        ..GeometryPrimitiveVisual::default()
    };
    assert_eq!(
        collapsed.validate_dimensions(),
        Err(ErrorReason::InvalidValue)
    );
}

#[test]
fn ambient_updates_accept_hdr_preserve_omission_and_reject_atomically() {
    let mut host = ipp_core::HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    assert_eq!(world.render_state().ambient_light, [0.0; 3]);
    let ambient = RenderStatePatch {
        ambient_light: Some([2.0, 0.5, 0.25]),
        ..Default::default()
    };
    world.enqueue_render_state_update(ambient).unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(report.render_state_changes[0].changes, ambient);
    world.enqueue_render_state_update(ambient).unwrap();
    assert!(
        world
            .update_for_test(0.0)
            .unwrap()
            .render_state_changes
            .is_empty()
    );
    for channel in [-1.0, f32::NAN, f32::INFINITY] {
        world
            .enqueue_render_state_update(RenderStatePatch {
                ambient_light: Some([channel, 0.0, 0.0]),
                show_all_debug_geometries: Some(true),
                ..Default::default()
            })
            .unwrap();
        assert!(
            world
                .update_for_test(0.0)
                .unwrap()
                .render_state_changes
                .is_empty()
        );
        assert!(!world.render_state().show_all_debug_geometries);
        assert_eq!(world.render_state().ambient_light, [2.0, 0.5, 0.25]);
    }
    world
        .enqueue_render_state_update(RenderStatePatch {
            debug_geometry_color: Some([0.0; 3]),
            ..Default::default()
        })
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(world.render_state().ambient_light, [2.0, 0.5, 0.25]);
    drop(world);
    let fresh = host.create_world(Default::default()).unwrap();
    assert_eq!(
        host.world_mut(fresh).unwrap().render_state().ambient_light,
        [0.0; 3]
    );
}
