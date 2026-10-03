//! Actual ConstraintSystem, shared ExpressionAsset demand and prepared properties.
//! Direct-core evidence; generated transport/render scenarios are maintained separately.

mod support;
use ipp_core::components::{CustomMaterial, ExpressionDriver, LinearDriver, Scalar};
use ipp_core::expressions::*;
use ipp_core::services::asset_management::{
    AssetUpload, AssetUploadIdentity, expression::EXPRESSION_TYPE,
};
use ipp_core::systems::animation::*;
use ipp_core::systems::constraints::*;
use ipp_core::*;
use std::mem::offset_of;
use support::selection::{ASSETS, CONSTRAINTS, RENDER, select};
use support::{HostWorldTestDriver, WorldTestDriver};

fn host() -> (HostRuntime, WorldId) {
    let mut host = HostRuntime::new();
    let id = host
        .create_world(
            WorldLimits::default(),
            &select(&[ASSETS, CONSTRAINTS, RENDER]),
        )
        .unwrap();
    (host, id)
}

fn submit(world: &mut WorldContext<'_>, operations: Vec<Command>) -> BatchOutcome {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap().outcomes.remove(0)
}

fn create(world: &mut WorldContext<'_>, value: ComponentValue) -> EntityId {
    submit(
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(EntityRef::Alias(1), value),
        ],
    )
    .result
    .unwrap()[0]
        .1
}

fn scalar_property() -> DriverProperty {
    DriverProperty {
        component: ComponentValue::SCALAR,
        offset: offset_of!(Scalar, value) as u32,
    }
}

fn scalar(world: &mut WorldContext<'_>, value: f32) -> EntityId {
    create(
        world,
        ComponentValue::Scalar(Scalar {
            value,
        }),
    )
}

fn value(world: &WorldContext<'_>, entity: EntityId) -> f32 {
    let snapshot = world.inspect(entity).unwrap();
    snapshot
        .components
        .iter()
        .find_map(|value| {
            if let ComponentValue::Scalar(value) = value {
                Some(value.value)
            } else {
                None
            }
        })
        .unwrap()
}

fn set(
    world: &mut WorldContext<'_>,
    entity: EntityId,
    property: DriverProperty,
    value: FieldValue,
) {
    let outcome = submit(
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: property.component,
            field: FieldWrite {
                offset: property.offset,
                value,
            },
        }],
    );
    assert!(outcome.result.is_ok(), "{outcome:?}");
}

fn upload(world: &mut WorldContext<'_>, asset: u64, declaration: ExpressionDeclaration) {
    world
        .enqueue_asset(AssetUpload {
            id: asset,
            key: AssetUploadIdentity {
                kind: EXPRESSION_TYPE,
                asset,
                variant: 0,
            },
            bytes: declaration.encode().unwrap(),
        })
        .unwrap();
    let report = world.await_upload_for_test();
    assert!(report.assets[0].result.is_ok(), "{report:?}");
}

fn expression(
    names: &[(&str, DynamicPropertyKind)],
    nodes: Vec<ExpressionNode>,
) -> ExpressionDeclaration {
    ExpressionDeclaration {
        inputs: names
            .iter()
            .map(|&(name, kind)| ExpressionInput {
                name: name.into(),
                kind,
            })
            .collect(),
        output: nodes.len() - 1,
        nodes,
    }
}

fn attach(
    world: &mut WorldContext<'_>,
    entity: EntityId,
    source: EntityId,
    target: DriverProperty,
    asset: u64,
    inputs: &[(&str, DriverProperty)],
) {
    let driver = ExpressionDriver {
        source,
        expression_source: format!("asset://19/{asset}").into(),
        target_component: u32::from(target.component),
        target_offset: target.offset,
        inputs: encode_expression_driver_inputs(
            &inputs
                .iter()
                .map(|&(name, property)| ExpressionDriverInput {
                    name: name.into(),
                    property,
                })
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        ..Default::default()
    };
    let outcome = submit(
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::ExpressionDriver(driver),
        )],
    );
    assert!(outcome.result.is_ok(), "{outcome:?}");
    for _ in 0..4 {
        world.update_for_test(0.0).unwrap();
    }
}

fn state(world: &WorldContext<'_>, entity: EntityId) -> ExpressionDriverState {
    world.expression_driver_status(entity).unwrap().state
}

#[test]
fn scalar_and_multiinput_dynamic_results_keep_exact_types() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let mut material = CustomMaterial::default();
    let x = material
        .properties
        .set("x", DynamicValue::Vec3([2.0, 3.0, 5.0]))
        .unwrap();
    let scale = material
        .properties
        .set("scale", DynamicValue::Vec3([7.0, 11.0, 13.0]))
        .unwrap();
    let output = material
        .properties
        .set("output", DynamicValue::Vec3([0.0; 3]))
        .unwrap();
    let source = create(&mut world, ComponentValue::CustomMaterial(material.clone()));
    let target = create(&mut world, ComponentValue::CustomMaterial(material));
    upload(
        &mut world,
        1,
        expression(
            &[
                ("scale", DynamicPropertyKind::Vec3),
                ("x", DynamicPropertyKind::Vec3),
            ],
            vec![
                ExpressionNode::Input(0),
                ExpressionNode::Input(1),
                ExpressionNode::Binary {
                    operator: BinaryOperator::Multiply,
                    left: 0,
                    right: 1,
                },
            ],
        ),
    );
    let property = |offset| DriverProperty {
        component: ComponentValue::CUSTOM_MATERIAL,
        offset,
    };
    attach(
        &mut world,
        target,
        source,
        property(output),
        1,
        &[("x", property(x)), ("scale", property(scale))],
    );
    assert_eq!(state(&world, target), ExpressionDriverState::Written);
    let component = world
        .inspect(target)
        .unwrap()
        .components
        .into_iter()
        .find(|v| v.type_id() == ComponentValue::CUSTOM_MATERIAL)
        .unwrap();
    assert_eq!(
        component.field(output).unwrap(),
        components::schema::FieldValue::Dynamic(DynamicValue::Vec3([14.0, 33.0, 65.0]))
    );
    // Fixed schema inputs also bind once and feed the same typed plan.
    let source = scalar(&mut world, 12.0);
    let target = scalar(&mut world, -99.0);
    upload(
        &mut world,
        2,
        expression(
            &[("x", DynamicPropertyKind::F32)],
            vec![
                ExpressionNode::Input(0),
                ExpressionNode::Constant(DynamicValue::F32(2.5)),
                ExpressionNode::Binary {
                    operator: BinaryOperator::Multiply,
                    left: 0,
                    right: 1,
                },
            ],
        ),
    );
    attach(
        &mut world,
        target,
        source,
        scalar_property(),
        2,
        &[("x", scalar_property())],
    );
    assert_eq!(value(&world, target), 30.0);
}

#[test]
fn mixed_chain_cycles_correction_and_downstream_retained_reads() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let a = scalar(&mut world, 2.0);
    let b = scalar(&mut world, 10.0);
    let c = scalar(&mut world, 20.0);
    let d = scalar(&mut world, 0.0);
    upload(
        &mut world,
        1,
        expression(
            &[("x", DynamicPropertyKind::F32)],
            vec![
                ExpressionNode::Input(0),
                ExpressionNode::Constant(DynamicValue::F32(3.0)),
                ExpressionNode::Binary {
                    operator: BinaryOperator::Add,
                    left: 0,
                    right: 1,
                },
            ],
        ),
    );
    assert!(
        submit(
            &mut world,
            vec![Command::insert_value(
                EntityRef::Handle(b),
                ComponentValue::LinearDriver(LinearDriver {
                    source: a,
                    scale: 2.0,
                    bias: 1.0
                })
            )]
        )
        .result
        .is_ok()
    );
    attach(
        &mut world,
        c,
        b,
        scalar_property(),
        1,
        &[("x", scalar_property())],
    );
    assert!(
        submit(
            &mut world,
            vec![Command::insert_value(
                EntityRef::Handle(d),
                ComponentValue::LinearDriver(LinearDriver {
                    source: c,
                    scale: 10.0,
                    bias: 0.0
                })
            )]
        )
        .result
        .is_ok()
    );
    assert_eq!(
        (value(&world, b), value(&world, c), value(&world, d)),
        (5.0, 8.0, 80.0)
    );
    set(
        &mut world,
        b,
        DriverProperty {
            component: ComponentValue::LINEAR_DRIVER,
            offset: offset_of!(LinearDriver, source) as u32,
        },
        FieldValue::Entity(EntityRef::Handle(c)),
    );
    assert_eq!(
        state(&world, c),
        ExpressionDriverState::Retained(ExpressionDriverReason::Cycle)
    );
    set(&mut world, a, scalar_property(), FieldValue::F32(100.0));
    assert_eq!(
        (value(&world, b), value(&world, c), value(&world, d)),
        (5.0, 8.0, 80.0)
    );
    set(
        &mut world,
        b,
        DriverProperty {
            component: ComponentValue::LINEAR_DRIVER,
            offset: offset_of!(LinearDriver, source) as u32,
        },
        FieldValue::Entity(EntityRef::Handle(a)),
    );
    assert_eq!(
        (value(&world, b), value(&world, c), value(&world, d)),
        (201.0, 204.0, 2040.0)
    );
    assert!(world.expression_driver_status(c).unwrap().recovered);
}

#[test]
fn same_entity_different_property_is_not_a_cycle() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let mut material = CustomMaterial::default();
    let input = material
        .properties
        .set("input", DynamicValue::U32(17))
        .unwrap();
    let output = material
        .properties
        .set("output", DynamicValue::U32(0))
        .unwrap();
    let entity = create(&mut world, ComponentValue::CustomMaterial(material));
    upload(
        &mut world,
        1,
        expression(
            &[("x", DynamicPropertyKind::U32)],
            vec![
                ExpressionNode::Input(0),
                ExpressionNode::Constant(DynamicValue::U32(5)),
                ExpressionNode::Binary {
                    operator: BinaryOperator::Multiply,
                    left: 0,
                    right: 1,
                },
            ],
        ),
    );
    let prop = |offset| DriverProperty {
        component: ComponentValue::CUSTOM_MATERIAL,
        offset,
    };
    attach(
        &mut world,
        entity,
        entity,
        prop(output),
        1,
        &[("x", prop(input))],
    );
    assert_eq!(state(&world, entity), ExpressionDriverState::Written);
    assert_eq!(
        world
            .inspect(entity)
            .unwrap()
            .components
            .into_iter()
            .find(|v| v.type_id() == ComponentValue::CUSTOM_MATERIAL)
            .unwrap()
            .field(output)
            .unwrap(),
        components::schema::FieldValue::Dynamic(DynamicValue::U32(85))
    );
    attach(
        &mut world,
        entity,
        entity,
        prop(output),
        1,
        &[("x", prop(output))],
    );
    assert_eq!(
        state(&world, entity),
        ExpressionDriverState::Retained(ExpressionDriverReason::Cycle)
    );
}

#[test]
fn division_failure_explicit_fallback_and_recovery() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let source = scalar(&mut world, 2.0);
    let target = scalar(&mut world, -1.0);
    let fallback = scalar(&mut world, -1.0);
    let nodes = vec![
        ExpressionNode::Constant(DynamicValue::F32(20.0)),
        ExpressionNode::Input(0),
        ExpressionNode::Binary {
            operator: BinaryOperator::Divide,
            left: 0,
            right: 1,
        },
    ];
    upload(
        &mut world,
        1,
        expression(&[("x", DynamicPropertyKind::F32)], nodes.clone()),
    );
    let mut fallback_nodes = nodes;
    fallback_nodes.extend([
        ExpressionNode::Constant(DynamicValue::F32(7.0)),
        ExpressionNode::Fallback {
            value: 2,
            replacement: 3,
        },
    ]);
    upload(
        &mut world,
        2,
        expression(&[("x", DynamicPropertyKind::F32)], fallback_nodes),
    );
    attach(
        &mut world,
        target,
        source,
        scalar_property(),
        1,
        &[("x", scalar_property())],
    );
    attach(
        &mut world,
        fallback,
        source,
        scalar_property(),
        2,
        &[("x", scalar_property())],
    );
    assert_eq!(value(&world, target), 10.0);
    set(&mut world, source, scalar_property(), FieldValue::F32(0.0));
    assert_eq!(value(&world, target), 10.0);
    assert_eq!(value(&world, fallback), 7.0);
    assert_eq!(
        state(&world, target),
        ExpressionDriverState::Retained(ExpressionDriverReason::Calculation)
    );
    let before = world.expression_driver_status(target);
    assert_eq!(world.expression_driver_status(target), before);
    set(&mut world, source, scalar_property(), FieldValue::F32(4.0));
    assert_eq!(value(&world, target), 5.0);
    assert!(world.expression_driver_status(target).unwrap().recovered);
}

#[test]
fn replacements_and_generation_reuse_require_explicit_rebinding() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let source = scalar(&mut world, 6.0);
    let target = scalar(&mut world, 0.0);
    upload(
        &mut world,
        1,
        expression(
            &[("x", DynamicPropertyKind::F32)],
            vec![ExpressionNode::Input(0)],
        ),
    );
    attach(
        &mut world,
        target,
        source,
        scalar_property(),
        1,
        &[("x", scalar_property())],
    );
    assert_eq!(value(&world, target), 6.0);
    assert!(
        submit(
            &mut world,
            vec![
                Command::RemoveComponent {
                    entity: EntityRef::Handle(source),
                    component: ComponentValue::SCALAR
                },
                Command::insert_value(
                    EntityRef::Handle(source),
                    ComponentValue::Scalar(Scalar {
                        value: 99.0
                    })
                )
            ]
        )
        .result
        .is_ok()
    );
    assert_eq!(value(&world, target), 6.0);
    assert_eq!(
        state(&world, target),
        ExpressionDriverState::Retained(ExpressionDriverReason::MissingInput {
            slot: 0
        })
    );
    set(
        &mut world,
        target,
        DriverProperty {
            component: ComponentValue::EXPRESSION_DRIVER,
            offset: offset_of!(ExpressionDriver, source) as u32,
        },
        FieldValue::Entity(EntityRef::Handle(source)),
    );
    assert_eq!(value(&world, target), 99.0);
    assert!(
        submit(
            &mut world,
            vec![
                Command::RemoveComponent {
                    entity: EntityRef::Handle(target),
                    component: ComponentValue::SCALAR
                },
                Command::insert_value(
                    EntityRef::Handle(target),
                    ComponentValue::Scalar(Scalar {
                        value: 123.0
                    })
                )
            ]
        )
        .result
        .is_ok()
    );
    assert_eq!(value(&world, target), 123.0);
    assert_eq!(
        state(&world, target),
        ExpressionDriverState::Retained(ExpressionDriverReason::TargetUnavailable)
    );
    attach(
        &mut world,
        target,
        source,
        scalar_property(),
        1,
        &[("x", scalar_property())],
    );
    assert_eq!(value(&world, target), 99.0);
    assert!(
        submit(
            &mut world,
            vec![Command::Delete {
                entity: EntityRef::Handle(source)
            }]
        )
        .result
        .is_ok()
    );
    let replacement = scalar(&mut world, 444.0);
    assert_eq!(replacement.index(), source.index());
    assert_ne!(replacement, source);
    assert_eq!(value(&world, target), 99.0);
    attach(
        &mut world,
        target,
        replacement,
        scalar_property(),
        1,
        &[("x", scalar_property())],
    );
    assert_eq!(value(&world, target), 444.0);
}

#[test]
fn malformed_mapping_and_mapping_or_type_refusals_are_observable() {
    let inputs = [ExpressionDriverInput {
        name: "x".into(),
        property: scalar_property(),
    }];
    let bytes = encode_expression_driver_inputs(&inputs).unwrap();
    assert_eq!(decode_expression_driver_inputs(&bytes).unwrap(), inputs);
    for end in 1..bytes.len() {
        assert!(decode_expression_driver_inputs(&bytes[..end]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode_expression_driver_inputs(&trailing).is_err());
    assert!(encode_expression_driver_inputs(&[inputs[0].clone(), inputs[0].clone()]).is_err());
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let source = scalar(&mut world, 2.0);
    let target = scalar(&mut world, 3.0);
    upload(
        &mut world,
        1,
        expression(
            &[("x", DynamicPropertyKind::U32)],
            vec![ExpressionNode::Input(0)],
        ),
    );
    // Target wrong type is retained; the value is not converted or zeroed.
    attach(
        &mut world,
        target,
        source,
        scalar_property(),
        1,
        &[("x", scalar_property())],
    );
    assert_eq!(
        state(&world, target),
        ExpressionDriverState::Retained(ExpressionDriverReason::TargetUnavailable)
    );
    assert_eq!(value(&world, target), 3.0);
    upload(
        &mut world,
        3,
        expression(
            &[("x", DynamicPropertyKind::Bool)],
            vec![
                ExpressionNode::Input(0),
                ExpressionNode::Constant(DynamicValue::F32(1.0)),
                ExpressionNode::Constant(DynamicValue::F32(2.0)),
                ExpressionNode::Ternary {
                    condition: 0,
                    then_node: 1,
                    else_node: 2,
                },
            ],
        ),
    );
    attach(
        &mut world,
        target,
        source,
        scalar_property(),
        3,
        &[("x", scalar_property())],
    );
    assert_eq!(
        state(&world, target),
        ExpressionDriverState::Retained(ExpressionDriverReason::InputType {
            slot: 0
        })
    );
    upload(
        &mut world,
        2,
        expression(
            &[("x", DynamicPropertyKind::F32)],
            vec![ExpressionNode::Input(0)],
        ),
    );
    attach(
        &mut world,
        target,
        source,
        scalar_property(),
        2,
        &[("other", scalar_property())],
    );
    assert_eq!(
        state(&world, target),
        ExpressionDriverState::Retained(ExpressionDriverReason::InputMapping)
    );
}

#[test]
fn dynamic_growth_removal_retyping_and_target_replacement_preserve_pins() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let mut material = CustomMaterial::default();
    let input = material
        .properties
        .set("input", DynamicValue::F32(4.0))
        .unwrap();
    let source = create(&mut world, ComponentValue::CustomMaterial(material));
    let target = scalar(&mut world, 0.0);
    upload(
        &mut world,
        1,
        expression(
            &[("x", DynamicPropertyKind::F32)],
            vec![ExpressionNode::Input(0)],
        ),
    );
    let prop = |offset| DriverProperty {
        component: ComponentValue::CUSTOM_MATERIAL,
        offset,
    };
    attach(
        &mut world,
        target,
        source,
        scalar_property(),
        1,
        &[("x", prop(input))],
    );
    let operations = (0..96)
        .map(|i| Command::SetDynamicProperty {
            entity: EntityRef::Handle(source),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: format!("extra_{i}"),
            value: DynamicValue::Mat4([i as f32; 16]),
        })
        .collect();
    assert!(submit(&mut world, operations).result.is_ok());
    set(
        &mut world,
        source,
        prop(input),
        FieldValue::Dynamic(DynamicValue::F32(9.0)),
    );
    assert_eq!(value(&world, target), 9.0);
    assert!(
        submit(
            &mut world,
            vec![
                Command::RemoveDynamicProperty {
                    entity: EntityRef::Handle(source),
                    component: ComponentValue::CUSTOM_MATERIAL,
                    name: "input".into()
                },
                Command::SetDynamicProperty {
                    entity: EntityRef::Handle(source),
                    component: ComponentValue::CUSTOM_MATERIAL,
                    name: "input".into(),
                    value: DynamicValue::U32(11)
                }
            ]
        )
        .result
        .is_ok()
    );
    assert_eq!(value(&world, target), 9.0);
    assert_eq!(
        state(&world, target),
        ExpressionDriverState::Retained(ExpressionDriverReason::MissingInput {
            slot: 0
        })
    );
    assert!(
        submit(
            &mut world,
            vec![Command::SetDynamicProperty {
                entity: EntityRef::Handle(source),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: "input".into(),
                value: DynamicValue::F32(12.0)
            }]
        )
        .result
        .is_ok()
    );
    assert_eq!(value(&world, target), 9.0);
    let new_key = world
        .inspect(source)
        .unwrap()
        .components
        .into_iter()
        .find_map(|v| {
            if let ComponentValue::CustomMaterial(value) = v {
                value.properties.key("input")
            } else {
                None
            }
        })
        .unwrap();
    assert_ne!(new_key, input);
    attach(
        &mut world,
        target,
        source,
        scalar_property(),
        1,
        &[("x", prop(new_key))],
    );
    assert_eq!(value(&world, target), 12.0);
    // A dynamic target itself also invalidates without writing another property.
    let mut material = CustomMaterial::default();
    let output = material
        .properties
        .set("output", DynamicValue::F32(0.0))
        .unwrap();
    let destination = create(&mut world, ComponentValue::CustomMaterial(material));
    attach(
        &mut world,
        destination,
        target,
        prop(output),
        1,
        &[("x", scalar_property())],
    );
    assert!(
        submit(
            &mut world,
            vec![
                Command::RemoveDynamicProperty {
                    entity: EntityRef::Handle(destination),
                    component: ComponentValue::CUSTOM_MATERIAL,
                    name: "output".into()
                },
                Command::SetDynamicProperty {
                    entity: EntityRef::Handle(destination),
                    component: ComponentValue::CUSTOM_MATERIAL,
                    name: "output".into(),
                    value: DynamicValue::F32(77.0)
                }
            ]
        )
        .result
        .is_ok()
    );
    assert_eq!(
        state(&world, destination),
        ExpressionDriverState::Retained(ExpressionDriverReason::TargetUnavailable)
    );
    let component = world
        .inspect(destination)
        .unwrap()
        .components
        .into_iter()
        .find_map(|v| {
            if let ComponentValue::CustomMaterial(value) = v {
                Some(value)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        component.properties.get("output"),
        Some(DynamicValue::F32(77.0))
    );
}

fn animate(
    world: &mut WorldContext<'_>,
    target: EntityId,
    property: DriverProperty,
    end: AnimationValue,
) -> AnimationControllerId {
    let start = match &end {
        AnimationValue::Field(components::schema::FieldValue::Dynamic(_)) => AnimationValue::Field(
            components::schema::FieldValue::Dynamic(DynamicValue::F32(0.0)),
        ),
        _ => AnimationValue::Field(components::schema::FieldValue::F32(0.0)),
    };
    let track_target = AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: property.component,
        offsets: vec![property.offset],
    });
    let clip = AnimationClip::new(
        2.0,
        vec![AnimationTrack {
            target: track_target.clone(),
            keys: vec![
                AnimationKeyframe {
                    time: 0.0,
                    value: start,
                    interpolation: AnimationInterpolation::Linear,
                },
                AnimationKeyframe {
                    time: 2.0,
                    value: end,
                    interpolation: AnimationInterpolation::Step,
                },
            ],
        }],
    )
    .unwrap();
    world
        .enqueue_asset(AssetUpload {
            id: 100,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 100,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    world.await_upload_for_test();
    world
        .create_animation_controller(AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: "asset://10/100".into(),
                variant: 0,
                track: 0,
                target,
                property: track_target,
                entity_bindings: Vec::new(),
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            }],
            ..Default::default()
        })
        .unwrap()
}

#[test]
fn rejected_destination_does_not_clear_animation_contribution() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let source = scalar(&mut world, 0.25);
    let target = create(
        &mut world,
        ComponentValue::CustomMaterial(CustomMaterial::default()),
    );
    let property = DriverProperty {
        component: ComponentValue::CUSTOM_MATERIAL,
        offset: offset_of!(CustomMaterial, alpha_cutoff) as u32,
    };
    upload(
        &mut world,
        1,
        expression(
            &[("x", DynamicPropertyKind::F32)],
            vec![ExpressionNode::Input(0)],
        ),
    );
    attach(
        &mut world,
        target,
        source,
        property,
        1,
        &[("x", scalar_property())],
    );
    let controller = animate(
        &mut world,
        target,
        property,
        AnimationValue::Field(components::schema::FieldValue::F32(0.4)),
    );
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Seek(1.0))
        .unwrap();
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Pause)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    let cutoff = |world: &WorldContext<'_>| {
        world
            .inspect(target)
            .unwrap()
            .components
            .into_iter()
            .find_map(|v| {
                if let ComponentValue::CustomMaterial(value) = v {
                    Some(value.alpha_cutoff)
                } else {
                    None
                }
            })
            .unwrap()
    };
    assert!(
        (cutoff(&world) - 0.45).abs() < 1e-6,
        "cutoff={} controller={:?}",
        cutoff(&world),
        world.animation_controller(controller)
    );
    set(&mut world, source, scalar_property(), FieldValue::F32(2.0));
    assert_eq!(
        state(&world, target),
        ExpressionDriverState::Retained(ExpressionDriverReason::TargetRejected(
            ErrorReason::InvalidValue
        ))
    );
    for _ in 0..4 {
        world.update_for_test(0.0).unwrap();
        assert!((cutoff(&world) - 0.45).abs() < 1e-6);
    }
    set(&mut world, source, scalar_property(), FieldValue::F32(0.5));
    assert!((cutoff(&world) - 0.7).abs() < 1e-6);
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(cutoff(&world), 0.5);
}

#[test]
fn invalid_scalar_input_keeps_destination_and_reports_invalid_input() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let source = scalar(&mut world, 1.0);
    let driver = scalar(&mut world, 0.0);
    let target = scalar(&mut world, 0.0);
    assert!(
        submit(
            &mut world,
            vec![Command::insert_value(
                EntityRef::Handle(driver),
                ComponentValue::LinearDriver(LinearDriver {
                    source,
                    scale: f32::MAX,
                    bias: 0.0
                })
            )]
        )
        .result
        .is_ok()
    );
    upload(
        &mut world,
        1,
        expression(
            &[("x", DynamicPropertyKind::F32)],
            vec![ExpressionNode::Input(0)],
        ),
    );
    attach(
        &mut world,
        target,
        driver,
        scalar_property(),
        1,
        &[("x", scalar_property())],
    );
    assert_eq!(value(&world, target), f32::MAX);
    set(&mut world, source, scalar_property(), FieldValue::F32(2.0));
    assert!(value(&world, driver).is_infinite());
    assert_eq!(value(&world, target), f32::MAX);
    assert_eq!(
        state(&world, target),
        ExpressionDriverState::Retained(ExpressionDriverReason::InvalidInput {
            slot: 0
        })
    );
}

#[test]
fn unload_and_recovery_drop_runtime_access_and_rebuild_without_reauthoring() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let source = scalar(&mut world, 4.0);
    let target = scalar(&mut world, 0.0);
    upload(
        &mut world,
        1,
        expression(
            &[("x", DynamicPropertyKind::F32)],
            vec![ExpressionNode::Input(0)],
        ),
    );
    attach(
        &mut world,
        target,
        source,
        scalar_property(),
        1,
        &[("x", scalar_property())],
    );
    let key = world
        .resolve_asset_key(AssetUploadIdentity {
            kind: EXPRESSION_TYPE,
            asset: 1,
            variant: 0,
        })
        .unwrap();
    world.asset_resources_mut().unload(key);
    drop(world);
    host.flush_resource_lifecycle();
    let mut world = host.world_mut(id).unwrap();
    assert_eq!(
        world.expression_driver_status(target).unwrap().availability,
        ExpressionDriverAvailability::Unavailable
    );
    assert!(
        world
            .asset_resources()
            .get_typed::<ipp_core::services::asset_management::expression::ExpressionAsset>(key)
            .is_none()
    );
    world.step(0.0).unwrap();
    assert_eq!(value(&world, target), 4.0);
    let tick = world.tick();
    drop(world);
    for _ in 0..16 {
        host.progress_assets();
        if host
            .world_mut(id)
            .unwrap()
            .expression_driver_status(target)
            .unwrap()
            .availability
            == ExpressionDriverAvailability::Ready
        {
            break;
        }
    }
    let mut world = host.world_mut(id).unwrap();
    let observation = world.expression_driver_status(target).unwrap();
    assert_eq!(
        observation.availability,
        ExpressionDriverAvailability::Ready
    );
    assert_eq!(
        observation.state,
        ExpressionDriverState::Retained(ExpressionDriverReason::AssetUnavailable)
    );
    assert_eq!(world.expression_driver_status(target), Some(observation));
    assert_eq!(world.tick(), tick);
    assert_eq!(value(&world, target), 4.0);
    world.step(0.0).unwrap();
    assert_eq!(state(&world, target), ExpressionDriverState::Written);
    set(&mut world, source, scalar_property(), FieldValue::F32(8.0));
    assert_eq!(value(&world, target), 8.0);
}

#[test]
fn snapshot_remaps_source_and_reconstructs_plan_and_dynamic_metadata() {
    use ipp_core::services::world_serialization::WorldLoadOptions;
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let retired = scalar(&mut world, 1.0);
    let source = scalar(&mut world, 13.0);
    assert!(
        submit(
            &mut world,
            vec![Command::Delete {
                entity: EntityRef::Handle(retired)
            }]
        )
        .result
        .is_ok()
    );
    let mut material = CustomMaterial::default();
    let output = material
        .properties
        .set("output", DynamicValue::F32(0.0))
        .unwrap();
    let target = create(&mut world, ComponentValue::CustomMaterial(material));
    upload(
        &mut world,
        1,
        expression(
            &[("x", DynamicPropertyKind::F32)],
            vec![
                ExpressionNode::Input(0),
                ExpressionNode::Constant(DynamicValue::F32(7.0)),
                ExpressionNode::Binary {
                    operator: BinaryOperator::Add,
                    left: 0,
                    right: 1,
                },
            ],
        ),
    );
    attach(
        &mut world,
        target,
        source,
        DriverProperty {
            component: ComponentValue::CUSTOM_MATERIAL,
            offset: output,
        },
        1,
        &[("x", scalar_property())],
    );
    drop(world);
    let memory = ipp_core::services::io::MemoryIoSource::default();
    memory
        .insert(
            "expression:snapshot".into(),
            expression(
                &[("x", DynamicPropertyKind::F32)],
                vec![
                    ExpressionNode::Input(0),
                    ExpressionNode::Constant(DynamicValue::F32(7.0)),
                    ExpressionNode::Binary {
                        operator: BinaryOperator::Add,
                        left: 0,
                        right: 1,
                    },
                ],
            )
            .encode()
            .unwrap(),
        )
        .unwrap();
    host.io_mut().register("expression:", memory).unwrap();
    let mut world = host.world_mut(id).unwrap();
    set(
        &mut world,
        target,
        DriverProperty {
            component: ComponentValue::EXPRESSION_DRIVER,
            offset: offset_of!(ExpressionDriver, expression_source) as u32,
        },
        FieldValue::String("expression:snapshot".into()),
    );
    for _ in 0..8 {
        world.update_for_test(0.0).unwrap();
    }
    drop(world);
    let bytes = host.save_world(id, 123, Default::default()).unwrap();
    let restored = host
        .load_world(
            &bytes,
            123,
            WorldLoadOptions {
                symbolic_id: Some("copy".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        )
        .unwrap()
        .root
        .id();
    for _ in 0..8 {
        host.update_world_for_test(restored, 0.0).unwrap();
    }
    let mut world = host.world_mut(restored).unwrap();
    let entities = world.entities();
    let new_target = entities
        .iter()
        .find(|entity| {
            entity
                .components
                .iter()
                .any(|value| value.type_id() == ComponentValue::EXPRESSION_DRIVER)
        })
        .unwrap()
        .id;
    let component = world
        .inspect(new_target)
        .unwrap()
        .components
        .into_iter()
        .find_map(|v| {
            if let ComponentValue::ExpressionDriver(driver) = v {
                Some(driver)
            } else {
                None
            }
        })
        .unwrap();
    assert_ne!(component.source, source);
    assert!(entities.iter().any(|entity| entity.id == component.source));
    assert_eq!(
        state(&world, new_target),
        ExpressionDriverState::Written,
        "driver={component:?} resources={:?}",
        world.resource_snapshots()
    );
    set(
        &mut world,
        component.source,
        scalar_property(),
        FieldValue::F32(23.0),
    );
    let material = world
        .inspect(new_target)
        .unwrap()
        .components
        .into_iter()
        .find_map(|v| {
            if let ComponentValue::CustomMaterial(material) = v {
                Some(material)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        material.properties.get("output"),
        Some(DynamicValue::F32(30.0))
    );
}

#[test]
fn expression_parameter_write_precedes_linear_driver_and_participates_in_cycles() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let source = scalar(&mut world, 5.0);
    let target = scalar(&mut world, 0.0);
    assert!(
        submit(
            &mut world,
            vec![Command::insert_value(
                EntityRef::Handle(target),
                ComponentValue::LinearDriver(LinearDriver {
                    source,
                    ..Default::default()
                })
            )]
        )
        .result
        .is_ok()
    );
    upload(
        &mut world,
        1,
        expression(
            &[("x", DynamicPropertyKind::F32)],
            vec![ExpressionNode::Input(0)],
        ),
    );
    let scale = DriverProperty {
        component: ComponentValue::LINEAR_DRIVER,
        offset: offset_of!(LinearDriver, scale) as u32,
    };
    attach(
        &mut world,
        target,
        source,
        scale,
        1,
        &[("x", scalar_property())],
    );
    assert_eq!(value(&world, target), 25.0);
    attach(
        &mut world,
        target,
        target,
        scale,
        1,
        &[("x", scalar_property())],
    );
    assert_eq!(
        state(&world, target),
        ExpressionDriverState::Retained(ExpressionDriverReason::Cycle)
    );
    assert_eq!(value(&world, target), 25.0);
    attach(
        &mut world,
        target,
        source,
        scale,
        1,
        &[("x", scalar_property())],
    );
    set(&mut world, source, scalar_property(), FieldValue::F32(7.0));
    assert_eq!(value(&world, target), 49.0);
}

#[test]
fn fixed_transform_destination_uses_shared_numeric_guard() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let source = scalar(&mut world, 3.0);
    let target = create(
        &mut world,
        ComponentValue::Transform(components::Transform::default()),
    );
    upload(
        &mut world,
        1,
        expression(
            &[("x", DynamicPropertyKind::F32)],
            vec![ExpressionNode::Input(0)],
        ),
    );
    attach(
        &mut world,
        target,
        source,
        DriverProperty {
            component: ComponentValue::TRANSFORM,
            offset: offset_of!(components::Transform, sx) as u32,
        },
        1,
        &[("x", scalar_property())],
    );
    let scale = |world: &WorldContext<'_>| {
        world
            .inspect(target)
            .unwrap()
            .components
            .into_iter()
            .find_map(|v| {
                if let ComponentValue::Transform(value) = v {
                    Some(value.sx)
                } else {
                    None
                }
            })
            .unwrap()
    };
    assert_eq!(scale(&world), 3.0);
    set(&mut world, source, scalar_property(), FieldValue::F32(-1.0));
    assert_eq!(scale(&world), 3.0);
    assert_eq!(
        state(&world, target),
        ExpressionDriverState::Retained(ExpressionDriverReason::TargetRejected(
            ErrorReason::InvalidValue
        ))
    );
}

#[test]
fn compiled_dynamic_animation_rebuilds_after_metadata_compacts_storage() {
    let (mut host, id) = host();
    let mut world = host.world_mut(id).unwrap();
    let mut material = CustomMaterial::default();
    material
        .properties
        .set("gap", DynamicValue::Mat4([1.0; 16]))
        .unwrap();
    let parameter = material
        .properties
        .set("foo_parameter", DynamicValue::F32(2.0))
        .unwrap();
    material.properties.remove("gap");
    let components::schema::FieldValue::Bytes(metadata) = material
        .properties
        .field(components::dynamic_properties::DYNAMIC_METADATA)
        .unwrap()
    else {
        unreachable!()
    };
    let target = create(&mut world, ComponentValue::CustomMaterial(material));
    let property = DriverProperty {
        component: ComponentValue::CUSTOM_MATERIAL,
        offset: parameter,
    };
    let controller = animate(
        &mut world,
        target,
        property,
        AnimationValue::Field(components::schema::FieldValue::Dynamic(DynamicValue::F32(
            2.0,
        ))),
    );
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Seek(1.0))
        .unwrap();
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Pause)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    let read = |world: &WorldContext<'_>| {
        world
            .inspect(target)
            .unwrap()
            .components
            .into_iter()
            .find_map(|v| {
                if let ComponentValue::CustomMaterial(value) = v {
                    value.properties.get("foo_parameter")
                } else {
                    None
                }
            })
            .unwrap()
    };
    assert_eq!(read(&world), DynamicValue::F32(3.0));
    assert!(
        submit(
            &mut world,
            vec![Command::InsertComponent {
                entity: EntityRef::Handle(target),
                component: ComponentValue::CUSTOM_MATERIAL,
                adopt: true,
                fields: vec![
                    FieldWrite {
                        offset: components::dynamic_properties::DYNAMIC_METADATA,
                        value: FieldValue::Bytes(metadata)
                    },
                    FieldWrite {
                        offset: parameter,
                        value: FieldValue::Dynamic(DynamicValue::F32(3.0))
                    }
                ]
            }]
        )
        .result
        .is_ok()
    );
    assert_eq!(read(&world), DynamicValue::F32(3.0));
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(read(&world), DynamicValue::F32(2.0));
}
