use super::*;

#[test]
fn canvas_sector_respects_hole_and_clockwise_angles() {
    let shape = PlotHitShape::Sector {
        center: [10.0, 10.0],
        inner_radius: 2.0,
        radius: 8.0,
        start: 0.0,
        sweep: std::f32::consts::FRAC_PI_2,
    };
    assert!(shape.contains_canvas_point([14.0, 6.0]));
    assert!(!shape.contains_canvas_point([6.0, 6.0]));
    assert!(!shape.contains_canvas_point([10.0, 9.0]));
}

#[test]
fn radial_prism_rejects_whole_box_false_hit_and_preserves_exit() {
    let shape = PlotHitShape::RadialPrism {
        center: [0.0; 3],
        radius: 2.0,
        start: 0.0,
        sweep: std::f32::consts::FRAC_PI_2,
        min_y: 0.0,
        max_y: 1.0,
    };
    let outside = GeometryRay {
        origin: [-1.0, 2.0, 1.0],
        direction: [0.0, -1.0, 0.0],
    };
    assert_eq!(shape.ray_intersection(&outside, 0.0, 10.0), None);
    let inside = GeometryRay {
        origin: [1.0, 0.5, 1.0],
        direction: [0.0, 1.0, 0.0],
    };
    assert_eq!(shape.ray_intersection(&inside, 0.0, 10.0), Some(0.5));
}

#[test]
fn large_sector_radial_side_does_not_pick_opposite_interior_line() {
    let shape = PlotHitShape::RadialPrism {
        center: [0.0; 3],
        radius: 2.0,
        start: 0.0,
        sweep: 1.5 * std::f32::consts::PI,
        min_y: 0.0,
        max_y: 1.0,
    };
    let ray = GeometryRay {
        origin: [1.0, 0.5, -1.0],
        direction: [-1.0, 0.0, 0.0],
    };
    let hit = shape.ray_intersection(&ray, 0.0, 10.0).unwrap();
    assert!((hit - (1.0 + 3.0_f64.sqrt())).abs() < 1e-6);
}

#[test]
fn canonical_label_identity_retains_all_u64_bits() {
    let label = PlotLabelRow {
        row_id: u64::MAX.to_string().into(),
        ..Default::default()
    };
    assert_eq!(
        label.source_row().unwrap(),
        crate::services::data::DataRowId(u64::MAX)
    );
    for invalid in ["01", "-1", "18446744073709551616", "1.0"] {
        assert!(
            PlotLabelRow {
                row_id: invalid.into(),
                ..Default::default()
            }
            .source_row()
            .is_err()
        );
    }
}
