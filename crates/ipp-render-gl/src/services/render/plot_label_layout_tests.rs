//! Focused selected-view placement invariants supplement real native captures.

use super::*;

// Tests specify independent rectangles; production uses its per-view broad phase.
fn place(
    bounds: [f32; 4],
    role: PlotPlaneLayout,
    size: [u32; 2],
    occupied: &[[f32; 4]],
    previous: Option<[f32; 2]>,
) -> [f32; 2] {
    let mut index = Occupancy::default();
    for &bounds in occupied {
        index.insert(bounds);
    }
    super::place(bounds, role, size, &index, previous)
}

const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

#[test]
fn coincident_endpoint_ticks_get_disjoint_local_lanes() {
    let bounds = [350.0, 270.0, 382.0, 283.0];
    let x = place(bounds, PlotPlaneLayout::Tick(0), [732, 348], &[], None);
    let occupied = translated(bounds, x);
    let z = place(
        bounds,
        PlotPlaneLayout::Tick(2),
        [732, 348],
        &[occupied],
        None,
    );
    assert_eq!(x, [0.0; 2]);
    assert_eq!(overlap(occupied, translated(bounds, z), PADDING), 0.0);
    assert!(z[0].hypot(z[1]) < 70.0);
}

#[test]
fn titles_are_outside_ticks_and_each_other_without_shrinking() {
    let tick = [300.0, 200.0, 336.0, 213.0];
    let title = [310.0, 209.0, 445.0, 222.0];
    let x = place(title, PlotPlaneLayout::Title(0), [732, 348], &[tick], None);
    let placed = translated(title, x);
    let z = place(
        title,
        PlotPlaneLayout::Title(2),
        [732, 348],
        &[tick, placed],
        None,
    );
    assert_eq!(overlap(placed, tick, PADDING), 0.0);
    assert_eq!(overlap(placed, translated(title, z), PADDING), 0.0);
    assert!(x[1] > 0.0);
    assert_eq!(placed[2] - placed[0], title[2] - title[0]);
}

#[test]
fn overlapping_callout_panels_are_kept_inside_the_viewport() {
    let first = [650.0, 15.0, 725.0, 37.0];
    let second = [670.0, 20.0, 745.0, 42.0];
    let shift = place(second, PlotPlaneLayout::Callout, [732, 348], &[first], None);
    let moved = translated(second, shift);
    assert_eq!(overlap(first, moved, PADDING), 0.0);
    assert!(moved[0] >= PADDING && moved[1] >= PADDING);
    assert!(moved[2] <= 732.0 - PADDING && moved[3] <= 348.0 - PADDING);
}

#[test]
fn small_camera_movement_keeps_a_valid_previous_lane() {
    let bounds = [100.0, 100.0, 140.0, 113.0];
    let obstacle = [118.0, 102.0, 158.0, 115.0];
    let prior = place(
        bounds,
        PlotPlaneLayout::Callout,
        [732, 348],
        &[obstacle],
        None,
    );
    let moved_bounds = translated(bounds, [0.2, 0.2]);
    let moved_obstacle = translated(obstacle, [0.1, 0.1]);
    assert_eq!(
        place(
            moved_bounds,
            PlotPlaneLayout::Callout,
            [732, 348],
            &[moved_obstacle],
            Some(prior)
        ),
        prior
    );
}

#[test]
fn pixel_translation_preserves_anchor_depth_under_perspective() {
    let mut model = IDENTITY;
    model[5] = -1.0;
    model[14] = -5.0;
    // Perspective with homogeneous W=-Z and deliberately non-square pixel scale.
    let projection = [
        1.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 0.0, 0.0, -2.0, 0.0,
    ];
    let before = project(point(model, [0.0; 2]), projection, [732, 348]).unwrap();
    let after_model = shifted_model(model, [24.0, -17.0], projection, [732, 348]).unwrap();
    let after = project(point(after_model, [0.0; 2]), projection, [732, 348]).unwrap();
    assert!((after[0] - before[0] - 24.0).abs() < 0.001);
    assert!((after[1] - before[1] + 17.0).abs() < 0.001);
    assert_eq!(after_model[14], model[14]);
    assert_eq!(&after_model[..12], &model[..12]);
    let invalid_projection = [0.0; 16];
    assert!(shifted_model(model, [10.0; 2], invalid_projection, [732, 348]).is_none());
}

#[test]
fn moved_callout_connector_starts_at_data_and_ends_at_panel() {
    let mut anchor = IDENTITY;
    anchor[5] = -1.0;
    anchor[12..15].copy_from_slice(&[2.0, 3.0, 4.0]);
    let mut panel = anchor;
    panel[12] += 0.4;
    panel[13] -= 0.7;
    let endpoint = [1.5, -2.0];
    let strip = connector_model(anchor, panel, endpoint, 0.03);
    assert_eq!(point(strip, [0.0, 0.0]), point(anchor, [0.0; 2]));
    let actual = point(strip, [1.0, 0.0]);
    let expected = point(panel, endpoint);
    for axis in 0..3 {
        assert!((actual[axis] - expected[axis]).abs() < 0.00001);
    }
    assert!((strip[4].hypot(strip[5]).hypot(strip[6]) - 0.03).abs() < 0.00001);
    let dot = strip[0] * strip[4] + strip[1] * strip[5] + strip[2] * strip[6];
    assert!(dot.abs() < 0.00001);
}

#[test]
fn impossible_density_is_bounded_deterministic_and_never_hides_labels() {
    let bounds = [0.0, 0.0, 400.0, 120.0];
    let blockers = vec![bounds; 40];
    let a = place(bounds, PlotPlaneLayout::Callout, [120, 80], &blockers, None);
    let b = place(bounds, PlotPlaneLayout::Callout, [120, 80], &blockers, None);
    assert_eq!(a, b);
    assert!(
        a.iter()
            .all(|offset| offset.is_finite() && offset.abs() <= 28.0 * RINGS as f32)
    );
}

#[test]
fn history_is_weak_source_qualified_and_resets_on_viewport_change() {
    let mut host = ipp_core::HostRuntime::new();
    let id = host.create_world(Default::default(), &[]).unwrap();
    host.frame(0.0).unwrap();
    let world = host.world_ref(id).unwrap();
    let publication = host.latest_publication(id).unwrap();
    let entity = ipp_core::EntityId::from_bits((1 << 32) | 1);
    let target = CanvasTarget {
        entity,
        component: ipp_core::ComponentValue::PLOT_POINTS3D,
        incarnation: 1,
    };
    let source = Arc::new(PlotPreparedGeometry::default());
    let mut model = IDENTITY;
    model[5] = -1.0;
    let retained = ipp_core::systems::plot::PlotPublishedPlane {
        part: 0,
        model,
        facing: ipp_core::systems::plot::PlotPlaneFacing::Camera,
        layout: PlotPlaneLayout::Callout,
        placement: ipp_core::systems::plot::PlotPlanePlacement::Fixed,
        bounds: Some([0.0, 0.0, 0.1, 0.05]),
        clip: [-1.0, -1.0, 1.0, 1.0],
        primitives: Arc::from([]),
    };
    let plane = ScenePlotPlane {
        entity: super::super::scene::RenderEntity {
            world,
            entity,
            incarnation: 1,
        },
        target,
        publication,
        model,
        chart_model: IDENTITY,
        geometry: &source,
        plane: &retained,
    };
    let viewport = WorldViewport {
        width: 732,
        height: 348,
        device_pixel_ratio: 1.0,
    };
    let mut state = PlotLabelLayoutState::default();
    state.arrange(std::slice::from_ref(&plane), IDENTITY, viewport);
    assert_eq!(state.previous.len(), 1);
    assert_eq!(Arc::strong_count(&source), 1);
    let key = (world, target, 0);
    state.previous.get_mut(&key).unwrap().offset = [2.0, 0.0];
    assert!(state.arrange(std::slice::from_ref(&plane), IDENTITY, viewport)[0].model[12] > 0.0);
    let replacement = Arc::new(PlotPreparedGeometry::default());
    let replacement_plane = ScenePlotPlane {
        geometry: &replacement,
        ..plane
    };
    assert_eq!(
        state.arrange(&[replacement_plane], IDENTITY, viewport)[0].model[12],
        0.0
    );
    state.arrange(
        &[],
        IDENTITY,
        WorldViewport {
            width: 400,
            ..viewport
        },
    );
    assert!(state.previous.is_empty());
}

#[test]
fn spatial_candidate_index_matches_independent_rectangles_and_bounds_cell_loops() {
    let rectangles = [
        [63.0, 60.0, 145.0, 80.0],
        [-30.0, -20.0, 4.0, 8.0],
        [500.0, 500.0, 530.0, 520.0],
        [-1e20, -1e20, 1e20, 1e20],
    ];
    let mut occupied = Occupancy::default();
    for rect in rectangles {
        occupied.insert(rect);
    }
    assert_eq!(occupied.wide.len(), 1);
    for candidate in [
        [58.0, 55.0, 75.0, 68.0],
        [30.0, 20.0, 50.0, 40.0],
        [-10.0, -10.0, 10.0, 10.0],
    ] {
        let expected = rectangles
            .iter()
            .map(|&rect| overlap(candidate, rect, 8.0))
            .sum::<f32>();
        assert_eq!(occupied.intersections(candidate, 8.0), expected);
    }
    assert!(occupied.cells.len() < 20);
}

#[test]
fn near_camera_overflow_is_not_admitted_to_projected_layout() {
    let mut projection = IDENTITY;
    projection[15] = f32::MIN_POSITIVE;
    assert!(project([1.0, 1.0, 0.0], projection, [732, 348]).is_none());
    let mut invalid_model = IDENTITY;
    invalid_model[12] = f32::MAX;
    assert!(shifted_model(invalid_model, [10.0; 2], projection, [732, 348]).is_none());
}

#[test]
fn finite_stations_outside_gl_depth_do_not_participate_in_layout() {
    // Perspective near=1, far=11, W=-Z. X/Y may be offscreen: the caller
    // admits actual ink bounds, rather than clipping individual anchor points.
    let projection = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.2, -1.0, 0.0, 0.0, -2.2, 0.0,
    ];
    for z in [1.0, -0.001, -0.5, -12.0] {
        assert!(project([0.0, 0.0, z], projection, [427, 600]).is_none());
    }
    assert!(project([20.0, 0.0, -5.0], projection, [427, 600]).is_some());
}

#[test]
fn offscreen_ink_keeps_its_station_and_cannot_displace_visible_labels() {
    let mut host = ipp_core::HostRuntime::new();
    let id = host.create_world(Default::default(), &[]).unwrap();
    host.frame(0.0).unwrap();
    let world = host.world_ref(id).unwrap();
    let publication = host.latest_publication(id).unwrap();
    let entity = ipp_core::EntityId::from_bits((1 << 32) | 1);
    let target = CanvasTarget {
        entity,
        component: ipp_core::ComponentValue::PLOT_POINTS3D,
        incarnation: 1,
    };
    let source = Arc::new(PlotPreparedGeometry::default());
    let mut model = IDENTITY;
    model[5] = -1.0;
    let label = ipp_core::systems::plot::PlotPublishedPlane {
        part: 0,
        model,
        facing: ipp_core::systems::plot::PlotPlaneFacing::Camera,
        layout: PlotPlaneLayout::Title(0),
        placement: PlotPlanePlacement::Fixed,
        bounds: Some([-0.1, -0.02, 0.1, 0.02]),
        clip: [-100.0, -100.0, 100.0, 100.0],
        primitives: Arc::from([]),
    };
    let visible = ipp_core::systems::plot::PlotPublishedPlane {
        part: 1,
        layout: PlotPlaneLayout::Callout,
        ..label.clone()
    };
    let connector = ipp_core::systems::plot::PlotPublishedPlane {
        part: 2,
        layout: PlotPlaneLayout::Connector {
            panel: 0,
            endpoint: [0.1, 0.0],
            width: 0.01,
        },
        ..label.clone()
    };
    let scene_plane = |plane| ScenePlotPlane {
        entity: super::super::scene::RenderEntity {
            world,
            entity,
            incarnation: 1,
        },
        target,
        publication,
        model,
        chart_model: IDENTITY,
        geometry: &source,
        plane,
    };
    for width in [427, 1280] {
        let viewport = WorldViewport {
            width,
            height: 600,
            device_pixel_ratio: 1.0,
        };
        for translation in [[-1.2, 0.0], [1.2, 0.0], [0.0, -1.2], [0.0, 1.2]] {
            let mut offscreen = scene_plane(&label);
            offscreen.model[12..14].copy_from_slice(&translation);
            let planes = [
                offscreen.clone(),
                scene_plane(&visible),
                scene_plane(&connector),
            ];
            // Stale history from a previous visible frame cannot pull it back in.
            let mut state = PlotLabelLayoutState {
                viewport: [width, 600],
                ..Default::default()
            };
            state.previous.insert(
                (world, target, 0),
                Previous {
                    source: Arc::downgrade(&source),
                    offset: [100.0, 100.0],
                },
            );
            let arranged = state.arrange(&planes, IDENTITY, viewport);
            let alone = PlotLabelLayoutState::default().arrange(&planes[1..2], IDENTITY, viewport);
            assert_eq!(arranged[0].model, offscreen.model);
            assert_eq!(arranged[1].model, alone[0].model);
            assert_eq!(arranged[2].model, planes[2].model);
            assert_eq!(state.previous.len(), 1);
            assert!(!state.previous.contains_key(&(world, target, 0)));
        }
        // Ink crossing either horizontal edge is still fitted; clipping only
        // its offscreen anchor would incorrectly suppress this useful label.
        for x in [-1.05, 1.05] {
            let mut partial = scene_plane(&label);
            partial.model[12] = x;
            let mut state = PlotLabelLayoutState::default();
            let arranged = state.arrange(&[partial.clone()], IDENTITY, viewport);
            assert_ne!(arranged[0].model, partial.model);
            assert_eq!(state.previous.len(), 1);
        }
    }
}

#[test]
fn title_lane_fits_near_view_edge_without_jumping_back_over_chart() {
    let title = [250.0, 330.0, 390.0, 342.0];
    let offset = place(title, PlotPlaneLayout::Title(0), [732, 348], &[], None);
    let arranged = translated(title, offset);
    assert!(arranged[3] <= 348.0 - PADDING);
    assert!(offset[1] >= -7.0 && offset[1] < 0.0);
}

#[test]
fn radial_candidates_preserve_ray_side_and_only_extend_under_collision() {
    let mut occupied = Occupancy::default();
    occupied.insert([100.0, 100.0, 180.0, 125.0]);
    let direction = [0.8, 0.6];
    let preferred = [100.0, 100.0];
    let placed = place_radial(
        [0.0, 0.0, 80.0, 25.0],
        preferred,
        direction,
        [960, 760],
        &occupied,
        Some([-500.0, 200.0]),
    );
    let delta = [placed[0] - preferred[0], placed[1] - preferred[1]];
    assert!(delta[0] >= 0.0 && delta[1] >= 0.0);
    assert!((delta[0] * direction[1] - delta[1] * direction[0]).abs() < 0.001);
    assert_eq!(
        occupied.intersections(translated([0.0, 0.0, 80.0, 25.0], placed), 17.5),
        0.0
    );
}

#[test]
fn radial_connector_attaches_to_inward_panel_edge_without_changing_anchor() {
    let mut panel = IDENTITY;
    panel[12] = 4.0;
    panel[13] = 2.0;
    let bounds = [0.0, 0.0, 2.0, 1.0];
    let edge = panel_edge(IDENTITY, panel, bounds);
    assert_eq!(edge[0], 0.0);
    assert!(edge[1] >= bounds[1] && edge[1] <= bounds[3]);
    let strip = connector_model(IDENTITY, panel, edge, 0.02);
    assert_eq!(&strip[12..15], &[0.0; 3]);
    assert_eq!(&strip[..3], &point(panel, edge));
}

#[test]
fn perimeter_lane_clears_frame_support_without_changing_axis_order() {
    let corners = [
        [100.0, 100.0],
        [400.0, 100.0],
        [400.0, 300.0],
        [100.0, 300.0],
    ];
    let bounds = [200.0, 180.0, 230.0, 190.0];
    let (offset, normal) = perimeter_station(bounds, &corners, [0.0, -1.0], 10.0, 2.5);
    let moved = translated(bounds, offset);
    assert_eq!(normal, [0.0, -1.0]);
    assert_eq!(moved[0], bounds[0]);
    assert_eq!(moved[3], 90.0);
    let occupied = Occupancy::default();
    let extra = place_constrained(
        moved,
        PlotPlaneLayout::Tick(0),
        [960, 760],
        &occupied,
        Some([0.0, 80.0]),
        Some(normal),
    );
    assert!(extra[0] * normal[0] + extra[1] * normal[1] >= 0.0);
}

#[test]
fn repeated_collision_scores_reuse_indices_and_preserve_sum_order() {
    let mut occupied = Occupancy::default();
    let rectangles: Vec<_> = (0..80)
        .map(|i| {
            let x = (i % 13) as f32 * 11.3;
            let y = (i / 13) as f32 * 7.1;
            [x, y, x + 80.3, y + 24.7]
        })
        .collect();
    for &bounds in &rectangles {
        occupied.insert(bounds);
    }
    let query = [45.2, 8.7, 122.9, 41.3];
    let expected: f32 = rectangles.iter().map(|&r| overlap(query, r, 4.0)).sum();
    assert_eq!(occupied.intersections(query, 4.0), expected);
    let capacity = occupied.candidates.borrow().capacity();
    for _ in 0..100 {
        assert_eq!(occupied.intersections(query, 4.0), expected);
        assert_eq!(occupied.candidates.borrow().capacity(), capacity);
        // An empty query must not retain previous candidate identities.
        assert_eq!(
            occupied.intersections([1000.0, 1000.0, 1010.0, 1010.0], 4.0),
            0.0
        );
    }
}

#[test]
fn prepared_numeric_ink_centers_on_its_vertical_tick_without_changing_lane() {
    let bounds = [110.0, 208.0, 140.0, 220.0];
    let station = [150.0, 204.0];
    for axis in [[0.0, -1.0], [0.6, -0.8]] {
        let offset = tick_center_offset(bounds, station, axis);
        let original = [(bounds[0] + bounds[2]) * 0.5, (bounds[1] + bounds[3]) * 0.5];
        let center = [original[0] + offset[0], original[1] + offset[1]];
        let along = (center[0] - station[0]) * axis[0] + (center[1] - station[1]) * axis[1];
        assert!(along.abs() < 0.0001);
        assert!((offset[0] * axis[1] - offset[1] * axis[0]).abs() < 0.0001);
        assert_eq!(
            translated(bounds, offset)[2] - translated(bounds, offset)[0],
            bounds[2] - bounds[0]
        );
    }
    assert_eq!(
        tick_center_offset(bounds, station, [0.0, -1.0]),
        [0.0, -10.0]
    );
}

#[test]
fn small_axis_ink_does_not_acquire_fixed_pixel_collision_lanes() {
    let mut occupied = Occupancy::default();
    for index in 0..5 {
        // Five readable stations on a projected 20px axis. Their .6px ink is
        // disjoint; fixed 4px padding formerly pushed alternate ticks by 5px.
        let x = 100.0 + index as f32 * 5.0;
        let bounds = [x, 100.0, x + 2.0, 100.6];
        let offset = place_tick(bounds, [0.0, 1.0], [720, 480], &occupied, None);
        assert_eq!(offset, [0.0; 2]);
        occupied.insert(bounds);
    }
    let bounds = [100.0, 100.0, 102.0, 100.6];
    let title = place_constrained(
        bounds,
        PlotPlaneLayout::Title(0),
        [720, 480],
        &occupied,
        None,
        Some([0.0, 1.0]),
    );
    assert!(title[0].hypot(title[1]) <= 2.4 + 0.0001);
}

#[test]
fn crowded_axis_labels_stay_local_and_reject_distant_history() {
    let mut occupied = Occupancy::default();
    occupied.insert([95.0, 95.0, 130.0, 155.0]);
    let bounds = [100.0, 100.0, 102.0, 100.6];
    let height = bounds[3] - bounds[1];
    for normal in [[0.0, 1.0], [0.6, 0.8]] {
        for previous in [None, Some([0.0, 61.0]), Some([100.0, -200.0])] {
            let tick = place_tick(bounds, normal, [720, 480], &occupied, previous);
            assert!(tick[0].hypot(tick[1]) <= height * 2.0 + 0.0001);
            assert!((tick[0] * normal[1] - tick[1] * normal[0]).abs() < 0.0001);
            assert!(tick[0] * normal[0] + tick[1] * normal[1] >= 0.0);
            let title = place_constrained(
                bounds,
                PlotPlaneLayout::Title(1),
                [720, 480],
                &occupied,
                previous,
                Some(normal),
            );
            assert!(title[0].hypot(title[1]) <= height * 4.0 + 0.0001);
            assert!(title[0] * normal[0] + title[1] * normal[1] >= -0.0001);
        }
    }
    // Fixed 6px hysteresis formerly preserved 5.5px even without obstacles.
    assert_eq!(
        place_tick(
            bounds,
            [0.0, 1.0],
            [720, 480],
            &Occupancy::default(),
            Some([0.0, 5.5]),
        ),
        [0.0; 2]
    );
}

#[test]
fn axis_history_shrinks_on_zoom_out_in_the_same_viewport() {
    let mut host = ipp_core::HostRuntime::new();
    let id = host.create_world(Default::default(), &[]).unwrap();
    host.frame(0.0).unwrap();
    let world = host.world_ref(id).unwrap();
    let publication = host.latest_publication(id).unwrap();
    let entity = ipp_core::EntityId::from_bits((1 << 32) | 1);
    let target = CanvasTarget {
        entity,
        component: ipp_core::ComponentValue::PLOT_POINTS3D,
        incarnation: 1,
    };
    let source = Arc::new(PlotPreparedGeometry::default());
    let mut model = IDENTITY;
    model[5] = -1.0;
    let retained: Vec<_> = (0..3)
        .map(|part| ipp_core::systems::plot::PlotPublishedPlane {
            part,
            model,
            facing: ipp_core::systems::plot::PlotPlaneFacing::Camera,
            layout: if part < 2 {
                PlotPlaneLayout::Tick(1)
            } else {
                PlotPlaneLayout::Title(1)
            },
            placement: PlotPlanePlacement::Axis {
                extent: [0.1, 0.1, 0.1],
                axis: 1,
            },
            bounds: Some([0.0, 0.0, 0.025, 0.02]),
            clip: [-1.0, -1.0, 1.0, 1.0],
            primitives: Arc::from([]),
        })
        .collect();
    let planes: Vec<_> = retained
        .iter()
        .map(|plane| ScenePlotPlane {
            entity: super::super::scene::RenderEntity {
                world,
                entity,
                incarnation: 1,
            },
            target,
            publication,
            model,
            chart_model: IDENTITY,
            geometry: &source,
            plane,
        })
        .collect();
    let viewport = WorldViewport {
        width: 720,
        height: 480,
        device_pixel_ratio: 1.0,
    };
    let mut state = PlotLabelLayoutState::default();
    let mut near_offset = 0.0;
    for scale in [1.0, 0.125, 1.0] {
        let mut projection = IDENTITY;
        projection[0] = scale;
        projection[5] = scale;
        let arranged = state.arrange(&planes, projection, viewport);
        assert_eq!(arranged.len(), planes.len());
        assert_eq!(state.viewport, [720, 480]);
        assert_eq!(state.previous.len(), 3);
        for (original, arranged) in planes.iter().zip(&arranged) {
            let bounds = project_bounds(
                model,
                original.plane.bounds.unwrap(),
                projection,
                state.viewport,
            )
            .unwrap();
            let height = bounds[3] - bounds[1];
            let (perimeter, normal) =
                axis_perimeter(original, bounds, projection, state.viewport).unwrap();
            let origin = project(point(model, [0.0; 2]), projection, state.viewport).unwrap();
            let moved =
                project(point(arranged.model, [0.0; 2]), projection, state.viewport).unwrap();
            let collision = [
                moved[0] - origin[0] - perimeter[0],
                moved[1] - origin[1] - perimeter[1],
            ];
            let limit = if original.plane.part < 2 {
                2.0
            } else {
                4.0
            };
            assert!(collision[0].hypot(collision[1]) <= height * limit + 0.0001);
            if original.plane.part < 2 {
                assert!((collision[0] * normal[1] - collision[1] * normal[0]).abs() < 0.0001);
            }
            assert_eq!(&arranged.model[..12], &model[..12]);
            assert_eq!(arranged.model[14], model[14]);
            if original.plane.part == 1 {
                let distance = state.previous[&(world, target, 1)].offset[0].abs();
                if scale == 1.0 {
                    assert!(distance > 0.0);
                    near_offset = distance;
                } else {
                    assert!(distance < near_offset * 0.2);
                }
            }
        }
    }
    assert_eq!(Arc::strong_count(&source), 1);
    assert_eq!(retained[0].model, model);

    for depth in [0.1, 10.0] {
        // The Z axis projects to a point; the Y axis of a deeper enclosure has
        // clipped frame corners. Neither fallback may widen the fitting range.
        let axis = if depth < 1.0 {
            2
        } else {
            1
        };
        let collapsed: Vec<_> = retained
            .iter()
            .map(|plane| ipp_core::systems::plot::PlotPublishedPlane {
                layout: if plane.part < 2 {
                    PlotPlaneLayout::Tick(axis)
                } else {
                    PlotPlaneLayout::Title(axis)
                },
                placement: PlotPlanePlacement::Axis {
                    extent: [0.1, 0.1, depth],
                    axis,
                },
                ..plane.clone()
            })
            .collect();
        let collapsed_planes: Vec<_> = planes
            .iter()
            .zip(&collapsed)
            .map(|(original, plane)| ScenePlotPlane {
                plane,
                ..original.clone()
            })
            .collect();
        // Reuse this exact view and inject a previously valid distant lane.
        for old in state.previous.values_mut() {
            old.offset = [61.0, 61.0];
        }
        let mut projection = IDENTITY;
        projection[0] = 0.125;
        projection[5] = 0.125;
        let arranged = state.arrange(&collapsed_planes, projection, viewport);
        for (original, arranged) in collapsed_planes.iter().zip(&arranged) {
            let bounds = project_bounds(
                model,
                original.plane.bounds.unwrap(),
                projection,
                state.viewport,
            )
            .unwrap();
            assert!(axis_perimeter(original, bounds, projection, state.viewport).is_none());
            let old = project(point(model, [0.0; 2]), projection, state.viewport).unwrap();
            let new = project(point(arranged.model, [0.0; 2]), projection, state.viewport).unwrap();
            if original.plane.part < 2 {
                assert_eq!(old, new);
            } else {
                assert!(
                    (new[0] - old[0]).hypot(new[1] - old[1])
                        <= (bounds[3] - bounds[1]) * 4.0 + 0.0001
                );
            }
        }
    }
}
