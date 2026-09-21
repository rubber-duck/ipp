//! Pure recipe output is accepted through ordinary owned asset publication.
#![cfg(feature = "builtin-assets")]

mod support;
use support::WorldTestDriver;

use ipp_core::services::asset_management::builtin;
use ipp_core::{ErrorReason, MeshKey, MeshUpload, TextureKey, TextureUpload};

#[test]
fn cube_extents_winding_white_rgb_and_face_uvs_survive_normal_publication() {
    let bytes = builtin::mesh("ipp://mesh/cube?width=2&height=4&length=6").unwrap();
    assert_eq!(bytes.len(), 1180);
    assert_eq!(&bytes[..8], b"IPPM\x03\0\0\0");
    assert_eq!(
        bytes,
        builtin::mesh("ipp://mesh/cube?width=2&height=4&length=6").unwrap()
    );
    let key = MeshKey {
        asset: 9,

        variant: 0,
    };
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    world
        .enqueue_mesh(MeshUpload {
            id: 1,
            key,
            bytes,
        })
        .unwrap();
    assert!(world.mesh(key).is_none());
    let stats = world.update_for_test(0.0).unwrap().assets[0]
        .result
        .as_ref()
        .copied()
        .unwrap();
    assert_eq!(
        (stats.source_bytes, stats.resident_bytes),
        (1180, 1128 + support::unskinned_mesh_metadata_bytes(36))
    );
    let mesh = world.mesh(key).unwrap();
    assert!(
        mesh.colors()
            .unwrap()
            .iter()
            .all(|color| *color == [1.0; 3])
    );
    assert!(mesh.texture_weights().is_none());
    for (face, normal) in [
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            &mesh.normals().unwrap()[face * 4..face * 4 + 4],
            &[normal; 4]
        );
    }
    let vertices = mesh.positions();
    for vertex in vertices {
        for axis in 0..3 {
            assert_eq!(vertex[axis].abs(), [1.0, 2.0, 3.0][axis]);
        }
    }
    for uv in mesh.uvs().unwrap().as_chunks::<4>().0 {
        assert_eq!(uv, &[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
    }
    for triangle in mesh.indices().as_chunks::<3>().0 {
        let [a, b, c] = [
            vertices[triangle[0] as usize],
            vertices[triangle[1] as usize],
            vertices[triangle[2] as usize],
        ];
        let u: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
        let v: [f32; 3] = std::array::from_fn(|i| c[i] - a[i]);
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let outward: f32 = (0..3).map(|i| cross[i] * (a[i] + b[i] + c[i])).sum();
        assert!(outward > 0.0, "triangle must face away from origin");
    }
}

#[test]
fn checker_rectangular_cells_cycle_exact_srgb_rgb_and_support_large_odd_dimensions() {
    let bytes =
        builtin::texture("ipp://texture/checkerboard?width=6&height=4&cellsX=3&cellsY=2").unwrap();
    assert_eq!(&bytes[..8], b"IPPT\x03\0\0\0");
    assert_eq!(
        bytes,
        builtin::texture("ipp://texture/checkerboard?width=6&height=4&cellsX=3&cellsY=2").unwrap()
    );
    let key = TextureKey {
        asset: 9,

        variant: 0,
    };
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    world
        .enqueue_texture(TextureUpload {
            id: 1,
            key,
            bytes,
        })
        .unwrap();
    assert!(world.texture(key).is_none());
    let stats = world.update_for_test(0.0).unwrap().assets[0]
        .result
        .as_ref()
        .copied()
        .unwrap();
    assert_eq!((stats.source_bytes, stats.resident_bytes), (112, 96));
    let pixels = world.texture(key).unwrap().pixels();
    let palette = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [0, 0, 0, 255],
    ];
    for (y, row) in pixels.as_chunks::<24>().0.iter().enumerate() {
        for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
            let cell = x / 2 + y / 2;
            assert_eq!(pixel, &palette[cell % palette.len()]);
        }
    }
    assert_eq!(
        &builtin::texture("ipp://texture/checkerboard?width=1&height=1&cellsX=1&cellsY=1").unwrap()
            [16..],
        &[255, 0, 0, 255]
    );
    let large =
        builtin::texture("ipp://texture/checkerboard?width=1024&height=1024&cellsX=16&cellsY=16")
            .unwrap();
    assert_eq!(large.len(), 16 + 1024 * 1024 * 4);
    assert_eq!(large.capacity(), large.len());
    let odd = builtin::texture("ipp://texture/checkerboard?width=1025&height=3&cellsX=5&cellsY=3")
        .unwrap();
    assert_eq!(odd.len(), 16 + 1025 * 3 * 4);
    assert_eq!(odd.capacity(), odd.len());

    let beyond_old_budget =
        builtin::texture("ipp://texture/checkerboard?width=3072&height=2048&cellsX=16&cellsY=16")
            .unwrap();
    assert_eq!(beyond_old_budget.len(), 16 + 3072 * 2048 * 4);
}

#[test]
fn query_order_and_form_decoding_preserve_recipe_output() {
    let cube = builtin::mesh("ipp://mesh/cube?width=2&height=4&length=6").unwrap();
    assert_eq!(
        builtin::mesh("ipp://mesh/cube?length=6&%77idth=%32&height=4").unwrap(),
        cube
    );
    assert_eq!(
        builtin::mesh("ipp://mesh/sphere?radius=%2B1e0").unwrap(),
        builtin::mesh("ipp://mesh/sphere?radius=1").unwrap()
    );
    assert_eq!(
        builtin::mesh("ipp://mesh/pill?height=2.8&radius=.65").unwrap(),
        builtin::mesh("ipp://mesh/pill?radius=.65&height=2.8").unwrap()
    );
    assert_eq!(
        builtin::texture("ipp://texture/checkerboard?cells%59=2&height=4&width=6&cellsX=3")
            .unwrap(),
        builtin::texture("ipp://texture/checkerboard?width=6&height=4&cellsX=3&cellsY=2").unwrap()
    );
}

#[test]
fn uv_grid_marks_both_axes_and_triangle_orientation_in_rgb8() {
    let uri = "ipp://texture/uv-grid?width=128&height=192&cellsX=2&cellsY=3";
    let bytes = builtin::texture(uri).unwrap();
    assert_eq!(bytes.len(), 16 + 128 * 192 * 4);
    assert_eq!(&bytes[..8], b"IPPT\x03\0\0\0");
    assert_eq!(
        bytes,
        builtin::texture("ipp://texture/uv-grid?cellsY=3&cellsX=2&height=192&%77idth=128").unwrap()
    );

    let pixel = |x: usize, y: usize| &bytes[16 + (y * 128 + x) * 4..][..3];
    assert_eq!(pixel(8, 8), [48, 48, 96]);
    assert_eq!(pixel(72, 8), [224, 48, 192]);
    assert_eq!(pixel(8, 72), [48, 136, 192]);
    assert_eq!(pixel(72, 136), [224, 224, 192]);
    assert_eq!(pixel(0, 32), [16; 3]); // Cell border.
    assert_eq!(pixel(24, 40), [240; 3]); // Light left triangle half.
    assert_eq!(pixel(40, 40), [16; 3]); // Dark right triangle half.
    assert_eq!(pixel(24, 20), [48, 48, 96]); // Outside the narrowing apex.
    assert_eq!(pixel(24, 56), [48, 48, 96]); // Below the flat base.

    let key = TextureKey {
        asset: 11,
        variant: 0,
    };
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    world
        .enqueue_texture(TextureUpload {
            id: 1,
            key,
            bytes,
        })
        .unwrap();
    assert!(world.texture(key).is_none());
    assert!(world.update_for_test(0.0).unwrap().assets[0].result.is_ok());
    let texture = world.texture(key).unwrap();
    assert_eq!((texture.width(), texture.height()), (128, 192));
}

#[test]
fn uv_grid_handles_small_rectangular_cells() {
    let uri = "ipp://texture/uv-grid?width=9&height=10&cellsX=3&cellsY=2";
    let size = 16 + 9 * 10 * 4;
    assert_eq!(builtin::texture(uri).unwrap().len(), size);
    assert_eq!(
        builtin::texture("ipp://texture/uv-grid?width=1&height=1&cellsX=1&cellsY=1")
            .unwrap()
            .len(),
        20
    );
    for invalid in [
        "ipp://texture/uv-grid?width=9&height=10&cellsX=2&cellsY=2",
        "ipp://texture/uv-grid?width=9&height=10&cellsX=0&cellsY=2",
        "ipp://texture/uv-grid?width=9&height=10&cellsX=3&cellsY=2&cellsX=3",
        "ipp://texture/uv-grid?width=9&height=10&cellsX=3&cellsY=2&font=none",
        "ipp://texture/uv-grid?width=9&height=10&cellsX=3",
    ] {
        assert_eq!(builtin::texture(invalid), Err(ErrorReason::InvalidAsset));
    }
}

#[test]
fn query_structure_names_and_decoding_are_strict() {
    for uri in [
        "ipp://mesh/cube?width=2&height=4&length=6&width=2",
        "ipp://mesh/cube?width=2&height=4&length=6&%77idth=2",
        "ipp://mesh/cube?width=2&height=4&length=6&unknown=1",
        "ipp://mesh/cube?width=2&height=4",
        "ipp://mesh/cube?width=&height=4&length=6",
        "ipp://mesh/cube?width=2&&height=4&length=6",
        "ipp://mesh/cube?&width=2&height=4&length=6",
        "ipp://mesh/cube?width=2&height=4&length=6&",
        "ipp://mesh/cube?width=2&height&length=6",
        "ipp://mesh/cube?width=2&height=4&length=6#fragment",
        "ipp://mesh/cube/?width=2&height=4&length=6",
        "ipp://mesh/cube?%GG=2&height=4&length=6",
        "ipp://mesh/cube?width=%&height=4&length=6",
        "ipp://mesh/cube?width=%0G&height=4&length=6",
        "ipp://mesh/cube?width=%FF&height=4&length=6",
        "ipp://mesh/cube?width=2+height=4&length=6",
        "ipp://mesh/cube?ｗidth=2&height=4&length=6",
        "ipp://mesh/cube(2,4,6)",
        "ipp://mesh/cube?width=2&height=4&length=6/",
        "ipp://mesh/plane(2,1,.05)",
    ] {
        assert_eq!(builtin::mesh(uri), Err(ErrorReason::InvalidAsset), "{uri}");
    }
    for uri in [
        "ipp://texture/checkerboard?width=1&height=1&cellsX=1&cellsY=1&cells%58=1",
        "ipp://texture/checkerboard?width=1&height=1&cellsX=1&cellsY=",
        "ipp://texture/checkerboard?width=1&height=1&cellsX=1&&cellsY=1",
        "ipp://texture/checkerboard?width=1&height=1&cellsX=1&cellsY=1#fragment",
        "ipp://texture/checkerboard/?width=1&height=1&cellsX=1&cellsY=1",
        "ipp://texture/checkerboard(1,1,1,1)",
    ] {
        assert_eq!(
            builtin::texture(uri),
            Err(ErrorReason::InvalidAsset),
            "{uri}"
        );
    }
}

#[test]
fn raw_and_encoded_uri_lengths_are_not_capped() {
    let prefix = "ipp://mesh/sphere?radius=1.";
    let uri = format!("{prefix}{}", "0".repeat(256 - prefix.len()));
    assert_eq!(uri.len(), 256);
    assert_eq!(
        builtin::mesh(&uri).unwrap(),
        builtin::mesh("ipp://mesh/sphere?radius=1").unwrap()
    );

    let oversized = format!("{uri}0");
    assert_eq!(oversized.len(), 257);
    assert_eq!(
        builtin::mesh(&oversized).unwrap(),
        builtin::mesh(&uri).unwrap()
    );

    let encoded = format!("{prefix}{}", "%30".repeat(80));
    assert!(encoded.len() > 256);
    assert_eq!(
        builtin::mesh(&encoded).unwrap(),
        builtin::mesh(&uri).unwrap()
    );
}

#[test]
fn unsupported_malformed_overflow_and_unbounded_recipes_reject_without_fallback() {
    for uri in [
        "",
        "ipp://mesh/cube(1,1,1)",
        "ipp://mesh/sphere(1)",
        "ipp://mesh/pill(1,2)",
        "ipp://mesh/capsule(1,2)",
        "ipp://mesh/capsule?radius=1&height=2",
        "ipp://mesh/cube",
        "ipp://mesh/cube?width=1&height=1",
        "ipp://mesh/cube?width=1&height=1&length=1&extra=1",
        "ipp://mesh/cube?width=0&height=1&length=1",
        "ipp://mesh/cube?width=-1&height=1&length=1",
        "ipp://mesh/cube?width=NaN&height=1&length=1",
        "ipp://mesh/cube?width=inf&height=1&length=1",
        "ipp://mesh/cube?width=1e99&height=1&length=1",
        "ipp://mesh/cube?width=1e-99&height=1&length=1",
        "ipp://mesh/cube?width=1&height=1&length=1#trailing",
        "ipp://mesh/cube/?width=1&height=1&length=1",
        "ipp://mesh/cube?width=1+1&height=1&length=1",
        "https://mesh/cube(1,1,1)",
    ] {
        assert_eq!(builtin::mesh(uri), Err(ErrorReason::InvalidAsset), "{uri}");
    }
    for uri in [
        "",
        "ipp://texture/checkerboard(1,1,1,1)",
        "ipp://texture/unknown(1,1,1,1)",
        "ipp://texture/unknown?width=1&height=1&cellsX=1&cellsY=1",
        "ipp://texture/checkerboard?width=1&height=1&cellsX=1",
        "ipp://texture/checkerboard?width=1&height=1&cellsX=1&cellsY=1&extra=1",
        "ipp://texture/checkerboard?width=0&height=1&cellsX=1&cellsY=1",
        "ipp://texture/checkerboard?width=1&height=0&cellsX=1&cellsY=1",
        "ipp://texture/checkerboard?width=1&height=1&cellsX=0&cellsY=1",
        "ipp://texture/checkerboard?width=1&height=1&cellsX=1&cellsY=0",
        "ipp://texture/checkerboard?width=3&height=4&cellsX=2&cellsY=2",
        "ipp://texture/checkerboard?width=4&height=3&cellsX=2&cellsY=2",
        "ipp://texture/checkerboard?width=2&height=2&cellsX=3&cellsY=1",
        "ipp://texture/checkerboard?width=4294967295&height=4294967295&cellsX=1&cellsY=1",
        "ipp://texture/checkerboard?width=4294967296&height=1&cellsX=1&cellsY=1",
        "ipp://texture/checkerboard?width=1&height=1&cellsX=1&cellsY=-1",
        "ipp://texture/checkerboard?width=1.0&height=1&cellsX=1&cellsY=1",
        "ipp://texture/checkerboard?width=%2B1&height=1&cellsX=1&cellsY=1",
        "ipp://texture/checkerboard?width=1&height=1&cellsX=1&cellsY=1\n",
        "ipp://texture/checkerboard?width=1e1&height=1&cellsX=1&cellsY=1",
    ] {
        assert_eq!(
            builtin::texture(uri),
            Err(ErrorReason::InvalidAsset),
            "{uri}"
        );
    }
    let mesh = format!(
        "ipp://mesh/cube?width={}1&height=1&length=1",
        "0".repeat(256)
    );
    let texture = format!(
        "ipp://texture/checkerboard?width={}1&height=1&cellsX=1&cellsY=1",
        "0".repeat(256)
    );
    assert!(builtin::mesh(&mesh).is_ok());
    assert!(builtin::texture(&texture).is_ok());
}

fn publish<'a>(
    host: &'a mut ipp_core::HostRuntime,
    uri: &str,
    counts: (usize, usize),
) -> (ipp_core::WorldContext<'a>, MeshKey) {
    publish_colored(host, uri, counts, 0)
}

fn publish_colored<'a>(
    host: &'a mut ipp_core::HostRuntime,
    uri: &str,
    counts: (usize, usize),
    grey_vertices: usize,
) -> (ipp_core::WorldContext<'a>, MeshKey) {
    let bytes = builtin::mesh(uri).unwrap_or_else(|reason| panic!("{uri}: {reason:?}"));
    assert_eq!(bytes, builtin::mesh(uri).unwrap(), "repeatable recipe");
    let weighted = uri.starts_with("ipp://mesh/plane?");
    let vertex_bytes = counts.0
        * if weighted {
            45
        } else {
            44
        };
    let header_bytes = if weighted {
        60
    } else {
        52
    };
    assert_eq!(bytes.len(), header_bytes + vertex_bytes + counts.1 * 2);
    assert!(bytes.len() <= 1 << 20);
    assert!(counts.0 <= 65536);
    let key = MeshKey {
        asset: 1,

        variant: 0,
    };
    let world_id = host.create_world(ipp_core::WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    world
        .enqueue_mesh(MeshUpload {
            id: 7,
            key,
            bytes,
        })
        .unwrap();
    assert!(world.mesh(key).is_none());
    let report = world.await_upload_for_test();
    assert_eq!(report.assets[0].id, 7);
    let stats = report.assets[0].result.as_ref().copied().unwrap();
    let mesh = world.mesh(key).unwrap();
    assert_eq!(
        stats.resident_bytes,
        vertex_bytes + counts.1 * 2 + support::unskinned_mesh_metadata_bytes(counts.1)
    );
    assert_eq!(mesh.vertex_bytes(), vertex_bytes);
    assert_eq!(mesh.vertex_count(), counts.0);
    if weighted {
        let weights = mesh.texture_weights().unwrap();
        assert_eq!(&weights[..4], &[255; 4]);
        assert!(weights[4..].iter().all(|&weight| weight == 0));
    } else {
        assert!(mesh.texture_weights().is_none());
    }
    let normals = mesh
        .normals()
        .expect("every public recipe includes normals");
    assert_eq!(normals.len(), mesh.vertex_count());
    for normal in normals {
        assert!(normal.iter().all(|value| value.is_finite()));
        assert!((normal.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>() - 1.0).abs() < 1e-6);
    }
    for (index, vertex) in mesh.positions().iter().enumerate() {
        assert!(vertex.iter().all(|v| v.is_finite()));
        assert_eq!(
            &mesh.colors().unwrap()[index],
            &[if index < grey_vertices {
                0.25
            } else {
                1.0
            }; 3]
        );
    }
    for uv in mesh.uvs().unwrap() {
        assert!(uv.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
    }
    for triangle in mesh.indices().as_chunks::<3>().0 {
        assert!(triangle.iter().all(|&i| (i as usize) < counts.0));
        let (normal, _) = triangle_geometry(mesh, triangle);
        assert!(normal.iter().any(|&v| v != 0.0), "nondegenerate {uri}");
        for &index in triangle {
            let smooth = normals[index as usize];
            assert!(
                (0..3)
                    .map(|i| normal[i] * f64::from(smooth[i]))
                    .sum::<f64>()
                    > 0.0,
                "outward vertex normal {uri}"
            );
        }
    }
    (world, key)
}

#[test]
fn planes_publish_a_square_and_closed_positive_normal_arrow() {
    for outline in [false, true] {
        let uri = if outline {
            "ipp://mesh/plane-outline?size=2&normalLength=1.25&stroke=0.05"
        } else {
            "ipp://mesh/plane?size=2&normalLength=1.25&stroke=0.05"
        };
        let counts = if outline {
            (217, 528)
        } else {
            (69, 150)
        };
        let mut publication_host = ipp_core::HostRuntime::new();
        let (world, key) = publish_colored(
            &mut publication_host,
            uri,
            counts,
            if outline {
                0
            } else {
                4
            },
        );
        let mesh = world.mesh(key).unwrap();
        let (arrow_vertex, arrow_index) = if outline {
            (152, 384)
        } else {
            (4, 6)
        };
        let arrow = &mesh.positions()[arrow_vertex..];
        assert_eq!(
            arrow.iter().map(|v| v[2]).fold(f32::INFINITY, f32::min),
            0.1
        );
        assert_eq!(
            arrow.iter().map(|v| v[2]).fold(f32::NEG_INFINITY, f32::max),
            1.35
        );
        assert!(arrow.iter().any(|v| v[..3] == [0.0, 0.0, 1.35]));
        assert!(arrow.iter().all(|v| v[0].hypot(v[1]) <= 0.100_001));
        assert_closed(mesh, &mesh.indices()[arrow_index..]);

        if outline {
            // Each capped edge is closed; intersecting tubes are not a boolean union.
            for edge in mesh.indices()[..arrow_index].as_chunks::<96>().0 {
                assert_closed(mesh, edge);
            }
            assert!(
                mesh.positions()[..arrow_vertex].iter().all(|v| {
                    (v[0].abs() - 1.0).abs() <= 0.026 || (v[1].abs() - 1.0).abs() <= 0.026
                }),
                "perimeter contains no diagonal or fill"
            );
        } else {
            assert_eq!(
                &mesh.uvs().unwrap()[..4],
                &[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]
            );
            for triangle in mesh.indices()[..6].as_chunks::<3>().0 {
                let (normal, center) = triangle_geometry(mesh, triangle);
                assert_eq!(normal[..2], [0.0, 0.0]);
                assert!(normal[2] > 0.0);
                assert_eq!(center[2], 0.0);
            }
        }
    }

    // Supported dimensional limits still produce ordinary bounded geometry.
    publish_colored(
        &mut ipp_core::HostRuntime::new(),
        "ipp://mesh/plane?size=4&normalLength=2&stroke=0.25",
        (69, 150),
        4,
    );
    publish(
        &mut ipp_core::HostRuntime::new(),
        "ipp://mesh/plane-outline?size=4&normalLength=2&stroke=0.25",
        (217, 528),
    );
}

#[test]
fn plane_pointer_offset_preserves_length_and_square_geometry() {
    for (recipe, arrow_vertex) in [("plane", 4), ("plane-outline", 152)] {
        let source = format!("ipp://mesh/{recipe}?size=2&normalLength=1.25&stroke=0.05");
        assert_eq!(
            builtin::mesh(&source).unwrap(),
            builtin::mesh(&format!("{source}&normalOffset=0.1")).unwrap()
        );

        let decode = |length, offset| {
            let bytes = builtin::mesh(&format!(
                "ipp://mesh/{recipe}?size=2&normalLength={length}&stroke=0.05&normalOffset={offset}"
            ))
            .unwrap();
            ipp_core::MeshAsset::decode(&bytes).unwrap().0
        };
        let attached = decode(1.25, 0.0);
        for length in [0.75, 1.25, 2.0] {
            for offset in [0.0, 0.25, 1.0] {
                let mesh = decode(length, offset);
                assert_eq!(
                    &mesh.positions()[..arrow_vertex],
                    &attached.positions()[..arrow_vertex],
                    "offset and length preserve the square"
                );
                let arrow = &mesh.positions()[arrow_vertex..];
                assert_eq!(
                    arrow.iter().map(|p| p[2]).fold(f32::INFINITY, f32::min),
                    offset
                );
                assert_eq!(
                    arrow.iter().map(|p| p[2]).fold(f32::NEG_INFINITY, f32::max),
                    offset + length
                );
                if length == 1.25 {
                    for (actual, original) in
                        arrow.iter().zip(&attached.positions()[arrow_vertex..])
                    {
                        assert_eq!(actual[..2], original[..2]);
                        assert!((actual[2] - original[2] - offset).abs() < 1e-6);
                    }
                    assert_eq!(mesh.indices(), attached.indices());
                    assert_eq!(mesh.uvs(), attached.uvs());
                    assert_eq!(mesh.normals(), attached.normals());
                }
            }
        }
    }
}

#[test]
fn invalid_plane_dimensions_and_unrepresentable_arrowheads_reject() {
    for recipe in ["plane", "plane-outline"] {
        for query in [
            "",
            "size=2&normalLength=1",
            "size=2&normalLength=1&stroke=.05&extra=4",
            "size=0&normalLength=1&stroke=.05",
            "size=2&normalLength=-1&stroke=.05",
            "size=2&normalLength=1&stroke=0",
            "size=NaN&normalLength=1&stroke=.05",
            "size=2&normalLength=inf&stroke=.05",
            "size=2&normalLength=1&stroke=.126",
            "size=1&normalLength=2&stroke=.126",
            "size=1e99&normalLength=1&stroke=.05",
            "size=2&normalLength=1&stroke=1e-99",
            "size=2&normalLength=1&stroke=1e-40",
            "size=2&normalLength=1&stroke=.05&normalOffset=",
            "size=2&normalLength=1&stroke=.05&normalOffset=0&normalOffset=1",
            "size=2&normalLength=1&stroke=.05&normalOffset=-.1",
            "size=2&normalLength=1&stroke=.05&normalOffset=NaN",
            "size=2&normalLength=1&stroke=.05&normalOffset=inf",
            "size=2&normalLength=1&stroke=.05&normalOffset=1e99",
            "size=2&normalLength=1&stroke=.05&normalOffset=1e-99",
            "size=2&normalLength=1&stroke=.05&normalOffset=1e20",
            "size=1e38&normalLength=1e38&stroke=1e36&normalOffset=3e38",
        ] {
            let uri = format!("ipp://mesh/{recipe}?{query}");
            assert_eq!(builtin::mesh(&uri), Err(ErrorReason::InvalidAsset), "{uri}");
        }
        assert_eq!(
            builtin::mesh(&format!(
                "ipp://mesh/{recipe}?size=2&normalLength=1&stroke=.05#junk"
            )),
            Err(ErrorReason::InvalidAsset)
        );
    }
}

#[test]
fn standalone_arrow_matches_both_plane_normal_meshes() {
    for (length, stroke) in [(1.25, 0.05), (2.0, 0.25), (1e-20, 1e-22), (1e20, 1e18)] {
        let uri = format!("ipp://mesh/arrow?length={length}&stroke={stroke}");
        let mut publication_host = ipp_core::HostRuntime::new();
        let (world, key) = publish(&mut publication_host, &uri, (65, 144));
        let arrow = world.mesh(key).unwrap();
        assert_closed(arrow, arrow.indices());
        assert_eq!(
            arrow
                .positions()
                .iter()
                .map(|v| v[2])
                .fold(f32::INFINITY, f32::min),
            0.0
        );
        assert_eq!(
            arrow
                .positions()
                .iter()
                .map(|v| v[2])
                .fold(f32::NEG_INFINITY, f32::max),
            length
        );
        for (recipe, base, index) in [("plane", 4, 6), ("plane-outline", 152, 384)] {
            let plane = ipp_core::MeshAsset::decode(
                &builtin::mesh(&format!(
                    "ipp://mesh/{recipe}?size={length}&normalLength={length}&stroke={stroke}&normalOffset=0"
                ))
                .unwrap(),
            )
            .unwrap()
            .0;
            assert_eq!(arrow.positions(), &plane.positions()[base..]);
            assert_eq!(arrow.normals().unwrap(), &plane.normals().unwrap()[base..]);
            assert_eq!(arrow.colors().unwrap(), &plane.colors().unwrap()[base..]);
            assert_eq!(arrow.uvs().unwrap(), &plane.uvs().unwrap()[base..]);
            assert_eq!(
                arrow.indices(),
                plane.indices()[index..]
                    .iter()
                    .map(|i| i - base as u16)
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn axis_arrows_preserve_winding_normals_and_independent_linear_colors() {
    let mut publication_host = ipp_core::HostRuntime::new();
    let (world, key) = publish(
        &mut publication_host,
        "ipp://mesh/arrow?length=1.25&stroke=.05",
        (65, 144),
    );
    let arrow = world.mesh(key).unwrap();
    let source = "ipp://mesh/axis?length=1.25&stroke=.05";
    for (suffix, colors) in [
        ("", [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
        (
            "&xColor=.25,.5,1&yColor=0,0,0&zColor=1,1,1",
            [[0.25, 0.5, 1.0], [0.0; 3], [1.0; 3]],
        ),
        (
            "&yColor=1,0,1",
            [[1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
        ),
    ] {
        let bytes = builtin::mesh(&format!("{source}{suffix}")).unwrap();
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        world
            .enqueue_mesh(MeshUpload {
                id: 1,
                key,
                bytes,
            })
            .unwrap();
        assert!(world.update_for_test(0.0).unwrap().assets[0].result.is_ok());
        let mesh = world.mesh(key).unwrap();
        assert_eq!(mesh.vertex_count(), 195);
        assert_eq!(mesh.indices().len(), 432);
        assert!(mesh.texture_weights().is_none());
        for (axis, color) in colors.iter().enumerate() {
            let base = axis * arrow.vertex_count();
            let indices = &mesh.indices()[axis * 144..(axis + 1) * 144];
            assert_closed(mesh, indices);
            for (i, position) in arrow.positions().iter().enumerate() {
                let rotate = |v: [f32; 3]| match axis {
                    0 => [v[2], v[0], v[1]],
                    1 => [v[1], v[2], v[0]],
                    _ => v,
                };
                assert_eq!(mesh.positions()[base + i], rotate(*position));
                assert_eq!(
                    mesh.normals().unwrap()[base + i],
                    rotate(arrow.normals().unwrap()[i])
                );
                assert_eq!(mesh.colors().unwrap()[base + i], *color);
                assert_eq!(mesh.uvs().unwrap()[base + i], arrow.uvs().unwrap()[i]);
            }
            for triangle in indices.as_chunks::<3>().0 {
                let (normal, _) = triangle_geometry(mesh, triangle);
                for &index in triangle {
                    assert!(
                        (0..3)
                            .map(|i| normal[i]
                                * f64::from(mesh.normals().unwrap()[index as usize][i]))
                            .sum::<f64>()
                            > 0.0
                    );
                }
            }
        }
    }
    assert_eq!(
        builtin::mesh(&format!("{source}&xColor=.25,.5,1")).unwrap(),
        builtin::mesh("ipp://mesh/axis?%78Color=.25%2C.5%2C1&stroke=.05&length=1.25").unwrap()
    );
    assert_eq!(
        builtin::mesh(source).unwrap(),
        builtin::mesh(&format!("{source}&zColor=0,0,1&xColor=1,0,0&yColor=0,1,0")).unwrap()
    );
}

#[test]
fn arrow_and_axis_reject_invalid_dimensions_and_color_recipes() {
    for recipe in ["arrow", "axis"] {
        for query in [
            "",
            "length=1",
            "stroke=.05",
            "length=1&stroke=0",
            "length=0&stroke=.05",
            "length=-1&stroke=.05",
            "length=NaN&stroke=.05",
            "length=inf&stroke=.05",
            "length=1&stroke=NaN",
            "length=1&stroke=1e-40",
            "length=1e-45&stroke=1e-45",
            "length=1&stroke=.126",
            "length=1&stroke=.05&extra=1",
            "length=1&stroke=.05&length=2",
            "length=1&stroke=.05#fragment",
            "length=1e99&stroke=.05",
        ] {
            let uri = format!("ipp://mesh/{recipe}?{query}");
            assert_eq!(builtin::mesh(&uri), Err(ErrorReason::InvalidAsset), "{uri}");
        }
    }
    for parameter in ["xColor", "yColor", "zColor"] {
        for value in [
            "",
            "red",
            "1,0",
            "1,0,0,1",
            "1,,0",
            "1,0,",
            "-0.1,0,0",
            "1.00000001,0,0",
            "NaN,0,0",
            "0,inf,0",
            "0,0,1e99",
            "%GG,0,0",
        ] {
            let uri = format!("ipp://mesh/axis?length=1&stroke=.05&{parameter}={value}");
            assert_eq!(builtin::mesh(&uri), Err(ErrorReason::InvalidAsset), "{uri}");
        }
        let uri =
            format!("ipp://mesh/axis?length=1&stroke=.05&{parameter}=1,0,0&{parameter}=0,1,0");
        assert_eq!(builtin::mesh(&uri), Err(ErrorReason::InvalidAsset));
    }
    assert_eq!(
        builtin::mesh("ipp://mesh/arrow?length=1&stroke=.05&xColor=1,0,0"),
        Err(ErrorReason::InvalidAsset)
    );
}

type Point = [f64; 3];

fn triangle_geometry(mesh: &ipp_core::MeshAsset, triangle: &[u16]) -> (Point, Point) {
    let [a, b, c] = std::array::from_fn(|i| {
        let v = mesh.positions()[triangle[i] as usize];
        [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
    });
    let u: Point = std::array::from_fn(|i| b[i] - a[i]);
    let v: Point = std::array::from_fn(|i| c[i] - a[i]);
    (
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ],
        std::array::from_fn(|i| (a[i] + b[i] + c[i]) / 3.0),
    )
}

fn assert_bounds(mesh: &ipp_core::MeshAsset, half: Point) {
    for (axis, expected) in half.into_iter().enumerate() {
        let low = mesh
            .positions()
            .iter()
            .map(|v| f64::from(v[axis]))
            .fold(f64::INFINITY, f64::min);
        let high = mesh
            .positions()
            .iter()
            .map(|v| f64::from(v[axis]))
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (low + expected).abs() <= expected * 1e-6,
            "axis {axis}: {low} != -{expected}"
        );
        assert!(
            (high - expected).abs() <= expected * 1e-6,
            "axis {axis}: {high} != {expected}"
        );
    }
}

// Weld only exact positions, including the separate UV and pole vertices. Every
// geometric edge must have two oppositely directed uses: no cracks/open ends.
fn assert_closed(mesh: &ipp_core::MeshAsset, indices: &[u16]) {
    use std::collections::BTreeMap;

    let position = |index: u16| {
        let vertex = mesh.positions()[index as usize];
        std::array::from_fn::<_, 3, _>(|i| {
            if vertex[i] == 0.0 {
                0
            } else {
                vertex[i].to_bits()
            }
        })
    };
    let mut edges = BTreeMap::<_, (u32, i32)>::new();
    for t in indices.as_chunks::<3>().0 {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (a, b) = (position(a), position(b));
            assert_ne!(a, b);
            let (key, direction) = if a < b {
                ((a, b), 1)
            } else {
                ((b, a), -1)
            };
            let entry = edges.entry(key).or_default();
            entry.0 += 1;
            entry.1 += direction;
        }
    }
    assert!(
        edges.values().all(|&uses| uses == (2, 0)),
        "closed consistently wound surface"
    );
}

#[test]
fn cones_have_closed_surfaces_slope_normals_and_separate_cap_uvs() {
    for (radius, height) in [
        (1.0, 2.0),
        (0.25, 3.0),
        (2.0, 0.5),
        (1e-30, 2e-30),
        (1e30, 2e30),
    ] {
        let uri = format!("ipp://mesh/cone?radius={radius}&height={height}");
        let mut publication_host = ipp_core::HostRuntime::new();
        let (world, key) = publish(&mut publication_host, &uri, (99, 192));
        let mesh = world.mesh(key).unwrap();
        assert_bounds(mesh, [radius, height * 0.5, radius]);
        assert_closed(mesh, mesh.indices());

        let normals = mesh.normals().unwrap();
        let uvs = mesh.uvs().unwrap();
        for (position, normal) in mesh.positions()[..33].iter().zip(normals) {
            let expected = [
                f64::from(position[0]) / radius * height,
                radius,
                f64::from(position[2]) / radius * height,
            ];
            for (actual, expected) in normal.iter().zip(expected) {
                assert!((f64::from(*actual) - expected / radius.hypot(height)).abs() < 1e-6);
            }
        }
        assert_eq!(mesh.positions()[0], mesh.positions()[32]);
        assert_eq!(normals[0], normals[32]);
        assert_eq!(uvs[0], [0.0, 1.0]);
        assert_eq!(uvs[32], [1.0, 1.0]);
        assert!(
            normals[65..]
                .iter()
                .all(|normal| *normal == [0.0, -1.0, 0.0])
        );
        for side in 0..33 {
            assert_eq!(mesh.positions()[side], mesh.positions()[65 + side]);
            assert_ne!(normals[side], normals[65 + side]);
        }
        for pair in mesh.indices().as_chunks::<6>().0 {
            let u: Vec<_> = pair[..3]
                .iter()
                .map(|&index| uvs[index as usize][0])
                .collect();
            assert!(
                u.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                    - u.iter().copied().fold(f32::INFINITY, f32::min)
                    <= 1.0 / 32.0
            );
            assert!(triangle_geometry(mesh, &pair[3..]).0[1] < 0.0);
        }
    }
}

#[test]
fn cone_outlines_have_four_side_tubes_interior_rings_and_a_base_rim() {
    for rings in [0, 1, 3, 16] {
        let uri = format!("ipp://mesh/cone-outline?radius=1&height=2&stroke=.02&rings={rings}");
        let mut publication_host = ipp_core::HostRuntime::new();
        let (world, key) = publish(
            &mut publication_host,
            &uri,
            (152 + (rings + 1) * 297, 384 + (rings + 1) * 1536),
        );
        let mesh = world.mesh(key).unwrap();
        // Four closed side tubes each reach the apex and one cardinal base point.
        for (side, endpoint) in [
            [1.0, -1.0, 0.0],
            [0.0, -1.0, 1.0],
            [-1.0, -1.0, 0.0],
            [0.0, -1.0, -1.0],
        ]
        .into_iter()
        .enumerate()
        {
            let vertices = &mesh.positions()[side * 38..(side + 1) * 38];
            assert_eq!(vertices[27], endpoint);
            assert_eq!(vertices[37], [0.0, 1.0, 0.0]);
            assert_closed(mesh, &mesh.indices()[side * 96..(side + 1) * 96]);
        }

        for ring in 0..=rings {
            let fraction = (ring + 1) as f64 / (rings + 1) as f64;
            let center_y = 1.0 - 2.0 * fraction;
            for point in &mesh.positions()[152 + ring * 297..152 + (ring + 1) * 297] {
                let radial = f64::from(point[0]).hypot(f64::from(point[2]));
                let distance = (radial - fraction).hypot(f64::from(point[1]) - center_y);
                assert!(
                    (distance - 0.01).abs() < 1e-6,
                    "ring {ring} follows its cone cross-section"
                );
            }
            assert_closed(
                mesh,
                &mesh.indices()[384 + ring * 1536..384 + (ring + 1) * 1536],
            );
        }
    }
    assert_eq!(
        builtin::mesh("ipp://mesh/cone-outline?rings=%31&stroke=.02&height=2&radius=1").unwrap(),
        builtin::mesh("ipp://mesh/cone-outline?radius=1&height=2&stroke=.02&rings=1").unwrap()
    );
    for rings in [
        "",
        "-1",
        "1.5",
        "1e0",
        "+1",
        "NaN",
        "17",
        "999999999999999999999999",
    ] {
        assert_eq!(
            builtin::mesh(&format!(
                "ipp://mesh/cone-outline?radius=1&height=2&stroke=.02&rings={rings}"
            )),
            Err(ErrorReason::InvalidAsset)
        );
    }
    for uri in [
        "ipp://mesh/cone?radius=2e38&height=1",
        "ipp://mesh/cone?radius=1e-45&height=1",
        "ipp://mesh/cone?radius=1&height=1e-45",
        "ipp://mesh/cone?radius=1&height=1&radius=1",
        "ipp://mesh/cone-outline?radius=1&height=2&stroke=.02",
        "ipp://mesh/cone-outline?radius=1&height=2&stroke=.02&rings=1&rings=1",
        "ipp://mesh/cone-outline?radius=1&height=2&stroke=.1&rings=16",
        "ipp://mesh/cone-outline?radius=1&height=2&stroke=1e-45&rings=1",
    ] {
        assert_eq!(builtin::mesh(uri), Err(ErrorReason::InvalidAsset), "{uri}");
    }
}

#[test]
fn spheres_and_pills_have_outward_closed_surfaces_and_uv_seams() {
    for (uri, radius, height, counts, rings) in [
        ("ipp://mesh/sphere?radius=1", 1.0, 2.0, (559, 2880), 15),
        (
            "ipp://mesh/pill?radius=.65&height=2.8",
            f64::from(0.65f32),
            f64::from(2.8f32),
            (592, 3072),
            16,
        ),
        (
            "ipp://mesh/pill?radius=1&height=2.0000002",
            1.0,
            f64::from(2.0000002f32),
            (592, 3072),
            16,
        ),
    ] {
        let mut publication_host = ipp_core::HostRuntime::new();
        let (world, key) = publish(&mut publication_host, uri, counts);
        let mesh = world.mesh(key).unwrap();
        assert_bounds(mesh, [radius, height * 0.5, radius]);
        assert_closed(mesh, mesh.indices());
        let half_body = height * 0.5 - radius;
        for (vertex, normal) in mesh.positions().iter().zip(mesh.normals().unwrap()) {
            let [x, y, z] = [
                f64::from(vertex[0]),
                f64::from(vertex[1]),
                f64::from(vertex[2]),
            ];
            let dy = y - y.clamp(-half_body, half_body);
            for (actual, expected) in normal.iter().zip([x / radius, dy / radius, z / radius]) {
                assert!((f64::from(*actual) - expected).abs() < 1e-6);
            }
            assert!(((x * x + dy * dy + z * z).sqrt() - radius).abs() < radius * 1e-6);
        }
        for triangle in mesh.indices().as_chunks::<3>().0 {
            let (normal, center) = triangle_geometry(mesh, triangle);
            let outward = [
                center[0],
                center[1] - center[1].clamp(-half_body, half_body),
                center[2],
            ];
            assert!(
                (0..3).map(|i| normal[i] * outward[i]).sum::<f64>() > 0.0,
                "outward {uri}"
            );
            let uvs = mesh.uvs().unwrap();
            let u: Vec<_> = triangle.iter().map(|&i| uvs[i as usize][0]).collect();
            assert!(
                u.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                    - u.iter().copied().fold(f32::INFINITY, f32::min)
                    <= 1.0 / 32.0
            );
        }
        for row in 0..rings {
            assert_eq!(mesh.positions()[row * 33], mesh.positions()[row * 33 + 32]);
            assert_eq!(
                mesh.normals().unwrap()[row * 33],
                mesh.normals().unwrap()[row * 33 + 32]
            );
            assert_eq!(mesh.uvs().unwrap()[row * 33][0], 0.0);
            assert_eq!(mesh.uvs().unwrap()[row * 33 + 32][0], 1.0);
        }
        for pole in &mesh.positions()[rings * 33..] {
            assert_eq!(pole[0], 0.0);
            assert_eq!(pole[2], 0.0);
            assert_eq!(f64::from(pole[1]).abs(), height * 0.5);
        }
    }
    for radius in ["1", ".65", "1e-30", "1e30"] {
        let r: f32 = radius.parse().unwrap();
        assert_eq!(
            builtin::mesh(&format!("ipp://mesh/sphere?radius={radius}")).unwrap(),
            builtin::mesh(&format!(
                "ipp://mesh/pill?radius={radius}&height={}",
                r * 2.0
            ))
            .unwrap()
        );
        assert_eq!(
            builtin::mesh(&format!(
                "ipp://mesh/sphere-outline?radius={radius}&stroke={}",
                r * 0.1
            ))
            .unwrap(),
            builtin::mesh(&format!(
                "ipp://mesh/pill-outline?radius={radius}&height={}&stroke={}",
                r * 2.0,
                r * 0.1
            ))
            .unwrap()
        );
    }
}

#[test]
fn debug_contours_are_closed_tubes_with_open_interiors_and_correct_bounds() {
    let stroke = f64::from(0.045f32);
    let r = f64::from(0.65f32);
    let h = f64::from(2.8f32);
    for (uri, half, counts, surfaces) in [
        (
            "ipp://mesh/cube-outline?width=2&height=2&length=2&stroke=.045",
            [1.0; 3],
            (456, 1152),
            vec![96; 12],
        ),
        (
            "ipp://mesh/sphere-outline?radius=1&stroke=.045",
            [1.0; 3],
            (1755, 9216),
            vec![3072; 3],
        ),
        (
            "ipp://mesh/pill-outline?radius=.65&height=2.8&stroke=.045",
            [r, h * 0.5, r],
            (2376, 12480),
            vec![3168, 3168, 3072, 3072],
        ),
    ] {
        let mut publication_host = ipp_core::HostRuntime::new();
        let (world, key) = publish(&mut publication_host, uri, counts);
        let mesh = world.mesh(key).unwrap();
        assert_bounds(mesh, half.map(|v| v + stroke * 0.5));
        let mut offset = 0;
        for count in surfaces {
            assert_closed(mesh, &mesh.indices()[offset..offset + count]);
            offset += count;
        }
        for vertex in mesh.positions() {
            let p = [
                f64::from(vertex[0]),
                f64::from(vertex[1]),
                f64::from(vertex[2]),
            ];
            // Every tube vertex lies by a characteristic contour. A triangulated
            // surface or latitude grid would put vertices in the open interior.
            let distance = if uri.contains("cube-outline") {
                (0..3)
                    .map(|axis| {
                        let a = (axis + 1) % 3;
                        let b = (axis + 2) % 3;
                        ((p[a].abs() - 1.0).powi(2) + (p[b].abs() - 1.0).powi(2)).sqrt()
                    })
                    .fold(f64::INFINITY, f64::min)
            } else if uri.contains("sphere-outline") {
                (0..3)
                    .map(|axis| {
                        let planar = (p[(axis + 1) % 3].powi(2) + p[(axis + 2) % 3].powi(2)).sqrt();
                        ((planar - 1.0).powi(2) + p[axis].powi(2)).sqrt()
                    })
                    .fold(f64::INFINITY, f64::min)
            } else {
                let half_body = h * 0.5 - r;
                let dy = p[1] - p[1].clamp(-half_body, half_body);
                let profiles = [0, 2]
                    .map(|axis| ((p[axis].hypot(dy) - r).powi(2) + p[2 - axis].powi(2)).sqrt());
                let rim =
                    ((p[0].hypot(p[2]) - r).powi(2) + (p[1].abs() - half_body).powi(2)).sqrt();
                profiles[0].min(profiles[1]).min(rim)
            };
            assert!(
                distance <= stroke * 0.50001,
                "{uri}: contour distance {distance}"
            );
        }
    }
}

#[test]
fn new_recipes_reject_malformed_extreme_and_collapsed_parameters() {
    for (name, names, args) in [
        ("sphere", vec!["radius"], vec!["1"]),
        ("pill", vec!["radius", "height"], vec!["1", "3"]),
        ("cone", vec!["radius", "height"], vec!["1", "2"]),
        (
            "cone-outline",
            vec!["radius", "height", "stroke", "rings"],
            vec!["1", "2", ".04", "1"],
        ),
        (
            "cube-outline",
            vec!["width", "height", "length", "stroke"],
            vec!["2", "2", "2", ".1"],
        ),
        ("sphere-outline", vec!["radius", "stroke"], vec!["1", ".1"]),
        (
            "pill-outline",
            vec!["radius", "height", "stroke"],
            vec!["1", "3", ".1"],
        ),
    ] {
        for bad in [
            "", "0", "-0", "-1", "NaN", "inf", "-inf", "1e99", "1e-99", " 1", "1 ", "1+1", "1\n",
            "１",
        ] {
            for index in 0..args.len() {
                if name == "cone-outline" && names[index] == "rings" && bad == "0" {
                    continue;
                }
                let mut invalid = args.clone();
                invalid[index] = bad;
                let query = names
                    .iter()
                    .zip(invalid)
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect::<Vec<_>>()
                    .join("&");
                let uri = format!("ipp://mesh/{name}?{query}");
                assert!(
                    matches!(builtin::mesh(&uri), Err(ErrorReason::InvalidAsset)),
                    "{uri}"
                );
            }
        }
        let query = names
            .iter()
            .zip(&args)
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("&");
        let missing = names[..names.len() - 1]
            .iter()
            .zip(&args)
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("&");
        for uri in [
            format!("ipp://mesh/{name}?{query}#trailing"),
            format!("ipp://mesh/{name}?{query}&extra=1"),
            format!("ipp://mesh/{name}/?{query}"),
            format!("ipp://mesh/{name}?{missing}"),
            format!("ipp://mesh/{name}?{}={}", names[0], "0".repeat(256)),
            format!("ipp://mesh/{name}({})", args.join(",")),
        ] {
            assert!(
                matches!(builtin::mesh(&uri), Err(ErrorReason::InvalidAsset)),
                "{uri}"
            );
        }
    }
    for uri in [
        "ipp://mesh/sphere?radius=2e38",
        "ipp://mesh/sphere?radius=1e-45",
        "ipp://mesh/pill?radius=1&height=1.9999999",
        "ipp://mesh/pill?radius=1&height=1e38",
        "ipp://mesh/pill?radius=1e-38&height=1",
        "ipp://mesh/cube-outline?width=1&height=1&length=1&stroke=.25000003",
        "ipp://mesh/cube-outline?width=3.4e38&height=3.4e38&length=3.4e38&stroke=1e37",
        "ipp://mesh/cube-outline?width=1&height=1&length=1&stroke=1e-45",
        "ipp://mesh/sphere-outline?radius=1&stroke=.50000006",
        "ipp://mesh/sphere-outline?radius=1&stroke=1e-45",
        "ipp://mesh/sphere-outline?radius=1.7e38&stroke=1e37",
        "ipp://mesh/pill-outline?radius=1&height=1.9999999&stroke=.1",
        "ipp://mesh/pill-outline?radius=1&height=3&stroke=.50000006",
        "ipp://mesh/pill-outline?radius=1&height=1e38&stroke=.1",
        "ipp://mesh/pill-outline?radius=1e-38&height=1&stroke=1e-39",
        "ipp://mesh/pill-outline?radius=1&height=3&stroke=1e-45",
    ] {
        assert!(
            matches!(builtin::mesh(uri), Err(ErrorReason::InvalidAsset)),
            "{uri}"
        );
    }
    // Safe non-unit scales and inclusive stroke boundaries stay supported. These
    // exercise finite f32 extremes without overflowing/underflowing validation.
    for scale in [f32::MIN_POSITIVE, 1.0, 1e38] {
        publish(
            &mut ipp_core::HostRuntime::new(),
            &format!("ipp://mesh/sphere?radius={scale}"),
            (559, 2880),
        );
        publish(
            &mut ipp_core::HostRuntime::new(),
            &format!("ipp://mesh/pill?radius={scale}&height={}", scale * 2.5),
            (592, 3072),
        );
        publish(
            &mut ipp_core::HostRuntime::new(),
            &format!(
                "ipp://mesh/cube-outline?width={scale}&height={scale}&length={scale}&stroke={}",
                scale * 0.25
            ),
            (456, 1152),
        );
        publish(
            &mut ipp_core::HostRuntime::new(),
            &format!(
                "ipp://mesh/sphere-outline?radius={scale}&stroke={}",
                scale * 0.5
            ),
            (1755, 9216),
        );
        publish(
            &mut ipp_core::HostRuntime::new(),
            &format!(
                "ipp://mesh/pill-outline?radius={scale}&height={}&stroke={}",
                scale * 2.5,
                scale * 0.5
            ),
            (2376, 12480),
        );
    }
}

#[test]
fn generated_shapes_obey_ordinary_immutable_asset_id_publication() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let key = MeshKey {
        asset: 91,

        variant: 0,
    };
    let next = MeshKey {
        asset: 92,
        ..key
    };
    let sphere = builtin::mesh("ipp://mesh/sphere?radius=1").unwrap();
    let pill = builtin::mesh("ipp://mesh/pill?radius=.65&height=2.8").unwrap();
    let mut malformed = pill.clone();
    malformed[16..20].copy_from_slice(&f32::NAN.to_le_bytes());
    for (id, key, bytes) in [
        (1, key, sphere.clone()),
        (2, key, pill.clone()),
        (3, next, malformed),
        (
            4,
            MeshKey {
                asset: 93,
                ..next
            },
            pill,
        ),
        (
            5,
            MeshKey {
                variant: 1,
                ..key
            },
            sphere,
        ),
    ] {
        world
            .enqueue_mesh(MeshUpload {
                id,
                key,
                bytes,
            })
            .unwrap();
    }
    assert!(world.mesh(key).is_none());
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(report.assets.len(), 5);
    assert!(report.assets[0].result.is_ok());
    assert_eq!(report.assets[1].result, Err("DuplicateAsset".into()));
    assert_eq!(report.assets[2].result, Err("InvalidAsset".into()));
    assert!(report.assets[3].result.is_ok());
    assert!(report.assets[4].result.is_ok());
    assert_eq!(world.mesh(key).unwrap().positions().len(), 559);
    assert_eq!(
        world
            .mesh(MeshKey {
                asset: 93,
                ..next
            })
            .unwrap()
            .positions()
            .len(),
        592
    );
    let retained = world.mesh(key).unwrap().positions().to_vec();
    world.update_for_test(0.0).unwrap();
    assert_eq!(world.mesh(key).unwrap().positions(), retained);
}

fn assert_winding_around_curve(
    mesh: &ipp_core::MeshAsset,
    indices: &[u16],
    nearest: impl Fn(Point) -> Point,
) {
    for triangle in indices.as_chunks::<3>().0 {
        let (normal, center) = triangle_geometry(mesh, triangle);
        let curve = nearest(center);
        let facing: f64 = (0..3).map(|i| normal[i] * (center[i] - curve[i])).sum();
        assert!(
            facing > 0.0,
            "tube faces away from its own centreline: {center:?}"
        );
    }
}

#[test]
fn contour_triangles_face_outward_including_inner_tube_walls_and_caps() {
    let mut publication_host = ipp_core::HostRuntime::new();
    let (world, key) = publish(
        &mut publication_host,
        "ipp://mesh/sphere-outline?radius=1&stroke=.5",
        (1755, 9216),
    );
    let mesh = world.mesh(key).unwrap();
    for (ring, axis) in [2, 1, 0].into_iter().enumerate() {
        assert_winding_around_curve(
            mesh,
            &mesh.indices()[ring * 3072..(ring + 1) * 3072],
            |mut p| {
                p[axis] = 0.0;
                let length = p.iter().map(|v| v * v).sum::<f64>().sqrt();
                p.map(|v| v / length)
            },
        );
    }
    let mut publication_host = ipp_core::HostRuntime::new();
    let (world, key) = publish(
        &mut publication_host,
        "ipp://mesh/pill-outline?radius=1&height=4&stroke=.5",
        (2376, 12480),
    );
    let mesh = world.mesh(key).unwrap();
    for (profile, axis) in [0, 2].into_iter().enumerate() {
        assert_winding_around_curve(
            mesh,
            &mesh.indices()[profile * 3168..(profile + 1) * 3168],
            |p| {
                let cap_y = p[1].clamp(-1.0, 1.0);
                let dy = p[1] - cap_y;
                let length = p[axis].hypot(dy);
                let mut nearest = [0.0; 3];
                nearest[axis] = p[axis] / length;
                nearest[1] = cap_y + dy / length;
                nearest
            },
        );
    }
    for (rim, y) in [1.0, -1.0].into_iter().enumerate() {
        assert_winding_around_curve(
            mesh,
            &mesh.indices()[6336 + rim * 3072..6336 + (rim + 1) * 3072],
            |p| {
                let length = p[0].hypot(p[2]);
                [p[0] / length, y, p[2] / length]
            },
        );
    }
    let mut publication_host = ipp_core::HostRuntime::new();
    let (world, key) = publish(
        &mut publication_host,
        "ipp://mesh/cube-outline?width=2&height=4&length=6&stroke=.5",
        (456, 1152),
    );
    let mesh = world.mesh(key).unwrap();
    for edge in 0..12 {
        let axis = edge / 4;
        let start = mesh.positions()[edge * 38 + 27];
        assert_winding_around_curve(mesh, &mesh.indices()[edge * 96..edge * 96 + 48], |p| {
            std::array::from_fn(|i| {
                if i == axis {
                    p[i]
                } else {
                    f64::from(start[i])
                }
            })
        });
        for cap in 0..2 {
            for triangle in mesh.indices()[edge * 96 + 48 + cap * 24..edge * 96 + 72 + cap * 24]
                .as_chunks::<3>()
                .0
            {
                let (normal, _) = triangle_geometry(mesh, triangle);
                assert!(
                    normal[axis]
                        * if cap == 0 {
                            -1.0
                        } else {
                            1.0
                        }
                        > 0.0
                );
            }
        }
    }
}
