use super::*;

#[test]
fn shared_edges_are_removed_but_coincident_exterior_edges_remain() {
    let a = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
    let b = [[-0.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    assert_eq!(boundary_masks(&[a, b]), [0b011, 0b110]);
    assert_eq!(boundary_masks(&[a, a]), [0b111, 0b111]);
    assert_eq!(boundary_masks(&[a, b, a]), [0b011, 0b110, 0b011]);
}

fn candidates(points: &[[i32; 4]], query: [f32; 4]) -> Vec<Triangle> {
    let mut result = Vec::new();
    let mut node = 0;
    while node < points[0][0] as usize {
        let b = points[1 + node * 2].map(|v| f32::from_bits(v as u32));
        let meta = points[2 + node * 2];
        if b[0] > query[2] || b[1] > query[3] || query[0] > b[2] || query[1] > b[3] {
            node = meta[0] as usize;
            continue;
        }
        for i in 0..meta[2] as usize {
            let ab = points[meta[1] as usize + i * 2].map(|v| f32::from_bits(v as u32));
            let c = points[meta[1] as usize + i * 2 + 1].map(|v| f32::from_bits(v as u32));
            result.push([[ab[0], ab[1]], [ab[2], ab[3]], [c[0], c[1]]]);
        }
        node += 1;
    }
    result
}

#[test]
fn spatial_queries_retain_every_intersecting_triangle_and_bound_storage() {
    let triangles: Vec<_> = (0..2048)
        .map(|i| {
            let x = ((i * 997) % 2048) as f32 * 2.0;
            [[x, -0.25], [x + 1.0, -0.25], [x + 0.5, 1.75]]
        })
        .collect();
    let atlas = pack_fill(&triangles, 4096);
    let SurfaceCurveTexels::Int32(points) = atlas.texels.curves else {
        panic!("exact floats");
    };
    assert!(points.len() <= triangles.len() * 4 + 1);
    assert_eq!(
        candidates(&points, [-1.0, -1.0, 5000.0, 3.0]).len(),
        triangles.len()
    );
    for i in (0..triangles.len()).step_by(13) {
        let t = triangles[i];
        let selected = candidates(&points, [t[0][0] + 0.25, 0.0, t[0][0] + 0.75, 1.0]);
        assert!(selected.contains(&t));
        assert!(
            selected.len() <= 8,
            "unrelated samples visited: {}",
            selected.len()
        );
    }
}

#[test]
fn mirrored_and_degenerate_bounds_do_not_lose_candidates() {
    let triangles = [
        [[0.0, -0.0], [-1e-6, 200.0], [-200.0, 0.5]],
        [[1.0, 1.0]; 3],
    ];
    let atlas = pack_fill(&triangles, 4096);
    let SurfaceCurveTexels::Int32(points) = atlas.texels.curves else {
        panic!("exact floats");
    };
    assert_eq!(candidates(&points, [-201.0, -1.0, 2.0, 201.0]), triangles);
}

#[test]
fn tall_area_triangles_partition_by_coverage_cost_instead_of_height() {
    let triangles: Vec<_> = (0..1200)
        .map(|i| {
            let x = i as f32;
            [
                [x, 800.0],
                [x + 1.0, (x * 1.7).sin() * 200.0 + 250.0],
                [x + 1.0, 800.0],
            ]
        })
        .collect();
    let atlas = pack_fill(&triangles, 4096);
    let SurfaceCurveTexels::Int32(points) = atlas.texels.curves else {
        panic!("exact floats");
    };
    for x in (0..1200).step_by(17) {
        let selected = candidates(&points, [x as f32 + 0.1, 700.0, x as f32 + 0.9, 701.0]);
        assert!(selected.contains(&triangles[x]));
        assert!(
            selected.len() <= 8,
            "distant source intervals overlap in hierarchy: {}",
            selected.len()
        );
    }
}

#[test]
fn direct_leaf_index_preserves_minified_candidates_and_bounds_storage() {
    let triangles: Vec<_> = (0..2048)
        .map(|i| {
            let x = ((i * 997) % 2048) as f32;
            [
                [x, 800.0],
                [x + 1.0, (x * 1.7).sin() * 200.0 + 250.0],
                [x + 1.0, 800.0],
            ]
        })
        .collect();
    for limit in [32, 4096] {
        let atlas = pack_fill(&triangles, limit);
        let bands: Vec<u32> = match atlas.texels.bands {
            SurfaceBandTexels::Uint16(v) => v.into_iter().map(u32::from).collect(),
            SurfaceBandTexels::Uint32(v) => v,
        };
        let SurfaceCurveTexels::Int32(points) = atlas.texels.curves else {
            panic!("exact floats");
        };
        assert!(bands.len() <= triangles.len() * 32 + BAND_HEADER_TEXELS as usize);
        assert_eq!(bands[0], 0, "tall strips index their narrow dimension");
        for level in 0..bands[1] as usize {
            let bins = bands[2 + level * 2] as usize;
            let header = bands[3 + level * 2] as usize;
            let width = 2048.0 / bins as f64;
            for bin in [0, bins / 3, bins - 1] {
                let first = bands[header + bin * 2] as usize;
                let count = bands[header + bin * 2 + 1] as usize;
                let selected = &bands[first..first + count];
                for fraction in [0.0, 0.5, 1.0] {
                    let x = (bin as f64 + fraction) * width;
                    for node in 0..points[0][0] as usize {
                        if points[2 + node * 2][2] == 0 {
                            continue;
                        }
                        let b = points[1 + node * 2].map(|v| f32::from_bits(v as u32));
                        if f64::from(b[0]) <= x + width && f64::from(b[2]) >= x - width {
                            assert!(
                                selected.contains(&(node as u32)),
                                "missing leaf at level {level}, bin {bin}"
                            );
                        }
                    }
                }
            }
        }
    }
}

fn rectangle(triangles: &mut Vec<Triangle>, x0: f32, y0: f32, x1: f32, y1: f32) {
    triangles.push([[x0, y0], [x1, y0], [x1, y1]]);
    triangles.push([[x0, y0], [x1, y1], [x0, y1]]);
}

fn brute_inside(triangles: &[Triangle], p: [f64; 2]) -> bool {
    triangles.iter().any(|t| {
        let [a, b, c] = t.map(|p| p.map(f64::from));
        if (b[0] - a[0]) * (c[1] - a[1]) <= (b[1] - a[1]) * (c[0] - a[0]) {
            return false;
        }
        (0..3).all(|i| {
            let a = t[i].map(f64::from);
            let b = t[(i + 1) % 3].map(f64::from);
            (b[0] - a[0]) * (p[1] - a[1]) >= (b[1] - a[1]) * (p[0] - a[0])
        })
    })
}

#[test]
fn uniform_cells_certify_whole_footprints_with_holes_overlaps_and_subpixel_spikes() {
    let mut geometry = Vec::new();
    // A ring with a hole, disconnected marks, arbitrary overlapping intervals
    // and a narrow spike. Subdivision creates shared internal triangle edges.
    for i in 0..80 {
        let x = i as f32;
        rectangle(&mut geometry, x, 0.0, x + 1.0, 16.0);
        rectangle(&mut geometry, x, 64.0, x + 1.0, 80.0);
    }
    rectangle(&mut geometry, 0.0, 16.0, 16.0, 64.0);
    rectangle(&mut geometry, 64.0, 16.0, 80.0, 64.0);
    rectangle(&mut geometry, 30.0, 30.0, 50.0, 50.0);
    rectangle(&mut geometry, 40.0, 32.0, 55.0, 45.0);
    geometry.push([[79.0, 0.0], [79.001, 0.0], [40.0, 60.0]]);
    geometry.reverse();

    for (scale, offset) in [(1.0, [0.0, 0.0]), (0.001, [-0.1, 0.3]), (8.0, [1e6, -1e6])] {
        let triangles: Vec<_> = geometry
            .iter()
            .map(|t| t.map(|p| [offset[0] + p[0] * scale, offset[1] + p[1] * scale]))
            .collect();
        let atlas = pack_fill(&triangles, 4096);
        let SurfaceCurveTexels::Int32(points) = atlas.texels.curves else {
            panic!("exact floats");
        };
        let bands: Vec<u32> = match atlas.texels.bands {
            SurfaceBandTexels::Uint16(v) => v.into_iter().map(u32::from).collect(),
            SurfaceBandTexels::Uint32(v) => v,
        };
        let size = points[0][2] as usize;
        let start = points[0][3] as usize;
        let bounds = float_data(points[1]);
        let mut certified = [0; 3];
        assert!(size >= 8);
        for y in 0..size {
            for x in 0..size {
                let index = y * size + x;
                let value = (bands[start + index / 8] >> (index % 8 * 2)) & 3;
                certified[value as usize] += 1;
                if value == 0 {
                    continue;
                }
                // Entire base cell plus the one-cell footprint guard, including
                // all corners. This oracle scans original geometry, not the BVH.
                for dy in [-1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0] {
                    for dx in [-1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0] {
                        let p = [
                            bounds[0] + (x as f64 + dx) * (bounds[2] - bounds[0]) / size as f64,
                            bounds[1] + (y as f64 + dy) * (bounds[3] - bounds[1]) / size as f64,
                        ];
                        assert_eq!(
                            brute_inside(&triangles, p),
                            value == 1,
                            "wrong certification at {x},{y}, scale {scale}, point {p:?}"
                        );
                    }
                }
            }
        }
        assert!(certified[1] > 100, "interiors must skip triangle work");
        assert!(certified[2] > 20, "holes must skip triangle work");
    }
}

#[test]
fn uniform_grid_stays_within_budget_and_skips_degenerate_extents() {
    for triangles in [
        vec![[[1.0, 1.0]; 3]],
        vec![[[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]]],
    ] {
        let atlas = pack_fill(&triangles, 4096);
        let SurfaceCurveTexels::Int32(points) = atlas.texels.curves else {
            panic!("exact floats");
        };
        assert_eq!(points[0][2], 0);
    }
    let mut triangles = Vec::new();
    for i in 0..100 {
        rectangle(&mut triangles, i as f32, 0.0, i as f32 + 1.0, 100.0);
    }
    for limit in [16, 32, 4096] {
        let atlas = pack_fill(&triangles, limit);
        let count = match atlas.texels.bands {
            SurfaceBandTexels::Uint16(v) => v.len(),
            SurfaceBandTexels::Uint32(v) => v.len(),
        };
        assert!(count <= (limit * limit) as usize);
        assert!(count <= triangles.len() * 32 + BAND_HEADER_TEXELS as usize);
    }
}
