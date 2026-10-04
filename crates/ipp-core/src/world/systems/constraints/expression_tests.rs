//! Real registered schema rows feeding and receiving a shared expression plan.

use super::*;
use crate::components::{RowsFixture, RowsFixtureItem, rows::Rows};
use crate::expressions::{ExpressionDeclaration, ExpressionInput, ExpressionNode};
use crate::services::asset_management::{
    AssetUpload, AssetUploadIdentity, expression::EXPRESSION_TYPE,
};
use crate::{
    Batch, Command, DynamicPropertyKind, DynamicValue, EntityRef, FieldValue, FieldWrite,
    HostRuntime, WorldContext, WorldLimits,
};
use std::mem::offset_of;

fn step(world: &mut WorldContext<'_>) -> crate::WorldUpdateReport {
    world.prepare_update(0.0).unwrap();
    world.poll_assets();
    crate::test_task_scheduler::poll_ready();
    world.poll_assets();
    world.step(0.0).unwrap()
}

fn submit(world: &mut WorldContext<'_>, operations: Vec<Command>) -> crate::BatchOutcome {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    step(world).outcomes.remove(0)
}

#[test]
fn declaration_candidate_work_is_operation_local_until_commit() {
    let mut staged = WorldMutationState::default();
    let declaration = (EntityId::from_bits(1), ComponentValue::EXPRESSION_DRIVER);
    staged.changed.insert(declaration, Some(1));
    for index in 2..=4096 {
        staged.changed.insert(
            (EntityId::from_bits(index), ComponentValue::SCALAR),
            Some(1),
        );
    }

    let candidates = |staged: &WorldMutationState, operation| {
        super::expression_binding::expression_declaration_candidates(staged, operation)
            .copied()
            .collect::<Vec<_>>()
    };
    // A growing batch, even with a driver already declared, adds no historical
    // candidates to an operation that touched no components.
    assert!(candidates(&staged, true).is_empty());
    let local = (EntityId::from_bits(4097), ComponentValue::SCALAR);
    staged.operation_components.insert(local);
    assert_eq!(candidates(&staged, true), [local]);

    // Newly inserted and explicit equal driver writes still enter the same
    // operation-local path; commit visits the cumulative changes once.
    staged.operation_components.insert(declaration);
    assert_eq!(candidates(&staged, true), [declaration, local]);
    let committed = candidates(&staged, false);
    assert_eq!(committed.len(), 4098);
    assert!(committed.contains(&(EntityId::from_bits(2), ComponentValue::SCALAR)));
}

#[test]
fn typed_row_result_range_guard_and_row_lifetime_use_shared_preparation() {
    let mut host = HostRuntime::new();
    crate::test_task_scheduler::install(&mut host);
    let id = host
        .create_world(
            WorldLimits::default(),
            &[
                crate::systems::animation::AnimationSystem::ID,
                crate::systems::asset_dependencies::AssetDependencySystem::ID,
                ConstraintSystem::ID,
            ],
        )
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let mut rows = Rows::new();
    rows.insert(
        3,
        RowsFixtureItem {
            weight: 0.5,
            mark: Some(0.0),
            ..Default::default()
        },
    )
    .unwrap();
    let entity = submit(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::RowsFixture(RowsFixture {
                    items: rows,
                    ..Default::default()
                }),
            ),
        ],
    )
    .result
    .unwrap()[0]
        .1;
    let input = Rows::<RowsFixtureItem>::offset(0, 3, 0).unwrap();
    let target = Rows::<RowsFixtureItem>::offset(0, 3, RowsFixture::MARK).unwrap();
    let declaration = ExpressionDeclaration {
        inputs: vec![ExpressionInput {
            name: "x".into(),
            kind: DynamicPropertyKind::F32,
        }],
        nodes: vec![ExpressionNode::Input(0)],
        output: 0,
    };
    world
        .enqueue_asset(AssetUpload {
            id: 1,
            key: AssetUploadIdentity {
                kind: EXPRESSION_TYPE,
                asset: 1,
                variant: 0,
            },
            bytes: declaration.encode().unwrap(),
        })
        .unwrap();
    for _ in 0..8 {
        step(&mut world);
    }
    let component = ComponentValue::ROWS_FIXTURE;
    let mapping = encode_expression_driver_inputs(&[ExpressionDriverInput {
        name: "x".into(),
        property: DriverProperty {
            component,
            offset: input,
        },
    }])
    .unwrap();
    assert!(
        submit(
            &mut world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::ExpressionDriver(ExpressionDriver {
                    source: entity,
                    expression_source: "asset://19/1".into(),
                    inputs: mapping,
                    target_component: u32::from(component),
                    target_offset: target,
                    ..Default::default()
                })
            )]
        )
        .result
        .is_ok()
    );
    for _ in 0..4 {
        step(&mut world);
    }
    let read = |world: &WorldContext<'_>| {
        world
            .inspect(entity)
            .unwrap()
            .components
            .into_iter()
            .find(|v| v.type_id() == component)
            .unwrap()
            .field(target)
            .unwrap()
    };
    let prepared = preparation(&mut world, entity);
    assert_eq!(
        read(&world),
        crate::components::schema::FieldValue::Dynamic(DynamicValue::F32(0.5))
    );
    assert!(
        submit(
            &mut world,
            vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component,
                field: FieldWrite {
                    offset: input,
                    value: FieldValue::Dynamic(DynamicValue::F32(2.0))
                }
            }]
        )
        .result
        .is_ok()
    );
    assert_eq!(
        read(&world),
        crate::components::schema::FieldValue::Dynamic(DynamicValue::F32(0.5))
    );
    assert_eq!(
        world.expression_driver_status(entity).unwrap().state,
        ExpressionDriverState::Retained(ExpressionDriverReason::TargetRejected(
            ErrorReason::InvalidValue
        ))
    );
    assert!(std::sync::Arc::ptr_eq(
        &prepared,
        &preparation(&mut world, entity)
    ));
    // Tables may grow without moving the component or invalidating a live slot.
    let mut rows = Rows::new();
    for slot in 0..128 {
        rows.insert(
            slot,
            RowsFixtureItem {
                weight: 0.75,
                mark: Some(0.5),
                ..Default::default()
            },
        )
        .unwrap();
    }
    assert!(
        submit(
            &mut world,
            vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component,
                field: FieldWrite {
                    offset: offset_of!(RowsFixture, items) as u32,
                    value: FieldValue::Rows(rows.encode())
                }
            }]
        )
        .result
        .is_ok()
    );
    assert_eq!(
        read(&world),
        crate::components::schema::FieldValue::Dynamic(DynamicValue::F32(0.75))
    );
    assert!(std::sync::Arc::ptr_eq(
        &prepared,
        &preparation(&mut world, entity)
    ));
    // Clearing and restoring an optional property in the same batch must
    // revoke its old access at the departure, even though the final kind matches.
    assert!(
        submit(
            &mut world,
            vec![
                write(entity, component, target, FieldValue::Unset),
                write(
                    entity,
                    component,
                    target,
                    FieldValue::Dynamic(DynamicValue::F32(0.25))
                ),
            ]
        )
        .result
        .is_ok()
    );
    assert_eq!(
        read(&world),
        crate::components::schema::FieldValue::Dynamic(DynamicValue::F32(0.25))
    );
    assert_eq!(
        world.expression_driver_status(entity).unwrap().state,
        ExpressionDriverState::Retained(ExpressionDriverReason::TargetUnavailable)
    );
    world
        .with_system::<ConstraintSystem, _>(ConstraintSystem::ID, |system, _| {
            assert!(system.state.expressions[&entity].runtime.is_none());
        })
        .unwrap();
}

fn preparation(world: &mut WorldContext<'_>, entity: EntityId) -> std::sync::Arc<()> {
    world
        .with_system::<ConstraintSystem, _>(ConstraintSystem::ID, |system, context| {
            assert!(
                context.world.fault.is_none(),
                "world fault {:?}",
                context.world.fault
            );
            system.state.expressions[&entity]
                .runtime
                .as_ref()
                .unwrap_or_else(|| {
                    panic!(
                        "prepared expression: {:?}, dirty={}",
                        system.state.expressions[&entity].status, system.state.numeric_dirty
                    )
                })
                .preparation
                .clone()
        })
        .unwrap()
}

fn dynamic_fixture() -> (HostRuntime, crate::WorldId, EntityId, u32, u32) {
    use crate::systems::{
        animation::AnimationSystem, asset_dependencies::AssetDependencySystem,
        geometry::GeometrySystem, hierarchy::FinalPropagationSystem, hierarchy::HierarchySystem,
        look_at::LookAtSystem, render::RenderSystem,
    };
    let mut host = HostRuntime::new();
    crate::test_task_scheduler::install(&mut host);
    let id = host
        .create_world(
            WorldLimits::default(),
            &[
                AnimationSystem::ID,
                AssetDependencySystem::ID,
                ConstraintSystem::ID,
                HierarchySystem::ID,
                LookAtSystem::ID,
                FinalPropagationSystem::ID,
                GeometrySystem::ID,
                RenderSystem::ID,
            ],
        )
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let mut material = crate::components::CustomMaterial::default();
    material
        .properties
        .set("gap", DynamicValue::Mat4([1.0; 16]))
        .unwrap();
    let input = material
        .properties
        .set("x", DynamicValue::F32(2.0))
        .unwrap();
    let output = material
        .properties
        .set("y", DynamicValue::F32(0.0))
        .unwrap();
    material.properties.remove("gap");
    let entity = submit(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::CustomMaterial(material),
            ),
        ],
    )
    .result
    .unwrap()[0]
        .1;
    let declaration = ExpressionDeclaration {
        inputs: vec![ExpressionInput {
            name: "x".into(),
            kind: DynamicPropertyKind::F32,
        }],
        nodes: vec![ExpressionNode::Input(0)],
        output: 0,
    };
    world
        .enqueue_asset(AssetUpload {
            id: 1,
            key: AssetUploadIdentity {
                kind: EXPRESSION_TYPE,
                asset: 1,
                variant: 0,
            },
            bytes: declaration.encode().unwrap(),
        })
        .unwrap();
    for _ in 0..8 {
        step(&mut world);
    }
    let inputs = encode_expression_driver_inputs(&[ExpressionDriverInput {
        name: "x".into(),
        property: DriverProperty {
            component: ComponentValue::CUSTOM_MATERIAL,
            offset: input,
        },
    }])
    .unwrap();
    assert!(
        submit(
            &mut world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::ExpressionDriver(ExpressionDriver {
                    source: entity,
                    expression_source: "asset://19/1".into(),
                    inputs,
                    target_component: u32::from(ComponentValue::CUSTOM_MATERIAL),
                    target_offset: output,
                    ..Default::default()
                }),
            )]
        )
        .result
        .is_ok()
    );
    for _ in 0..4 {
        step(&mut world);
    }
    drop(world);
    host.progress_assets();
    step(&mut host.world_mut(id).unwrap());
    (host, id, entity, input, output)
}

fn material(world: &WorldContext<'_>, entity: EntityId) -> crate::components::CustomMaterial {
    world
        .inspect(entity)
        .unwrap()
        .components
        .into_iter()
        .find_map(|value| {
            if let ComponentValue::CustomMaterial(value) = value {
                Some(value)
            } else {
                None
            }
        })
        .unwrap()
}

fn write(entity: EntityId, component: u16, offset: u32, value: FieldValue) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component,
        field: FieldWrite {
            offset,
            value,
        },
    }
}

#[test]
fn value_edits_property_growth_and_graph_changes_keep_prepared_runtime() {
    let (mut host, id, entity, input, _) = dynamic_fixture();
    let mut world = host.world_mut(id).unwrap();
    let prepared = preparation(&mut world, entity);
    for value in [3.0, 5.0, 7.0] {
        assert!(
            submit(
                &mut world,
                vec![write(
                    entity,
                    ComponentValue::CUSTOM_MATERIAL,
                    input,
                    FieldValue::Dynamic(DynamicValue::F32(value))
                )]
            )
            .result
            .is_ok()
        );
        assert!(std::sync::Arc::ptr_eq(
            &prepared,
            &preparation(&mut world, entity)
        ));
        assert_eq!(
            material(&world, entity).properties.get("y"),
            Some(DynamicValue::F32(value))
        );
    }
    // Growth fills an unrelated gap and then reallocates the buffer. Neither
    // operation changes the selected descriptors or their compiled access.
    for index in 0..32 {
        assert!(
            submit(
                &mut world,
                vec![Command::SetDynamicProperty {
                    entity: EntityRef::Handle(entity),
                    component: ComponentValue::CUSTOM_MATERIAL,
                    name: format!("unrelated_{index}"),
                    value: DynamicValue::Vec4([1.0; 4]),
                }]
            )
            .result
            .is_ok()
        );
        assert!(std::sync::Arc::ptr_eq(
            &prepared,
            &preparation(&mut world, entity)
        ));
    }
    assert!(
        submit(
            &mut world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::Scalar(crate::components::Scalar {
                    value: 1.0
                })
            )]
        )
        .result
        .is_ok()
    );
    assert!(std::sync::Arc::ptr_eq(
        &prepared,
        &preparation(&mut world, entity)
    ));
    // A new mixed-graph declaration forces graph preparation. The existing
    // expression still owns exactly its original plan, access and scratch.
    assert!(
        submit(
            &mut world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::LinearDriver(LinearDriver {
                    source: entity,
                    ..Default::default()
                })
            )]
        )
        .result
        .is_ok()
    );
    assert!(std::sync::Arc::ptr_eq(
        &prepared,
        &preparation(&mut world, entity)
    ));
}

#[test]
fn metadata_compaction_reprepares_only_changed_layout_and_preserves_pins() {
    let (mut host, id, entity, input, output) = dynamic_fixture();
    let mut world = host.world_mut(id).unwrap();
    let prepared = preparation(&mut world, entity);
    let before = material(&world, entity);
    let metadata = before
        .properties
        .field(crate::components::dynamic_properties::DYNAMIC_METADATA)
        .unwrap();
    let crate::components::schema::FieldValue::Bytes(metadata) = metadata else {
        unreachable!()
    };
    assert!(
        submit(
            &mut world,
            vec![Command::InsertComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CUSTOM_MATERIAL,
                adopt: true,
                fields: vec![
                    FieldWrite {
                        offset: crate::components::dynamic_properties::DYNAMIC_METADATA,
                        value: FieldValue::Bytes(metadata.clone())
                    },
                    FieldWrite {
                        offset: input,
                        value: FieldValue::Dynamic(DynamicValue::F32(9.0))
                    },
                    FieldWrite {
                        offset: output,
                        value: FieldValue::Dynamic(DynamicValue::F32(0.0))
                    },
                ],
            }]
        )
        .result
        .is_ok()
    );
    let after = material(&world, entity);
    assert_ne!(
        before.properties.descriptors()["x"].offset,
        after.properties.descriptors()["x"].offset
    );
    assert_eq!(before.properties.key("x"), after.properties.key("x"));
    assert!(!std::sync::Arc::ptr_eq(
        &prepared,
        &preparation(&mut world, entity)
    ));
    assert_eq!(after.properties.get("y"), Some(DynamicValue::F32(9.0)));
    // Once compacted, decoding identical layout and writing ordinary values
    // retains the runtime even though metadata was explicitly rewritten.
    let compact = preparation(&mut world, entity);
    assert!(
        submit(
            &mut world,
            vec![Command::InsertComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CUSTOM_MATERIAL,
                adopt: true,
                fields: vec![
                    FieldWrite {
                        offset: crate::components::dynamic_properties::DYNAMIC_METADATA,
                        value: FieldValue::Bytes(metadata)
                    },
                    FieldWrite {
                        offset: input,
                        value: FieldValue::Dynamic(DynamicValue::F32(4.0))
                    },
                    FieldWrite {
                        offset: output,
                        value: FieldValue::Dynamic(DynamicValue::F32(0.0))
                    },
                ],
            }]
        )
        .result
        .is_ok()
    );
    assert!(std::sync::Arc::ptr_eq(
        &compact,
        &preparation(&mut world, entity)
    ));
    assert_eq!(
        material(&world, entity).properties.get("y"),
        Some(DynamicValue::F32(4.0))
    );
}

#[test]
fn same_batch_departures_revoke_before_identity_or_storage_reuse() {
    for target in [false, true] {
        for retype in [false, true] {
            let (mut host, id, entity, _, _) = dynamic_fixture();
            let mut world = host.world_mut(id).unwrap();
            let prepared = preparation(&mut world, entity);
            let before = material(&world, entity);
            let metadata = before
                .properties
                .field(crate::components::dynamic_properties::DYNAMIC_METADATA)
                .unwrap();
            let crate::components::schema::FieldValue::Bytes(metadata) = metadata else {
                unreachable!()
            };
            let name = if target {
                "y"
            } else {
                "x"
            };
            let depart = if retype {
                Command::SetDynamicProperty {
                    entity: EntityRef::Handle(entity),
                    component: ComponentValue::CUSTOM_MATERIAL,
                    name: name.into(),
                    value: DynamicValue::Vec3([1.0; 3]),
                }
            } else {
                Command::RemoveDynamicProperty {
                    entity: EntityRef::Handle(entity),
                    component: ComponentValue::CUSTOM_MATERIAL,
                    name: name.into(),
                }
            };
            // Deliberately restore the old key and kind through metadata in the
            // same batch. Final-state pin equality must not revive a departure.
            assert!(
                submit(
                    &mut world,
                    vec![
                        write(
                            entity,
                            ComponentValue::EXPRESSION_DRIVER,
                            offset_of!(ExpressionDriver, source) as u32,
                            FieldValue::Entity(EntityRef::Handle(entity))
                        ),
                        depart,
                        write(
                            entity,
                            ComponentValue::CUSTOM_MATERIAL,
                            crate::components::dynamic_properties::DYNAMIC_METADATA,
                            FieldValue::Bytes(metadata)
                        )
                    ]
                )
                .result
                .is_ok()
            );
            world
                .with_system::<ConstraintSystem, _>(ConstraintSystem::ID, |system, _| {
                    let binding = &system.state.expressions[&entity];
                    if target {
                        assert!(binding.target.is_none());
                        assert!(binding.runtime.is_none());
                    } else {
                        assert!(binding.inputs[0].1.is_none());
                        assert!(binding.runtime.as_ref().unwrap().sources[0].is_none());
                    }
                })
                .unwrap();
            if !target {
                assert!(!std::sync::Arc::ptr_eq(
                    &prepared,
                    &preparation(&mut world, entity)
                ));
            }
        }
    }
}

#[test]
fn component_replacement_revokes_access_until_explicit_equal_rebind() {
    let (mut host, id, entity, _, _) = dynamic_fixture();
    let mut world = host.world_mut(id).unwrap();
    let prepared = preparation(&mut world, entity);
    let replacement = material(&world, entity);
    assert!(
        submit(
            &mut world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::CustomMaterial(replacement)
            )]
        )
        .result
        .is_ok()
    );
    world
        .with_system::<ConstraintSystem, _>(ConstraintSystem::ID, |system, _| {
            let binding = &system.state.expressions[&entity];
            assert!(binding.runtime.is_none());
            assert!(binding.target.is_none());
            assert!(binding.inputs[0].1.is_none());
        })
        .unwrap();
    assert!(
        submit(
            &mut world,
            vec![write(
                entity,
                ComponentValue::EXPRESSION_DRIVER,
                offset_of!(ExpressionDriver, target_component) as u32,
                FieldValue::U32(u32::MAX)
            )]
        )
        .result
        .is_err()
    );
    world
        .with_system::<ConstraintSystem, _>(ConstraintSystem::ID, |system, _| {
            let binding = &system.state.expressions[&entity];
            assert!(binding.runtime.is_none());
            assert!(binding.target.is_none());
            assert!(binding.inputs[0].1.is_none());
        })
        .unwrap();
    assert!(
        submit(
            &mut world,
            vec![write(
                entity,
                ComponentValue::EXPRESSION_DRIVER,
                offset_of!(ExpressionDriver, source) as u32,
                FieldValue::Entity(EntityRef::Handle(entity))
            )]
        )
        .result
        .is_ok()
    );
    assert!(!std::sync::Arc::ptr_eq(
        &prepared,
        &preparation(&mut world, entity)
    ));
    assert_eq!(
        material(&world, entity).properties.get("y"),
        Some(DynamicValue::F32(2.0))
    );
}
