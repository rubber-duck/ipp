use super::*;
use ipp_core::systems::plot::{PlotMesh, PlotPlaneFacing, PlotPlaneLayout, PlotPublishedPlane};

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

fn projection(camera: [f32; 16], center: [f32; 3]) -> [f32; 16] {
    let mut projection = IDENTITY;
    for (row, scale) in [0.12, 0.12, 0.02].into_iter().enumerate() {
        for col in 0..3 {
            projection[col * 4 + row] = camera[row * 4 + col] * scale;
        }
        projection[12 + row] = -(0..3)
            .map(|col| projection[col * 4 + row] * center[col])
            .sum::<f32>();
    }
    projection
}

#[test]
fn grid_faces_follow_camera_depth_with_scale_invariant_angular_ties() {
    let extent = [10.0, 6.0, 8.0];
    for scale in [1e-20, 1e-6, 1.0, 1e6, 1e20] {
        for yaw in [0.0, 1.1, -2.4] {
            let mut chart = camera(yaw, 0.0, 0.0);
            for col in 0..3 {
                for row in 0..3 {
                    chart[col * 4 + row] *= scale * [1.2, 0.8, 1.1][col];
                }
            }
            for delta in [-2e-6, 0.0, 2e-6] {
                let view = camera(yaw + delta, -0.4, 0.7);
                assert_eq!(far_bounds(chart, view, extent)[0], 0.0);
                let moved = model(
                    chart,
                    chart,
                    view,
                    PlotPlanePlacement::Grid {
                        extent,
                        normal: 0,
                    },
                )
                .unwrap();
                assert_eq!(&moved[..12], &chart[..12]);
                assert_eq!(moved, chart);
            }
        }
    }
    let mut opposite = IDENTITY;
    opposite[8..11].copy_from_slice(&[-1.0, -1.0, -1.0]);
    assert_eq!(far_bounds(IDENTITY, opposite, extent), extent);
    let mut reflected = IDENTITY;
    reflected[0] = -2.0;
    reflected[10] = -3.0;
    assert_eq!(far_bounds(reflected, IDENTITY, extent), [0.0, 0.0, 8.0]);
}

#[test]
fn standard_departure_and_return_have_distinct_visibility_thresholds() {
    let policy = AxisPlacementPolicy::default();
    assert!((policy.returning - policy.departure * 1.2).abs() < f32::EPSILON);
    assert_eq!(choose_edge([0.9, 0.0, 0.0, 0.20], 3, Some(3), policy), 3);
    assert_eq!(choose_edge([0.9, 0.0, 0.0, 0.199], 3, Some(3), policy), 0);
    for standard in [0.20, 0.239, 0.21, 0.24, 0.225, 0.201] {
        assert_eq!(
            choose_edge([0.8, 0.1, 0.1, standard], 3, Some(0), policy),
            0
        );
    }
    assert_eq!(choose_edge([0.95, 0.1, 0.1, 0.241], 3, Some(0), policy), 3);
    assert_eq!(choose_edge([0.42, 0.0, 0.40, 0.0], 3, Some(2), policy), 2);
    assert_eq!(choose_edge([0.46, 0.0, 0.40, 0.0], 3, Some(2), policy), 0);
}

#[test]
fn non_default_policy_changes_selection_and_duration_independently() {
    let policy = AxisPlacementPolicy {
        preferred: [0, 1, 2],
        departure: 0.4,
        returning: 0.48,
        alternative_gain: 0.15,
        duration: 4.0,
        label_clearance: 1.1,
        tick_clearance: 0.5,
    };
    assert_eq!(
        choose_edge([0.39, 0.52, 0.1, 0.0], policy.preferred[0], Some(0), policy),
        0
    );
    assert_eq!(
        choose_edge([0.39, 0.56, 0.1, 0.0], policy.preferred[0], Some(0), policy),
        1
    );
    assert_eq!(
        choose_edge([0.481, 0.9, 0.1, 0.0], policy.preferred[0], Some(1), policy),
        0
    );
    let mut driver = AxisTransition::new(0, 10.0);
    driver.update(2, [0.1, 0.8, 0.9, 0.2], 10.0, policy, [6.0, 8.0]);
    assert_eq!(driver.sample(12.0, policy.duration), 1.0);
    assert_eq!(driver.sample(14.0, policy.duration), 2.0);
    assert_eq!(driver.sample(12.0, 2.0), 2.0);
}

#[test]
fn opposite_edge_routes_through_the_more_visible_corner_and_never_the_volume() {
    let policy = AxisPlacementPolicy::default();
    for (scores, middle) in [([0.8, 0.9, 0.1, 0.0], 0.0), ([0.1, 0.9, 0.8, 0.0], 2.0)] {
        let mut driver = AxisTransition::new(3, 0.0);
        driver.update(1, scores, 4.0, policy, [6.0, 8.0]);
        assert_eq!(driver.sample(4.0, policy.duration), 3.0);
        assert_eq!(driver.sample(5.0, policy.duration).rem_euclid(4.0), middle);
        assert_eq!(driver.sample(6.0, policy.duration).rem_euclid(4.0), 1.0);
        for axis in 0..3u8 {
            let extent = [10.0, 6.0, 8.0];
            for step in 0..=40 {
                let station = perimeter_point(
                    driver.sample(4.0 + step as f64 / 20.0, policy.duration),
                    extent,
                    axis,
                );
                assert_eq!(station[usize::from(axis)], 0.0);
                assert!((0..3).any(|other| other != usize::from(axis)
                    && (station[other] == 0.0 || station[other] == extent[other])));
                assert!(
                    station
                        .iter()
                        .enumerate()
                        .all(|(index, value)| *value >= 0.0 && *value <= extent[index])
                );
            }
        }
    }
}

#[test]
fn retargeting_samples_current_position_and_reuses_same_target_driver() {
    let policy = AxisPlacementPolicy::default();
    let mut driver = AxisTransition::new(3, 0.0);
    driver.update(1, [0.9, 1.0, 0.1, 0.0], 0.0, policy, [6.0, 8.0]);
    let before = perimeter_point(driver.sample(0.6, policy.duration), [10.0, 6.0, 8.0], 2);
    driver.update(2, [0.1, 0.0, 1.0, 0.0], 0.6, policy, [6.0, 8.0]);
    assert_eq!(
        perimeter_point(driver.sample(0.6, policy.duration), [10.0, 6.0, 8.0], 2),
        before
    );
    let started = driver.started;
    driver.update(2, [0.1, 0.0, 1.0, 0.0], 0.8, policy, [6.0, 8.0]);
    assert_eq!(driver.started, started);
    assert_eq!(driver.sample(2.6, policy.duration).rem_euclid(4.0), 2.0);
    for phase in [0.0, 1.0, 2.0, 3.0, 4.0] {
        let a = perimeter_point(phase - 1e-5, [10.0, 6.0, 8.0], 2);
        let b = perimeter_point(phase + 1e-5, [10.0, 6.0, 8.0], 2);
        assert!(a.iter().zip(b).all(|(a, b)| (*a - b).abs() < 0.0001));
    }
}

#[test]
fn content_visibility_distinguishes_hidden_and_clear_enclosure_edges() {
    let extent = [10.0, 6.0, 8.0];
    let view = camera(0.5, -0.4, 0.0);
    let matrix = projection(view, [5.0, 3.0, 4.0]);
    let viewport = WorldViewport {
        width: 960,
        height: 760,
        device_pixel_ratio: 1.0,
    };
    let bounds = [[0.0; 3], extent];
    let scores = visibility_scores(IDENTITY, view, matrix, viewport, extent, 1, Some(bounds));
    assert!(scores[0] < 0.01);
    assert!(scores[3] > 0.8);
    let edge_on = camera(0.5, -std::f32::consts::FRAC_PI_2, 0.0);
    assert!(
        visibility_scores(
            IDENTITY,
            edge_on,
            projection(edge_on, [5.0, 3.0, 4.0]),
            viewport,
            extent,
            1,
            Some(bounds)
        )
        .iter()
        .all(|score| *score < 0.001)
    );
    assert!(!hidden_by_content(
        [0.0, 3.0, 8.0],
        [1.0, 1.0, 1.0],
        bounds,
        f64::INFINITY
    ));
    assert!(hidden_by_content(
        [0.0, 3.0, 0.0],
        [1.0, 1.0, 1.0],
        bounds,
        f64::INFINITY
    ));
    assert!(!hidden_by_content(
        [0.0, 0.0, 0.0],
        [1.0; 3],
        [[0.0; 3], [10.0, 0.0, 8.0]],
        f64::INFINITY
    ));
}

#[test]
fn shared_axis_driver_preserves_stations_streaming_and_view_lifetimes() {
    let mut host = ipp_core::HostRuntime::new();
    let id = host.create_world(Default::default(), &[]).unwrap();
    host.frame(0.0).unwrap();
    let world = host.world_ref(id).unwrap();
    let entity = ipp_core::EntityId::from_bits((1 << 32) | 1);
    let target = CanvasTarget {
        entity,
        component: ipp_core::ComponentValue::PLOT_POINTS3D,
        incarnation: 1,
    };
    let publication = host.latest_publication(id).unwrap();
    let extent = [10.0, 6.0, 8.0];
    let geometry = || {
        Arc::new(PlotPreparedGeometry {
            meshes: vec![PlotMesh {
                positions: vec![[0.0; 3], extent],
                ..Default::default()
            }],
            ..Default::default()
        })
    };
    let source = geometry();
    let retained: Vec<_> = (0..3u8)
        .flat_map(|axis| {
            (0..3u32).map(move |part| {
                let mut model = IDENTITY;
                model[12 + usize::from(axis)] = extent[usize::from(axis)] * part as f32 / 2.0;
                PlotPublishedPlane {
                    part: u32::from(axis) * 3 + part,
                    model,
                    facing: if part == 0 {
                        PlotPlaneFacing::Fixed
                    } else {
                        PlotPlaneFacing::Camera
                    },
                    layout: if part == 0 {
                        PlotPlaneLayout::None
                    } else {
                        PlotPlaneLayout::Tick(axis)
                    },
                    placement: PlotPlanePlacement::Axis {
                        extent,
                        axis,
                    },
                    bounds: None,
                    clip: [-1.0, -1.0, 1.0, 1.0],
                    primitives: Arc::from([]),
                }
            })
        })
        .collect();
    let planes = |source| {
        retained
            .iter()
            .map(|plane| ScenePlotPlane {
                entity: super::super::scene::RenderEntity {
                    world,
                    entity,
                    incarnation: 1,
                },
                target,
                publication,
                model: plane.model,
                chart_model: IDENTITY,
                geometry: source,
                plane,
            })
            .collect::<Vec<_>>()
    };
    let viewport = WorldViewport {
        width: 960,
        height: 760,
        device_pixel_ratio: 1.0,
    };
    let mut state = PlotViewPlacementState::default();
    let front = camera(0.5, -0.4, 0.0);
    let placed = state
        .place(
            &planes(&source),
            front,
            projection(front, [5.0, 3.0, 4.0]),
            viewport,
            0.0,
        )
        .unwrap();
    for axis in 0..3usize {
        let expected = match axis {
            0 => [0.0, 0.0, 8.0],
            1 => [0.0, 0.0, 8.0],
            _ => [0.0, 6.0, 0.0],
        };
        assert_eq!(&placed[axis * 3].model[12..15], &expected);
        for part in 1..3 {
            for (row, coordinate) in expected.iter().enumerate() {
                assert_eq!(
                    placed[axis * 3 + part].model[12 + row],
                    coordinate + retained[axis * 3 + part].model[12 + row]
                );
            }
        }
    }
    let rear = camera(-2.6, -0.4, 0.0);
    state
        .place(
            &planes(&source),
            rear,
            projection(rear, [5.0, 3.0, 4.0]),
            viewport,
            1.0,
        )
        .unwrap();
    let x = state.charts[&(world, target)].axes[0].unwrap();
    assert_ne!(x.target, 3);
    assert_eq!(x.started, 1.0);
    let replacement = geometry();
    let during = state
        .place(
            &planes(&replacement),
            rear,
            projection(rear, [5.0, 3.0, 4.0]),
            viewport,
            1.5,
        )
        .unwrap();
    assert_eq!(state.charts[&(world, target)].axes[0].unwrap().started, 1.0);
    assert_eq!(Arc::strong_count(&source), 1);
    assert_eq!(Arc::strong_count(&replacement), 1);
    assert_ne!(during[0].model, placed[0].model);
    assert_eq!(&during[0].model[..12], &retained[0].model[..12]);
    let settled = state
        .place(
            &planes(&replacement),
            rear,
            projection(rear, [5.0, 3.0, 4.0]),
            viewport,
            3.1,
        )
        .unwrap();
    assert_eq!(
        &settled[0].model[12..15],
        &perimeter_point(f64::from(x.target), extent, 0)
    );
    state
        .place(
            &planes(&replacement),
            rear,
            projection(rear, [5.0, 3.0, 4.0]),
            viewport,
            0.5,
        )
        .unwrap();
    assert_eq!(state.charts[&(world, target)].axes[0].unwrap().started, 0.5);
    state.place(&[], rear, IDENTITY, viewport, 0.6).unwrap();
    assert!(state.charts.is_empty());
    assert!(PlotViewPlacementState::default().charts.is_empty());
}

#[test]
fn instant_duration_and_finite_eye_reach_are_bounded() {
    let mut driver = AxisTransition::new(0, 0.0);
    driver.update(
        2,
        [0.0, 0.8, 1.0, 0.2],
        0.0,
        AxisPlacementPolicy::default(),
        [6.0, 8.0],
    );
    for duration in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(driver.sample(0.0, duration), driver.end);
    }
    let bounds = [[0.0; 3], [1.0; 3]];
    assert!(!hidden_by_content(
        [-1.0, 0.5, 0.5],
        [1.0, 0.0, 0.0],
        bounds,
        0.5
    ));
    assert!(hidden_by_content(
        [-1.0, 0.5, 0.5],
        [1.0, 0.0, 0.0],
        bounds,
        2.0
    ));
    assert!(!hidden_by_content(
        [2.0, 0.5, 0.5],
        [1.0, 0.0, 0.0],
        bounds,
        f64::INFINITY
    ));
}

#[test]
fn normalized_face_routes_rescale_and_marker_overhang_does_not_hide_all_edges() {
    for phase in [0.4, 1.6, 2.3, 3.8] {
        let small = perimeter_point(phase, [10.0, 5.0, 8.0], 0);
        let large = perimeter_point(phase, [20.0, 10.0, 24.0], 0);
        assert_eq!(large, [0.0, small[1] * 2.0, small[2] * 3.0]);
    }
    let view = camera(0.5, -0.4, 0.0);
    let viewport = WorldViewport {
        width: 960,
        height: 760,
        device_pixel_ratio: 1.0,
    };
    let scores = visibility_scores(
        IDENTITY,
        view,
        projection(view, [5.0, 3.0, 4.0]),
        viewport,
        [10.0, 6.0, 8.0],
        1,
        Some([[-0.1; 3], [10.1, 6.1, 8.1]]),
    );
    assert!(scores[0] < 0.01);
    assert!(scores[3] > 0.8);
}
