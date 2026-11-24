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
        let geometry =
            prepare_line(&line(interpolation), &frame, &input(&ids, &x, &y), None).unwrap();
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
    let geometry = prepare_line(&chart, &fixed_frame(), &input(&ids, &x, &y), None).unwrap();
    assert_eq!(
        geometry.hits[0].row,
        PlotRowIdentity {
            series: 1,
            row_id: DataRowId(large)
        }
    );
    assert!(geometry.canvas.iter().any(|paint| matches!(&paint.kind, PlotPrimitiveKind::Text { text, .. } if text.as_ref() == "exact identity")));
    let styled = prepare_line(
        &chart,
        &fixed_frame(),
        &input(&ids, &x, &y),
        Some(&PlotCalloutStyle::default()),
    )
    .unwrap();
    assert_eq!(geometry.hits, styled.hits);
    let marks = |output: &PlotPreparedGeometry| {
        output
            .canvas
            .iter()
            .filter(|item| item.data_mark)
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(marks(&geometry), marks(&styled));
    assert!(styled.canvas.iter().any(|paint| matches!(
        paint.kind,
        PlotPrimitiveKind::CalloutBox {
            border_width: 0.0,
            ..
        }
    )));
}

#[test]
fn automatic_bar_range_includes_zero_and_keeps_negative_direction() {
    let ids = [DataRowId(20), DataRowId(30)];
    let x = values(&[1.0, 2.0]);
    let y = values(&[-10.0, 20.0]);
    let mut chart = PlotBars2d::default();
    chart.series.push(PlotSeriesRow::default()).unwrap();
    let frame = PlotFrame2d::default();
    let geometry = prepare_bars(&chart, &frame, &input(&ids, &x, &y), None).unwrap();
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
fn crate_layers_fit_both_signs_and_pick_only_the_visible_wood() {
    let ids = [DataRowId(11), DataRowId(27)];
    let x = values(&[1.0, 3.0]);
    let mut chart = PlotBars2d {
        stacked_blocks: true,
        block_offset: 0.125,
        gap: 0.375,
        ..Default::default()
    };
    chart.series.push(PlotSeriesRow::default()).unwrap();
    let frame = PlotFrame2d {
        width: 400.,
        height: 100.,
        padding_left: 0.,
        padding_right: 0.,
        padding_top: 0.,
        padding_bottom: 0.,
        min_x: 0.,
        max_x: 20.,
        min_y: -50.,
        max_y: 50.,
        ..fixed_frame()
    };
    // The 25-unit slot holds a 20-unit square and 2.5 units of offset per side.
    for (value, heights) in [
        (47., vec![20., 20., 7.]),
        (42., vec![20., 22.]),
        (3., vec![3.]),
    ] {
        let y = values(&[value, -value]);
        let g = prepare_bars(&chart, &frame, &input(&ids, &x, &y), None).unwrap();
        assert_eq!(g.hits.len(), 2 * heights.len());
        let marks: Vec<_> = g.canvas.iter().filter(|item| item.data_mark).collect();
        assert_eq!(marks.len(), g.hits.len());
        for (row, &id) in ids.iter().enumerate() {
            let mid = [20., 60.][row];
            let mut near = 50.;
            for (layer, &height) in heights.iter().enumerate() {
                let index = row * heights.len() + layer;
                let hit = &g.hits[index];
                assert_eq!(
                    hit.row,
                    PlotRowIdentity {
                        row_id: id,
                        series: 0
                    }
                );
                let PlotHitShape::Rect([l, t, r, b]) = hit.shape else {
                    panic!("crate hit")
                };
                assert!((r - l - 20.).abs() < 1e-5);
                assert_eq!(b - t, height);
                assert_eq!(marks[index].position, [l, t]);
                assert!(
                    matches!(marks[index].kind, PlotPrimitiveKind::Box { size, .. } if size == [r-l,b-t])
                );
                let center = (l + r) * 0.5;
                if layer == 0 {
                    assert_eq!(center, mid);
                } else {
                    assert!(center != mid && (center - mid).abs() <= 2.5);
                }
                assert!(hit.shape.contains_canvas_point([center, (t + b) * 0.5]));
                assert!(!hit.shape.contains_canvas_point([l - 0.1, (t + b) * 0.5]));
                let (base, end) = if row == 0 {
                    (b, t)
                } else {
                    (t, b)
                };
                assert_eq!(base, near);
                near = end;
            }
            assert_eq!(
                near,
                if row == 0 {
                    50. - value
                } else {
                    50. + value
                }
            );
        }
    }
}

#[test]
fn clipped_crate_stacks_keep_their_original_layer_boundaries_and_offsets() {
    let row = PlotRowIdentity {
        row_id: DataRowId(1),
        series: 0,
    };
    let mut full = PlotPreparedGeometry::default();
    let mut clipped = PlotPreparedGeometry::default();
    let draw = |g: &mut PlotPreparedGeometry, clip| {
        stacked_bar(g, row, [1.; 4], [0., 100.], [24., 10.], clip, 0.1)
    };
    draw(&mut full, [0., 10., 24., 100.]);
    draw(&mut clipped, [0., 27., 24., 69.]);
    let expected: Vec<_> = full
        .hits
        .iter()
        .filter_map(|hit| {
            let PlotHitShape::Rect([l, t, r, b]) = hit.shape else {
                panic!("crate hit")
            };
            (t < 69. && b > 27.).then_some(PlotHitShape::Rect([l, t.max(27.), r, b.min(69.)]))
        })
        .collect();
    assert_eq!(
        expected,
        clipped
            .hits
            .iter()
            .map(|hit| hit.shape.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn leaning_bars_keep_value_edges_and_pick_their_silhouette() {
    let ids = [DataRowId(14), DataRowId(27)];
    let x = values(&[1.0, 2.0]);
    let y = values(&[-10.0, 20.0]);
    let input = input(&ids, &x, &y);
    let mut chart = PlotBars2d::default();
    chart.series.push(PlotSeriesRow::default()).unwrap();

    let mut frame = PlotFrame2d::default();
    let original = prepare_bars(&chart, &frame, &input, None).unwrap();
    frame.bar_lean = 0.012;
    let tilted = prepare_bars(&chart, &frame, &input, None).unwrap();

    for (flat, lean) in original.hits.iter().zip(&tilted.hits) {
        let PlotHitShape::Rect(bounds) = flat.shape else {
            panic!("rectangular bar")
        };
        let PlotHitShape::SlantedBar(corners) = lean.shape else {
            panic!("leaning bar")
        };
        assert_eq!(lean.row, flat.row);
        let negative = flat.row.row_id == ids[0];
        let (base, value, base_y, value_y) = if negative {
            (
                [corners[0], corners[1]],
                [corners[3], corners[2]],
                bounds[1],
                bounds[3],
            )
        } else {
            (
                [corners[3], corners[2]],
                [corners[0], corners[1]],
                bounds[3],
                bounds[1],
            )
        };
        assert_eq!([value[0][1], value[1][1]], [value_y; 2]);
        assert!((base[0][1] - base_y) * (base[1][1] - base_y) < 0.0);
        assert!(((base[0][1] + base[1][1]) * 0.5 - base_y).abs() < 1e-4);
        assert_eq!(corners[3][0] > corners[0][0], negative);
        let span = base[1][0] - base[0][0];
        for t in [0.25, 0.75] {
            let x = base[0][0] + t * span;
            let edge = base[0][1] + t * (base[1][1] - base[0][1]);
            let toward_value = (value_y - base_y).signum() * 0.1;
            assert!(lean.shape.contains_canvas_point([x, edge + toward_value]));
            assert!(!lean.shape.contains_canvas_point([x, edge - toward_value]));
        }
    }
}

#[test]
fn clipped_bar_base_and_value_edges_stay_level_without_a_visible_zero() {
    let frame = PlotFrame2d {
        bar_lean: 0.012,
        ..fixed_frame()
    };
    let row = PlotRowIdentity {
        series: 0,
        row_id: DataRowId(1),
    };
    for baseline in [-5.0, 40.0] {
        let [a, b, c, d] = bar_corners([10.0, 10.0, 30.0, 30.0], baseline, row, &frame);
        assert_eq!([a[1], b[1], c[1], d[1]], [10.0, 10.0, 30.0, 30.0]);
    }
}

#[test]
fn transparent_planar_grid_omits_only_grid_strokes() {
    let mut frame = fixed_frame();
    let min = [0.0, 0.0];
    let max = [10.0, 100.0];
    let mapping = PlotFrameMapping2d::new(&frame, min, max).unwrap();
    let lined = CartesianPaint::new(&frame, mapping, min, max).prepare();
    let grid_color = frame.grid_color();
    frame.grid_alpha = 0.0;
    let paper = CartesianPaint::new(&frame, mapping, min, max).prepare();
    let visible_parts = lined
        .canvas
        .iter()
        .filter(|p| p.color != grid_color)
        .map(|p| (&p.kind, p.position, p.color));
    assert_eq!(
        lined.canvas.len() - paper.canvas.len(),
        2 * (frame.ticks as usize + 1)
    );
    assert!(visible_parts.eq(paper.canvas.iter().map(|p| (&p.kind, p.position, p.color))));
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
    let geometry =
        prepare_bars(&chart, &PlotFrame2d::default(), &input(&ids, &x, &y), None).unwrap();
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
    let geometry = prepare_bars(&chart, &frame, &input(&ids, &x, &y), None).unwrap();
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
    let geometry = prepare_pie(&chart, &fixed_frame(), &input(&ids, &x, &y), None).unwrap();
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
    // The instanced arc retains its hole; no opaque disk paints over sectors.
    assert!(geometry.canvas.iter().all(|paint| matches!(
        paint.kind,
        PlotPrimitiveKind::Shape {
            shape: crate::systems::canvas::CanvasBoxShape::Arc { .. },
            ..
        }
    )));
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
    assert!(prepare_line(&line(0), &fixed_frame(), &input(&ids, &x, &y), None).is_err());
    let mut chart = line(0);
    chart.series.get_mut(0).unwrap().x = "absent".into();
    assert!(prepare_line(&chart, &fixed_frame(), &input(&ids, &x, &x), None).is_err());
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

#[test]
fn only_data_marks_select_sibling_paint_in_all_2d_families() {
    let ids = [DataRowId(1), DataRowId(2), DataRowId(3)];
    let x = values(&[1.0, 3.0, 5.0]);
    let y = values(&[10.0, 60.0, 30.0]);
    let input = input(&ids, &x, &y);
    let frame = fixed_frame();
    for interpolation in [0, 1] {
        let mut chart = line(interpolation);
        chart.marker_size = 6.0;
        let prepared = prepare_line(&chart, &frame, &input, None).unwrap();
        assert_eq!(prepared.hits.len(), 3);
        assert_eq!(
            prepared.canvas.iter().filter(|item| item.data_mark).count(),
            4
        );
        assert!(
            prepared
                .canvas
                .iter()
                .filter(|item| item.data_mark)
                .all(|item| matches!(
                    item.kind,
                    PlotPrimitiveKind::Path(_) | PlotPrimitiveKind::Shape { .. }
                ))
        );
        assert!(
            prepared
                .canvas
                .iter()
                .any(|item| !item.data_mark && matches!(item.kind, PlotPrimitiveKind::Text { .. }))
        );
    }

    let mut bars = PlotBars2d::default();
    bars.series.push(PlotSeriesRow::default()).unwrap();
    let prepared = prepare_bars(&bars, &frame, &input, None).unwrap();
    assert_eq!(
        prepared.canvas.iter().filter(|item| item.data_mark).count(),
        3
    );
    assert!(
        prepared
            .canvas
            .iter()
            .filter(|item| item.data_mark)
            .all(|item| matches!(item.kind, PlotPrimitiveKind::Box { .. }))
    );

    let mut pie = PlotPie2d::default();
    pie.series.push(PlotSeriesRow::default()).unwrap();
    let prepared = prepare_pie(&pie, &frame, &input, None).unwrap();
    assert_eq!(
        prepared.canvas.iter().filter(|item| item.data_mark).count(),
        3
    );
    assert!(prepared.canvas.iter().all(|item| item.data_mark));
}

#[test]
fn authored_highlight_color_preserves_marks_and_picking_for_every_2d_family() {
    use crate::components::schema::ComponentLifecycle;

    let ids = [DataRowId(1), DataRowId(2)];
    let x = values(&[2.0, 6.0]);
    let y = values(&[30.0, 70.0]);
    let input = input(&ids, &x, &y);
    let before = fixed_frame();
    assert_eq!(before.highlight_color(), [1.0; 4]);
    let after = PlotFrame2d {
        highlight_red: 0.2,
        highlight_green: 0.7,
        highlight_blue: 0.9,
        highlight_alpha: 0.8,
        ..before.clone()
    };
    assert!(after.validate().is_ok());
    let invalid = PlotFrame2d {
        highlight_alpha: f32::NAN,
        ..after.clone()
    };
    assert!(invalid.validate().is_err());

    macro_rules! compare {
        ($chart:ty, $prepare:ident) => {{
            let mut chart = <$chart>::default();
            chart.series.push(PlotSeriesRow::default()).unwrap();
            chart
                .labels
                .push(PlotLabelRow {
                    row_id: "1".into(),
                    text: "selected".into(),
                    highlighted: true,
                    offset: [24.0, -30.0],
                    ..Default::default()
                })
                .unwrap();
            let mut original = $prepare(&chart, &before, &input, None).unwrap();
            let styled = $prepare(&chart, &after, &input, None).unwrap();
            let mut highlighted_parts = 0;
            for primitive in &mut original.canvas {
                if primitive.color == before.highlight_color() {
                    assert!(!primitive.data_mark);
                    primitive.color = after.highlight_color();
                    highlighted_parts += 1;
                }
            }
            assert!(highlighted_parts >= 3);
            assert_eq!(original, styled);
        }};
    }

    compare!(PlotLine2d, prepare_line);
    compare!(PlotBars2d, prepare_bars);
    compare!(PlotPie2d, prepare_pie);
}

#[test]
fn dense_lines_publish_compact_segments_with_every_source_endpoint() {
    let ids: Vec<_> = (0..600).map(DataRowId).collect();
    let x = values(
        &(0..600)
            .map(|i| i as f32 * 10.0 / 599.0)
            .collect::<Vec<_>>(),
    );
    let y = values(
        &(0..600)
            .map(|i| 50.0 + (i as f32 * 0.7).sin() * 30.0)
            .collect::<Vec<_>>(),
    );
    let frame = fixed_frame();
    let geometry = prepare_line(&line(0), &frame, &input(&ids, &x, &y), None).unwrap();
    let strokes: Vec<_> = geometry
        .canvas
        .iter()
        .filter_map(|p| match &p.kind {
            PlotPrimitiveKind::Path(path) if path.is_stroke() => Some(path),
            _ => None,
        })
        .collect();
    assert_eq!(strokes.len(), 1);
    let PlotPathGeometry::Stroke {
        segments,
        radius,
    } = &strokes[0].geometry
    else {
        unreachable!()
    };
    assert_eq!(segments.len(), 599);
    assert_eq!(*radius, line(0).line_width / 2.0);
    assert_eq!(geometry.hits.len(), 600);
    for (i, segment) in segments.iter().enumerate() {
        let PlotHitShape::Circle {
            center: a,
            ..
        } = geometry.hits[i].shape
        else {
            panic!("source row mark")
        };
        let PlotHitShape::Circle {
            center: b,
            ..
        } = geometry.hits[i + 1].shape
        else {
            panic!("source row mark")
        };
        assert_eq!(*segment, [a[0], a[1], b[0], b[1]]);
    }
}
