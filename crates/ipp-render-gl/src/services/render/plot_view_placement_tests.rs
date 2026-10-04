use super::*;
const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn camera(yaw: f32, pitch: f32, roll: f32) -> [f32; 16] {
    let right = [yaw.cos(), 0.0, -yaw.sin()];
    let up = [
        yaw.sin() * pitch.sin(),
        pitch.cos(),
        yaw.cos() * pitch.sin(),
    ];
    let mut camera = IDENTITY;
    for row in 0..3 {
        camera[row] = right[row] * roll.cos() + up[row] * roll.sin();
        camera[4 + row] = up[row] * roll.cos() - right[row] * roll.sin();
    }
    camera[8..11].copy_from_slice(&[
        yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ]);
    camera
}

#[test]
fn edge_on_support_is_stable_and_near_under_composed_rotations_and_scales() {
    let extent = [10.0, 5.0, 10.0];
    for scale in [1e-20, 1e-6, 1.0, 1e6, 1e20] {
        for yaw in [0.0, 1.1, -2.4] {
            let mut chart = camera(yaw, 0.0, 0.0);
            for col in 0..3 {
                for row in 0..3 {
                    chart[col * 4 + row] *= scale * [1.2, 0.8, 1.1][col];
                }
            }
            chart[12..15].copy_from_slice(&[7.0 * scale, scale, -3.0 * scale]);
            let expected = point(chart, [0.0, 0.0, extent[2]]);
            for delta in [-2e-6, 0.0, 2e-6] {
                for roll in [0.0, 0.7] {
                    let mut view = camera(yaw + delta, -0.4, roll);
                    // Camera basis scale is immaterial to support direction.
                    for col in 0..3 {
                        for row in 0..3 {
                            view[col * 4 + row] *= scale * [0.7, 1.3, 2.0][col];
                        }
                    }
                    let axis = model(
                        chart,
                        chart,
                        view,
                        PlotPlanePlacement::Axis {
                            extent,
                            axis: 1,
                        },
                    )
                    .unwrap();
                    assert_eq!(&axis[..12], &chart[..12]);
                    assert_eq!(
                        point(axis, [0.0; 3]),
                        expected,
                        "scale={scale} yaw={yaw} delta={delta} roll={roll}"
                    );
                    assert_eq!(far_bounds(chart, view, extent)[0], 0.0);
                    for tick in 0..=4 {
                        let height = extent[1] * tick as f32 / 4.0;
                        let mut label = chart;
                        label[12..15].copy_from_slice(&point(chart, [0.0, height, 0.0]));
                        let label = model(
                            label,
                            chart,
                            view,
                            PlotPlanePlacement::Axis {
                                extent,
                                axis: 1,
                            },
                        )
                        .unwrap();
                        for row in 0..3 {
                            let difference = (point(label, [0.0; 3])[row]
                                - point(axis, [0.0, height, 0.0])[row])
                                .abs();
                            assert!(difference <= scale * 1e-5);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn meaningful_views_still_switch_support_edges_and_far_faces() {
    let extent = [10.0, 5.0, 8.0];
    for yaw in [-0.12, 0.12] {
        let view = camera(yaw, -0.4, 0.5);
        let placed = model(
            IDENTITY,
            IDENTITY,
            view,
            PlotPlanePlacement::Axis {
                extent,
                axis: 1,
            },
        )
        .unwrap();
        assert_eq!(
            &placed[12..15],
            &[
                0.0,
                0.0,
                if yaw < 0.0 {
                    0.0
                } else {
                    8.0
                }
            ]
        );
        assert_eq!(
            far_bounds(IDENTITY, view, extent)[0],
            if yaw < 0.0 {
                10.0
            } else {
                0.0
            }
        );
    }
    // From the rear the near tie is at local Z=0, not a hardcoded high side.
    let placed = model(
        IDENTITY,
        IDENTITY,
        camera(std::f32::consts::PI, -0.4, 0.0),
        PlotPlanePlacement::Axis {
            extent,
            axis: 1,
        },
    )
    .unwrap();
    assert_eq!(&placed[12..15], &[10.0, 0.0, 0.0]);
}

#[test]
fn reflected_chart_uses_physical_left_and_near_edges_without_reflecting_axis_basis() {
    let mut chart = IDENTITY;
    chart[0] = -2.0;
    chart[5] = 0.3;
    chart[10] = -3.0;
    let extent = [10.0, 5.0, 8.0];
    for yaw in [-2e-6, 0.0, 2e-6] {
        let placed = model(
            chart,
            chart,
            camera(yaw, -0.4, 0.3),
            PlotPlanePlacement::Axis {
                extent,
                axis: 1,
            },
        )
        .unwrap();
        assert_eq!(&placed[..12], &chart[..12]);
        assert_eq!(&placed[12..15], &[-20.0, 0.0, 0.0]);
    }
}

#[test]
fn collapsed_or_view_parallel_basis_has_a_finite_deterministic_tie() {
    let extent = [10.0, 5.0, 8.0];
    for col in [0, 4, 8] {
        let mut chart = IDENTITY;
        chart[col..col + 3].fill(0.0);
        for yaw in [-2e-6, 0.0, 2e-6] {
            let placed = model(
                chart,
                chart,
                camera(yaw, -std::f32::consts::FRAC_PI_2, 0.0),
                PlotPlanePlacement::Axis {
                    extent,
                    axis: 1,
                },
            )
            .unwrap();
            assert!(placed.iter().all(|v| v.is_finite()));
            assert_eq!(&placed[12..15], &[0.0; 3]);
        }
    }
}

#[test]
fn far_faces_follow_opposite_and_below_camera_without_reflecting_coordinates() {
    let extent = [10.0, 6.0, 8.0];
    let mut camera = IDENTITY;
    camera[8] = 1.0;
    camera[9] = 1.0;
    assert_eq!(far_bounds(IDENTITY, camera, extent), [0.0; 3]);
    camera[8] = -1.0;
    camera[9] = -1.0;
    camera[10] = -1.0;
    assert_eq!(far_bounds(IDENTITY, camera, extent), extent);
    let moved = model(
        IDENTITY,
        IDENTITY,
        camera,
        PlotPlanePlacement::Axis {
            extent,
            axis: 0,
        },
    )
    .unwrap();
    assert_eq!(&moved[..12], &IDENTITY[..12]);
    assert_eq!(&moved[12..15], &[0.0, 6.0, 0.0]);
}

#[test]
fn composed_reflections_and_rotations_select_world_depth_not_local_signs() {
    let mut chart = IDENTITY;
    chart[0] = -2.0;
    chart[10] = -3.0;
    chart[12] = 40.0;
    let mut camera = IDENTITY;
    camera[8] = 1.0;
    let extent = [10.0, 6.0, 8.0];
    assert_eq!(far_bounds(chart, camera, extent), [10.0, 0.0, 8.0]);
    let placed = model(
        chart,
        chart,
        camera,
        PlotPlanePlacement::Grid {
            extent,
            normal: 2,
        },
    )
    .unwrap();
    assert_eq!(&placed[..12], &chart[..12]);
    assert_eq!(placed[14], -24.0);
}

#[test]
fn rotated_chart_chooses_far_local_faces_from_the_composed_camera() {
    let chart = [
        0.0, 0.0, -2.0, 0.0, 0.0, 1.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 7.0, 2.0, 9.0, 1.0,
    ];
    let mut camera = IDENTITY;
    camera[8] = 1.0;
    let extent = [10.0, 6.0, 8.0];
    assert_eq!(far_bounds(chart, camera, extent), [10.0, 0.0, 0.0]);
    let moved = model(
        chart,
        chart,
        camera,
        PlotPlanePlacement::Axis {
            extent,
            axis: 1,
        },
    )
    .unwrap();
    assert_eq!(&moved[..12], &chart[..12]);
    assert_eq!(&moved[12..15], &[7.0, 2.0, 9.0]);
}

#[test]
fn vertical_axis_and_each_tick_share_the_left_projected_support_edge() {
    let extent = [10.0, 6.0, 8.0];
    let charts = [
        IDENTITY,
        [
            0.0, 0.0, -2.0, 0.0, 0.0, 1.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 7.0, 2.0, 9.0, 1.0,
        ],
    ];
    for chart in charts {
        for side in [1.0, -1.0] {
            // Opposing yaw views, tilted up basis and a rolled view. The
            // independent perpendicular support calculation uses projected
            // corners, never the renderer's far-bound or station selector.
            for roll in [0.0_f32, 0.4] {
                let right = [0.8 * side, 0.0, -0.6 * side];
                let up = [-0.3 * side, 0.8660254, -0.4 * side];
                let mut camera = IDENTITY;
                for row in 0..3 {
                    camera[row] = right[row] * roll.cos() + up[row] * roll.sin();
                    camera[4 + row] = up[row] * roll.cos() - right[row] * roll.sin();
                }
                let screen = |world: [f32; 3]| -> [f32; 2] {
                    [0, 4].map(|col| (0..3).map(|r| world[r] * camera[col + r]).sum())
                };
                let y = screen([chart[4], chart[5], chart[6]]);
                let support = |world: [f32; 3]| {
                    let p = screen(world);
                    p[0] * y[1] - p[1] * y[0]
                };
                let minimum = [0.0, extent[0]]
                    .into_iter()
                    .flat_map(|x| [0.0, extent[2]].map(|z| support(point(chart, [x, 0.0, z]))))
                    .fold(f32::INFINITY, f32::min);
                let placement = PlotPlanePlacement::Axis {
                    extent,
                    axis: 1,
                };
                let axis = model(chart, chart, camera, placement).unwrap();
                assert!((support(point(axis, [0.0; 3])) - minimum).abs() < 0.0001);
                assert_eq!(&axis[..12], &chart[..12]);
                for tick in 0..=4 {
                    let height = extent[1] * tick as f32 / 4.0;
                    let mut text = chart;
                    text[12..15].copy_from_slice(&point(chart, [0.0, height, 0.0]));
                    let text = model(text, chart, camera, placement).unwrap();
                    assert_eq!(point(text, [0.0; 3]), point(axis, [0.0, height, 0.0]));
                }
            }
        }
    }
}
