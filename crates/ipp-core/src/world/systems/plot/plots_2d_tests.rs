//! Focused algorithm cases; execution is deferred to the combined Plot gate.

use super::*;
use crate::{
    DynamicValue,
    expressions::{ExpressionInvalid, ExpressionResult},
    services::data::DataRowId,
    systems::data_bindings::DataBindingColumnView,
};

fn values(values: &[f32]) -> Vec<ExpressionResult> {
    values
        .iter()
        .map(|value| ExpressionResult::Valid(DynamicValue::F32(*value)))
        .collect()
}

fn input<'a>(
    ids: &'a [DataRowId],
    x: &'a [ExpressionResult],
    y: &'a [ExpressionResult],
) -> PlotPreparedInput<'a> {
    PlotPreparedInput {
        row_ids: ids,
        columns: vec![
            DataBindingColumnView {
                name: "x",
                kind: DynamicPropertyKind::F32,
                values: x,
            },
            DataBindingColumnView {
                name: "y",
                kind: DynamicPropertyKind::F32,
                values: y,
            },
            DataBindingColumnView {
                name: "value",
                kind: DynamicPropertyKind::F32,
                values: y,
            },
        ],
    }
}

fn fixed_frame() -> PlotFrame2d {
    PlotFrame2d {
        width: 400.0,
        height: 300.0,
        automatic_x: false,
        automatic_y: false,
        min_x: 0.0,
        max_x: 10.0,
        min_y: 0.0,
        max_y: 100.0,
        ..Default::default()
    }
}

fn line(interpolation: u32) -> PlotLine2d {
    let mut chart = PlotLine2d {
        interpolation,
        marker_size: 0.0,
        ..Default::default()
    };
    chart.series.push(PlotSeriesRow::default()).unwrap();
    chart
}

#[test]
fn invalid_samples_split_both_interpolations_without_bridging_the_gap() {
    let ids: Vec<_> = (0..5).map(DataRowId).collect();
    let x = values(&[0.0, 2.0, 4.0, 8.0, 10.0]);
    let mut y = values(&[10.0, 65.0, 0.0, 35.0, 80.0]);
    y[2] = ExpressionResult::Invalid(ExpressionInvalid::Calculation);
    let frame = fixed_frame();
    let gap_left = PlotFrameMapping2d::new(&frame, [0.0, 0.0], [10.0, 100.0])
        .unwrap()
        .map([2.0, 0.0])[0]
        + 2.0;
    let gap_right = PlotFrameMapping2d::new(&frame, [0.0, 0.0], [10.0, 100.0])
        .unwrap()
        .map([8.0, 0.0])[0]
        - 2.0;
    for interpolation in [0, 1] {
        let geometry = prepare_line(&line(interpolation), &frame, &input(&ids, &x, &y)).unwrap();
        let paths: Vec<_> = geometry
            .canvas
            .iter()
            .filter(|paint| paint.color == PlotSeriesRow::default().color)
            .filter_map(|paint| match &paint.kind {
                PlotPrimitiveKind::Path(path) => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), 2);
        assert!(
            paths
                .iter()
                .all(|path| path.bounds[2] <= gap_left || path.bounds[0] >= gap_right)
        );
        assert_eq!(
            geometry
                .hits
                .iter()
                .map(|hit| hit.row.row_id.0)
                .collect::<Vec<_>>(),
            [0, 1, 3, 4]
        );
    }
}

#[test]
fn source_identity_and_series_slot_survive_deleted_rows_and_large_u64_labels() {
    let large = u64::MAX - 3;
    let ids = [DataRowId(large), DataRowId(5)];
    let x = values(&[2.0, 8.0]);
    let y = values(&[65.0, 80.0]);
    let mut chart = line(0);
    chart.series.remove(0);
    let slot = chart.series.push(PlotSeriesRow::default()).unwrap();
    chart
        .labels
        .push(PlotLabelRow {
            series: slot,
            row_id: large.to_string().into(),
            text: "exact identity".into(),
            highlighted: true,
            offset: [12.0, -40.0],
            ..Default::default()
        })
        .unwrap();
    let geometry = prepare_line(&chart, &fixed_frame(), &input(&ids, &x, &y)).unwrap();
    assert_eq!(
        geometry.hits[0].row,
        PlotRowIdentity {
            series: 1,
            row_id: DataRowId(large)
        }
    );
    assert!(geometry.canvas.iter().any(|paint| matches!(&paint.kind, PlotPrimitiveKind::Text { text, .. } if text.as_ref() == "exact identity")));
}

#[test]
fn automatic_bar_range_includes_zero_and_keeps_negative_direction() {
    let ids = [DataRowId(20), DataRowId(30)];
    let x = values(&[1.0, 2.0]);
    let y = values(&[-10.0, 20.0]);
    let mut chart = PlotBars2d::default();
    chart.series.push(PlotSeriesRow::default()).unwrap();
    let frame = PlotFrame2d::default();
    let geometry = prepare_bars(&chart, &frame, &input(&ids, &x, &y)).unwrap();
    let baseline = frame.padding_top
        + (frame.height - frame.padding_top - frame.padding_bottom) * (20.0 / 30.0);
    let PlotHitShape::Rect(negative) = geometry.hits[0].shape else {
        panic!("bar hit")
    };
    let PlotHitShape::Rect(positive) = geometry.hits[1].shape else {
        panic!("bar hit")
    };
    assert!((negative[1] - baseline).abs() < 1e-4);
    assert!((positive[3] - baseline).abs() < 1e-4);
    assert!(negative[3] > baseline && positive[1] < baseline);
}

#[test]
fn multiple_bar_series_use_one_range_and_disjoint_group_slots() {
    let ids = [DataRowId(0)];
    let x = values(&[4.0]);
    let y = values(&[40.0]);
    let mut chart = PlotBars2d::default();
    chart.series.push(PlotSeriesRow::default()).unwrap();
    chart
        .series
        .push(PlotSeriesRow {
            color: [1.0, 0.5, 0.0, 1.0],
            ..Default::default()
        })
        .unwrap();
    let geometry = prepare_bars(&chart, &PlotFrame2d::default(), &input(&ids, &x, &y)).unwrap();
    let PlotHitShape::Rect(a) = geometry.hits[0].shape else {
        panic!("bar hit")
    };
    let PlotHitShape::Rect(b) = geometry.hits[1].shape else {
        panic!("bar hit")
    };
    assert!(a[2] <= b[0]);
    assert_eq!((a[1], a[3]), (b[1], b[3]));
    assert_eq!(
        (geometry.hits[0].row.series, geometry.hits[1].row.series),
        (0, 1)
    );
}

#[test]
fn supplied_bin_values_are_not_counted_or_aggregated() {
    let ids = [DataRowId(9), DataRowId(10), DataRowId(11)];
    let x = values(&[1.0, 3.0, 5.0]);
    let y = values(&[2.0, 9.0, 3.0]);
    let mut chart = PlotBars2d::default();
    chart.series.push(PlotSeriesRow::default()).unwrap();
    let frame = fixed_frame();
    let geometry = prepare_bars(&chart, &frame, &input(&ids, &x, &y)).unwrap();
    assert_eq!(geometry.hits.len(), 3);
    let heights: Vec<_> = geometry
        .hits
        .iter()
        .map(|hit| match hit.shape {
            PlotHitShape::Rect(rect) => rect[3] - rect[1],
            _ => panic!("bar hit"),
        })
        .collect();
    assert!((heights[1] / heights[0] - 4.5).abs() < 1e-4);
    assert!((heights[2] / heights[0] - 1.5).abs() < 1e-4);
}

#[test]
fn pie_normalizes_positive_values_and_preserves_a_hollow_center() {
    let ids: Vec<_> = (0..5).map(DataRowId).collect();
    let x = values(&[0.0; 5]);
    let y = values(&[40.0, 30.0, 20.0, 10.0, -10.0]);
    let mut chart = PlotPie2d {
        inner_radius: 0.35,
        ..Default::default()
    };
    chart.series.push(PlotSeriesRow::default()).unwrap();
    let geometry = prepare_pie(&chart, &fixed_frame(), &input(&ids, &x, &y)).unwrap();
    assert_eq!(geometry.hits.len(), 4);
    let sweeps: Vec<_> = geometry
        .hits
        .iter()
        .map(|hit| match hit.shape {
            PlotHitShape::Sector {
                sweep,
                radius,
                inner_radius,
                ..
            } => {
                assert!((inner_radius / radius - 0.35).abs() < 1e-6);
                sweep
            }
            _ => panic!("sector hit"),
        })
        .collect();
    assert!((sweeps.iter().sum::<f32>() - TAU).abs() < 1e-5);
    assert!((sweeps[0] / TAU - 0.4).abs() < 1e-6);
    // The hole is a reversed inner arc, not an opaque disk painted over sectors.
    assert!(
        geometry
            .canvas
            .iter()
            .all(|paint| matches!(paint.kind, PlotPrimitiveKind::Path(_)))
    );
}

#[test]
fn monotone_smoothing_stays_within_each_sample_interval() {
    let points = [[0.0, 100.0], [30.0, 20.0], [60.0, 60.0], [100.0, 0.0]];
    let tangents = monotone_tangents(&points);
    assert_eq!(tangents[1], Some(0.0));
    assert_eq!(tangents[2], Some(0.0));
    for index in 0..points.len() - 1 {
        let a = points[index];
        let b = points[index + 1];
        let dx = (b[0] - a[0]) / 3.0;
        let c = a[1] + tangents[index].unwrap() * dx;
        let d = b[1] - tangents[index + 1].unwrap() * dx;
        for sample in 0..=100 {
            let t = sample as f32 / 100.0;
            let y = (1.0 - t).powi(3) * a[1]
                + 3.0 * (1.0 - t).powi(2) * t * c
                + 3.0 * (1.0 - t) * t * t * d
                + t.powi(3) * b[1];
            assert!(y >= a[1].min(b[1]) - 1e-4 && y <= a[1].max(b[1]) + 1e-4);
        }
    }
    assert_eq!(
        monotone_tangents(&[[0.0, 0.0], [0.0, 1.0], [-1.0, 2.0]]),
        [None, None, None]
    );
}

#[test]
fn mismatched_or_absent_selectors_fail_instead_of_drawing_zero() {
    let ids = [DataRowId(1)];
    let x = values(&[2.0]);
    let y = vec![ExpressionResult::Valid(DynamicValue::Bool(true))];
    assert!(prepare_line(&line(0), &fixed_frame(), &input(&ids, &x, &y)).is_err());
    let mut chart = line(0);
    chart.series.get_mut(0).unwrap().x = "absent".into();
    assert!(prepare_line(&chart, &fixed_frame(), &input(&ids, &x, &x)).is_err());
}

#[test]
fn exactly_straight_smooth_run_uses_nondegenerate_strip_edges() {
    let a = [10.0, 10.0];
    let b = [90.0, 80.0];
    let delta = sub(b, a);
    let cubic = [
        a,
        add(a, [delta[0] / 3.0, delta[1] / 3.0]),
        add(a, [delta[0] * 2.0 / 3.0, delta[1] * 2.0 / 3.0]),
        b,
    ];
    let mut contours = Vec::new();
    smooth_stroke(cubic, 2.5, [0.0, 0.0, 100.0, 100.0], &mut contours, 0);
    assert!(
        contours[0]
            .segments
            .iter()
            .all(|segment| matches!(segment, QuadraticSegment::Line { .. }))
    );
    assert_eq!(contours[0].segments.len(), 3);
}

#[test]
fn curved_stroke_preserves_the_cubic_midpoint_with_regular_strip_edges() {
    let mut contours = Vec::new();
    smooth_stroke(
        [[0.0, 90.0], [25.0, 10.0], [75.0, 10.0], [100.0, 90.0]],
        2.5,
        [0.0, 0.0, 100.0, 100.0],
        &mut contours,
        0,
    );
    let strips: Vec<_> = contours
        .iter()
        .filter(|contour| {
            contour
                .segments
                .iter()
                .all(|segment| matches!(segment, QuadraticSegment::Line { .. }))
        })
        .collect();
    assert!(
        strips.len() > 4,
        "a curved cubic cannot become its single endpoint chord"
    );
    let endpoints: Vec<_> = strips
        .iter()
        .map(|contour| {
            let QuadraticSegment::Line {
                to: b1,
            } = contour.segments[0]
            else {
                unreachable!()
            };
            let QuadraticSegment::Line {
                to: b2,
            } = contour.segments[1]
            else {
                unreachable!()
            };
            mid(b1, b2)
        })
        .collect();
    assert!(
        endpoints
            .iter()
            .any(|point| (point[0] - 50.0).abs() < 1e-4 && (point[1] - 30.0).abs() < 1e-4)
    );
}
