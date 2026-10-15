use super::*;

fn projection(depth: f32) -> [f32; 16] {
    [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., depth, 0., 0., 0., 0., 1.,
    ]
}

#[test]
fn separated_layers_follow_view_and_reuse_unchanged_indices() {
    let positions = [
        [0., 0., 1.],
        [1., 0., 1.],
        [0., 1., 1.],
        [0., 0., 3.],
        [1., 0., 3.],
        [0., 1., 3.],
    ];
    let original = [0, 1, 2, 3, 4, 5];
    let mut order = TriangleOrder::default();
    assert!(order.update(&positions, &original, Some(projection(1.))));
    assert_eq!(order.indices, [3, 4, 5, 0, 1, 2]);
    assert!(!order.update(&positions, &original, Some(projection(1.))));
    assert!(!order.update(&positions, &original, Some(projection(2.))));
    assert!(order.update(&positions, &original, Some(projection(-1.))));
    assert_eq!(order.indices, original);
    assert!(order.update(&positions, &original, Some(projection(1.))));
    assert!(order.update(&positions, &original, None));
    assert_eq!(order.indices, original);
    assert!(!order.update(&positions, &original, None));
}

#[test]
fn long_sloping_wall_orders_by_overlap_not_its_distant_centre() {
    // The small cap overlaps only the near end of a long wall. Its centre
    // suggests the opposite order, producing triangular patches at the seam.
    let positions = [
        [0., 0., 0.],
        [10., 0., 10.],
        [0., 10., 0.],
        [0., 0., 2.],
        [1., 0., 2.],
        [0., 1., 2.],
    ];
    let original = [0, 1, 2, 3, 4, 5];
    let mut order = TriangleOrder::default();
    assert!(order.update(&positions, &original, Some(projection(1.))));
    assert_eq!(order.indices, [3, 4, 5, 0, 1, 2]);
    assert!(order.update(&positions, &original, Some(projection(-1.))));
    assert_eq!(order.indices, original);
}

#[test]
fn coplanar_and_edge_only_contacts_do_not_create_false_constraints() {
    let positions = [[0., 0., 1.], [1., 0., 1.], [0., 1., 1.]];
    let original = [0, 1, 2, 2, 1, 0];
    let mut order = TriangleOrder::default();
    assert!(!order.update(&positions, &original, Some(projection(1.))));
    assert_eq!(order.indices, original);
    let a = ProjectedTriangle::new(&positions, &[0, 1, 2], projection(1.));
    let b = ProjectedTriangle::new(
        &[[1., 0., 2.], [1., 1., 2.], [0., 1., 2.]],
        &[0, 1, 2],
        projection(1.),
    );
    assert_eq!(overlap_order(&a, &b), None);
}

#[test]
fn intersecting_faces_and_near_plane_crossings_remain_finite_and_complete() {
    let positions = [
        [0., 0., -1.],
        [1., 0., 1.],
        [0., 1., 1.],
        [0., 0., 1.],
        [1., 0., -1.],
        [0., 1., -1.],
    ];
    let original = [0, 1, 2, 3, 4, 5];
    let mut matrix = projection(1.);
    matrix[11] = 1.;
    matrix[15] = 0.;
    let mut order = TriangleOrder::default();
    order.update(&positions, &original, Some(matrix));
    assert_eq!(order.indices.len(), original.len());
    order.update(&positions, &original, Some(projection(1.)));
    let mut indices = order.indices.clone();
    indices.sort();
    assert_eq!(indices, original);
}
