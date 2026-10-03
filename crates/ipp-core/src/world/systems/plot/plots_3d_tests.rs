//! Geometry-only checks supplement the maintained real native Plot scenario.

use super::*;
use crate::expressions::{ExpressionInvalid, ExpressionResult};
use crate::services::data::DataRowId;
use crate::systems::data_bindings::DataBindingColumnView;
use crate::{DynamicPropertyKind, DynamicValue};

fn input<'a>(
    rows: &'a [DataRowId],
    columns: &'a [(&'a str, &'a [ExpressionResult])],
) -> PlotPreparedInput<'a> {
    PlotPreparedInput {
        row_ids: rows,
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

fn numbers(values: &[f32]) -> Vec<ExpressionResult> {
    values
        .iter()
        .map(|&v| ExpressionResult::Valid(DynamicValue::F32(v)))
        .collect()
}

fn fixed_frame() -> PlotFrame3d {
    PlotFrame3d {
        width: 10.0,
        height: 10.0,
        depth: 10.0,
        min_x: 0.0,
        min_y: -5.0,
        min_z: 0.0,
        max_x: 10.0,
        max_y: 5.0,
        max_z: 10.0,
        automatic_x: false,
        automatic_y: false,
        automatic_z: false,
        ..PlotFrame3d::default()
    }
}

#[test]
fn single_row_bars_keep_exact_source_ids_and_negative_zero_baseline() {
    let rows = [DataRowId(9_007_199_254_740_993), DataRowId(u64::MAX)];
    let x = numbers(&[2.0, 5.0]);
    let value = numbers(&[-3.0, 4.0]);
    let columns = [("x", x.as_slice()), ("value", value.as_slice())];
    let mut chart = PlotGridBars3d::default();
    chart
        .series
        .insert(
            7,
            PlotSeriesRow {
                z: "".into(),
                ..PlotSeriesRow::default()
            },
        )
        .unwrap();
    let output = prepare_bars(&chart, &fixed_frame(), &input(&rows, &columns)).unwrap();
    assert_eq!(output.hits.len(), 2);
    assert_eq!(
        output.hits[0].row,
        PlotRowIdentity {
            series: 7,
            row_id: rows[0]
        }
    );
    assert_eq!(output.hits[1].row.row_id, DataRowId(u64::MAX));
    let PlotHitShape::Box {
        min,
        max,
    } = output.hits[0].shape
    else {
        panic!("bar proxy")
    };
    assert_eq!([min[1], max[1]], [2.0, 5.0]);
    // Face geometry plus ordinary edge strips, still one compact mesh batch.
    assert_eq!(output.meshes.len(), 1);
    assert!(output.meshes[0].indices.len() > 72);
}

#[test]
fn cuboid_faces_are_outward_and_closed() {
    let mut builder = MeshBuilder::default();
    builder.cuboid([-1.0; 3], [1.0; 3], [1.0; 4]).unwrap();
    let mesh = &builder.chunks[0];
    assert_eq!(mesh.indices.len(), 36);
    for face in mesh.positions.as_chunks::<3>().0.iter() {
        let normal = cross(sub(face[1], face[0]), sub(face[2], face[0]));
        let centroid =
            std::array::from_fn(|axis| (face[0][axis] + face[1][axis] + face[2][axis]) / 3.0);
        assert!(dot(normal, centroid) > 0.0);
    }
}

#[test]
fn disconnected_points_do_not_create_connecting_faces() {
    let rows = [DataRowId(4), DataRowId(19)];
    let x = numbers(&[1.0, 9.0]);
    let y = numbers(&[0.0, 2.0]);
    let z = numbers(&[2.0, 8.0]);
    let columns = [
        ("x", x.as_slice()),
        ("y", y.as_slice()),
        ("z", z.as_slice()),
    ];
    let mut chart = PlotPoints3d::default();
    chart.series.push(PlotSeriesRow::default()).unwrap();
    let output = prepare_points(&chart, &fixed_frame(), &input(&rows, &columns)).unwrap();
    assert_eq!(output.hits.len(), 2);
    // Face geometry plus ordinary edge strips, still one compact mesh batch.
    assert_eq!(output.meshes.len(), 1);
    assert!(output.meshes[0].indices.len() > 72);
    for triangle in output
        .meshes
        .iter()
        .flat_map(|mesh| mesh.positions.as_chunks::<3>().0.iter())
    {
        let width = triangle
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max)
            - triangle.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        assert!(width <= chart.marker_size + 1e-6);
    }
}

fn sites(values: &[[f64; 3]]) -> Vec<Sample> {
    values
        .iter()
        .enumerate()
        .map(|(i, &point)| Sample {
            row: PlotRowIdentity {
                series: 0,
                row_id: DataRowId(i as u64),
            },
            point,
            color: [1.0; 4],
            valid: true,
        })
        .collect()
}

#[test]
fn irregular_surface_triangulates_without_adding_or_aggregating_sites() {
    let sites = sites(&[
        [0.0, 1.0, 0.0],
        [4.0, 2.0, 0.0],
        [0.0, 3.0, 3.0],
        [4.0, 4.0, 3.0],
        [1.3, 2.0, 1.7],
    ]);
    let triangles = triangulate(&sites).unwrap();
    assert_eq!(triangles.len(), 4);
    assert!(triangles.iter().all(|t| t.contains(&4)));
    let area: f64 = triangles
        .iter()
        .map(|&[a, b, c]| {
            orientation(
                [sites[a].point[0], sites[a].point[2]],
                [sites[b].point[0], sites[b].point[2]],
                [sites[c].point[0], sites[c].point[2]],
            ) * 0.5
        })
        .sum();
    assert!((area - 12.0).abs() < 1e-10);
}

#[test]
fn invalid_grid_height_removes_incident_faces_and_retains_other_cells() {
    let rows: Vec<_> = (0..16).map(DataRowId).collect();
    let x = numbers(&(0..16).map(|i| (i / 4) as f32).collect::<Vec<_>>());
    let z = numbers(&(0..16).map(|i| (i % 4) as f32).collect::<Vec<_>>());
    let mut y = numbers(&[1.0; 16]);
    y[5] = ExpressionResult::Invalid(ExpressionInvalid::MissingInput {
        slot: 0,
    });
    let columns = [
        ("x", x.as_slice()),
        ("y", y.as_slice()),
        ("z", z.as_slice()),
    ];
    let mut chart = PlotHeightSurface3d {
        wireframe: false,
        ..PlotHeightSurface3d::default()
    };
    chart.series.push(PlotSeriesRow::default()).unwrap();
    let output = prepare_surface(&chart, &fixed_frame(), &input(&rows, &columns)).unwrap();
    assert_eq!(output.hits.len(), 15);
    // A cell diagonal can leave its unaffected face; six faces touch this site.
    assert_eq!(
        output
            .meshes
            .iter()
            .map(|mesh| mesh.indices.len())
            .sum::<usize>(),
        12 * 3
    );
}

#[test]
fn duplicate_surface_coordinates_are_rejected_and_collinear_sites_are_empty() {
    let duplicate = sites(&[[0.0, 1.0, 0.0], [0.0, 2.0, 0.0], [1.0, 3.0, 1.0]]);
    assert_eq!(
        triangulate(&duplicate).unwrap_err(),
        ErrorReason::InvalidValue
    );
    assert!(
        triangulate(&sites(&[[0.0, 1.0, 0.0], [1.0, 2.0, 1.0], [2.0, 3.0, 2.0]]))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn pie_extents_use_independent_physical_radius_and_height() {
    let rows = [DataRowId(0)];
    let share = numbers(&[1.0]);
    let radius = numbers(&[2.0]);
    let height = numbers(&[3.0]);
    let columns = [
        ("share", share.as_slice()),
        ("radius", radius.as_slice()),
        ("height", height.as_slice()),
    ];
    let mut chart = PlotPie3d::default();
    chart
        .series
        .push(PlotSeriesRow {
            value: "share".into(),
            radius: "radius".into(),
            height: "height".into(),
            ..PlotSeriesRow::default()
        })
        .unwrap();
    let frame = fixed_frame();
    let output = prepare_pie(&chart, &frame, &input(&rows, &columns)).unwrap();
    let points: Vec<_> = output
        .meshes
        .iter()
        .flat_map(|mesh| mesh.positions.iter().copied())
        .collect();
    for axis in 0..3 {
        let extent = points
            .iter()
            .map(|p| p[axis])
            .fold(f32::NEG_INFINITY, f32::max)
            - points.iter().map(|p| p[axis]).fold(f32::INFINITY, f32::min);
        assert!((extent - [4.0, 3.0, 4.0][axis]).abs() <= frame.line_width);
    }
}

#[test]
fn chunking_retains_all_triangles_without_wrapping_indices() {
    let mut builder = MeshBuilder::default();
    for _ in 0..22_000 {
        builder
            .face(
                [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                [1.0; 4],
                false,
            )
            .unwrap();
    }
    assert_eq!(builder.chunks.len(), 2);
    assert_eq!(
        builder
            .chunks
            .iter()
            .map(|mesh| mesh.indices.len())
            .sum::<usize>(),
        66_000
    );
    for mesh in builder.chunks {
        assert!(mesh.positions.len() <= 65535);
        assert!(
            mesh.indices
                .iter()
                .all(|&i| usize::from(i) < mesh.positions.len())
        );
    }
}

#[test]
fn source_keyed_label_matches_full_u64_and_series_slot() {
    let rows = [
        DataRowId(9_007_199_254_740_993),
        DataRowId(9_007_199_254_740_994),
    ];
    let x = numbers(&[1.0, 9.0]);
    let y = numbers(&[0.0, 2.0]);
    let z = numbers(&[2.0, 8.0]);
    let columns = [
        ("x", x.as_slice()),
        ("y", y.as_slice()),
        ("z", z.as_slice()),
    ];
    let mut chart = PlotPoints3d::default();
    chart.series.insert(7, PlotSeriesRow::default()).unwrap();
    chart
        .labels
        .push(PlotLabelRow {
            series: 7,
            row_id: rows[0].0.to_string().into(),
            text: "Exact row".into(),
            offset: [1.0, -1.0],
            highlighted: true,
            ..PlotLabelRow::default()
        })
        .unwrap();
    chart
        .labels
        .push(PlotLabelRow {
            series: 8,
            row_id: rows[1].0.to_string().into(),
            text: "Another series".into(),
            ..PlotLabelRow::default()
        })
        .unwrap();
    let output = prepare_points(&chart, &fixed_frame(), &input(&rows, &columns)).unwrap();
    let texts: Vec<_> = output
        .planes
        .iter()
        .flat_map(|plane| &plane.primitives)
        .filter_map(|primitive| {
            if let PlotPrimitiveKind::Text {
                text,
                ..
            } = &primitive.kind
            {
                Some(text.as_ref())
            } else {
                None
            }
        })
        .collect();
    assert!(texts.contains(&"Exact row"));
    assert!(!texts.contains(&"Another series"));
    assert_eq!(output.hits[0].row.row_id, rows[0]);
}

#[test]
fn edge_prism_faces_point_outward() {
    let mut builder = MeshBuilder::default();
    builder
        .line([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], 0.1, [1.0; 4])
        .unwrap();
    for face in builder.chunks[0].positions.as_chunks::<3>().0.iter() {
        let normal = cross(sub(face[1], face[0]), sub(face[2], face[0]));
        let centroid =
            std::array::from_fn(|axis| (face[0][axis] + face[1][axis] + face[2][axis]) / 3.0);
        assert!(dot(normal, centroid) > 0.0);
    }
}

#[test]
fn pie_angles_follow_shares_while_radial_prisms_keep_independent_dimensions() {
    let rows = [DataRowId(21), DataRowId(22)];
    let share = numbers(&[1.0, 3.0]);
    let radius = numbers(&[2.0, 1.0]);
    let height = numbers(&[3.0, 0.5]);
    let columns = [
        ("share", share.as_slice()),
        ("radius", radius.as_slice()),
        ("height", height.as_slice()),
    ];
    let mut chart = PlotPie3d::default();
    chart
        .series
        .insert(
            4,
            PlotSeriesRow {
                value: "share".into(),
                radius: "radius".into(),
                height: "height".into(),
                ..PlotSeriesRow::default()
            },
        )
        .unwrap();
    let output = prepare_pie(&chart, &fixed_frame(), &input(&rows, &columns)).unwrap();
    assert_eq!(output.hits.len(), 2);
    for (i, hit) in output.hits.iter().enumerate() {
        let PlotHitShape::RadialPrism {
            radius,
            start,
            sweep,
            min_y,
            max_y,
            ..
        } = hit.shape
        else {
            panic!("exact radial proxy")
        };
        assert_eq!(radius, [2.0, 1.0][i]);
        assert_eq!([min_y, max_y], [0.0, [3.0, 0.5][i]]);
        assert!((start - [0.0, std::f32::consts::FRAC_PI_2][i]).abs() < 1e-6);
        assert!(
            (sweep
                - [
                    std::f32::consts::FRAC_PI_2,
                    3.0 * std::f32::consts::FRAC_PI_2
                ][i])
                .abs()
                < 1e-6
        );
        assert_eq!(
            hit.row,
            PlotRowIdentity {
                series: 4,
                row_id: rows[i]
            }
        );
    }
}

#[test]
fn text_faces_the_view_and_cartesian_paths_retain_positive_bases_for_far_stations() {
    let mut output = PlotPreparedGeometry::default();
    let frame = fixed_frame();
    axes(&mut output, &frame, [-1.0, 0.0, -2.0], [3.0, 10.0, 8.0]).unwrap();
    let fixed: Vec<_> = output
        .planes
        .iter()
        .filter(|p| p.facing == PlotPlaneFacing::Fixed)
        .collect();
    assert_eq!(fixed.len(), 6);
    assert!(fixed.iter().all(|p| {
        p.primitives
            .iter()
            .all(|primitive| !matches!(primitive.kind, PlotPrimitiveKind::Text { .. }))
    }));
    let labels: Vec<_> = output
        .planes
        .iter()
        .filter(|p| p.facing == PlotPlaneFacing::Camera)
        .collect();
    assert_eq!(labels.len(), (3 * (frame.ticks + 1) + 3) as usize);
    for axis in 0..3u8 {
        let ticks: Vec<_> = labels
            .iter()
            .filter(|p| p.layout == PlotPlaneLayout::Tick(axis))
            .collect();
        assert_eq!(ticks.len(), (frame.ticks + 1) as usize);
        assert_eq!(ticks.first().unwrap().model[12 + axis as usize], 0.0);
        assert_eq!(
            ticks.last().unwrap().model[12 + axis as usize],
            frame.size()[axis as usize]
        );
        assert!(ticks.iter().all(|p| p.placement
            == PlotPlanePlacement::Axis {
                extent: frame.size(),
                axis
            }));
    }
}

#[test]
fn annotation_paint_shares_one_exact_data_anchor_and_rotation_enclosure() {
    let mut labels = Rows::<PlotLabelRow>::default();
    labels
        .push(PlotLabelRow {
            row_id: "42".into(),
            text: "Exact anchor".into(),
            offset: [2.0, -1.0],
            connector: true,
            ..PlotLabelRow::default()
        })
        .unwrap();
    let point = [3.0, 4.0, 5.0];
    let mut output = PlotPreparedGeometry::default();
    let mut meshes = MeshBuilder::default();
    annotate(
        &mut output,
        &mut meshes,
        &Labels::new(&labels).unwrap(),
        PlotMark {
            row: PlotRowIdentity {
                series: 0,
                row_id: DataRowId(42),
            },
            point,
            color: [1.0; 4],
            radial: None,
        },
        &fixed_frame(),
        0.0,
    )
    .unwrap();
    assert_eq!(output.planes.len(), 2);
    let plane = &output.planes[0];
    assert_eq!(plane.facing, PlotPlaneFacing::Camera);
    assert_eq!(&plane.model[12..15], &point);
    assert_eq!(plane.layout, PlotPlaneLayout::Callout);
    assert_eq!(plane.primitives.len(), 2);
    assert!(matches!(
        plane.primitives[0].kind,
        PlotPrimitiveKind::Box { .. }
    ));
    assert!(matches!(
        plane.primitives[1].kind,
        PlotPrimitiveKind::Text { .. }
    ));
    let connector = &output.planes[1];
    assert_eq!(&connector.model[12..15], &point);
    assert_eq!(
        connector.layout,
        PlotPlaneLayout::Connector {
            panel: plane.part,
            endpoint: [2.0, -1.0],
            width: fixed_frame().line_width,
        }
    );
    assert_eq!(connector.primitives.len(), 1);
    let PlotPrimitiveKind::Path(path) = &connector.primitives[0].kind else {
        panic!("retained unit strip");
    };
    assert_eq!(path.bounds, [0.0, -0.5, 1.0, 0.5]);
    let radius = plane.clip[2].hypot(plane.clip[1]);
    let bounds = finish(output, meshes).unwrap().bounds.unwrap();
    for axis in 0..3 {
        assert!(bounds[0][axis] <= point[axis] - radius);
        assert!(bounds[1][axis] >= point[axis] + radius);
    }
}

#[test]
fn cuboid_tones_separate_top_and_both_side_directions() {
    let mut builder = MeshBuilder::default();
    builder.cuboid([-1.0; 3], [1.0; 3], [1.0; 4]).unwrap();
    let mesh = &builder.chunks[0];
    let mut tones = BTreeMap::<(usize, bool), f32>::new();
    for (points, colors) in mesh
        .positions
        .as_chunks::<3>()
        .0
        .iter()
        .zip(mesh.colors.as_chunks::<3>().0.iter())
    {
        let normal = cross(sub(points[1], points[0]), sub(points[2], points[0]));
        let axis = (0..3)
            .max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs()))
            .unwrap();
        tones.insert((axis, normal[axis] > 0.0), colors[0][1]);
    }
    assert!(tones[&(1, true)] > tones[&(0, true)] * 2.0);
    assert!(tones[&(0, true)] > tones[&(2, true)] * 1.5);
    assert_eq!(tones[&(0, true)], tones[&(0, false)]);
    assert_eq!(tones[&(2, true)], tones[&(2, false)]);
}

#[test]
fn sparse_high_label_slots_do_not_collide_with_axis_planes_or_change_rows() {
    let row_ids = [DataRowId(u64::MAX)];
    let x = numbers(&[1.0]);
    let y = numbers(&[2.0]);
    let z = numbers(&[3.0]);
    let mut chart = PlotPoints3d::default();
    chart.series.insert(7, PlotSeriesRow::default()).unwrap();
    for slot in [
        Rows::<PlotLabelRow>::MAX_SLOTS - 2,
        Rows::<PlotLabelRow>::MAX_SLOTS - 1,
    ] {
        chart
            .labels
            .insert(
                slot,
                PlotLabelRow {
                    series: 7,
                    row_id: u64::MAX.to_string().into(),
                    text: "Sparse identity".into(),
                    ..PlotLabelRow::default()
                },
            )
            .unwrap();
    }
    let columns = [
        ("x", x.as_slice()),
        ("y", y.as_slice()),
        ("z", z.as_slice()),
    ];
    let output = prepare_points(&chart, &fixed_frame(), &input(&row_ids, &columns)).unwrap();
    let parts: std::collections::BTreeSet<_> = output.planes.iter().map(|p| p.part).collect();
    assert_eq!(parts.len(), output.planes.len());
    assert_eq!(
        output.hits[0].row,
        PlotRowIdentity {
            series: 7,
            row_id: DataRowId(u64::MAX)
        }
    );
}

#[test]
fn pie_radial_metadata_uses_each_slice_height_and_keeps_exact_anchor_and_identity() {
    let rows = [DataRowId(9_007_199_254_740_993), DataRowId(2)];
    let values = numbers(&[1.0, 1.0]);
    let radii = numbers(&[2.0, 4.0]);
    let heights = numbers(&[1.0, 7.0]);
    let columns = [
        ("v", values.as_slice()),
        ("r", radii.as_slice()),
        ("h", heights.as_slice()),
    ];
    let mut chart = PlotPie3d::default();
    chart
        .series
        .insert(
            7,
            PlotSeriesRow {
                value: "v".into(),
                radius: "r".into(),
                height: "h".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for row in rows {
        chart
            .labels
            .push(PlotLabelRow {
                series: 7,
                row_id: row.0.to_string().into(),
                text: "Slice".into(),
                offset: [-3.0, 4.0],
                connector: true,
                ..Default::default()
            })
            .unwrap();
    }
    let result = prepare_pie(&chart, &fixed_frame(), &input(&rows, &columns)).unwrap();
    let panels: Vec<_> = result
        .planes
        .iter()
        .filter(|p| p.layout == PlotPlaneLayout::Callout)
        .collect();
    for (i, panel) in panels.iter().enumerate() {
        let PlotPlanePlacement::Radial {
            center,
            rim,
            spacing,
        } = panel.placement
        else {
            panic!()
        };
        assert_eq!(center[1], [1.0, 7.0][i]);
        assert_eq!(rim[1], center[1]);
        assert_eq!(panel.model[13], center[1]);
        assert_eq!(spacing, 5.0);
        let radius = (rim[0] - center[0]).hypot(rim[2] - center[2]);
        assert!((radius - [2.0, 4.0][i]).abs() < 1e-5);
        let anchor_radius = (panel.model[12] - center[0]).hypot(panel.model[14] - center[2]);
        assert!((anchor_radius - radius * 0.72).abs() < 1e-5);
        assert_eq!(
            result.hits[i].row,
            PlotRowIdentity {
                series: 7,
                row_id: rows[i]
            }
        );
    }
}

#[test]
fn unshaded_triangles_preserve_colors_and_exact_area_rejection() {
    let colors = [
        [0.1, 0.2, 0.3, 1.0],
        [0.4, 0.5, 0.6, 1.0],
        [0.7, 0.8, 0.9, 1.0],
    ];
    let mut builder = MeshBuilder::default();
    builder
        .triangle([[0.0; 3], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]], colors, false)
        .unwrap();
    assert!(builder.chunks.is_empty());
    let points = [[0.0; 3], [1e-20, 0.0, 0.0], [0.0, 1e-20, 0.0]];
    builder.triangle(points, colors, false).unwrap();
    assert_eq!(builder.chunks[0].positions, points);
    assert_eq!(builder.chunks[0].colors, colors.map(|c| [c[0], c[1], c[2]]));
    assert_eq!(builder.chunks[0].indices, [0, 1, 2]);
    assert!(builder.triangle([[f32::NAN; 3]; 3], colors, false).is_err());
}
