use super::{midpoint, monotonic_edges, winding_grid};

type Edge = [[f32; 2]; 3];

fn add_loop(edges: &mut Vec<Edge>, vertices: &[[f32; 2]]) {
    for (&a, &c) in vertices.iter().zip(vertices.iter().cycle().skip(1)) {
        monotonic_edges(edges, a, midpoint(a, c), c);
    }
}

fn crossing([a, b, c]: Edge, [x, y]: [f32; 2]) -> i32 {
    if y < a[1].min(c[1]) || y >= a[1].max(c[1]) {
        return 0;
    }

    let sample = |axis: usize, t: f64| {
        let u = 1.0 - t;
        u * u * f64::from(a[axis]) + 2.0 * u * t * f64::from(b[axis]) + t * t * f64::from(c[axis])
    };
    let increasing = c[1] > a[1];
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..48 {
        let t = (low + high) * 0.5;
        if (sample(1, t) < f64::from(y)) == increasing {
            low = t;
        } else {
            high = t;
        }
    }

    if sample(0, (low + high) * 0.5) > f64::from(x) {
        if increasing {
            1
        } else {
            -1
        }
    } else {
        0
    }
}

#[test]
fn cell_winding_matches_all_edges_at_cell_and_contour_boundaries() {
    let mut edges = Vec::new();
    add_loop(
        &mut edges,
        &[[-2.0, -2.0], [14.0, -2.0], [14.0, 14.0], [-2.0, 14.0]],
    );
    add_loop(
        &mut edges,
        &[[2.0, 2.0], [2.0, 11.0], [11.0, 11.0], [11.0, 2.0]],
    );
    monotonic_edges(&mut edges, [0.0, 0.0], [11.0, 16.0], [7.0, 0.0]);

    // This second loop overlaps the first and reaches winding two. A signed
    // base must preserve both nonzero winding and even-odd fill at once.
    add_loop(
        &mut edges,
        &[[-1.0, -1.0], [6.0, -1.0], [6.0, 13.0], [-1.0, 13.0]],
    );
    let bounds = [-2.0, -2.0, 14.0, 14.0];
    let index = winding_grid(&edges, bounds, 16).unwrap();
    let side = (-index[0][0]) as usize;
    let descriptors = &index[2..2 + side * side];
    assert!(descriptors.iter().any(|range| range[2] != 0.0));
    let packed = &index[2 + descriptors.len()..];
    let mut coordinates: Vec<f32> = (0..64).map(|v| -2.0 + v as f32 * 0.25).collect();
    for i in 1..side {
        let edge = bounds[0] + i as f32 / index[1][1];
        coordinates.extend([edge.next_down(), edge, edge.next_up()]);
    }

    for &y in &coordinates {
        for &x in &coordinates {
            let column = (((x - bounds[0]) * index[1][1]) as usize).min(side - 1);
            let row = (((y - bounds[1]) * index[0][2]) as usize).min(side - 1);
            let [first, length, base, _] = descriptors[row * side + column];
            let actual = (first as usize..(first + length) as usize).fold(base as i32, |n, id| {
                n + crossing(edges[packed[id / 4][id % 4] as usize], [x, y])
            });
            let expected: i32 = edges.iter().map(|edge| crossing(*edge, [x, y])).sum();
            assert_eq!(actual, expected, "sample [{x}, {y}]");
        }
    }
}
