use super::*;
use crate::{services::data::DataRowId, systems::plot::plots_2d::prepare_line};

fn frame() -> (PlotFrame2d, PlotFrameMapping2d) {
    let frame = PlotFrame2d {
        width: 100.0,
        height: 100.0,
        padding_left: 0.0,
        padding_right: 0.0,
        padding_top: 0.0,
        padding_bottom: 0.0,
        ..Default::default()
    };
    let mapping = PlotFrameMapping2d::new(&frame, [0.0, -1.0], [1.0, 1.0]).unwrap();
    (frame, mapping)
}

fn sample(x: f64, y: f64, lower: Option<f64>, id: u64) -> Sample {
    Sample {
        row: PlotRowIdentity {
            series: 4,
            row_id: DataRowId(id),
        },
        point: [x, y],
        lower,
        color: [0.0, 1.0, 1.0, 0.5],
    }
}

fn triangles(geometry: &PlotPreparedGeometry) -> Vec<Triangle> {
    geometry
        .canvas
        .iter()
        .flat_map(|p| match &p.kind {
            PlotPrimitiveKind::Path(path) => match &path.geometry {
                PlotPathGeometry::Fill(t) => t.to_vec(),
                _ => vec![],
            },
            _ => vec![],
        })
        .collect()
}

fn area(p: Triangle) -> f64 {
    ((f64::from(p[1][0]) - f64::from(p[0][0])) * (f64::from(p[2][1]) - f64::from(p[0][1]))
        - (f64::from(p[1][1]) - f64::from(p[0][1])) * (f64::from(p[2][0]) - f64::from(p[0][0])))
        / 2.0
}

#[test]
fn crossing_baseline_has_two_lobes_without_filling_the_bow_tie_exterior() {
    let (_, mapping) = frame();
    let chart = PlotLine2d {
        fill_opacity: 0.4,
        ..Default::default()
    };
    for run in [
        vec![
            sample(0.0, 1.0, Some(0.0), 7),
            sample(1.0, -1.0, Some(0.0), 9),
        ],
        vec![
            sample(1.0, -1.0, Some(0.0), 7),
            sample(0.0, 1.0, Some(0.0), 9),
        ],
    ] {
        let mut geometry = PlotPreparedGeometry::default();
        prepare(&mut geometry, &run, &chart, mapping).unwrap();
        let triangles = triangles(&geometry);
        assert_eq!(triangles.len(), 2);
        assert!((triangles.iter().copied().map(area).sum::<f64>() - 2500.0).abs() < 0.01);
        assert!(geometry.hits.iter().all(|h| h.row.row_id == DataRowId(7)));
        assert!(geometry.hits[0].shape.contains_canvas_point([25.0, 40.0]));
        assert!(!geometry.hits[0].shape.contains_canvas_point([50.0, 25.0]));
        assert!((geometry.canvas[0].color[3] - 0.2).abs() < 1e-6);
    }
}

#[test]
fn lower_gaps_repeated_x_and_source_order_are_preserved() {
    let (_, mapping) = frame();
    let run = [
        sample(0.0, 0.8, Some(-0.5), 1),
        sample(0.5, 0.8, Some(-0.5), 2),
        sample(0.6, 0.9, None, 3),
        sample(0.8, 0.8, Some(-0.5), 4),
        sample(0.8, 0.6, Some(-0.5), 5),
        sample(1.0, 0.8, Some(-0.5), 6),
    ];
    let mut geometry = PlotPreparedGeometry::default();
    prepare(
        &mut geometry,
        &run,
        &PlotLine2d {
            fill_opacity: 1.0,
            ..Default::default()
        },
        mapping,
    )
    .unwrap();
    assert_eq!(
        geometry
            .hits
            .iter()
            .map(|h| h.row.row_id.0)
            .collect::<Vec<_>>(),
        [1, 5]
    );
    assert!(
        geometry
            .hits
            .iter()
            .all(|h| !h.shape.contains_canvas_point([60.0, 50.0]))
    );
    assert_eq!(geometry.canvas.len(), 2);
}

#[test]
fn smooth_crossing_bands_clip_to_fixed_frame_and_keep_positive_triangle_orientation() {
    let (_, mapping) = frame();
    let run = [
        sample(-0.2, 2.0, Some(-2.0), 1),
        sample(0.3, -0.6, Some(0.7), 2),
        sample(0.7, 0.8, Some(-0.5), 3),
        sample(1.2, -2.0, Some(2.0), 4),
    ];
    let mut geometry = PlotPreparedGeometry::default();
    prepare(
        &mut geometry,
        &run,
        &PlotLine2d {
            interpolation: 1,
            fill_opacity: 0.3,
            ..Default::default()
        },
        mapping,
    )
    .unwrap();
    let triangles = triangles(&geometry);
    assert!(triangles.len() > 20);
    assert!(triangles.iter().copied().all(|p| area(p) > 0.0));
    assert!(
        triangles
            .iter()
            .flatten()
            .flatten()
            .all(|v| *v >= -0.0001 && *v <= 100.0001)
    );
}

#[test]
fn disabled_fill_never_binds_an_unavailable_lower_column() {
    use crate::components::schema::ComponentLifecycle;
    let mut chart = PlotLine2d {
        line_width: 0.0,
        ..Default::default()
    };
    chart
        .series
        .push(PlotSeriesRow {
            lower: "absent".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(chart.validate().is_ok());
    let input = PlotPreparedInput {
        row_ids: &[],
        columns: vec![
            crate::systems::data_bindings::DataBindingColumnView {
                name: "x",
                kind: crate::DynamicPropertyKind::F32,
                values: &[],
            },
            crate::systems::data_bindings::DataBindingColumnView {
                name: "y",
                kind: crate::DynamicPropertyKind::F32,
                values: &[],
            },
        ],
    };
    assert!(prepare_line(&chart, &frame().0, &input, None).is_ok());
    chart.fill_opacity = 0.5;
    assert!(prepare_line(&chart, &frame().0, &input, None).is_err());
    chart.fill_opacity = 1.1;
    assert!(chart.validate().is_err());
}
