use super::{cross, fill_grid};

#[test]
fn certified_and_boundary_cells_preserve_the_triangle_union_and_its_hole() {
    let mut triangles = Vec::new();
    for x in 0..16 {
        for y in 0..16 {
            if (4..12).contains(&x) && (4..12).contains(&y) {
                continue;
            }
            let (x, y) = (x as f32, y as f32);
            triangles.extend([
                [[x, y], [x + 1.0, y], [x + 1.0, y + 1.0]],
                [[x, y], [x + 1.0, y + 1.0], [x, y + 1.0]],
            ]);
        }
    }
    // A separate series could have cells covering the hole; overlaps within
    // this particular paint count once, including the shared diagonal.
    triangles.push([[8.0, 9.0], [15.0, 10.0], [7.0, 13.0]]);
    let grid = fill_grid(&triangles, [0.0, 0.0, 16.0, 16.0]).unwrap();
    let side = (-grid[0][0]) as usize;
    let cells = &grid[2..2 + side * side];
    let ids = &grid[2 + cells.len()..];
    assert!(cells.iter().any(|c| c[1] == 0.0 && c[2] == 1.0));
    assert!(cells.iter().any(|c| c[1] == 0.0 && c[2] == 0.0));
    for j in 0..128 {
        for i in 0..128 {
            let p = [f64::from(i) * 0.125, f64::from(j) * 0.125];
            let inside = |&[a, b, c]: &[[f32; 2]; 3]| {
                cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
            };
            let [first, n, base, _] = cells
                [(p[1] as f32 * grid[0][2]) as usize * side + (p[0] as f32 * grid[1][1]) as usize];
            let indexed = base != 0.0
                || (first as usize..(first + n) as usize)
                    .any(|k| inside(&triangles[ids[k / 4][k % 4] as usize]));
            assert_eq!(indexed, triangles.iter().any(inside), "sample {p:?}");
        }
    }
}

#[test]
fn broad_overlapping_triangles_and_mixed_winding_use_the_banded_fallback() {
    let triangle = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    assert!(fill_grid(&[triangle; 256], [0.0, 0.0, 1.0, 1.0]).is_none());

    let mut mixed = [triangle; 16];
    mixed[0].swap(1, 2);
    assert!(fill_grid(&mixed, [0.0, 0.0, 1.0, 1.0]).is_none());
}
