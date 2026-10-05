use super::*;
use crate::DynamicValue;
use crate::expressions::{ExpressionInvalid, ExpressionResult};
use crate::services::data::DataRowId;
use crate::systems::data_bindings::DataBindingColumnView;

fn numbers(values: &[f32]) -> Vec<ExpressionResult> {
    values
        .iter()
        .map(|&value| ExpressionResult::Valid(DynamicValue::F32(value)))
        .collect()
}

fn input<'a>(
    ids: &'a [DataRowId],
    columns: &[(&'a str, &'a [ExpressionResult])],
) -> PlotPreparedInput<'a> {
    PlotPreparedInput {
        row_ids: ids,
        columns: columns
            .iter()
            .map(|&(name, values)| DataBindingColumnView {
                name,
                kind: DynamicPropertyKind::F32,
                values,
            })
            .collect(),
    }
}

fn series(row: PlotSeriesRow) -> Rows<PlotSeriesRow> {
    let mut rows = Rows::default();
    rows.push(row).unwrap();
    rows
}

#[test]
fn associations_follow_actual_selectors_and_allow_reuse_on_one_axis() {
    let mut rows = series(PlotSeriesRow {
        x: "position".into(),
        y: "temperature".into(),
        ..Default::default()
    });
    rows.push(PlotSeriesRow {
        x: "position".into(),
        y: "temperature".into(),
        visible: false,
        ..Default::default()
    })
    .unwrap();
    let frame = PlotFrame2d::default();
    let chart = CartesianChart::Canvas {
        frame: &frame,
        series: &rows,
        bars: false,
    };
    assert_eq!(chart.axis("temperature"), Ok(1));
    assert_eq!(chart.axis("position"), Ok(0));
    assert_eq!(chart.axis("y"), Err(ErrorReason::InvalidField));
    assert_eq!(chart.axis("color"), Err(ErrorReason::InvalidField));

    rows.push(PlotSeriesRow {
        x: "temperature".into(),
        y: "other".into(),
        visible: false,
        ..Default::default()
    })
    .unwrap();
    let chart = CartesianChart::Canvas {
        frame: &frame,
        series: &rows,
        bars: false,
    };
    assert_eq!(chart.axis("temperature"), Err(ErrorReason::InvalidValue));
}

#[test]
fn three_dimensional_bars_use_value_for_y_and_other_charts_use_y() {
    let rows = series(PlotSeriesRow {
        y: "unused".into(),
        value: "height".into(),
        ..Default::default()
    });
    let frame = PlotFrame3d::default();
    let bars = CartesianChart::Scene {
        frame: &frame,
        series: &rows,
        bars: true,
    };
    assert_eq!(bars.axis("height"), Ok(1));
    assert_eq!(bars.axis("unused"), Err(ErrorReason::InvalidField));
    assert_eq!(bars.axis("z"), Ok(2));
    let points = CartesianChart::Scene {
        frame: &frame,
        series: &rows,
        bars: false,
    };
    assert_eq!(points.axis("unused"), Ok(1));
    assert_eq!(points.axis("height"), Err(ErrorReason::InvalidField));
}

#[test]
fn automatic_bar_references_include_zero_baseline_spacing_and_signed_endpoints() {
    let ids = [DataRowId(2), DataRowId(7)];
    let x = numbers(&[-3.0, 1.0]);
    let values = numbers(&[-40.0, -10.0]);
    let rows = series(PlotSeriesRow {
        y: "temperature".into(),
        ..Default::default()
    });
    let frame = PlotFrame2d::default();
    let chart = CartesianChart::Canvas {
        frame: &frame,
        series: &rows,
        bars: true,
    };
    let outputs = vec!["x".into(), "temperature".into()];
    let references = chart
        .references(
            &input(&ids, &[("x", &x), ("temperature", &values)]),
            &outputs,
        )
        .unwrap();
    assert_eq!(references[0].maximum, 5.0);
    assert_eq!(references[1].maximum, 40.0);
}

#[test]
fn fitted_reference_tracks_display_changes_without_using_authored_bounds() {
    let ids = [DataRowId(1), DataRowId(2)];
    let x = numbers(&[0.0, 1.0]);
    let rows = series(PlotSeriesRow {
        y: "height".into(),
        ..Default::default()
    });
    let mut frame = PlotFrame2d {
        min_y: -999.0,
        max_y: 999.0,
        ..Default::default()
    };
    let outputs = vec!["height".into()];
    for (display, expected) in [
        ([10.0, 20.0], 20.0),
        ([20.0, 35.0], 35.0),
        ([-70.0, 10.0], 70.0),
    ] {
        let height = numbers(&display);
        let chart = CartesianChart::Canvas {
            frame: &frame,
            series: &rows,
            bars: false,
        };
        let references = chart
            .references(&input(&ids, &[("x", &x), ("height", &height)]), &outputs)
            .unwrap();
        assert_eq!(references[0].maximum, expected);
    }
    frame.automatic_y = false;
    let height = numbers(&[10.0, 20.0]);
    let chart = CartesianChart::Canvas {
        frame: &frame,
        series: &rows,
        bars: false,
    };
    assert_eq!(
        chart
            .references(&input(&ids, &[("x", &x), ("height", &height)]), &outputs)
            .unwrap()[0]
            .maximum,
        999.0
    );
}

#[test]
fn references_preserve_each_existing_degenerate_fit_rule() {
    let ids = [DataRowId(1)];
    let x = numbers(&[0.0]);
    let y = numbers(&[-10.0]);
    let rows = series(PlotSeriesRow {
        z: "".into(),
        ..Default::default()
    });
    let outputs = vec!["x".into(), "y".into()];
    let frame2d = PlotFrame2d::default();
    let chart2d = CartesianChart::Canvas {
        frame: &frame2d,
        series: &rows,
        bars: false,
    };
    let references = chart2d
        .references(&input(&ids, &[("x", &x), ("y", &y)]), &outputs)
        .unwrap();
    assert_eq!(references[0].maximum, 0.05);
    assert_eq!(references[1].maximum, 10.5);
    let frame3d = PlotFrame3d::default();
    let chart3d = CartesianChart::Scene {
        frame: &frame3d,
        series: &rows,
        bars: false,
    };
    let references = chart3d
        .references(&input(&ids, &[("x", &x), ("y", &y)]), &outputs)
        .unwrap();
    assert_eq!(references[0].maximum, 0.5);
    assert_eq!(references[1].maximum, 15.0);
}

#[test]
fn references_fit_only_valid_visible_samples_and_require_scalar_f32() {
    let ids = [DataRowId(1), DataRowId(2), DataRowId(3)];
    let x = numbers(&[0.0, 1.0, 2.0]);
    let mut y = numbers(&[5.0, 10.0, 900.0]);
    y[2] = ExpressionResult::Invalid(ExpressionInvalid::Calculation);
    let hidden = numbers(&[9000.0; 3]);
    let mut rows = series(PlotSeriesRow::default());
    rows.push(PlotSeriesRow {
        y: "hidden".into(),
        visible: false,
        ..Default::default()
    })
    .unwrap();
    let frame = PlotFrame2d::default();
    let chart = CartesianChart::Canvas {
        frame: &frame,
        series: &rows,
        bars: false,
    };
    let outputs = vec!["y".into()];
    let mut input = input(&ids, &[("x", &x), ("y", &y), ("hidden", &hidden)]);
    assert_eq!(chart.references(&input, &outputs).unwrap()[0].maximum, 10.0);
    input.columns[1].kind = DynamicPropertyKind::I32;
    assert!(matches!(
        chart.references(&input, &outputs),
        Err(ErrorReason::InvalidValue)
    ));
}
