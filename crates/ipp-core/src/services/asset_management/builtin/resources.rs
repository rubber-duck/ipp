use super::*;

/// Generate a cube, sphere, pill, cone, plane, arrow, axis or a supported `-outline` recipe as IPPM
/// v3 with RGB, UVs and unit normals. The filled plane also includes texture weights.
/// Dimensions are positive finite local-space metres.
/// Pills use total end-to-end height; outline stroke is a tube diameter.
/// Unknown, unbounded or numerically collapsed recipes are rejected.
#[cfg(feature = "builtin-assets")]
pub fn mesh(uri: &str) -> Result<Vec<u8>, ErrorReason> {
    #[cfg(feature = "skeletal-animation")]
    if uri == "ipp://mesh/rig-strip" {
        return rig(crate::MESH_TYPE, uri);
    }
    mesh_with_attributes(uri, false)
}

#[cfg(feature = "builtin-assets")]
fn mesh_with_attributes(uri: &str, positions_only: bool) -> Result<Vec<u8>, ErrorReason> {
    if !uri.starts_with("ipp://mesh/cube?") {
        return shapes::mesh(uri, positions_only);
    }

    let args = arguments(uri, "ipp://mesh/cube?", ["width", "height", "length"])?;
    let mut half = [0.0; 3];
    for (value, arg) in half.iter_mut().zip(args) {
        let extent: f32 = arg.parse().map_err(|_| ErrorReason::InvalidAsset)?;
        *value = extent * 0.5;
        if !extent.is_finite() || extent <= 0.0 || *value == 0.0 {
            return Err(ErrorReason::InvalidAsset);
        }
    }

    Ok(cube(half, positions_only))
}

fn cube(half: [f32; 3], positions_only: bool) -> Vec<u8> {
    // Four counter-clockwise corners per outward-facing side, viewed from outside.
    let faces = [
        [[-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]],
        [
            [1., -1., -1.],
            [-1., -1., -1.],
            [-1., 1., -1.],
            [1., 1., -1.],
        ],
        [[1., -1., 1.], [1., -1., -1.], [1., 1., -1.], [1., 1., 1.]],
        [
            [-1., -1., -1.],
            [-1., -1., 1.],
            [-1., 1., 1.],
            [-1., 1., -1.],
        ],
        [[-1., 1., 1.], [1., 1., 1.], [1., 1., -1.], [-1., 1., -1.]],
        [
            [-1., -1., -1.],
            [1., -1., -1.],
            [1., -1., 1.],
            [-1., -1., 1.],
        ],
    ];
    let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let mut bytes = Vec::with_capacity(if positions_only {
        388
    } else {
        1180
    });
    bytes.extend(b"IPPM");
    for value in [
        3u32,
        24,
        36,
        if positions_only {
            1
        } else {
            4
        },
    ] {
        bytes.extend(value.to_le_bytes());
    }
    for (semantic, format, width) in [(0, 1, 12), (1, 1, 12), (2, 2, 8), (4, 1, 12)] {
        if positions_only && semantic != 0 {
            continue;
        }
        bytes.extend([semantic, format, 0, 0]);
        bytes.extend((24u32 * width).to_le_bytes());
    }
    for face in faces {
        for position in face {
            for axis in 0..3 {
                bytes.extend((position[axis] * half[axis]).to_le_bytes());
            }
        }
    }
    if !positions_only {
        for _ in 0..24 * 3 {
            bytes.extend(1.0f32.to_le_bytes());
        }
        for _ in 0..6 {
            for uv in uv {
                for value in uv {
                    bytes.extend(f32::to_le_bytes(value));
                }
            }
        }
        for normal in [
            [0.0f32, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
        ] {
            for _ in 0..4 {
                for value in normal {
                    bytes.extend(value.to_le_bytes());
                }
            }
        }
    }
    for face in 0..6u16 {
        for index in [0, 1, 2, 0, 2, 3] {
            bytes.extend((face * 4 + index).to_le_bytes());
        }
    }
    bytes
}

/// Generate a packed RGBA8 checker or orientation grid as IPPTv3.
/// No I/O or global asset identities; dimensions divide evenly into cells.
#[cfg(feature = "builtin-assets")]
pub fn texture(uri: &str) -> Result<Vec<u8>, ErrorReason> {
    let uv_grid = uri.starts_with("ipp://texture/uv-grid?");
    let args = arguments(
        uri,
        if uv_grid {
            "ipp://texture/uv-grid?"
        } else {
            "ipp://texture/checkerboard?"
        },
        ["width", "height", "cellsX", "cellsY"],
    )?;
    let mut values = [0; 4];
    for (value, arg) in values.iter_mut().zip(args) {
        if !arg.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ErrorReason::InvalidAsset);
        }
        *value = arg.parse::<u32>().map_err(|_| ErrorReason::InvalidAsset)?;
        if *value == 0 {
            return Err(ErrorReason::InvalidAsset);
        }
    }

    let [width, height, cells_x, cells_y] = values;
    let pixel_bytes = crate::services::asset_management::texture::pixel_bytes(width, height)?;
    if !width.is_multiple_of(cells_x) || !height.is_multiple_of(cells_y) {
        return Err(ErrorReason::InvalidAsset);
    }
    let cell_width = width / cells_x;
    let cell_height = height / cells_y;
    let payload_bytes = pixel_bytes as usize + 16;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(payload_bytes)
        .map_err(|_| ErrorReason::Capacity)?;
    bytes.extend(b"IPPT");
    for value in [3, width, height] {
        bytes.extend(value.to_le_bytes());
    }
    for y in 0..height {
        for x in 0..width {
            let color = if uv_grid {
                uv_grid_color(x, y, cell_width, cell_height, cells_x, cells_y)
            } else {
                match (x / cell_width + y / cell_height) % 4 {
                    0 => [255, 0, 0],
                    1 => [0, 255, 0],
                    2 => [0, 0, 255],
                    _ => [0, 0, 0],
                }
            };
            bytes.extend(color);
            bytes.push(255);
        }
    }
    Ok(bytes)
}

#[cfg(feature = "builtin-assets")]
fn uv_grid_color(
    x: u32,
    y: u32,
    cell_width: u32,
    cell_height: u32,
    cells_x: u32,
    cells_y: u32,
) -> [u8; 3] {
    // Sample pixel centres in a 1024-unit cell. Wide integer intermediates keep
    // large and rectangular recipes deterministic without floating-point edges.
    let local_x = (u64::from(x % cell_width) * 2 + 1) * 512 / u64::from(cell_width);
    let local_y = (u64::from(y % cell_height) * 2 + 1) * 512 / u64::from(cell_height);
    if !(32..992).contains(&local_x) || !(32..992).contains(&local_y) {
        return [16; 3];
    }

    // Apex (1/2, 1/4), base (1/4, 3/4)..(3/4, 3/4), pointing toward -V.
    // Different halves reveal horizontal mirroring as well as upside-down UVs.
    if (256..=768).contains(&local_y) && local_x.abs_diff(512) * 2 <= local_y - 256 {
        return if local_x < 512 {
            [240; 3]
        } else {
            [16; 3]
        };
    }

    let cell_x = x / cell_width;
    let cell_y = y / cell_height;
    [
        (48 + u64::from(cell_x) * 176 / u64::from((cells_x - 1).max(1))) as u8,
        (48 + u64::from(cell_y) * 176 / u64::from((cells_y - 1).max(1))) as u8,
        if (cell_x + cell_y).is_multiple_of(2) {
            96
        } else {
            192
        },
    ]
}

/// Generate private position-only geometry without acquiring a public source.
pub fn debug_mesh(
    value: &crate::systems::geometry::GeometryPrimitiveVisual,
) -> Result<crate::MeshAsset, ErrorReason> {
    value.validate_dimensions()?;
    let bytes = if value.shape == 0 && !value.outline {
        cube(
            [value.width * 0.5, value.height * 0.5, value.length * 0.5],
            true,
        )
    } else {
        shapes::debug_mesh(value)?
    };
    crate::MeshAsset::decode(&bytes).map(|(mesh, _)| mesh)
}

#[cfg(all(test, feature = "builtin-assets"))]
mod tests {
    use super::*;

    #[test]
    fn private_geometry_matches_public_recipe_positions_and_indices() {
        for (shape, name, dimensions) in [
            (0, "cube", "width=2&height=3&length=4"),
            (1, "sphere", "radius=0.75"),
            (2, "pill", "radius=0.75&height=3"),
            (
                3,
                "plane",
                "size=2&normalLength=1&stroke=0.05&normalOffset=0",
            ),
        ] {
            for outline in [false, true] {
                let value = crate::systems::geometry::GeometryPrimitiveVisual {
                    shape,
                    outline,
                    width: 2.0,
                    height: 3.0,
                    length: 4.0,
                    radius: 0.75,
                    size: 2.0,
                    normal_length: 1.0,
                    stroke: 0.05,
                };
                let form = if outline {
                    "-outline"
                } else {
                    ""
                };
                let mut uri = format!("ipp://mesh/{name}{form}?{dimensions}");
                if outline && shape != 3 {
                    uri.push_str("&stroke=0.05");
                }

                let private = debug_mesh(&value).unwrap();
                let (recipe, _) =
                    crate::MeshAsset::decode(&mesh_with_attributes(&uri, true).unwrap()).unwrap();
                assert_eq!(private.positions(), recipe.positions(), "{uri}");
                assert_eq!(private.indices(), recipe.indices(), "{uri}");
            }
        }
    }
}
