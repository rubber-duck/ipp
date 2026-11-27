use super::{MAX_PIXELS, reserve};
use crate::canvas::CanvasVertex;

fn triangle(points: [[f32; 4]; 3]) -> [CanvasVertex; 3] {
    points.map(|clip| CanvasVertex {
        clip,
        local: [0.; 2],
        world: [0.; 4],
        right: [0.; 3],
        down: [0.; 3],
        normal: [0.; 3],
        slot: 0,
    })
}

#[test]
fn overlapping_paths_get_disjoint_addresses_in_top_left_pixel_order() {
    let vertices = triangle([[-1.5, 1., 0., 1.], [0., 1., 0., 1.], [0., 0., 0., 1.]]);
    let mut used = 0;
    assert_eq!(
        reserve(&vertices, [16, 8], &mut used),
        Some([0., 0., 9., 0.])
    );
    assert_eq!(used, 45);
    assert_eq!(
        reserve(&vertices, [16, 8], &mut used),
        Some([0., 0., 9., 45.])
    );
    assert_eq!(used, 90);
}

#[test]
fn eye_plane_crossings_and_full_buffers_leave_the_cursor_unchanged() {
    let mut vertices = triangle([[-0.5, 0.5, 0., 1.], [0.5, 0.5, 0., 1.], [0., -0.5, 0., 1.]]);
    let mut used = MAX_PIXELS - 2;
    assert!(reserve(&vertices, [100, 100], &mut used).is_none());
    assert_eq!(used, MAX_PIXELS - 2);

    used = 0;
    vertices[2].clip[3] = -1.;
    assert!(reserve(&vertices, [100, 100], &mut used).is_none());
    assert_eq!(used, 0);
}
