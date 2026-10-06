use super::super::runtime_state::{
    ColumnInterpolationRate, ColumnInterpolationRow, PreparedColumn,
};
use super::*;
use crate::{
    DynamicPropertyKind, DynamicValue, expressions::*, services::asset_management::AssetKey,
};

fn fixture() -> (DynamicProperties, DataBindingRuntime) {
    let mut properties = DynamicProperties::default();
    properties
        .set("a_interp_percent", DynamicValue::F32(10.0))
        .unwrap();
    properties.set("b_interp", DynamicValue::F32(4.0)).unwrap();
    let mut runtime = DataBindingRuntime::default();
    let plan = PreparedExpression::prepare(&ExpressionDeclaration {
        inputs: vec![],
        nodes: vec![ExpressionNode::Constant(DynamicValue::F32(0.0))],
        output: 0,
    })
    .unwrap();
    for (name, rate) in [
        (
            "a",
            ColumnInterpolationRate::Percent {
                percentage: properties.descriptors()["a_interp_percent"],
                reference: None,
            },
        ),
        (
            "b",
            ColumnInterpolationRate::Fixed(properties.descriptors()["b_interp"]),
        ),
    ] {
        let mut interpolation = interpolation::new(rate);
        interpolation.targets = vec![ExpressionResult::Valid(DynamicValue::F32(100.0))];
        interpolation.active_rows = vec![ColumnInterpolationRow {
            index: 0,
            residual: [0.0; 4],
        }];
        runtime.columns.push(PreparedColumn {
            name: name.into(),
            kind: DynamicPropertyKind::F32,
            asset: AssetKey {
                slot: 0,
                generation: 0,
            },
            scratch: plan.scratch(),
            plan: plan.clone(),
            raw_identity: None,
            inputs: vec![],
            parameters: vec![],
            values: vec![ExpressionResult::Valid(DynamicValue::F32(10.0))],
            interpolation: Some(interpolation),
        });
    }
    (properties, runtime)
}

fn value(runtime: &DataBindingRuntime, column: usize) -> f32 {
    match runtime.columns[column].values[0] {
        ExpressionResult::Valid(DynamicValue::F32(value)) => value,
        _ => panic!("scalar"),
    }
}

#[test]
fn references_validate_before_mixed_columns_step_and_each_frame_consumes_once() {
    let (properties, mut runtime) = fixture();
    interpolation::advance_or_defer(&mut runtime, &properties, 1, 1.0);
    assert_eq!(value(&runtime, 0), 10.0);
    assert_eq!(value(&runtime, 1), 10.0);
    assert!(matches!(
        advance(&properties, &mut runtime, &[], 1, 1.0),
        Err(DataBindingInterpolationError::MissingReference { .. })
    ));
    for references in [
        vec![DataBindingInterpolationReference {
            output: "a",
            maximum: f64::NAN,
        }],
        vec![DataBindingInterpolationReference {
            output: "wrong",
            maximum: 10.0,
        }],
        vec![
            DataBindingInterpolationReference {
                output: "a",
                maximum: 10.0
            };
            2
        ],
    ] {
        assert!(advance(&properties, &mut runtime, &references, 1, 1.0).is_err());
        assert_eq!(value(&runtime, 1), 10.0);
    }
    assert_eq!(
        advance(
            &properties,
            &mut runtime,
            &[DataBindingInterpolationReference {
                output: "a",
                maximum: 10.0
            }],
            1,
            1.0
        ),
        Ok(true)
    );
    assert_eq!(value(&runtime, 0), 11.0);
    assert_eq!(value(&runtime, 1), 14.0);
    assert_eq!(
        advance(
            &properties,
            &mut runtime,
            &[DataBindingInterpolationReference {
                output: "a",
                maximum: 100.0
            }],
            1,
            1.0
        ),
        Ok(false)
    );
    interpolation::advance_or_defer(&mut runtime, &properties, 2, 1.0);
    advance(
        &properties,
        &mut runtime,
        &[DataBindingInterpolationReference {
            output: "a",
            maximum: 11.0,
        }],
        2,
        1.0,
    )
    .unwrap();
    assert!((value(&runtime, 0) - 12.1).abs() < 0.00001);
}

#[test]
fn missed_reference_frames_and_zero_references_never_earn_catchup() {
    let (properties, mut runtime) = fixture();
    interpolation::advance_or_defer(&mut runtime, &properties, 1, 100.0);
    interpolation::advance_or_defer(&mut runtime, &properties, 2, 1.0);
    assert_eq!(
        advance(
            &properties,
            &mut runtime,
            &[DataBindingInterpolationReference {
                output: "a",
                maximum: 10.0
            }],
            1,
            100.0
        ),
        Ok(false)
    );
    advance(
        &properties,
        &mut runtime,
        &[DataBindingInterpolationReference {
            output: "a",
            maximum: 0.0,
        }],
        2,
        1.0,
    )
    .unwrap();
    assert_eq!(value(&runtime, 0), 10.0);
    assert_eq!(
        runtime.columns[0]
            .interpolation
            .as_ref()
            .unwrap()
            .active_rows[0]
            .residual,
        [0.0; 4]
    );
    interpolation::advance_or_defer(&mut runtime, &properties, 3, 1.0);
    advance(
        &properties,
        &mut runtime,
        &[DataBindingInterpolationReference {
            output: "a",
            maximum: 10.0,
        }],
        3,
        1.0,
    )
    .unwrap();
    assert_eq!(value(&runtime, 0), 11.0);
}
