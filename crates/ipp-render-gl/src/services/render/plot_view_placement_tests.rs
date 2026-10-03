use super::*;
const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

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
