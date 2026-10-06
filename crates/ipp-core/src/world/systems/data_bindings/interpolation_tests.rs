use super::*;
use crate::{
    DynamicPropertyKind,
    expressions::{ExpressionDeclaration, ExpressionInvalid, ExpressionNode, PreparedExpression},
    services::asset_management::AssetKey,
};

const RATE: f32 = 2.0;

fn ids(values: &[u64]) -> Vec<DataRowId> {
    values.iter().copied().map(DataRowId).collect()
}

fn scalar(value: f32) -> ExpressionResult {
    ExpressionResult::Valid(DynamicValue::F32(value))
}

fn invalid() -> ExpressionResult {
    ExpressionResult::Invalid(ExpressionInvalid::Calculation)
}

/// A settled fixed-rate scalar column displaying `values` at their targets.
fn column(values: &[f32]) -> (DynamicProperties, PreparedColumn) {
    let mut properties = DynamicProperties::default();
    properties.set("y_interp", DynamicValue::F32(RATE)).unwrap();
    let plan = PreparedExpression::prepare(&ExpressionDeclaration {
        inputs: vec![],
        nodes: vec![ExpressionNode::Constant(DynamicValue::F32(0.0))],
        output: 0,
    })
    .unwrap();
    let mut interpolation = new(ColumnInterpolationRate::Fixed(
        properties.descriptors()["y_interp"],
    ));
    interpolation.targets = values.iter().copied().map(scalar).collect();
    let column = PreparedColumn {
        name: "y".into(),
        kind: DynamicPropertyKind::F32,
        asset: AssetKey {
            slot: 0,
            generation: 0,
        },
        scratch: plan.scratch(),
        plan,
        raw_identity: None,
        inputs: vec![],
        parameters: vec![],
        values: values.iter().copied().map(scalar).collect(),
        interpolation: Some(interpolation),
    };
    (properties, column)
}

/// Reconcile like the evaluator, then publish the newly projected targets.
fn arrive(
    column: &mut PreparedColumn,
    previous: &[DataRowId],
    current: &[DataRowId],
    by_position: bool,
    targets: &[f32],
) {
    let indices = correspondence(previous, current.iter().copied(), by_position);
    reorder(column, &indices);
    let interpolation = column.interpolation.as_mut().unwrap();
    for (stored, &target) in interpolation.targets.iter_mut().zip(targets) {
        *stored = scalar(target);
    }
    retarget(column);
}

fn active(column: &PreparedColumn) -> Vec<usize> {
    column
        .interpolation
        .as_ref()
        .unwrap()
        .active_rows
        .iter()
        .map(|row| row.index)
        .collect()
}

#[test]
fn correspondence_follows_identity_by_default_and_persisting_slots_by_position() {
    let previous = ids(&[1, 2, 3]);
    assert_eq!(
        correspondence(&previous, ids(&[2, 3, 4]).into_iter(), false),
        [Some(1), Some(2), None]
    );
    assert_eq!(
        correspondence(&previous, ids(&[4, 5, 6]).into_iter(), false),
        [None, None, None]
    );
    assert_eq!(
        correspondence(&previous, ids(&[4, 5, 6]).into_iter(), true),
        [Some(0), Some(1), Some(2)]
    );
    assert_eq!(
        correspondence(&ids(&[1, 2]), ids(&[3, 4, 5]).into_iter(), true),
        [Some(0), Some(1), None]
    );
    assert_eq!(
        correspondence(&previous, ids(&[4]).into_iter(), true),
        [Some(0)]
    );
    assert_eq!(
        correspondence(&[], ids(&[1, 2]).into_iter(), true),
        [None, None]
    );
}

#[test]
fn positional_window_replacement_retargets_every_slot_from_its_display() {
    let (properties, mut column) = column(&[10.0, 20.0, 30.0]);
    arrive(
        &mut column,
        &ids(&[1, 2, 3]),
        &ids(&[4, 5, 6]),
        true,
        &[15.0, 16.0, 35.0],
    );
    assert_eq!(column.values, [scalar(10.0), scalar(20.0), scalar(30.0)]);
    assert_eq!(active(&column), [0, 1, 2]);

    assert!(advance(&mut column, &properties, 1.0, None));
    assert_eq!(column.values, [scalar(12.0), scalar(18.0), scalar(32.0)]);
    advance(&mut column, &properties, 10.0, None);
    assert_eq!(column.values, [scalar(15.0), scalar(16.0), scalar(35.0)]);
    assert!(active(&column).is_empty());
}

#[test]
fn identity_window_replacement_initializes_new_rows_immediately() {
    let (properties, mut column) = column(&[10.0, 20.0, 30.0]);
    arrive(
        &mut column,
        &ids(&[1, 2, 3]),
        &ids(&[4, 5, 6]),
        false,
        &[15.0, 16.0, 35.0],
    );
    assert_eq!(column.values, [scalar(15.0), scalar(16.0), scalar(35.0)]);
    assert!(active(&column).is_empty());
    assert!(!advance(&mut column, &properties, 1.0, None));
}

#[test]
fn positional_growth_initializes_appearing_slots_immediately() {
    let (_, mut reordered) = column(&[10.0, 20.0]);
    let indices = correspondence(&ids(&[1, 2]), ids(&[3, 4, 5]).into_iter(), true);
    reorder(&mut reordered, &indices);
    assert_eq!(reordered.values, [scalar(10.0), scalar(20.0), invalid()]);
    assert_eq!(
        reordered.interpolation.as_ref().unwrap().targets,
        [scalar(10.0), scalar(20.0), invalid()]
    );

    let (properties, mut grown) = column(&[10.0, 20.0]);
    arrive(
        &mut grown,
        &ids(&[1, 2]),
        &ids(&[3, 4, 5]),
        true,
        &[14.0, 20.0, 50.0],
    );
    assert_eq!(grown.values, [scalar(10.0), scalar(20.0), scalar(50.0)]);
    assert_eq!(active(&grown), [0]);
    advance(&mut grown, &properties, 1.0, None);
    assert_eq!(grown.values, [scalar(12.0), scalar(20.0), scalar(50.0)]);
}

#[test]
fn positional_shrink_releases_leaving_slots_and_keeps_persisting_motion() {
    let (properties, mut column) = column(&[10.0, 20.0, 30.0]);
    let interpolation = column.interpolation.as_mut().unwrap();
    interpolation.targets = vec![scalar(20.0), scalar(20.0), scalar(40.0)];
    retarget(&mut column);
    assert_eq!(active(&column), [0, 2]);

    arrive(&mut column, &ids(&[1, 2, 3]), &ids(&[4]), true, &[0.0]);
    assert_eq!(column.values, [scalar(10.0)]);
    assert_eq!(
        column.interpolation.as_ref().unwrap().targets,
        [scalar(0.0)]
    );
    assert_eq!(active(&column), [0]);
    advance(&mut column, &properties, 1.0, None);
    assert_eq!(column.values, [scalar(8.0)]);
}

#[test]
fn positional_invalid_targets_stay_invalid_and_valid_ones_initialize_fresh() {
    let (_, mut column) = column(&[10.0, 20.0]);
    let indices = correspondence(&ids(&[1, 2]), ids(&[3, 4]).into_iter(), true);
    reorder(&mut column, &indices);
    let interpolation = column.interpolation.as_mut().unwrap();
    interpolation.targets = vec![invalid(), scalar(25.0)];
    retarget(&mut column);
    assert_eq!(column.values, [invalid(), scalar(20.0)]);
    assert_eq!(active(&column), [1]);

    let indices = correspondence(&ids(&[3, 4]), ids(&[5, 6]).into_iter(), true);
    reorder(&mut column, &indices);
    let interpolation = column.interpolation.as_mut().unwrap();
    interpolation.targets = vec![scalar(5.0), scalar(25.0)];
    retarget(&mut column);
    assert_eq!(column.values, [scalar(5.0), scalar(20.0)]);
    assert_eq!(active(&column), [1]);
}
