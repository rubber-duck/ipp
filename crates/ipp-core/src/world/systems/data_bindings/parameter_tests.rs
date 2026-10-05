//! Private compiled numeric-notification boundary, before .6's animation integration.

use super::*;
use crate::{
    Batch, Command, ComponentValue, DynamicValue, EntityMetadata, EntityRef, ErrorReason,
    HostRuntime, WorldLimits,
    components::schema::FieldValue,
    expressions::*,
    services::{
        asset_management::{AssetSource, expression::EXPRESSION_TYPE},
        data::*,
    },
    systems::{System, SystemId, SystemNumericContext},
};

#[test]
fn numeric_parameter_invalidation_reads_the_live_typed_store_and_runtime_clone_is_empty() {
    let mut host = HostRuntime::new();
    crate::test_task_scheduler::install(&mut host);
    let world = host
        .create_world(
            WorldLimits::default(),
            &[DataBindingSystem::ID, SystemId("ipp.asset-dependencies")],
        )
        .unwrap();
    let declaration = ExpressionDeclaration {
        inputs: vec![ExpressionInput {
            name: "parameter".into(),
            kind: crate::DynamicPropertyKind::F32,
        }],
        nodes: vec![ExpressionNode::Input(0)],
        output: 0,
    };
    host.asset_resources_mut()
        .register_client_source(
            world,
            AssetSource {
                kind: EXPRESSION_TYPE,
                uri: format!("producer://{}/19/1", world.0).into(),
                variant: 0,
            },
            declaration.encode().unwrap(),
        )
        .unwrap();
    let producer = host
        .data_sources_mut()
        .create_source(
            "dataset:numeric".into(),
            DataSourceKind::Buffer,
            DataSchema {
                columns: vec![DataColumn::new("raw", crate::DynamicPropertyKind::U32)],
            },
        )
        .unwrap();
    host.data_sources_mut()
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: vec![vec![DynamicValue::U32(4)]],
            }],
        )
        .unwrap();
    let mut binding = BufferDataSourceBinding {
        source: "dataset:numeric".into(),
        ..Default::default()
    };
    binding
        .properties
        .set(
            "output",
            DynamicValue::Asset(AssetSource {
                kind: EXPRESSION_TYPE,
                uri: "asset://19/1".into(),
                variant: 0,
            }),
        )
        .unwrap();
    let key = binding
        .properties
        .set("output_parameter", DynamicValue::F32(1.0))
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: EntityMetadata {
                        symbolic_id: Some("binding".into()),
                        ..Default::default()
                    },
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(0),
                    ComponentValue::BufferDataSourceBinding(binding),
                ),
            ],
        })
        .unwrap();
    host.progress_assets();
    crate::test_task_scheduler::poll_ready();
    host.progress_assets();
    host.frame(0.0).unwrap();
    let mut context = host.world_mut(world).unwrap();
    let entity = context.lookup_id("binding").unwrap();
    let incarnation = context
        .data_binding_view(entity, Default::default())
        .unwrap()
        .binding_incarnation;
    let stored = context
        .world
        .components
        .buffer_data_source_binding_mut(entity.index() as usize)
        .unwrap();
    assert!(!stored.runtime.needs_prepare);
    let cloned = stored.clone();
    assert!(cloned.runtime.consumer.is_none());
    assert!(cloned.runtime.source.is_none());
    assert!(cloned.runtime.columns.is_empty());
    assert!(cloned.runtime.rows.is_empty());
    assert!(cloned.runtime.evaluated_tick.is_none());
    assert!(cloned.runtime.dirty);
    stored.runtime.dirty = false;
    context
        .with_system::<DataBindingSystem, _>(DataBindingSystem::ID, |system, runtime| {
            system.before_numeric_update(&mut SystemNumericContext {
                world_data: runtime.world,
                changed: &[(entity, ComponentValue::BUFFER_DATA_SOURCE_BINDING)],
            });
            runtime
                .world
                .components
                .write_numeric_properties(
                    ComponentValue::BUFFER_DATA_SOURCE_BINDING,
                    entity.index() as usize,
                    &[(key, FieldValue::Dynamic(DynamicValue::F32(8.0)))],
                )
                .unwrap();
        })
        .unwrap();
    // Numeric invalidation marks reevaluation; it cannot manufacture a new view.
    let before = context
        .data_binding_view(entity, Default::default())
        .unwrap();
    assert_eq!(
        before.columns[0].values,
        [ExpressionResult::Valid(DynamicValue::F32(1.0))]
    );
    assert!(!before.dirty);
    drop(context);
    host.progress_assets();
    crate::test_task_scheduler::poll_ready();
    host.progress_assets();
    host.frame(0.0).unwrap();
    let context = host.world_mut(world).unwrap();
    let after = context
        .data_binding_view(entity, Default::default())
        .unwrap();
    assert_eq!(
        after.columns[0].values,
        [ExpressionResult::Valid(DynamicValue::F32(8.0))]
    );
    assert_eq!(after.binding_incarnation, incarnation);
    assert!(after.dirty);
}

#[test]
fn numeric_interpolation_speed_writes_preserve_positive_finite_admission() {
    use crate::components::schema::ComponentLifecycle;

    let mut binding = BufferDataSourceBinding::default();
    let speed = binding
        .properties
        .set("output_interp", DynamicValue::F32(4.0))
        .unwrap();
    let parameter = binding
        .properties
        .set("output_parameter", DynamicValue::F32(4.0))
        .unwrap();
    for value in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(
            binding
                .validate_numeric_properties(&[(
                    speed,
                    FieldValue::Dynamic(DynamicValue::F32(value))
                )])
                .is_err()
        );
    }
    assert_eq!(
        binding
            .validate_numeric_properties(&[(speed, FieldValue::Dynamic(DynamicValue::F32(1.0)))]),
        Ok(())
    );
    assert_eq!(
        binding.validate_numeric_properties(&[(
            parameter,
            FieldValue::Dynamic(DynamicValue::F32(-1.0))
        )]),
        Ok(())
    );
    assert_eq!(
        binding.properties.get("output_interp"),
        Some(DynamicValue::F32(4.0))
    );
}

#[test]
fn percentage_rate_and_reference_admission_are_typed_and_modes_are_exclusive() {
    use crate::components::schema::ComponentLifecycle;

    let mut binding = BufferDataSourceBinding::default();
    let percentage = binding
        .properties
        .set("x_interp_percent", DynamicValue::F32(1.0))
        .unwrap();
    let reference = binding
        .properties
        .set("x_interp_reference", DynamicValue::F32(0.0))
        .unwrap();
    assert_eq!(binding.validate(), Ok(()));
    assert_eq!(
        binding.validate_numeric_properties(&[(
            reference,
            FieldValue::Dynamic(DynamicValue::F32(0.0))
        )]),
        Ok(())
    );
    for (key, value) in [
        (percentage, 0.0),
        (percentage, -1.0),
        (reference, -1.0),
        (reference, f32::INFINITY),
        (percentage, f32::NAN),
    ] {
        assert!(
            binding
                .validate_numeric_properties(&[(
                    key,
                    FieldValue::Dynamic(DynamicValue::F32(value))
                )])
                .is_err()
        );
    }
    binding
        .properties
        .set("x_interp", DynamicValue::F32(2.0))
        .unwrap();
    assert_eq!(binding.validate(), Err(ErrorReason::InvalidValue));
}
