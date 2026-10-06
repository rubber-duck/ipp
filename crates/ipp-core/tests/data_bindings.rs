//! Real local Host, assets and Data Service evidence, independent of transports.

mod support;
use support::task_scheduler::HostTaskTestDriver;

use ipp_core::{
    Batch, Command, ComponentValue as C, DynamicPropertyKind as K, DynamicValue as V, EntityId,
    EntityMetadata, EntityRef, ErrorReason, FieldValue, FieldWrite, HostRuntime, WorldId,
    WorldLimits,
    expressions::{
        BinaryOperator, ExpressionDeclaration, ExpressionInput, ExpressionInvalid,
        ExpressionNode as N, ExpressionResult as R,
    },
    services::{
        asset_management::{AssetSource, formats::expression::EXPRESSION_TYPE},
        data::*,
    },
    systems::{SystemId, data_bindings::*},
};

fn world(host: &mut HostRuntime) -> WorldId {
    host.create_world(
        WorldLimits::default(),
        &[DataBindingSystem::ID, SystemId("ipp.asset-dependencies")],
    )
    .unwrap()
}

fn definition(inputs: &[(&str, K)], nodes: Vec<N>, output: usize) -> ExpressionDeclaration {
    ExpressionDeclaration {
        inputs: inputs
            .iter()
            .map(|(name, kind)| ExpressionInput {
                name: (*name).into(),
                kind: *kind,
            })
            .collect(),
        nodes,
        output,
    }
}

fn asset(
    host: &mut HostRuntime,
    world: WorldId,
    id: u64,
    declaration: ExpressionDeclaration,
) -> AssetSource {
    host.asset_resources_mut()
        .register_client_source(
            world,
            AssetSource {
                kind: EXPRESSION_TYPE,
                uri: format!("producer://{}/19/{id}", world.0).into(),
                variant: 0,
            },
            declaration.encode().unwrap(),
        )
        .unwrap();
    AssetSource {
        kind: EXPRESSION_TYPE,
        uri: format!("asset://19/{id}").into(),
        variant: 0,
    }
}

fn identity(host: &mut HostRuntime, world: WorldId, id: u64, column: &str, kind: K) -> AssetSource {
    asset(
        host,
        world,
        id,
        definition(&[(&format!("column:{column}"), kind)], vec![N::Input(0)], 0),
    )
}

fn apply(
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
    host.frame_for_test(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
}

fn create(host: &mut HostRuntime, world: WorldId, name: &str, value: C) -> EntityId {
    apply(
        host,
        world,
        vec![
            Command::Create {
                alias: 0,
                metadata: EntityMetadata {
                    symbolic_id: Some(name.into()),
                    ..Default::default()
                },
                adopt: false,
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value: Box::new(value),
            },
        ],
    )
    .result
    .unwrap();
    host.world_mut(world).unwrap().lookup_id(name).unwrap()
}

fn buffer(source: &str, output: &str, asset: AssetSource) -> BufferDataSourceBinding {
    let mut value = BufferDataSourceBinding {
        source: source.into(),
        ..Default::default()
    };
    value.properties.set(output, V::Asset(asset)).unwrap();
    value
}

fn stream(
    source: &str,
    output: &str,
    asset: AssetSource,
    windows: &[DataWindow],
) -> StreamingDataSourceBinding {
    let mut value = StreamingDataSourceBinding {
        source: source.into(),
        ..Default::default()
    };
    value.properties.set(output, V::Asset(asset)).unwrap();
    value.set_windows(windows).unwrap();
    value
}

fn producer(
    host: &mut HostRuntime,
    name: &str,
    kind: DataSourceKind,
    columns: Vec<DataColumn>,
) -> DataProducerHandle {
    host.data_sources_mut()
        .create_source(
            name.into(),
            kind,
            DataSchema {
                columns,
            },
        )
        .unwrap()
}

fn append(host: &mut HostRuntime, producer: DataProducerHandle, rows: Vec<Vec<V>>) {
    host.data_sources_mut()
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows,
            }],
        )
        .unwrap();
}

fn view(host: &mut HostRuntime, world: WorldId, entity: EntityId) -> DataBindingViewResult {
    host.world_mut(world)
        .unwrap()
        .data_binding_view_owned(entity, Default::default())
        .unwrap()
}

fn set_binding_property(entity: EntityId, name: &str, value: V) -> Command {
    Command::SetDynamicProperty {
        entity: EntityRef::Handle(entity),
        component: C::BUFFER_DATA_SOURCE_BINDING,
        name: name.into(),
        value,
    }
}

#[test]
fn interpolation_uses_host_delta_retargets_and_stops_after_exact_settlement() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:interp",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::F32)],
    );
    append(&mut host, source, vec![vec![V::F32(2.0)]]);
    let expression = identity(&mut host, world, 1, "x", K::F32);
    let mut binding = buffer("dataset:interp", "x", expression);
    binding.properties.set("x_interp", V::F32(4.0)).unwrap();
    let entity = create(
        &mut host,
        world,
        "interp",
        C::BufferDataSourceBinding(binding),
    );
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(2.0))]
    );

    host.data_sources_mut()
        .apply_batch(
            source,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: vec![V::F32(12.0)],
            }],
        )
        .unwrap();
    host.frame_for_test(0.5).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(4.0))]
    );
    host.frame_for_test(0.25).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(5.0))]
    );
    assert_eq!(
        host.data_sources()
            .read_source(source.source())
            .unwrap()
            .rows()
            .next()
            .unwrap()
            .values,
        [V::F32(12.0)]
    );

    host.data_sources_mut()
        .apply_batch(
            source,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: vec![V::F32(-3.0)],
            }],
        )
        .unwrap();
    host.frame_for_test(0.25).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(4.0))]
    );
    apply(
        &mut host,
        world,
        vec![set_binding_property(entity, "x_interp", V::F32(2.0))],
    )
    .result
    .unwrap();
    host.frame_for_test(0.5).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(3.0))]
    );
    host.frame_for_test(10.0).unwrap();
    let settled = view(&mut host, world, entity);
    assert_eq!(settled.columns[0].values, [R::Valid(V::F32(-3.0))]);
    #[cfg(feature = "instrumentation")]
    host.world_mut(world)
        .unwrap()
        .acknowledge_data_binding_for_test(entity, settled.binding_incarnation)
        .unwrap();
    host.frame_for_test(1.0).unwrap();
    let idle = view(&mut host, world, entity);
    assert_eq!(idle.evaluated_tick, settled.evaluated_tick);
    assert_eq!(idle.dirty, !cfg!(feature = "instrumentation"));

    host.data_sources_mut()
        .apply_batch(
            source,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: vec![V::F32(100.0)],
            }],
        )
        .unwrap();
    host.frame_for_test(0.5).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(-2.0))]
    );
    apply(
        &mut host,
        world,
        vec![Command::RemoveDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: C::BUFFER_DATA_SOURCE_BINDING,
            name: "x_interp".into(),
        }],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(100.0))]
    );
}

#[test]
fn interpolation_preserves_only_live_row_identities_and_resets_definition_and_source() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:interp-rows",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::F32)],
    );
    append(
        &mut host,
        source,
        vec![vec![V::F32(0.0)], vec![V::F32(10.0)]],
    );
    let expression = identity(&mut host, world, 1, "x", K::F32);
    let mut binding = buffer("dataset:interp-rows", "x", expression);
    binding.properties.set("x_interp", V::F32(2.0)).unwrap();
    let entity = create(
        &mut host,
        world,
        "interp-rows",
        C::BufferDataSourceBinding(binding),
    );
    host.data_sources_mut()
        .apply_batch(
            source,
            [
                DataDelta::Edit {
                    row: DataRowId(1),
                    values: vec![V::F32(20.0)],
                },
                DataDelta::Edit {
                    row: DataRowId(2),
                    values: vec![V::F32(30.0)],
                },
                DataDelta::Insert {
                    index: 0,
                    rows: vec![vec![V::F32(50.0)]],
                },
            ],
        )
        .unwrap();
    host.frame_for_test(1.0).unwrap();
    let inserted = view(&mut host, world, entity);
    assert_eq!(inserted.row_ids, [DataRowId(3), DataRowId(1), DataRowId(2)]);
    assert_eq!(
        inserted.columns[0].values,
        [
            R::Valid(V::F32(50.0)),
            R::Valid(V::F32(2.0)),
            R::Valid(V::F32(12.0))
        ]
    );
    host.data_sources_mut()
        .apply_batch(
            source,
            [DataDelta::Remove {
                row: DataRowId(1),
            }],
        )
        .unwrap();
    host.frame_for_test(0.5).unwrap();
    let removed = view(&mut host, world, entity);
    assert_eq!(removed.row_ids, [DataRowId(3), DataRowId(2)]);
    assert_eq!(
        removed.columns[0].values,
        [R::Valid(V::F32(50.0)), R::Valid(V::F32(13.0))]
    );

    let replacement_definition = identity(&mut host, world, 2, "x", K::F32);
    apply(
        &mut host,
        world,
        vec![set_binding_property(
            entity,
            "x",
            V::Asset(replacement_definition),
        )],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(50.0)), R::Valid(V::F32(30.0))]
    );

    host.data_sources_mut().destroy_source(source).unwrap();
    let replacement = producer(
        &mut host,
        "dataset:interp-rows",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::F32)],
    );
    append(
        &mut host,
        replacement,
        vec![vec![V::F32(-50.0)], vec![V::F32(-30.0)]],
    );
    host.frame_for_test(0.25).unwrap();
    let replaced = view(&mut host, world, entity);
    assert_ne!(replaced.source, removed.source);
    assert_eq!(replaced.row_ids, [DataRowId(1), DataRowId(2)]);
    assert_eq!(
        replaced.columns[0].values,
        [R::Valid(V::F32(-50.0)), R::Valid(V::F32(-30.0))]
    );
}

#[test]
fn interpolation_moves_vector_lanes_independently_and_invalid_results_initialize_fresh() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:interp-vector",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::Vec3)],
    );
    append(&mut host, source, vec![vec![V::Vec3([0.0, 10.0, -2.0])]]);
    let expression = identity(&mut host, world, 1, "x", K::Vec3);
    let mut binding = buffer("dataset:interp-vector", "x", expression);
    binding.properties.set("x_interp", V::F32(4.0)).unwrap();
    let entity = create(
        &mut host,
        world,
        "interp-vector",
        C::BufferDataSourceBinding(binding),
    );
    host.data_sources_mut()
        .apply_batch(
            source,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: vec![V::Vec3([10.0, -10.0, -1.0])],
            }],
        )
        .unwrap();
    host.frame_for_test(0.5).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::Vec3([2.0, 8.0, -1.0]))]
    );

    let expression = asset(
        &mut host,
        world,
        2,
        definition(&[("parameter", K::F32)], vec![N::Input(0)], 0),
    );
    apply(
        &mut host,
        world,
        vec![set_binding_property(entity, "x", V::Asset(expression))],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Invalid(ExpressionInvalid::MissingInput {
            slot: 0
        })]
    );
    apply(
        &mut host,
        world,
        vec![set_binding_property(entity, "x_parameter", V::F32(30.0))],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(30.0))]
    );
    apply(
        &mut host,
        world,
        vec![set_binding_property(entity, "x_parameter", V::F32(100.0))],
    )
    .result
    .unwrap();
    host.frame_for_test(0.5).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(32.0))]
    );
}

#[test]
fn interpolation_speed_admission_rejects_invalid_values_and_reports_unsupported_outputs() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:interp-kind",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::U32)],
    );
    append(&mut host, source, vec![vec![V::U32(5)]]);
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let entity = create(
        &mut host,
        world,
        "interp-kind",
        C::BufferDataSourceBinding(buffer("dataset:interp-kind", "x", expression)),
    );
    for value in [
        V::F32(0.0),
        V::F32(-1.0),
        V::F32(f32::INFINITY),
        V::F32(f32::NAN),
        V::U32(2),
    ] {
        assert!(
            apply(
                &mut host,
                world,
                vec![set_binding_property(entity, "x_interp", value)]
            )
            .result
            .is_err()
        );
    }
    apply(
        &mut host,
        world,
        vec![set_binding_property(entity, "x_interp", V::F32(1.0))],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, world, entity).availability,
        DataBindingAvailability::Unavailable(DataBindingUnavailable::InputType {
            output: "x".into(),
            input: "x_interp".into()
        })
    );
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveDynamicProperty {
                entity: EntityRef::Handle(entity),
                component: C::BUFFER_DATA_SOURCE_BINDING,
                name: "x_interp".into(),
            },
            set_binding_property(entity, "x_interp_percent", V::F32(1.0)),
        ],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, world, entity).availability,
        DataBindingAvailability::Unavailable(DataBindingUnavailable::InputType {
            output: "x".into(),
            input: "x_interp_percent".into()
        })
    );
}

#[test]
fn interpolation_accumulates_sub_ulp_motion_across_unchanged_projection_and_row_reindexing() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:interp-small",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::F32)],
    );
    let start = 100_000_000.0f32;
    append(&mut host, source, vec![vec![V::F32(start)]]);
    let expression = identity(&mut host, world, 1, "x", K::F32);
    let mut binding = buffer("dataset:interp-small", "x", expression);
    binding.properties.set("x_interp", V::F32(1.0)).unwrap();
    let entity = create(
        &mut host,
        world,
        "interp-small",
        C::BufferDataSourceBinding(binding),
    );
    host.data_sources_mut()
        .apply_batch(
            source,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: vec![V::F32(start + 16.0)],
            }],
        )
        .unwrap();
    for frame in 1..=961 {
        if frame == 300 {
            host.data_sources_mut()
                .apply_batch(
                    source,
                    [DataDelta::Insert {
                        index: 0,
                        rows: vec![vec![V::F32(5.0)]],
                    }],
                )
                .unwrap();
        }
        if frame % 60 == 0 {
            apply(
                &mut host,
                world,
                vec![set_binding_property(
                    entity,
                    "unrelated_parameter",
                    V::F32(frame as f32),
                )],
            )
            .result
            .unwrap();
        }
        host.frame_for_test(1.0 / 60.0).unwrap();
        let page = view(&mut host, world, entity);
        let index = page
            .row_ids
            .iter()
            .position(|id| *id == DataRowId(1))
            .unwrap();
        let R::Valid(V::F32(displayed)) = page.columns[0].values[index] else {
            panic!("numeric output")
        };
        assert!(f64::from(displayed - start) <= f64::from(frame) / 60.0 + 1e-9);
    }
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(5.0)), R::Valid(V::F32(start + 16.0))]
    );
}

#[test]
fn interpolation_window_changes_keep_retained_rows_and_initialize_reappearing_rows() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:interp-window",
        DataSourceKind::Streaming,
        vec![DataColumn::new("x", K::F32)],
    );
    let expression = asset(
        &mut host,
        world,
        1,
        definition(
            &[("column:x", K::F32), ("parameter", K::F32)],
            vec![
                N::Input(0),
                N::Input(1),
                N::Binary {
                    operator: BinaryOperator::Add,
                    left: 0,
                    right: 1,
                },
            ],
            2,
        ),
    );
    let mut binding = stream(
        "dataset:interp-window",
        "x",
        expression.clone(),
        &[DataWindow::Count(2)],
    );
    binding.properties.set("x_parameter", V::F32(0.0)).unwrap();
    binding.properties.set("x_interp", V::F32(2.0)).unwrap();
    let entity = create(
        &mut host,
        world,
        "interp-window",
        C::StreamingDataSourceBinding(binding),
    );
    create(
        &mut host,
        world,
        "retention",
        C::StreamingDataSourceBinding(stream(
            "dataset:interp-window",
            "x",
            expression,
            &[DataWindow::Count(2)],
        )),
    );
    append(
        &mut host,
        source,
        vec![vec![V::F32(0.0)], vec![V::F32(10.0)]],
    );
    host.frame_for_test(0.0).unwrap();
    apply(
        &mut host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: C::STREAMING_DATA_SOURCE_BINDING,
            name: "x_parameter".into(),
            value: V::F32(20.0),
        }],
    )
    .result
    .unwrap();
    host.frame_for_test(0.5).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(1.0)), R::Valid(V::F32(11.0))]
    );
    apply(&mut host, world, vec![set_count(entity, 1)])
        .result
        .unwrap();
    host.frame_for_test(0.5).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(12.0))]
    );
    apply(&mut host, world, vec![set_count(entity, 2)])
        .result
        .unwrap();
    let widened = view(&mut host, world, entity);
    assert_eq!(widened.row_ids, [DataRowId(1), DataRowId(2)]);
    assert_eq!(
        widened.columns[0].values,
        [R::Valid(V::F32(20.0)), R::Valid(V::F32(12.0))]
    );
}

#[test]
fn interpolation_sub_ulp_progress_survives_same_side_retargets_and_other_lane_changes() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:interp-retarget",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::Vec3)],
    );
    let start = 100_000_000.0f32;
    append(&mut host, source, vec![vec![V::Vec3([start, start, 0.0])]]);
    let expression = identity(&mut host, world, 1, "x", K::Vec3);
    let mut binding = buffer("dataset:interp-retarget", "x", expression);
    binding.properties.set("x_interp", V::F32(1.0)).unwrap();
    let entity = create(
        &mut host,
        world,
        "interp-retarget",
        C::BufferDataSourceBinding(binding),
    );
    for frame in 1..=961 {
        host.data_sources_mut()
            .apply_batch(
                source,
                [DataDelta::Edit {
                    row: DataRowId(1),
                    values: vec![V::Vec3([
                        start
                            + if frame % 2 == 0 {
                                16.0
                            } else {
                                32.0
                            },
                        start + 16.0,
                        if frame % 2 == 0 {
                            100.0
                        } else {
                            -100.0
                        },
                    ])],
                }],
            )
            .unwrap();
        host.frame_for_test(1.0 / 60.0).unwrap();
    }
    let page = view(&mut host, world, entity);
    let R::Valid(V::Vec3(value)) = page.columns[0].values[0] else {
        panic!("vector output")
    };
    assert_eq!(&value[..2], &[start + 16.0, start + 16.0]);

    // A reversal must not spend accrued positive-direction movement backwards.
    host.data_sources_mut()
        .apply_batch(
            source,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: vec![V::Vec3([start + 32.0, start + 16.0, 0.0])],
            }],
        )
        .unwrap();
    host.frame_for_test(7.0).unwrap();
    host.data_sources_mut()
        .apply_batch(
            source,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: vec![V::Vec3([start, start + 16.0, 0.0])],
            }],
        )
        .unwrap();
    host.frame_for_test(1.0).unwrap();
    let reversed = view(&mut host, world, entity);
    let R::Valid(V::Vec3(value)) = reversed.columns[0].values[0] else {
        panic!("vector output")
    };
    assert_eq!(value[0], start + 16.0);
}

#[test]
fn exact_raw_identity_preserves_all_source_kinds_and_float_bits_after_edits() {
    fn bits(value: &V) -> Vec<u32> {
        let lanes: &[f32] = match value {
            V::F32(value) => std::slice::from_ref(value),
            V::Vec2(value) => value,
            V::Vec3(value) => value,
            V::Vec4(value) | V::Mat2(value) => value,
            V::Mat3(value) => value,
            V::Mat4(value) => value,
            _ => &[],
        };
        lanes.iter().map(|value| value.to_bits()).collect()
    }

    let mut host = crate::support::task_scheduler::host();
    let world = host
        .create_world(
            WorldLimits::default(),
            &[
                DataBindingSystem::ID,
                SystemId("ipp.asset-dependencies"),
                SystemId("ipp.constraints"),
                SystemId("ipp.hierarchy"),
            ],
        )
        .unwrap();
    let mut values = vec![
        V::F32(-0.0),
        V::I32(i32::MIN),
        V::U32(u32::MAX),
        V::Bool(true),
        V::Vec2([-0.0, 1.25]),
        V::Vec3([-0.0, 2.0, -3.0]),
        V::Vec4([-0.0, 2.0, -3.0, 4.0]),
        V::Mat2([-0.0, 1.0, 2.0, 3.0]),
        V::Mat3([-0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]),
        V::Mat4([
            -0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ]),
        V::Text("label α".into()),
    ];
    let source = producer(
        &mut host,
        "dataset:identity-kinds",
        DataSourceKind::Buffer,
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let name = format!("column{index}");
                if value.kind() == K::Text {
                    DataColumn::text(name, 64)
                } else {
                    DataColumn::new(name, value.kind())
                }
            })
            .collect(),
    );
    append(&mut host, source, vec![values.clone()]);
    let mut binding = BufferDataSourceBinding {
        source: "dataset:identity-kinds".into(),
        ..Default::default()
    };
    for (index, value) in values.iter().enumerate() {
        binding
            .properties
            .set(
                &format!("output{index}"),
                V::Asset(identity(
                    &mut host,
                    world,
                    index as u64 + 1,
                    &format!("column{index}"),
                    value.kind(),
                )),
            )
            .unwrap();
    }
    let entity = create(
        &mut host,
        world,
        "identities",
        C::BufferDataSourceBinding(binding),
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            C::Scalar(Default::default()),
        )],
    )
    .result
    .unwrap();
    let consumer = host
        .world_mut(world)
        .unwrap()
        .register_data_binding_presentation_consumer(entity, C::SCALAR)
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .finish_data_binding_presentation(consumer)
        .unwrap();
    for edit in [false, true] {
        if edit {
            for value in &mut values {
                match value {
                    V::F32(value) => *value = 0.0,
                    V::Vec2(value) => value[0] = 0.0,
                    V::Vec3(value) => value[0] = 0.0,
                    V::Vec4(value) | V::Mat2(value) => value[0] = 0.0,
                    V::Mat3(value) => value[0] = 0.0,
                    V::Mat4(value) => value[0] = 0.0,
                    _ => {}
                }
            }
            host.data_sources_mut()
                .apply_batch(
                    source,
                    [DataDelta::Edit {
                        row: DataRowId(1),
                        values: values.clone(),
                    }],
                )
                .unwrap();
            host.frame_for_test(0.0).unwrap();
        }
        let output = view(&mut host, world, entity);
        assert_eq!(output.row_ids, [DataRowId(1)]);
        assert!(
            !output.dirty,
            "equal values preserve the presentation handoff even when zero bits change"
        );
        for column in output.columns {
            let index: usize = column.name.strip_prefix("output").unwrap().parse().unwrap();
            let R::Valid(value) = &column.values[0] else {
                panic!("identity must be valid");
            };
            assert_eq!(value, &values[index]);
            assert_eq!(bits(value), bits(&values[index]));
        }
    }
    let mut malformed = values.clone();
    malformed[0] = V::F32(f32::NAN);
    let error = host
        .data_sources_mut()
        .apply_batch(
            source,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: malformed,
            }],
        )
        .unwrap_err();
    assert_eq!(error.reason, DataError::InvalidRow);
    host.frame_for_test(0.0).unwrap();
    assert_eq!(view(&mut host, world, entity).row_ids, [DataRowId(1)]);
}

#[test]
fn parameter_identity_and_general_identity_shaped_graph_keep_evaluator_semantics() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:identity-shapes",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::F32)],
    );
    append(&mut host, source, vec![vec![V::F32(-0.0)]]);
    let parameter = asset(
        &mut host,
        world,
        1,
        definition(&[("parameter", K::F32)], vec![N::Input(0)], 0),
    );
    let general = asset(
        &mut host,
        world,
        2,
        definition(
            &[("column:x", K::F32)],
            vec![
                N::Input(0),
                N::Constant(V::F32(0.0)),
                N::Binary {
                    operator: BinaryOperator::Add,
                    left: 0,
                    right: 1,
                },
            ],
            2,
        ),
    );
    let mut binding = buffer("dataset:identity-shapes", "parameter", parameter);
    binding
        .properties
        .set("general", V::Asset(general))
        .unwrap();
    let entity = create(
        &mut host,
        world,
        "shapes",
        C::BufferDataSourceBinding(binding),
    );
    let output = view(&mut host, world, entity);
    assert_eq!(
        output.columns[1].values,
        [R::Invalid(ExpressionInvalid::MissingInput {
            slot: 0
        })]
    );
    let R::Valid(V::F32(value)) = output.columns[0].values[0] else {
        panic!("general graph must calculate");
    };
    assert_eq!(value.to_bits(), 0.0_f32.to_bits());
    apply(
        &mut host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: C::BUFFER_DATA_SOURCE_BINDING,
            name: "parameter_parameter".into(),
            value: V::F32(-4.5),
        }],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[1].values,
        [R::Valid(V::F32(-4.5))]
    );
}

#[test]
fn buffer_projects_typed_values_and_preserves_row_identity_through_edits_and_insertions() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let producer = producer(
        &mut host,
        "dataset:buffer",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::F32)],
    );
    append(
        &mut host,
        producer,
        vec![vec![V::F32(2.0)], vec![V::F32(6.0)]],
    );
    let expression = asset(
        &mut host,
        world,
        1,
        definition(
            &[("column:x", K::F32), ("parameter", K::F32)],
            vec![
                N::Input(0),
                N::Input(1),
                N::Binary {
                    operator: BinaryOperator::Multiply,
                    left: 0,
                    right: 1,
                },
            ],
            2,
        ),
    );
    let mut value = buffer("dataset:buffer", "scaled", expression);
    value
        .properties
        .set("scaled_parameter", V::F32(3.0))
        .unwrap();
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(value),
    );
    let first = view(&mut host, world, entity);
    assert_eq!(first.availability, DataBindingAvailability::Ready);
    assert_eq!(first.row_ids, [DataRowId(1), DataRowId(2)]);
    assert_eq!(first.columns[0].name, "scaled");
    assert_eq!(first.columns[0].kind, K::F32);
    assert_eq!(
        first.columns[0].values,
        [R::Valid(V::F32(6.0)), R::Valid(V::F32(18.0))]
    );
    assert_eq!(first.evaluated_tick, Some(1));
    assert!(first.dirty);
    host.data_sources_mut()
        .apply_batch(
            producer,
            [
                DataDelta::Edit {
                    row: DataRowId(1),
                    values: vec![V::F32(4.0)],
                },
                DataDelta::Insert {
                    index: 1,
                    rows: vec![vec![V::F32(5.0)]],
                },
            ],
        )
        .unwrap();
    // Queries observe the last completed cut until the Host evaluates.
    assert_eq!(view(&mut host, world, entity), first);
    host.frame_for_test(0.0).unwrap();
    let second = view(&mut host, world, entity);
    assert_eq!(second.row_ids, [DataRowId(1), DataRowId(3), DataRowId(2)]);
    assert_eq!(
        second.columns[0].values,
        [
            R::Valid(V::F32(12.0)),
            R::Valid(V::F32(15.0)),
            R::Valid(V::F32(18.0))
        ]
    );
    assert_eq!(second.source, first.source);
    assert_eq!(second.binding_incarnation, first.binding_incarnation);
}

#[test]
fn explicit_invalidity_and_parameter_writes_use_shared_expression_assets() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let producer = producer(
        &mut host,
        "dataset:invalid",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::I32)],
    );
    append(&mut host, producer, vec![vec![V::I32(8)], vec![V::I32(0)]]);
    let expression = asset(
        &mut host,
        world,
        1,
        definition(
            &[("column:x", K::I32), ("parameter", K::I32)],
            vec![
                N::Input(0),
                N::Input(1),
                N::Binary {
                    operator: BinaryOperator::Divide,
                    left: 0,
                    right: 1,
                },
            ],
            2,
        ),
    );
    let mut value = buffer("dataset:invalid", "divided", expression);
    value
        .properties
        .set("divided_parameter", V::I32(0))
        .unwrap();
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(value),
    );
    let first = view(&mut host, world, entity);
    assert_eq!(first.availability, DataBindingAvailability::Ready);
    assert_eq!(
        first.columns[0].values,
        [
            R::Invalid(ExpressionInvalid::Calculation),
            R::Invalid(ExpressionInvalid::Calculation)
        ]
    );
    apply(
        &mut host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: C::BUFFER_DATA_SOURCE_BINDING,
            name: "divided_parameter".into(),
            value: V::I32(2),
        }],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::I32(4)), R::Valid(V::I32(0))]
    );
}

#[test]
fn stream_demand_precedes_outcome_and_union_release_is_exact_across_worlds() {
    let mut host = crate::support::task_scheduler::host();
    let left = world(&mut host);
    let right = world(&mut host);
    let producer = producer(
        &mut host,
        "dataset:stream",
        DataSourceKind::Streaming,
        vec![DataColumn::new("x", K::U32)],
    );
    let left_asset = identity(&mut host, left, 1, "x", K::U32);
    let right_asset = identity(&mut host, right, 1, "x", K::U32);
    let left_entity = create(
        &mut host,
        left,
        "left",
        C::StreamingDataSourceBinding(stream(
            "dataset:stream",
            "x",
            left_asset,
            &[DataWindow::Count(2)],
        )),
    );
    let right_entity = create(
        &mut host,
        right,
        "right",
        C::StreamingDataSourceBinding(stream(
            "dataset:stream",
            "x",
            right_asset.clone(),
            &[DataWindow::Count(4)],
        )),
    );
    append(
        &mut host,
        producer,
        (0..5).map(|n| vec![V::U32(n)]).collect(),
    );
    assert_eq!(
        host.data_sources()
            .read_source(producer.source())
            .unwrap()
            .len(),
        4
    );
    host.frame_for_test(0.0).unwrap();
    assert_eq!(
        view(&mut host, left, left_entity).row_ids,
        [DataRowId(4), DataRowId(5)]
    );
    assert_eq!(
        view(&mut host, right, right_entity).row_ids,
        [DataRowId(2), DataRowId(3), DataRowId(4), DataRowId(5)]
    );
    // The handoff preserves another World's demand and retires the replaced window.
    for (count, expected) in [(4, vec![2, 3, 4, 5]), (1, vec![5]), (4, vec![4, 5])] {
        apply(
            &mut host,
            right,
            vec![Command::insert_value(
                EntityRef::Handle(right_entity),
                C::StreamingDataSourceBinding(stream(
                    "dataset:stream",
                    "x",
                    right_asset.clone(),
                    &[DataWindow::Count(count)],
                )),
            )],
        )
        .result
        .unwrap();
        assert_eq!(
            view(&mut host, right, right_entity).row_ids,
            expected.into_iter().map(DataRowId).collect::<Vec<_>>()
        );
        assert_eq!(
            view(&mut host, left, left_entity).row_ids,
            [DataRowId(4), DataRowId(5)]
        );
    }
    host.destroy_world(right);
    assert_eq!(
        host.data_sources()
            .read_source(producer.source())
            .unwrap()
            .len(),
        2
    );
    apply(
        &mut host,
        left,
        vec![Command::Delete {
            entity: EntityRef::Handle(left_entity),
        }],
    )
    .result
    .unwrap();
    assert!(
        host.data_sources()
            .read_source(producer.source())
            .unwrap()
            .is_empty()
    );
}

fn set_count(entity: EntityId, count: usize) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component: C::STREAMING_DATA_SOURCE_BINDING,
        field: FieldWrite {
            offset: std::mem::offset_of!(StreamingDataSourceBinding, windows) as u32,
            value: FieldValue::Bytes(encode_data_windows(&[DataWindow::Count(count)]).unwrap()),
        },
    }
}

fn set_buffer_source(entity: EntityId, source: &str) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component: C::BUFFER_DATA_SOURCE_BINDING,
        field: FieldWrite {
            offset: std::mem::offset_of!(BufferDataSourceBinding, source) as u32,
            value: FieldValue::String(source.into()),
        },
    }
}

#[test]
fn retained_window_handoff_preserves_final_union_and_expires_after_failed_shrink() {
    for replace_incoming in [false, true] {
        let mut host = crate::support::task_scheduler::host();
        let world = world(&mut host);
        let source = producer(
            &mut host,
            "dataset:union",
            DataSourceKind::Streaming,
            vec![DataColumn::new("x", K::U32)],
        );
        let expression = identity(&mut host, world, 1, "x", K::U32);
        let a = create(
            &mut host,
            world,
            "a",
            C::StreamingDataSourceBinding(stream(
                "dataset:union",
                "x",
                expression.clone(),
                &[DataWindow::Count(3)],
            )),
        );
        let b = create(
            &mut host,
            world,
            "b",
            C::StreamingDataSourceBinding(stream(
                "dataset:union",
                "x",
                expression.clone(),
                &[DataWindow::Count(1)],
            )),
        );
        append(
            &mut host,
            source,
            (1..=3).map(|n| vec![V::U32(n)]).collect(),
        );
        host.frame_for_test(0.0).unwrap();
        let a_before = view(&mut host, world, a);
        let b_before = view(&mut host, world, b);
        assert_eq!(a_before.row_ids, [DataRowId(1), DataRowId(2), DataRowId(3)]);
        let incoming = if replace_incoming {
            Command::insert_value(
                EntityRef::Handle(b),
                C::StreamingDataSourceBinding(stream(
                    "dataset:union",
                    "x",
                    expression,
                    &[DataWindow::Count(3)],
                )),
            )
        } else {
            set_count(b, 3)
        };
        apply(&mut host, world, vec![incoming, set_count(a, 1)])
            .result
            .unwrap();
        let after = view(&mut host, world, b);
        assert_eq!(
            after.row_ids,
            [DataRowId(1), DataRowId(2), DataRowId(3)],
            "completed demand union must preserve retained history"
        );
        assert_eq!(after.source, b_before.source);
        assert_eq!(
            after.binding_incarnation == b_before.binding_incarnation,
            !replace_incoming
        );
        assert_eq!(
            view(&mut host, world, a).binding_incarnation,
            a_before.binding_incarnation
        );

        // A rejected tail keeps the applied shrink and must finish service cleanup.
        let outcome = apply(
            &mut host,
            world,
            vec![
                set_count(b, 1),
                Command::RemoveComponent {
                    entity: EntityRef::Handle(a),
                    component: C::SCALAR,
                },
                set_count(b, 3),
            ],
        );
        assert!(outcome.result.is_err());
        assert_eq!(view(&mut host, world, b).row_ids, [DataRowId(3)]);
        assert_eq!(
            host.data_sources()
                .read_source(source.source())
                .unwrap()
                .len(),
            1
        );
        apply(&mut host, world, vec![set_count(b, 3)])
            .result
            .unwrap();
        assert_eq!(view(&mut host, world, b).row_ids, [DataRowId(3)]);
        host.destroy_world(world);
        assert!(
            host.data_sources()
                .read_source(source.source())
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn retained_detached_buffer_source_swap_preserves_incarnations_and_collects_after_error() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let mut sources = Vec::new();
    let mut entities = Vec::new();
    for (name, value) in [("dataset:a", 7), ("dataset:b", 9)] {
        let source = producer(
            &mut host,
            name,
            DataSourceKind::Buffer,
            vec![DataColumn::new("x", K::U32)],
        );
        append(&mut host, source, vec![vec![V::U32(value)]]);
        let entity = create(
            &mut host,
            world,
            name,
            C::BufferDataSourceBinding(buffer(name, "x", expression.clone())),
        );
        host.data_sources_mut().detach_producer(source).unwrap();
        sources.push(source);
        entities.push(entity);
    }
    let before = entities
        .iter()
        .map(|&entity| view(&mut host, world, entity))
        .collect::<Vec<_>>();
    apply(
        &mut host,
        world,
        vec![
            set_buffer_source(entities[1], "dataset:a"),
            set_buffer_source(entities[0], "dataset:b"),
        ],
    )
    .result
    .unwrap();
    for (index, expected) in [(0, 9), (1, 7)] {
        let after = view(&mut host, world, entities[index]);
        assert_eq!(after.availability, DataBindingAvailability::Ready);
        assert_eq!(after.source, Some(sources[1 - index].source()));
        assert_eq!(after.binding_incarnation, before[index].binding_incarnation);
        assert_eq!(after.columns[0].values, [R::Valid(V::U32(expected))]);
    }
    // The accepted first retarget survives failure; the rejected tail cannot retain A.
    let outcome = apply(
        &mut host,
        world,
        vec![
            set_buffer_source(entities[1], "dataset:b"),
            Command::RemoveComponent {
                entity: EntityRef::Handle(entities[0]),
                component: C::SCALAR,
            },
            set_buffer_source(entities[0], "dataset:a"),
        ],
    );
    assert!(outcome.result.is_err());
    assert!(
        host.data_sources()
            .read_source(sources[0].source())
            .is_err()
    );
    assert_eq!(
        view(&mut host, world, entities[1]).source,
        Some(sources[1].source())
    );
    apply(&mut host, world, vec![set_buffer_source(entities[1], "")])
        .result
        .unwrap();
    assert_eq!(
        view(&mut host, world, entities[1]).availability,
        DataBindingAvailability::Unavailable(DataBindingUnavailable::Source(
            DataError::MissingSource
        ))
    );
    assert!(host.data_sources().read_source(sources[1].source()).is_ok());
    apply(
        &mut host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entities[0]),
            component: C::BUFFER_DATA_SOURCE_BINDING,
        }],
    )
    .result
    .unwrap();
    assert!(
        host.data_sources()
            .read_source(sources[1].source())
            .is_err()
    );
    host.destroy_world(world);
}

#[test]
fn sole_stream_replacement_preserves_history_and_retires_demand_after_failed_tail() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:replacement",
        DataSourceKind::Streaming,
        vec![DataColumn::new("x", K::U32)],
    );
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let binding = stream(
        "dataset:replacement",
        "x",
        expression,
        &[DataWindow::Count(2)],
    );
    let entity = create(
        &mut host,
        world,
        "binding",
        C::StreamingDataSourceBinding(binding.clone()),
    );
    append(
        &mut host,
        source,
        (1..=3).map(|n| vec![V::U32(n)]).collect(),
    );
    host.frame_for_test(0.0).unwrap();
    let before = view(&mut host, world, entity);
    assert_eq!(before.row_ids, [DataRowId(2), DataRowId(3)]);
    let outcome = apply(
        &mut host,
        world,
        vec![
            Command::insert_value(
                EntityRef::Handle(entity),
                C::StreamingDataSourceBinding(binding),
            ),
            Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: C::SCALAR,
            },
        ],
    );
    assert!(outcome.result.is_err()); // Applied replacement survives the rejected tail.
    let after = view(&mut host, world, entity);
    assert_eq!(after.availability, DataBindingAvailability::Ready);
    assert_eq!(after.source, before.source);
    assert_ne!(after.binding_incarnation, before.binding_incarnation);
    assert_eq!(after.row_ids, [DataRowId(2), DataRowId(3)]);
    assert_eq!(
        after.columns[0].values,
        [R::Valid(V::U32(2)), R::Valid(V::U32(3))]
    );
    apply(
        &mut host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: C::STREAMING_DATA_SOURCE_BINDING,
        }],
    )
    .result
    .unwrap();
    assert!(
        host.data_sources()
            .read_source(source.source())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn detached_buffer_replacement_preserves_source_and_fences_old_presentation() {
    let mut host = crate::support::task_scheduler::host();
    let world = host
        .create_world(
            WorldLimits::default(),
            &[
                DataBindingSystem::ID,
                SystemId("ipp.asset-dependencies"),
                SystemId("ipp.constraints"),
            ],
        )
        .unwrap();
    let source = producer(
        &mut host,
        "dataset:detached",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::U32)],
    );
    append(&mut host, source, vec![vec![V::U32(7)]]);
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let binding = buffer("dataset:detached", "x", expression.clone());
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(binding.clone()),
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            C::Scalar(Default::default()),
        )],
    )
    .result
    .unwrap();
    let old_presentation = host
        .world_mut(world)
        .unwrap()
        .register_data_binding_presentation_consumer(entity, C::SCALAR)
        .unwrap();
    let before = view(&mut host, world, entity);
    host.data_sources_mut().detach_producer(source).unwrap();
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            C::BufferDataSourceBinding(binding),
        )],
    )
    .result
    .unwrap();
    let after = view(&mut host, world, entity);
    assert_eq!(after.availability, DataBindingAvailability::Ready);
    assert_eq!(after.source, before.source);
    assert_ne!(after.binding_incarnation, before.binding_incarnation);
    assert_eq!(after.row_ids, [DataRowId(1)]);
    assert_eq!(after.columns[0].values, [R::Valid(V::U32(7))]);
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .finish_data_binding_presentation(old_presentation),
        Err(ErrorReason::InvalidEntity)
    );

    // A changed source must release the detached old incarnation, not retarget it.
    let next = producer(
        &mut host,
        "dataset:next",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::U32)],
    );
    append(&mut host, next, vec![vec![V::U32(9)]]);
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            C::BufferDataSourceBinding(buffer("dataset:next", "x", expression)),
        )],
    )
    .result
    .unwrap();
    assert!(host.data_sources().read_source(source.source()).is_err());
    let current = view(&mut host, world, entity);
    assert_ne!(current.source, before.source);
    assert_eq!(current.columns[0].values, [R::Valid(V::U32(9))]);
    host.data_sources_mut().detach_producer(next).unwrap();
    host.destroy_world(world);
    assert!(host.data_sources().read_source(next.source()).is_err());
}

#[test]
fn detached_source_kind_switch_stays_unavailable_until_a_new_compatible_incarnation() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:kind-switch",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::U32)],
    );
    append(&mut host, source, vec![vec![V::U32(7)]]);
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(buffer("dataset:kind-switch", "x", expression.clone())),
    );
    let before = view(&mut host, world, entity);
    host.data_sources_mut().detach_producer(source).unwrap();
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: C::BUFFER_DATA_SOURCE_BINDING,
            },
            Command::insert_value(
                EntityRef::Handle(entity),
                C::StreamingDataSourceBinding(stream(
                    "dataset:kind-switch",
                    "x",
                    expression,
                    &[DataWindow::Count(2)],
                )),
            ),
        ],
    )
    .result
    .unwrap();
    let mismatch = view(&mut host, world, entity);
    assert_eq!(mismatch.source, before.source);
    assert_ne!(mismatch.binding_incarnation, before.binding_incarnation);
    assert_eq!(
        mismatch.availability,
        DataBindingAvailability::Unavailable(DataBindingUnavailable::Source(
            DataError::KindMismatch
        ))
    );
    assert!(mismatch.columns.is_empty());
    assert_eq!(
        host.data_sources()
            .read_source(source.source())
            .unwrap()
            .len(),
        1
    );

    let next = producer(
        &mut host,
        "dataset:kind-switch",
        DataSourceKind::Streaming,
        vec![DataColumn::new("x", K::U32)],
    );
    append(&mut host, next, vec![vec![V::U32(9)]]);
    host.frame_for_test(0.0).unwrap();
    let ready = view(&mut host, world, entity);
    assert_eq!(ready.source, Some(next.source()));
    assert_eq!(ready.binding_incarnation, mismatch.binding_incarnation);
    assert_eq!(ready.row_ids, [DataRowId(1)]);
    assert_eq!(ready.columns[0].values, [R::Valid(V::U32(9))]);
    assert!(host.data_sources().read_source(source.source()).is_err());
}

#[test]
fn source_reincarnation_revalidates_kind_and_schema_without_reusing_rows() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:replace",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::F32)],
    );
    append(&mut host, source, vec![vec![V::F32(4.0)]]);
    let expression = identity(&mut host, world, 1, "x", K::F32);
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(buffer("dataset:replace", "x", expression)),
    );
    let first = view(&mut host, world, entity);
    host.data_sources_mut().detach_producer(source).unwrap();
    let wrong = producer(
        &mut host,
        "dataset:replace",
        DataSourceKind::Streaming,
        vec![DataColumn::new("x", K::F32)],
    );
    host.frame_for_test(0.0).unwrap();
    let mismatch = view(&mut host, world, entity);
    assert_eq!(
        mismatch.availability,
        DataBindingAvailability::Unavailable(DataBindingUnavailable::Source(
            DataError::KindMismatch
        ))
    );
    assert_ne!(mismatch.source, first.source);
    assert!(mismatch.row_ids.is_empty());
    host.data_sources_mut().detach_producer(wrong).unwrap();
    let wrong_type = producer(
        &mut host,
        "dataset:replace",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::U32)],
    );
    append(&mut host, wrong_type, vec![vec![V::U32(9)]]);
    host.frame_for_test(0.0).unwrap();
    assert!(matches!(
        view(&mut host, world, entity).availability,
        DataBindingAvailability::Unavailable(DataBindingUnavailable::InputType { .. })
    ));
    host.data_sources_mut().destroy_source(wrong_type).unwrap();
    let recovered = producer(
        &mut host,
        "dataset:replace",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::F32)],
    );
    append(&mut host, recovered, vec![vec![V::F32(7.0)]]);
    host.frame_for_test(0.0).unwrap();
    let current = view(&mut host, world, entity);
    assert_eq!(current.availability, DataBindingAvailability::Ready);
    assert_eq!(current.row_ids, [DataRowId(1)]);
    assert_eq!(current.columns[0].values, [R::Valid(V::F32(7.0))]);
    assert_ne!(current.source, first.source);
    assert_eq!(current.binding_incarnation, first.binding_incarnation);
}

#[test]
fn one_binding_per_entity_and_window_admission_fail_before_component_changes() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let value = stream(
        "dataset:windows",
        "x",
        expression.clone(),
        &[DataWindow::Range {
            column: "x".into(),
            width: 10.0,
            anchor: DataWindowAnchor::Supplied(V::U32(20)),
        }],
    );
    let original = value.windows.clone();
    let entity = create(
        &mut host,
        world,
        "binding",
        C::StreamingDataSourceBinding(value),
    );
    let incarnation = view(&mut host, world, entity).binding_incarnation;
    let outcome = apply(
        &mut host,
        world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: Box::new(C::BufferDataSourceBinding(buffer(
                "dataset:windows",
                "x",
                expression,
            ))),
        }],
    );
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ErrorReason::InvalidValue
    );
    assert_eq!(
        view(&mut host, world, entity).binding_incarnation,
        incarnation
    );
    assert!(
        host.world_mut(world)
            .unwrap()
            .component_incarnation(entity, C::BUFFER_DATA_SOURCE_BINDING)
            .is_none()
    );
    let rewound = encode_data_windows(&[DataWindow::Range {
        column: "x".into(),
        width: 10.0,
        anchor: DataWindowAnchor::Supplied(V::U32(19)),
    }])
    .unwrap();
    let offset = std::mem::offset_of!(StreamingDataSourceBinding, windows) as u32;
    for bytes in [rewound, b"malformed".to_vec()] {
        let outcome = apply(
            &mut host,
            world,
            vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: C::STREAMING_DATA_SOURCE_BINDING,
                field: FieldWrite {
                    offset,
                    value: FieldValue::Bytes(bytes),
                },
            }],
        );
        assert_eq!(
            outcome.result.unwrap_err().reason,
            ErrorReason::InvalidValue
        );
        let snapshot = host.world_mut(world).unwrap().inspect(entity).unwrap();
        let stored = snapshot
            .components
            .iter()
            .find_map(|v| {
                if let C::StreamingDataSourceBinding(v) = v {
                    Some(v)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(stored.windows, original);
    }
    // A fresh component lifetime starts a fresh window demand and may rewind.
    let expression = identity(&mut host, world, 2, "x", K::U32);
    apply(
        &mut host,
        world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: Box::new(C::StreamingDataSourceBinding(stream(
                "dataset:windows",
                "x",
                expression,
                &[DataWindow::Range {
                    column: "x".into(),
                    width: 10.0,
                    anchor: DataWindowAnchor::Supplied(V::U32(1)),
                }],
            ))),
        }],
    )
    .result
    .unwrap();
    assert_ne!(
        view(&mut host, world, entity).binding_incarnation,
        incarnation
    );
}

#[test]
fn raw_range_windows_intersect_count_and_latest_anchor_never_rewinds() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:range",
        DataSourceKind::Streaming,
        vec![DataColumn::new("raw", K::F32)],
    );
    let expression = asset(
        &mut host,
        world,
        1,
        definition(
            &[("column:raw", K::F32)],
            vec![
                N::Input(0),
                N::Constant(V::F32(100.0)),
                N::Binary {
                    operator: BinaryOperator::Multiply,
                    left: 0,
                    right: 1,
                },
            ],
            2,
        ),
    );
    let entity = create(
        &mut host,
        world,
        "binding",
        C::StreamingDataSourceBinding(stream(
            "dataset:range",
            "scaled",
            expression,
            &[
                DataWindow::Count(3),
                DataWindow::Range {
                    column: "raw".into(),
                    width: 2.0,
                    anchor: DataWindowAnchor::Latest,
                },
            ],
        )),
    );
    append(
        &mut host,
        source,
        vec![
            vec![V::F32(1.0)],
            vec![V::F32(5.0)],
            vec![V::F32(4.0)],
            vec![V::F32(2.0)],
            vec![V::F32(3.0)],
        ],
    );
    host.frame_for_test(0.0).unwrap();
    let output = view(&mut host, world, entity);
    assert_eq!(output.row_ids, [DataRowId(3), DataRowId(5)]);
    assert_eq!(
        output.columns[0].values,
        [R::Valid(V::F32(400.0)), R::Valid(V::F32(300.0))]
    );
    assert_eq!(
        host.data_sources()
            .read_source(source.source())
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn final_notification_boundary_catches_data_and_host_time_after_prepared_update() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:time",
        DataSourceKind::Streaming,
        vec![DataColumn::new("t", K::F32)],
    );
    let expression = identity(&mut host, world, 1, "t", K::F32);
    let entity = create(
        &mut host,
        world,
        "binding",
        C::StreamingDataSourceBinding(stream(
            "dataset:time",
            "time",
            expression,
            &[DataWindow::Range {
                column: "t".into(),
                width: 1.0,
                anchor: DataWindowAnchor::HostTime {
                    units_per_second: 1.0,
                },
            }],
        )),
    );
    host.world_mut(world).unwrap().prepare_update(0.0).unwrap();
    append(&mut host, source, vec![vec![V::F32(0.0)]]);
    host.frame_for_test(0.0).unwrap();
    assert_eq!(view(&mut host, world, entity).row_ids, [DataRowId(1)]);
    host.world_mut(world).unwrap().prepare_update(0.0).unwrap();
    host.frame_for_test(2.0).unwrap();
    assert!(view(&mut host, world, entity).row_ids.is_empty());
}

#[test]
fn default_stream_cap_lives_in_data_service_and_explicit_windows_bypass_it() {
    let mut host = crate::support::task_scheduler::host();
    host.data_sources_mut().configure(DataServiceConfig {
        default_stream_bytes: 0,
    });
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:cap",
        DataSourceKind::Streaming,
        vec![DataColumn::new("x", K::U32)],
    );
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let default = create(
        &mut host,
        world,
        "default",
        C::StreamingDataSourceBinding(stream("dataset:cap", "x", expression.clone(), &[])),
    );
    append(&mut host, source, vec![vec![V::U32(1)]]);
    assert!(
        host.data_sources()
            .read_source(source.source())
            .unwrap()
            .is_empty()
    );
    let explicit = create(
        &mut host,
        world,
        "explicit",
        C::StreamingDataSourceBinding(stream(
            "dataset:cap",
            "x",
            expression,
            &[DataWindow::Count(1)],
        )),
    );
    append(&mut host, source, vec![vec![V::U32(2)]]);
    host.frame_for_test(0.0).unwrap();
    assert!(view(&mut host, world, default).row_ids.is_empty());
    assert_eq!(view(&mut host, world, explicit).row_ids, [DataRowId(2)]);
}

#[test]
fn unavailable_assets_inputs_and_source_names_remain_observable_and_recover() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let missing = AssetSource {
        kind: EXPRESSION_TYPE,
        uri: "memory:pending-expression".into(),
        variant: 0,
    };
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(buffer("dataset:missing", "value", missing)),
    );
    assert_eq!(
        view(&mut host, world, entity).availability,
        DataBindingAvailability::Unavailable(DataBindingUnavailable::Source(
            DataError::MissingSource
        ))
    );
    let source = producer(
        &mut host,
        "dataset:missing",
        DataSourceKind::Buffer,
        vec![DataColumn::new("raw", K::F32)],
    );
    append(&mut host, source, vec![vec![V::F32(7.0)]]);
    host.frame_for_test(0.0).unwrap();
    assert!(matches!(
        view(&mut host, world, entity).availability,
        DataBindingAvailability::Unavailable(DataBindingUnavailable::MissingAsset { .. })
    ));
    let expression = identity(&mut host, world, 1, "absent", K::F32);
    apply(
        &mut host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: C::BUFFER_DATA_SOURCE_BINDING,
            name: "value".into(),
            value: V::Asset(expression),
        }],
    )
    .result
    .unwrap();
    assert!(matches!(
        view(&mut host, world, entity).availability,
        DataBindingAvailability::Unavailable(DataBindingUnavailable::MissingInput { .. })
    ));
    let parameter = asset(
        &mut host,
        world,
        2,
        definition(&[("parameter", K::F32)], vec![N::Input(0)], 0),
    );
    apply(
        &mut host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: C::BUFFER_DATA_SOURCE_BINDING,
            name: "value".into(),
            value: V::Asset(parameter),
        }],
    )
    .result
    .unwrap();
    let absent = view(&mut host, world, entity);
    assert_eq!(absent.availability, DataBindingAvailability::Ready);
    assert_eq!(
        absent.columns[0].values,
        [R::Invalid(ExpressionInvalid::MissingInput {
            slot: 0
        })]
    );
    apply(
        &mut host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: C::BUFFER_DATA_SOURCE_BINDING,
            name: "value_parameter".into(),
            value: V::F32(9.0),
        }],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(9.0))]
    );
    let wrong_kind = AssetSource {
        kind: ipp_core::services::asset_management::AssetTypeId(2),
        uri: "asset://2/3".into(),
        variant: 0,
    };
    let rejected = apply(
        &mut host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: C::BUFFER_DATA_SOURCE_BINDING,
            name: "value".into(),
            value: V::Asset(wrong_kind),
        }],
    );
    assert_eq!(
        rejected.result.unwrap_err().reason,
        ErrorReason::InvalidAsset
    );
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(9.0))]
    );
}

#[test]
fn typed_projection_outputs_cover_vectors_booleans_and_text_without_source_schema_changes() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:types",
        DataSourceKind::Buffer,
        vec![
            DataColumn::new("vector", K::Vec3),
            DataColumn::new("enabled", K::Bool),
            DataColumn::text("label", 64),
        ],
    );
    append(
        &mut host,
        source,
        vec![vec![
            V::Vec3([2.0, 4.0, 6.0]),
            V::Bool(true),
            V::Text("sample α".into()),
        ]],
    );
    let mut value = BufferDataSourceBinding {
        source: "dataset:types".into(),
        ..Default::default()
    };
    for (id, output, raw, kind) in [
        (1, "position", "vector", K::Vec3),
        (2, "visible", "enabled", K::Bool),
        (3, "caption", "label", K::Text),
    ] {
        value
            .properties
            .set(output, V::Asset(identity(&mut host, world, id, raw, kind)))
            .unwrap();
    }
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(value),
    );
    let output = view(&mut host, world, entity);
    assert_eq!(
        output
            .columns
            .iter()
            .map(|column| (column.name.as_str(), column.kind))
            .collect::<Vec<_>>(),
        [
            ("caption", K::Text),
            ("position", K::Vec3),
            ("visible", K::Bool)
        ]
    );
    assert_eq!(
        output.columns[0].values,
        [R::Valid(V::Text("sample α".into()))]
    );
    assert_eq!(
        output.columns[1].values,
        [R::Valid(V::Vec3([2.0, 4.0, 6.0]))]
    );
    assert_eq!(output.columns[2].values, [R::Valid(V::Bool(true))]);
    assert_eq!(
        host.data_sources()
            .read_source(source.source())
            .unwrap()
            .schema()
            .columns[0]
            .name,
        "vector"
    );
}

#[test]
fn query_pages_are_bounded_by_rows_and_payload_and_never_trigger_evaluation() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:page",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::U32)],
    );
    append(
        &mut host,
        source,
        (0..2050).map(|n| vec![V::U32(n)]).collect(),
    );
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(buffer("dataset:page", "value", expression)),
    );
    let world_context = host.world_mut(world).unwrap();
    let first = world_context
        .data_binding_view(
            entity,
            DataBindingViewQuery {
                offset: 0,
                limit: usize::MAX,
            },
        )
        .unwrap();
    assert_eq!(first.row_ids.len(), DATA_BINDING_QUERY_MAX_ROWS);
    assert_eq!(first.next_offset, Some(1024));
    let second = world_context
        .data_binding_view_owned(
            entity,
            DataBindingViewQuery {
                offset: 1024,
                limit: 2,
            },
        )
        .unwrap();
    assert_eq!(second.row_ids, [DataRowId(1025), DataRowId(1026)]);
    assert_eq!(
        second.columns[0].values,
        [R::Valid(V::U32(1024)), R::Valid(V::U32(1025))]
    );
    let last = world_context
        .data_binding_view(
            entity,
            DataBindingViewQuery {
                offset: usize::MAX,
                limit: usize::MAX,
            },
        )
        .unwrap();
    assert!(last.row_ids.is_empty());
    assert_eq!(last.next_offset, None);
    assert_eq!(world_context.tick(), 1);
    drop(world_context);

    let text_source = producer(
        &mut host,
        "dataset:text-page",
        DataSourceKind::Buffer,
        vec![DataColumn::text("text", DATA_BINDING_QUERY_MAX_BYTES * 2)],
    );
    let text = "a".repeat(600_000);
    append(
        &mut host,
        text_source,
        vec![
            vec![V::Text(text.into())],
            vec![V::Text("b".repeat(600_000).into())],
        ],
    );
    let expression = identity(&mut host, world, 2, "text", K::Text);
    let text_entity = create(
        &mut host,
        world,
        "text-binding",
        C::BufferDataSourceBinding(buffer("dataset:text-page", "text", expression)),
    );
    let page = view(&mut host, world, text_entity);
    assert_eq!(page.row_ids.len(), 1);
    assert_eq!(page.next_offset, Some(1));
    host.data_sources_mut()
        .apply_batch(
            text_source,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: vec![V::Text("c".repeat(DATA_BINDING_QUERY_MAX_BYTES + 1).into())],
            }],
        )
        .unwrap();
    host.frame_for_test(0.0).unwrap();
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .data_binding_view_owned(text_entity, Default::default()),
        Err(ErrorReason::Capacity)
    );
}

#[test]
fn graphics_only_release_preserves_completed_cpu_view_and_acknowledged_dirty() {
    let mut host = crate::support::task_scheduler::host();
    let world = host
        .create_world(
            WorldLimits::default(),
            &[
                DataBindingSystem::ID,
                SystemId("ipp.asset-dependencies"),
                SystemId("ipp.constraints"),
            ],
        )
        .unwrap();
    let source = producer(
        &mut host,
        "dataset:graphics",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::U32)],
    );
    append(&mut host, source, vec![vec![V::U32(7)]]);
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(buffer("dataset:graphics", "x", expression)),
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            C::Scalar(Default::default()),
        )],
    )
    .result
    .unwrap();
    let consumer = host
        .world_mut(world)
        .unwrap()
        .register_data_binding_presentation_consumer(entity, C::SCALAR)
        .unwrap();
    let key = host
        .asset_resources()
        .find(&AssetSource {
            kind: EXPRESSION_TYPE,
            uri: format!("producer://{}/19/1", world.0).into(),
            variant: 0,
        })
        .unwrap();
    // Both an unacknowledged and an acknowledged completed view survive; no evaluation.
    for acknowledge in [false, true] {
        if acknowledge {
            host.world_mut(world)
                .unwrap()
                .finish_data_binding_presentation(consumer)
                .unwrap();
        }
        let before = view(&mut host, world, entity);
        assert_eq!(before.dirty, !acknowledge);
        host.asset_resources_mut().invalidate_graphics(key);
        host.flush_resource_lifecycle();
        assert!(host.asset_resources().get(key).unwrap().decoded_available());
        assert_eq!(view(&mut host, world, entity), before);
        host.frame_for_test(0.0).unwrap();
        assert_eq!(view(&mut host, world, entity), before); // No rebuild or new dirty handoff.
    }
}

#[test]
fn cpu_release_invalidates_binding_before_payload_or_identity_is_released() {
    use ipp_core::{services::asset_management::AssetLifecycleKind, systems::*};

    #[derive(Default)]
    struct ReleaseObserver {
        entity: Option<EntityId>,
        observed: Vec<AssetLifecycleKind>,
    }

    impl ReleaseObserver {
        const ID: SystemId = SystemId("fixture.binding-release-observer");
    }

    struct ReleaseObserverFactory;

    impl SystemFactory for ReleaseObserverFactory {
        fn id(&self) -> SystemId {
            ReleaseObserver::ID
        }

        fn dependencies(&self) -> &[SystemDependency] {
            &[SystemDependency::Required(DataBindingSystem::ID)]
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::<ReleaseObserver>::default())
        }
    }

    impl System for ReleaseObserver {
        fn before_asset_release(
            &mut self,
            context: &mut SystemAssetContext<'_>,
            event: &ipp_core::services::asset_management::AssetLifecycleEvent,
        ) {
            let Some(entity) = self.entity else {
                return;
            };
            assert!(
                context
                    .world
                    .asset_resources()
                    .get(event.key)
                    .unwrap()
                    .decoded_available()
            );
            let view = context
                .world
                .data_binding_view_owned(entity, Default::default())
                .unwrap();
            assert!(matches!(
                view.availability,
                DataBindingAvailability::Unavailable(DataBindingUnavailable::MissingAsset { .. })
            ));
            assert!(view.columns.is_empty());
            assert!(view.row_ids.is_empty());
            assert!(view.dirty);
            self.observed.push(event.kind);
        }

        fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}
    }

    for revoke in [false, true] {
        let mut factories = compiled_system_factories();
        factories.push(std::sync::Arc::new(ReleaseObserverFactory));
        let mut host = crate::support::task_scheduler::with_factories(factories).unwrap();
        let world = host
            .create_world(
                WorldLimits::default(),
                &[
                    ReleaseObserver::ID,
                    DataBindingSystem::ID,
                    SystemId("ipp.asset-dependencies"),
                ],
            )
            .unwrap();
        let source = producer(
            &mut host,
            "dataset:release",
            DataSourceKind::Buffer,
            vec![DataColumn::new("x", K::U32)],
        );
        append(&mut host, source, vec![vec![V::U32(7)]]);
        let expression = identity(&mut host, world, 1, "x", K::U32);
        let entity = create(
            &mut host,
            world,
            "binding",
            C::BufferDataSourceBinding(buffer("dataset:release", "x", expression)),
        );
        let key = host
            .asset_resources()
            .find(&AssetSource {
                kind: EXPRESSION_TYPE,
                uri: format!("producer://{}/19/1", world.0).into(),
                variant: 0,
            })
            .unwrap();
        host.world_mut(world)
            .unwrap()
            .with_system::<ReleaseObserver, _>(ReleaseObserver::ID, |system, _| {
                system.entity = Some(entity)
            })
            .unwrap();
        if revoke {
            host.asset_resources_mut().revoke_resource(key);
        } else {
            host.asset_resources_mut().unload(key);
        }
        host.flush_resource_lifecycle();
        host.world_mut(world)
            .unwrap()
            .with_system::<ReleaseObserver, _>(ReleaseObserver::ID, |system, _| {
                assert_eq!(
                    system.observed,
                    [if revoke {
                        AssetLifecycleKind::Removed
                    } else {
                        AssetLifecycleKind::StatusChanged
                    }]
                );
            })
            .unwrap();
        if revoke {
            assert!(host.asset_resources().get(key).is_none());
        } else {
            assert!(!host.asset_resources().get(key).unwrap().decoded_available());
            host.frame_for_test(0.0).unwrap();
            assert_eq!(
                view(&mut host, world, entity).columns[0].values,
                [R::Valid(V::U32(7))]
            );
        }
    }
}

#[test]
fn active_asset_demand_survives_producer_release_and_unload_invalidates_frozen_views() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:asset",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::U32)],
    );
    append(&mut host, source, vec![vec![V::U32(7)]]);
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(buffer("dataset:asset", "x", expression)),
    );
    let source_ref = AssetSource {
        kind: EXPRESSION_TYPE,
        uri: format!("producer://{}/19/1", world.0).into(),
        variant: 0,
    };
    let key = host.asset_resources().find(&source_ref).unwrap();
    host.asset_resources_mut()
        .release_client_source(world, &source_ref);
    host.flush_resource_lifecycle();
    assert!(host.asset_resources().get(key).unwrap().data().is_some());
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::U32(7))]
    );
    // No World frame between request and flush: old plans/output cannot stay ready
    // behind an idle or previously prepared World at the Host release barrier.
    host.world_mut(world).unwrap().prepare_update(0.0).unwrap();
    host.asset_resources_mut().unload(key);
    host.flush_resource_lifecycle();
    assert!(matches!(
        view(&mut host, world, entity).availability,
        DataBindingAvailability::Unavailable(DataBindingUnavailable::MissingAsset { .. })
    ));
    assert!(view(&mut host, world, entity).row_ids.is_empty());
    host.frame_for_test(0.0).unwrap();
    assert_eq!(
        view(&mut host, world, entity).availability,
        DataBindingAvailability::Ready
    );
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::U32(7))]
    );
    apply(
        &mut host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: C::BUFFER_DATA_SOURCE_BINDING,
        }],
    )
    .result
    .unwrap();
    host.asset_resources_mut().set_idle_resident_bytes_target(0);
    host.flush_resource_lifecycle();
    assert!(host.asset_resources().get(key).is_none());
}

#[test]
fn bindings_persist_metadata_and_restore_empty_until_source_and_definitions_are_resupplied() {
    for streaming in [false, true] {
        let mut host = crate::support::task_scheduler::host();
        let world = world(&mut host);
        let kind = if streaming {
            DataSourceKind::Streaming
        } else {
            DataSourceKind::Buffer
        };
        let source = producer(
            &mut host,
            "dataset:persist",
            kind,
            vec![DataColumn::text("payload_only_column", 1024)],
        );
        let expression = asset(
            &mut host,
            world,
            1,
            definition(&[("parameter", K::F32)], vec![N::Input(0)], 0),
        );
        let value = if streaming {
            let mut value = stream(
                "dataset:persist",
                "output",
                expression,
                &[DataWindow::Count(2)],
            );
            value
                .properties
                .set("output_parameter", V::F32(12.5))
                .unwrap();
            C::StreamingDataSourceBinding(value)
        } else {
            let mut value = buffer("dataset:persist", "output", expression);
            value
                .properties
                .set("output_parameter", V::F32(12.5))
                .unwrap();
            C::BufferDataSourceBinding(value)
        };
        let entity = create(&mut host, world, "persist-binding", value);
        append(
            &mut host,
            source,
            vec![vec![V::Text("NEVER_PERSIST_SOURCE_PAYLOAD".into())]],
        );
        host.frame_for_test(0.0).unwrap();
        assert_eq!(
            view(&mut host, world, entity).columns[0].values,
            [R::Valid(V::F32(12.5))]
        );
        let captured = host
            .world_mut(world)
            .unwrap()
            .capture_world(Default::default())
            .unwrap();
        let bytes = host.save_world(world, 123, Default::default()).unwrap();
        let encoded = String::from_utf8_lossy(&bytes);
        assert!(encoded.contains("dataset:persist"));
        assert!(encoded.contains("output_parameter"));
        assert!(!encoded.contains("NEVER_PERSIST_SOURCE_PAYLOAD"));
        assert!(!encoded.contains("payload_only_column"));
        assert!(!bytes.windows(4).any(|part| part == b"IPPE"));
        let mut restored_host = crate::support::task_scheduler::host();
        let restored = restored_host
            .load_world(
                &bytes,
                123,
                Default::default(),
                Default::default(),
                Default::default(),
            )
            .unwrap()
            .root
            .id();
        let restored_entity = restored_host
            .world_mut(restored)
            .unwrap()
            .lookup_id("persist-binding")
            .unwrap();
        let before = view(&mut restored_host, restored, restored_entity);
        assert_eq!(
            before.availability,
            DataBindingAvailability::Unavailable(DataBindingUnavailable::NotEvaluated)
        );
        assert_eq!(before.evaluated_tick, None);
        assert!(before.row_ids.is_empty());
        assert!(before.dirty);
        assert_eq!(
            restored_host
                .world_mut(restored)
                .unwrap()
                .capture_world(Default::default())
                .unwrap()
                .entities,
            captured.entities
        );
        restored_host.frame_for_test(0.0).unwrap();
        assert_eq!(
            view(&mut restored_host, restored, restored_entity).availability,
            DataBindingAvailability::Unavailable(DataBindingUnavailable::Source(
                DataError::MissingSource
            ))
        );
        let resupplied = producer(
            &mut restored_host,
            "dataset:persist",
            kind,
            vec![DataColumn::new("different_schema", K::U32)],
        );
        append(&mut restored_host, resupplied, vec![vec![V::U32(3)]]);
        identity(&mut restored_host, restored, 1, "different_schema", K::U32);
        restored_host.frame_for_test(0.0).unwrap();
        let after = view(&mut restored_host, restored, restored_entity);
        assert_eq!(after.availability, DataBindingAvailability::Ready);
        assert_eq!(after.columns[0].kind, K::U32);
        assert_eq!(after.columns[0].values, [R::Valid(V::U32(3))]);
    }
}

#[cfg(feature = "instrumentation")]
#[test]
fn queries_keep_dirty_and_notifications_until_success_and_source_or_parameter_changes_redirty() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:dirty",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::U32)],
    );
    append(&mut host, source, vec![vec![V::U32(1)]]);
    let expression = asset(
        &mut host,
        world,
        1,
        definition(
            &[("column:x", K::U32), ("parameter", K::U32)],
            vec![
                N::Input(0),
                N::Input(1),
                N::Binary {
                    operator: BinaryOperator::Add,
                    left: 0,
                    right: 1,
                },
            ],
            2,
        ),
    );
    let mut value = buffer("dataset:dirty", "x", expression.clone());
    value.properties.set("x_parameter", V::U32(10)).unwrap();
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(value),
    );
    let first = view(&mut host, world, entity);
    assert!(first.dirty);
    assert!(view(&mut host, world, entity).dirty);
    host.frame_for_test(0.0).unwrap();
    assert!(view(&mut host, world, entity).dirty);
    host.world_mut(world)
        .unwrap()
        .acknowledge_data_binding_for_test(entity, first.binding_incarnation)
        .unwrap();
    assert!(!view(&mut host, world, entity).dirty);
    append(&mut host, source, vec![vec![V::U32(2)]]);
    for _ in 0..3 {
        let observation = view(&mut host, world, entity);
        assert!(!observation.dirty);
        assert_eq!(observation.row_ids, [DataRowId(1)]);
    }
    host.frame_for_test(0.0).unwrap();
    assert!(view(&mut host, world, entity).dirty);
    host.world_mut(world)
        .unwrap()
        .acknowledge_data_binding_for_test(entity, first.binding_incarnation)
        .unwrap();
    host.frame_for_test(0.0).unwrap();
    assert!(!view(&mut host, world, entity).dirty);
    apply(
        &mut host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: C::BUFFER_DATA_SOURCE_BINDING,
            name: "x_parameter".into(),
            value: V::U32(20),
        }],
    )
    .result
    .unwrap();
    assert!(view(&mut host, world, entity).dirty);
    host.world_mut(world)
        .unwrap()
        .acknowledge_data_binding_for_test(entity, first.binding_incarnation)
        .unwrap();
    let mut replacement = buffer("dataset:dirty", "x", expression);
    replacement
        .properties
        .set("x_parameter", V::U32(20))
        .unwrap();
    apply(
        &mut host,
        world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: Box::new(C::BufferDataSourceBinding(replacement)),
        }],
    )
    .result
    .unwrap();
    let current = view(&mut host, world, entity);
    assert_ne!(current.binding_incarnation, first.binding_incarnation);
    assert!(current.dirty);
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .acknowledge_data_binding_for_test(entity, first.binding_incarnation),
        Err(ErrorReason::InvalidEntity)
    );
}

#[test]
fn presentation_success_is_fenced_by_the_single_registered_component_lifetime() {
    let mut host = crate::support::task_scheduler::host();
    let world = host
        .create_world(
            WorldLimits::default(),
            &[
                DataBindingSystem::ID,
                SystemId("ipp.asset-dependencies"),
                SystemId("ipp.constraints"),
                SystemId("ipp.hierarchy"),
            ],
        )
        .unwrap();
    let source = producer(
        &mut host,
        "dataset:consumer",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::U32)],
    );
    append(&mut host, source, vec![vec![V::U32(2)]]);
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let entity = create(
        &mut host,
        world,
        "binding",
        C::BufferDataSourceBinding(buffer("dataset:consumer", "x", expression)),
    );
    // Existing ordinary components exercise the core registration API without
    // introducing a Plot implementation or a presentation evaluator.
    apply(
        &mut host,
        world,
        vec![
            Command::insert_value(EntityRef::Handle(entity), C::Scalar(Default::default())),
            Command::insert_value(EntityRef::Handle(entity), C::Transform(Default::default())),
        ],
    )
    .result
    .unwrap();
    let mut context = host.world_mut(world).unwrap();
    let first = context
        .register_data_binding_presentation_consumer(entity, C::SCALAR)
        .unwrap();
    assert_eq!(
        context
            .register_data_binding_presentation_consumer(entity, C::SCALAR)
            .unwrap(),
        first
    );
    assert_eq!(
        context.register_data_binding_presentation_consumer(entity, C::TRANSFORM),
        Err(ErrorReason::InvalidValue)
    );
    assert!(
        context
            .data_binding_view(entity, Default::default())
            .unwrap()
            .dirty
    );
    context.finish_data_binding_presentation(first).unwrap();
    assert!(
        !context
            .data_binding_view(entity, Default::default())
            .unwrap()
            .dirty
    );
    drop(context);
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            C::Scalar(Default::default()),
        )],
    )
    .result
    .unwrap();
    let mut context = host.world_mut(world).unwrap();
    assert_eq!(
        context.finish_data_binding_presentation(first),
        Err(ErrorReason::InvalidEntity)
    );
    let replacement = context
        .register_data_binding_presentation_consumer(entity, C::TRANSFORM)
        .unwrap();
    assert!(
        context
            .data_binding_view(entity, Default::default())
            .unwrap()
            .dirty
    );
    context
        .finish_data_binding_presentation(replacement)
        .unwrap();
    context
        .release_data_binding_presentation_consumer(replacement)
        .unwrap();
    assert!(
        context
            .data_binding_view(entity, Default::default())
            .unwrap()
            .dirty
    );
    assert_eq!(
        context.finish_data_binding_presentation(replacement),
        Err(ErrorReason::InvalidEntity)
    );
}

#[test]
fn composition_admission_exposes_registry_ids_and_does_not_require_animation() {
    assert_eq!(C::BUFFER_DATA_SOURCE_BINDING, 100);
    assert_eq!(C::STREAMING_DATA_SOURCE_BINDING, 101);
    assert!(C::supports_dynamic_properties(100));
    assert!(C::supports_dynamic_properties(101));
    let mut host = crate::support::task_scheduler::host();
    let selected = world(&mut host);
    assert_eq!(
        host.world_mut(selected)
            .unwrap()
            .system_ids()
            .collect::<Vec<_>>(),
        [SystemId("ipp.asset-dependencies"), DataBindingSystem::ID]
    );
    let bare = host.create_world(WorldLimits::default(), &[]).unwrap();
    let outcome = apply(
        &mut host,
        bare,
        vec![
            Command::Create {
                alias: 0,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(0),
                C::BufferDataSourceBinding(Default::default()),
            ),
        ],
    );
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ErrorReason::UnsupportedDependency
    );
}

#[test]
fn shared_source_and_definition_keep_independent_world_parameter_outputs() {
    let mut host = crate::support::task_scheduler::host();
    let left = world(&mut host);
    let right = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:shared",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::F32)],
    );
    append(&mut host, source, vec![vec![V::F32(5.0)]]);
    let memory = ipp_core::services::io::MemoryIoSource::default();
    memory
        .insert(
            "expression:shared".into(),
            definition(
                &[("column:x", K::F32), ("parameter", K::F32)],
                vec![
                    N::Input(0),
                    N::Input(1),
                    N::Binary {
                        operator: BinaryOperator::Multiply,
                        left: 0,
                        right: 1,
                    },
                ],
                2,
            )
            .encode()
            .unwrap(),
        )
        .unwrap();
    host.io_mut().register("expression:", memory).unwrap();
    let shared = AssetSource {
        kind: EXPRESSION_TYPE,
        uri: "expression:shared".into(),
        variant: 0,
    };
    let mut l = buffer("dataset:shared", "scaled", shared.clone());
    l.properties.set("scaled_parameter", V::F32(2.0)).unwrap();
    let mut r = buffer("dataset:shared", "scaled", shared.clone());
    r.properties.set("scaled_parameter", V::F32(3.0)).unwrap();
    let l_entity = create(&mut host, left, "left", C::BufferDataSourceBinding(l));
    let r_entity = create(&mut host, right, "right", C::BufferDataSourceBinding(r));
    host.frame_for_test(0.0).unwrap();
    let key = host.asset_resources().find(&shared).unwrap();
    assert!(
        host.world_mut(left)
            .unwrap()
            .resource_snapshots()
            .iter()
            .any(|resource| resource.id == key.to_u64())
    );
    assert!(
        host.world_mut(right)
            .unwrap()
            .resource_snapshots()
            .iter()
            .any(|resource| resource.id == key.to_u64())
    );
    assert_eq!(
        view(&mut host, left, l_entity).columns[0].values,
        [R::Valid(V::F32(10.0))]
    );
    let right_before = view(&mut host, right, r_entity);
    assert_eq!(right_before.columns[0].values, [R::Valid(V::F32(15.0))]);
    apply(
        &mut host,
        left,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(l_entity),
            component: C::BUFFER_DATA_SOURCE_BINDING,
            name: "scaled_parameter".into(),
            value: V::F32(4.0),
        }],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, left, l_entity).columns[0].values,
        [R::Valid(V::F32(20.0))]
    );
    assert_eq!(view(&mut host, right, r_entity), right_before);
    host.destroy_world(left);
    host.asset_resources_mut().set_idle_resident_bytes_target(0);
    host.flush_resource_lifecycle();
    assert!(host.asset_resources().get(key).unwrap().data().is_some());
    assert_eq!(
        view(&mut host, right, r_entity).columns[0].values,
        [R::Valid(V::F32(15.0))]
    );
    host.destroy_world(right);
    host.flush_resource_lifecycle();
    assert!(host.asset_resources().get(key).is_none());
}

#[test]
fn absent_parameters_preserve_lazy_fallback_and_reached_missing_input_results() {
    for streaming in [false, true] {
        let mut host = crate::support::task_scheduler::host();
        let world = world(&mut host);
        let source = producer(
            &mut host,
            "dataset:optional",
            if streaming {
                DataSourceKind::Streaming
            } else {
                DataSourceKind::Buffer
            },
            vec![DataColumn::new("select", K::Bool)],
        );
        let mut properties = ipp_core::DynamicProperties::default();
        let declarations = [
            (
                "direct",
                definition(&[("parameter", K::F32)], vec![N::Input(0)], 0),
            ),
            (
                "fallback",
                definition(
                    &[("parameter", K::F32)],
                    vec![
                        N::Input(0),
                        N::Constant(V::F32(17.0)),
                        N::Fallback {
                            value: 0,
                            replacement: 1,
                        },
                    ],
                    2,
                ),
            ),
            (
                "lazy",
                definition(
                    &[("column:select", K::Bool), ("parameter", K::F32)],
                    vec![
                        N::Input(0),
                        N::Input(1),
                        N::Constant(V::F32(23.0)),
                        N::Ternary {
                            condition: 0,
                            then_node: 1,
                            else_node: 2,
                        },
                    ],
                    3,
                ),
            ),
        ];
        for (index, (name, declaration)) in declarations.into_iter().enumerate() {
            let expression = asset(&mut host, world, index as u64 + 1, declaration);
            properties.set(name, V::Asset(expression)).unwrap();
        }
        // Streaming must register retention before arrival; neither binding has parameters.
        let binding = if streaming {
            C::StreamingDataSourceBinding(StreamingDataSourceBinding {
                source: "dataset:optional".into(),
                properties,
                ..Default::default()
            })
        } else {
            C::BufferDataSourceBinding(BufferDataSourceBinding {
                source: "dataset:optional".into(),
                properties,
                ..Default::default()
            })
        };
        let entity = create(&mut host, world, "optional", binding);
        append(
            &mut host,
            source,
            vec![vec![V::Bool(false)], vec![V::Bool(true)]],
        );
        host.frame_for_test(0.0).unwrap();
        let result = view(&mut host, world, entity);
        assert_eq!(result.availability, DataBindingAvailability::Ready);
        assert_eq!(result.row_ids, [DataRowId(1), DataRowId(2)]);
        assert_eq!(
            result.columns[0].values,
            [
                R::Invalid(ExpressionInvalid::MissingInput {
                    slot: 0
                }),
                R::Invalid(ExpressionInvalid::MissingInput {
                    slot: 0
                })
            ]
        );
        assert_eq!(
            result.columns[1].values,
            [R::Valid(V::F32(17.0)), R::Valid(V::F32(17.0))]
        );
        assert_eq!(
            result.columns[2].values,
            [
                R::Valid(V::F32(23.0)),
                R::Invalid(ExpressionInvalid::MissingInput {
                    slot: 1
                })
            ]
        );
    }
}

#[test]
fn optional_parameter_removal_recreation_and_retype_rebuild_input_descriptors() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:lifetimes",
        DataSourceKind::Buffer,
        vec![DataColumn::new("raw", K::U32)],
    );
    append(&mut host, source, vec![vec![V::U32(5)]]);
    let expression = asset(
        &mut host,
        world,
        1,
        definition(
            &[("parameter", K::F32)],
            vec![
                N::Input(0),
                N::Constant(V::F32(31.0)),
                N::Fallback {
                    value: 0,
                    replacement: 1,
                },
            ],
            2,
        ),
    );
    let entity = create(
        &mut host,
        world,
        "lifetimes",
        C::BufferDataSourceBinding(buffer("dataset:lifetimes", "output", expression)),
    );
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(31.0))]
    );
    let set = |name: &str, value| Command::SetDynamicProperty {
        entity: EntityRef::Handle(entity),
        component: C::BUFFER_DATA_SOURCE_BINDING,
        name: name.into(),
        value,
    };
    let remove = || Command::RemoveDynamicProperty {
        entity: EntityRef::Handle(entity),
        component: C::BUFFER_DATA_SOURCE_BINDING,
        name: "output_parameter".into(),
    };
    for (commands, expected) in [
        (vec![set("output_parameter", V::F32(7.0))], 7.0),
        // Reuse released storage with another companion; it must never supply this input.
        (
            vec![remove(), set("unrelated_parameter", V::F32(99.0))],
            31.0,
        ),
        (vec![set("output_parameter", V::F32(11.0))], 11.0),
    ] {
        apply(&mut host, world, commands).result.unwrap();
        let result = view(&mut host, world, entity);
        assert_eq!(result.availability, DataBindingAvailability::Ready);
        assert_eq!(result.columns[0].values, [R::Valid(V::F32(expected))]);
    }
    apply(&mut host, world, vec![set("output_parameter", V::U32(123))])
        .result
        .unwrap();
    let wrong = view(&mut host, world, entity);
    assert_eq!(
        wrong.availability,
        DataBindingAvailability::Unavailable(DataBindingUnavailable::InputType {
            output: "output".into(),
            input: "parameter".into()
        })
    );
    assert!(wrong.columns.is_empty());
    assert!(wrong.row_ids.is_empty());
    apply(&mut host, world, vec![remove()]).result.unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(31.0))]
    );
    apply(
        &mut host,
        world,
        vec![set("output_parameter", V::F32(13.0))],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(13.0))]
    );
}

#[derive(Clone, Copy, Default)]
enum ConsumerTestMode {
    #[default]
    Skip,
    Fail,
    Succeed,
}

#[derive(Default)]
struct ConsumerTestSystem {
    members: std::collections::BTreeSet<EntityId>,
    consumers: std::collections::BTreeMap<EntityId, DataBindingPresentationConsumer>,
    mode: ConsumerTestMode,
    observed: Vec<(bool, Vec<R>)>,
    failures: usize,
    stale_rejections: usize,
    reference: Option<f64>,
    interpolation_requests: usize,
    interpolation_steps: usize,
}

impl ConsumerTestSystem {
    const ID: SystemId = SystemId("fixture.binding-consumer");
}

struct ConsumerTestFactory;

impl ipp_core::systems::SystemFactory for ConsumerTestFactory {
    fn id(&self) -> SystemId {
        ConsumerTestSystem::ID
    }

    fn dependencies(&self) -> &[ipp_core::systems::SystemDependency] {
        &[ipp_core::systems::SystemDependency::Required(
            DataBindingSystem::ID,
        )]
    }

    fn capabilities(&self) -> ipp_core::systems::SystemCapabilities {
        ipp_core::systems::SystemCapabilities::new([C::SCALAR], [])
    }

    fn create(
        &self,
        _: &mut ipp_core::systems::SystemInitContext<'_>,
    ) -> Result<Box<dyn ipp_core::systems::System>, ipp_core::systems::SystemInitError> {
        Ok(Box::new(ConsumerTestSystem::default()))
    }
}

impl ipp_core::systems::System for ConsumerTestSystem {
    fn after_commit(&mut self, context: &mut ipp_core::systems::SystemCommitContext<'_>) {
        for (entity, component) in context.changed_components() {
            if ![C::SCALAR, C::BUFFER_DATA_SOURCE_BINDING].contains(&component) {
                continue;
            }
            let world = context.world();
            if world.component_incarnation(entity, C::SCALAR).is_some()
                && world
                    .component_incarnation(entity, C::BUFFER_DATA_SOURCE_BINDING)
                    .is_some()
            {
                self.members.insert(entity);
            } else {
                self.members.remove(&entity);
                self.consumers.remove(&entity);
            }
        }
    }

    fn update(&mut self, context: &mut ipp_core::systems::SystemUpdateContext<'_, '_>) {
        for &entity in &self.members {
            let consumer = context
                .world
                .register_data_binding_presentation_consumer(entity, C::SCALAR)
                .unwrap();
            if let Some(previous) = self.consumers.insert(entity, consumer)
                && previous != consumer
            {
                assert_eq!(
                    context.world.finish_data_binding_presentation(previous),
                    Err(ErrorReason::InvalidEntity)
                );
                assert_eq!(
                    context.data_binding_interpolation_request(previous),
                    Err(ErrorReason::InvalidEntity)
                );
                self.stale_rejections += 1;
            }
            if let Some(request) = context
                .data_binding_interpolation_request(consumer)
                .unwrap()
            {
                self.interpolation_requests += 1;
                if matches!(self.mode, ConsumerTestMode::Succeed) {
                    let references: Vec<_> = request
                        .outputs
                        .iter()
                        .map(|output| DataBindingInterpolationReference {
                            output,
                            maximum: self.reference.expect("test reference"),
                        })
                        .collect();
                    context
                        .advance_data_binding_interpolation(consumer, &references)
                        .unwrap();
                    self.interpolation_steps += 1;
                    assert_eq!(
                        context.advance_data_binding_interpolation(consumer, &references),
                        Ok(false)
                    );
                }
            }
            let view = context
                .world
                .data_binding_view(entity, Default::default())
                .unwrap();
            assert_eq!(*view.availability, DataBindingAvailability::Ready);
            self.observed
                .push((view.dirty, view.columns[0].values.to_vec()));
            // This local result represents successful/failed preparation; no rendering
            // implementation or alternate World mutation route is involved.
            let prepared = match self.mode {
                ConsumerTestMode::Skip => continue,
                ConsumerTestMode::Fail => Err(ErrorReason::Unavailable),
                ConsumerTestMode::Succeed => Ok(()),
            };
            match prepared {
                Ok(()) => context
                    .world
                    .finish_data_binding_presentation(consumer)
                    .unwrap(),
                Err(_) => self.failures += 1,
            }
        }
    }
}

#[test]
fn explicit_percentage_reference_changes_and_rate_mode_switches_keep_the_display() {
    let mut host = crate::support::task_scheduler::host();
    let world = world(&mut host);
    let source = producer(
        &mut host,
        "dataset:percentage",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::F32)],
    );
    append(&mut host, source, vec![vec![V::F32(10.0)]]);
    let expression = identity(&mut host, world, 1, "x", K::F32);
    let mut binding = buffer("dataset:percentage", "x", expression);
    binding
        .properties
        .set("x_interp_percent", V::F32(10.0))
        .unwrap();
    binding
        .properties
        .set("x_interp_reference", V::F32(20.0))
        .unwrap();
    let entity = create(
        &mut host,
        world,
        "percentage",
        C::BufferDataSourceBinding(binding),
    );
    host.data_sources_mut()
        .apply_batch(
            source,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: vec![V::F32(100.0)],
            }],
        )
        .unwrap();
    let scalar = |host: &mut HostRuntime| view(host, world, entity).columns[0].values[0].clone();
    host.frame_for_test(0.5).unwrap();
    assert_eq!(scalar(&mut host), R::Valid(V::F32(11.0)));
    apply(
        &mut host,
        world,
        vec![set_binding_property(
            entity,
            "x_interp_reference",
            V::F32(0.0),
        )],
    )
    .result
    .unwrap();
    host.frame_for_test(100.0).unwrap();
    assert_eq!(scalar(&mut host), R::Valid(V::F32(11.0)));
    apply(
        &mut host,
        world,
        vec![set_binding_property(
            entity,
            "x_interp_reference",
            V::F32(40.0),
        )],
    )
    .result
    .unwrap();
    host.frame_for_test(0.5).unwrap();
    assert_eq!(scalar(&mut host), R::Valid(V::F32(13.0)));
    let remove = |name: &str| Command::RemoveDynamicProperty {
        entity: EntityRef::Handle(entity),
        component: C::BUFFER_DATA_SOURCE_BINDING,
        name: name.into(),
    };
    apply(
        &mut host,
        world,
        vec![
            remove("x_interp_percent"),
            remove("x_interp_reference"),
            set_binding_property(entity, "x_interp", V::F32(4.0)),
        ],
    )
    .result
    .unwrap();
    assert_eq!(scalar(&mut host), R::Valid(V::F32(13.0)));
    host.frame_for_test(0.5).unwrap();
    assert_eq!(scalar(&mut host), R::Valid(V::F32(15.0)));
    apply(
        &mut host,
        world,
        vec![
            remove("x_interp"),
            set_binding_property(entity, "x_interp_reference", V::F32(10.0)),
            set_binding_property(entity, "x_interp_percent", V::F32(50.0)),
        ],
    )
    .result
    .unwrap();
    host.frame_for_test(0.5).unwrap();
    assert_eq!(scalar(&mut host), R::Valid(V::F32(17.5)));
    apply(&mut host, world, vec![remove("x_interp_reference")])
        .result
        .unwrap();
    host.frame_for_test(20.0).unwrap();
    assert_eq!(scalar(&mut host), R::Valid(V::F32(17.5))); // Implicit ref has no consumer; no stale explicit rate.
    assert_eq!(
        view(&mut host, world, entity).availability,
        DataBindingAvailability::Ready
    );
}

#[test]
fn percentage_consumer_authority_advances_once_and_missed_frames_do_not_catch_up() {
    let mut factories = ipp_core::systems::compiled_system_factories();
    factories.push(std::sync::Arc::new(ConsumerTestFactory));
    let mut host = crate::support::task_scheduler::with_factories(factories).unwrap();
    let world = host
        .create_world(
            WorldLimits::default(),
            &[
                ConsumerTestSystem::ID,
                DataBindingSystem::ID,
                SystemId("ipp.asset-dependencies"),
            ],
        )
        .unwrap();
    let source = producer(
        &mut host,
        "dataset:consumer-percent",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::F32)],
    );
    append(&mut host, source, vec![vec![V::F32(10.0)]]);
    let expression = identity(&mut host, world, 1, "x", K::F32);
    let mut binding = buffer("dataset:consumer-percent", "x", expression.clone());
    binding
        .properties
        .set("x_interp_percent", V::F32(10.0))
        .unwrap();
    let entity = create(
        &mut host,
        world,
        "consumer-percent",
        C::BufferDataSourceBinding(binding),
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            C::Scalar(Default::default()),
        )],
    )
    .result
    .unwrap();
    let mode = |host: &mut HostRuntime, mode| {
        host.world_mut(world)
            .unwrap()
            .with_system::<ConsumerTestSystem, _>(ConsumerTestSystem::ID, |system, _| {
                system.mode = mode;
                system.reference = Some(10.0);
            })
            .unwrap()
    };
    mode(&mut host, ConsumerTestMode::Succeed);
    host.frame_for_test(1.0).unwrap();
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .system::<ConsumerTestSystem>(ConsumerTestSystem::ID)
            .unwrap()
            .interpolation_requests,
        0
    );
    host.data_sources_mut()
        .apply_batch(
            source,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: vec![V::F32(100.0)],
            }],
        )
        .unwrap();
    host.frame_for_test(1.0).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(11.0))]
    );
    mode(&mut host, ConsumerTestMode::Skip);
    host.frame_for_test(50.0).unwrap();
    mode(&mut host, ConsumerTestMode::Succeed);
    host.frame_for_test(1.0).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(12.0))]
    );
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: C::SCALAR,
            },
            Command::insert_value(EntityRef::Handle(entity), C::Scalar(Default::default())),
        ],
    )
    .result
    .unwrap();
    host.frame_for_test(1.0).unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(13.0))]
    );
    let mut replacement = buffer("dataset:consumer-percent", "x", expression);
    replacement
        .properties
        .set("x_interp_percent", V::F32(10.0))
        .unwrap();
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: C::BUFFER_DATA_SOURCE_BINDING,
            },
            Command::insert_value(
                EntityRef::Handle(entity),
                C::BufferDataSourceBinding(replacement),
            ),
        ],
    )
    .result
    .unwrap();
    assert_eq!(
        view(&mut host, world, entity).columns[0].values,
        [R::Valid(V::F32(100.0))]
    );
    let context = host.world_mut(world).unwrap();
    let consumer = context
        .system::<ConsumerTestSystem>(ConsumerTestSystem::ID)
        .unwrap();
    assert_eq!(consumer.stale_rejections, 2);
    assert_eq!(consumer.interpolation_steps, 3);
}

#[test]
fn ordinary_system_reads_completed_views_and_acknowledges_only_success() {
    let mut factories = ipp_core::systems::compiled_system_factories();
    factories.push(std::sync::Arc::new(ConsumerTestFactory));
    let mut host = crate::support::task_scheduler::with_factories(factories).unwrap();
    let world = host
        .create_world(
            WorldLimits::default(),
            &[
                ConsumerTestSystem::ID,
                DataBindingSystem::ID,
                SystemId("ipp.asset-dependencies"),
            ],
        )
        .unwrap();
    let source = producer(
        &mut host,
        "dataset:system-consumer",
        DataSourceKind::Buffer,
        vec![DataColumn::new("x", K::U32)],
    );
    append(&mut host, source, vec![vec![V::U32(2)]]);
    let expression = identity(&mut host, world, 1, "x", K::U32);
    let entity = create(
        &mut host,
        world,
        "consumer",
        C::BufferDataSourceBinding(buffer("dataset:system-consumer", "x", expression)),
    );
    assert!(view(&mut host, world, entity).dirty); // Zero consumer and read-only observation.
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            C::Scalar(Default::default()),
        )],
    )
    .result
    .unwrap();
    assert!(view(&mut host, world, entity).dirty); // Registered System skipped preparation.
    let set_mode = |host: &mut HostRuntime, mode| {
        host.world_mut(world)
            .unwrap()
            .with_system::<ConsumerTestSystem, _>(ConsumerTestSystem::ID, |system, _| {
                system.mode = mode
            })
            .unwrap();
    };
    set_mode(&mut host, ConsumerTestMode::Fail);
    host.frame_for_test(0.0).unwrap();
    assert!(view(&mut host, world, entity).dirty);
    set_mode(&mut host, ConsumerTestMode::Succeed);
    host.frame_for_test(0.0).unwrap();
    assert!(!view(&mut host, world, entity).dirty);
    host.world_mut(world)
        .unwrap()
        .with_system::<ConsumerTestSystem, _>(ConsumerTestSystem::ID, |system, runtime| {
            assert_eq!(system.failures, 1);
            assert_eq!(system.observed.len(), 3);
            assert!(
                system
                    .observed
                    .iter()
                    .all(|(dirty, values)| *dirty && *values == [R::Valid(V::U32(2))])
            );
            // The owned System query also remains a read-only observation.
            assert!(
                !runtime
                    .data_binding_view_owned(entity, Default::default())
                    .unwrap()
                    .dirty
            );
        })
        .unwrap();
    set_mode(&mut host, ConsumerTestMode::Skip);
    append(&mut host, source, vec![vec![V::U32(4)]]);
    host.frame_for_test(0.0).unwrap();
    assert!(view(&mut host, world, entity).dirty);
    set_mode(&mut host, ConsumerTestMode::Succeed);
    host.frame_for_test(0.0).unwrap();
    assert!(!view(&mut host, world, entity).dirty);
    // Replacing the existing consumer rebuilds initial state and rejects its old handle.
    set_mode(&mut host, ConsumerTestMode::Skip);
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            C::Scalar(Default::default()),
        )],
    )
    .result
    .unwrap();
    assert!(view(&mut host, world, entity).dirty);
    host.world_mut(world)
        .unwrap()
        .with_system::<ConsumerTestSystem, _>(ConsumerTestSystem::ID, |system, runtime| {
            assert_eq!(system.stale_rejections, 1);
            runtime
                .release_data_binding_presentation_consumer(
                    system.consumers.remove(&entity).unwrap(),
                )
                .unwrap();
        })
        .unwrap();
    assert!(view(&mut host, world, entity).dirty);
    apply(
        &mut host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: C::SCALAR,
        }],
    )
    .result
    .unwrap();
    host.world_mut(world)
        .unwrap()
        .with_system::<ConsumerTestSystem, _>(ConsumerTestSystem::ID, |system, _| {
            assert!(system.members.is_empty())
        })
        .unwrap();
}
