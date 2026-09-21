use super::*;

#[cfg(feature = "builtin-assets")]
pub(in crate::services::asset_management::builtin) fn mesh(
    uri: &str,
    positions_only: bool,
) -> Result<Vec<u8>, ErrorReason> {
    let mut mesh = BuiltinMesh {
        normals: !positions_only,
        ..BuiltinMesh::default()
    };
    if uri.starts_with("ipp://mesh/sphere?") {
        let [radius] = dimensions(uri, "ipp://mesh/sphere?", ["radius"])?;
        solid(&mut mesh, radius, diameter(radius)?)?;
    } else if uri.starts_with("ipp://mesh/pill?") {
        let [radius, height] = dimensions(uri, "ipp://mesh/pill?", ["radius", "height"])?;
        pill_height(radius, height)?;
        solid(&mut mesh, radius, height)?;
    } else if uri.starts_with("ipp://mesh/cone?") {
        let [radius, height] = dimensions(uri, "ipp://mesh/cone?", ["radius", "height"])?;
        diameter(radius)?;
        cone::generate(&mut mesh, radius, height)?;
    } else if uri.starts_with("ipp://mesh/cone-outline?") {
        cone::outline_recipe(&mut mesh, uri)?;
    } else if uri.starts_with("ipp://mesh/cube-outline?") {
        let [width, height, depth, stroke] = dimensions(
            uri,
            "ipp://mesh/cube-outline?",
            ["width", "height", "length", "stroke"],
        )?;
        validate_stroke([width, height, depth], stroke)?;
        cube_outline(&mut mesh, [width, height, depth], stroke)?;
    } else if uri.starts_with("ipp://mesh/sphere-outline?") {
        let [radius, stroke] = dimensions(uri, "ipp://mesh/sphere-outline?", ["radius", "stroke"])?;
        validate_stroke([diameter(radius)?; 3], stroke)?;
        sphere_outline(&mut mesh, radius, stroke)?;
    } else if uri.starts_with("ipp://mesh/pill-outline?") {
        let [radius, height, stroke] = dimensions(
            uri,
            "ipp://mesh/pill-outline?",
            ["radius", "height", "stroke"],
        )?;
        pill_height(radius, height)?;
        validate_stroke([diameter(radius)?, height, diameter(radius)?], stroke)?;
        if height == 2.0 * radius {
            sphere_outline(&mut mesh, radius, stroke)?;
        } else {
            pill_outline(&mut mesh, radius, height, stroke)?;
        }
    } else if uri.starts_with("ipp://mesh/arrow?") {
        arrow::recipe(&mut mesh, uri)?;
    } else if uri.starts_with("ipp://mesh/axis?") {
        axis::recipe(&mut mesh, uri)?;
    } else if uri.starts_with("ipp://mesh/plane?") {
        plane::recipe(&mut mesh, uri, false)?;
        if positions_only {
            return mesh.encode_positions();
        }
        let mut weights = vec![0; mesh.vertices.len()];
        weights[..4].fill(255);
        return mesh.encode_weighted(&weights);
    } else if uri.starts_with("ipp://mesh/plane-outline?") {
        plane::recipe(&mut mesh, uri, true)?;
    } else {
        return Err(ErrorReason::InvalidAsset);
    }

    if positions_only {
        mesh.encode_positions()
    } else {
        mesh.encode()
    }
}

// Dimensions have already been validated by GeometryPrimitiveVisual. Keep
// private generation independent of the optional URI parser and recipe catalog.
pub(in crate::services::asset_management::builtin) fn debug_mesh(
    value: &crate::systems::geometry::GeometryPrimitiveVisual,
) -> Result<Vec<u8>, ErrorReason> {
    let mut mesh = BuiltinMesh::default();
    let radius = f64::from(value.radius);
    let height = f64::from(value.height);
    let stroke = f64::from(value.stroke);
    match (value.shape, value.outline) {
        (0, true) => {
            let extents = [value.width, value.height, value.length].map(f64::from);
            validate_stroke(extents, stroke)?;
            cube_outline(&mut mesh, extents, stroke)?;
        }
        (1, false) => solid(&mut mesh, radius, diameter(radius)?)?,
        (1, true) => {
            validate_stroke([diameter(radius)?; 3], stroke)?;
            sphere_outline(&mut mesh, radius, stroke)?;
        }
        (2, outline) => {
            pill_height(radius, height)?;
            if outline {
                validate_stroke([diameter(radius)?, height, diameter(radius)?], stroke)?;
                if height == 2.0 * radius {
                    sphere_outline(&mut mesh, radius, stroke)?;
                } else {
                    pill_outline(&mut mesh, radius, height, stroke)?;
                }
            } else {
                solid(&mut mesh, radius, height)?;
            }
        }
        (3, outline) => plane::generate(
            &mut mesh,
            f64::from(value.size),
            f64::from(value.normal_length),
            stroke,
            0.0,
            outline,
        )?,
        _ => return Err(ErrorReason::InvalidValue),
    }

    mesh.encode_positions()
}

#[cfg(feature = "builtin-assets")]
pub(super) fn dimensions<const N: usize>(
    uri: &str,
    prefix: &str,
    names: [&str; N],
) -> Result<[f64; N], ErrorReason> {
    let args = super::super::arguments(uri, prefix, names)?;
    let mut values = [0.0; N];
    for (value, arg) in values.iter_mut().zip(args) {
        let parsed: f32 = arg.parse().map_err(|_| ErrorReason::InvalidAsset)?;
        if !parsed.is_finite() || parsed <= 0.0 {
            return Err(ErrorReason::InvalidAsset);
        }
        *value = f64::from(parsed);
    }
    Ok(values)
}

pub(super) fn diameter(radius: f64) -> Result<f64, ErrorReason> {
    let diameter = radius * 2.0;
    if diameter > f64::from(f32::MAX) {
        return Err(ErrorReason::InvalidAsset);
    }
    Ok(diameter)
}

fn pill_height(radius: f64, height: f64) -> Result<(), ErrorReason> {
    if height < diameter(radius)? {
        return Err(ErrorReason::InvalidAsset);
    }
    Ok(())
}

pub(super) fn validate_stroke(extents: BuiltinMeshPoint, stroke: f64) -> Result<(), ErrorReason> {
    if extents
        .into_iter()
        .any(|extent| stroke > extent * 0.25 || extent + stroke > f64::from(f32::MAX))
    {
        return Err(ErrorReason::InvalidAsset);
    }
    Ok(())
}

fn add(a: BuiltinMeshPoint, b: BuiltinMeshPoint) -> BuiltinMeshPoint {
    std::array::from_fn(|i| a[i] + b[i])
}

fn scale(a: BuiltinMeshPoint, scale: f64) -> BuiltinMeshPoint {
    a.map(|value| value * scale)
}

fn cross(a: BuiltinMeshPoint, b: BuiltinMeshPoint) -> BuiltinMeshPoint {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

// Exact cardinal samples and repeated seams avoid tiny pole offsets and cracks.
pub(super) fn circle_sample(index: usize, segments: usize) -> (f64, f64) {
    let index = index % segments;
    if (index * 4).is_multiple_of(segments) {
        return match index * 4 / segments {
            0 => (0.0, 1.0),
            1 => (1.0, 0.0),
            2 => (0.0, -1.0),
            _ => (-1.0, 0.0),
        };
    }
    (TAU * index as f64 / segments as f64).sin_cos()
}

impl BuiltinMesh {
    pub(super) fn vertex(
        &mut self,
        position: BuiltinMeshPoint,
        outward: BuiltinMeshPoint,
        uv: [f64; 2],
    ) -> Result<usize, ErrorReason> {
        let position = position.map(|value| value as f32);
        let uv = uv.map(|value| value as f32);
        if position.iter().chain(&uv).any(|value| !value.is_finite())
            || self.vertices.len() > u16::MAX as usize
        {
            return Err(ErrorReason::InvalidAsset);
        }

        let index = self.vertices.len();
        self.vertices.push(BuiltinMeshVertex {
            position,
            color: [1.0; 3],
            uv,
            outward,
        });
        Ok(index)
    }

    pub(super) fn triangle(&mut self, indices: [usize; 3]) -> Result<(), ErrorReason> {
        let [a, b, c] = indices.map(|index| &self.vertices[index]);
        let ab = std::array::from_fn(|i| f64::from(b.position[i]) - f64::from(a.position[i]));
        let ac = std::array::from_fn(|i| f64::from(c.position[i]) - f64::from(a.position[i]));
        let normal = cross(ab, ac);
        let outward = add(add(a.outward, b.outward), c.outward);
        let orientation: f64 = (0..3).map(|i| normal[i] * outward[i]).sum();
        // Validate the actual stored f32 geometry, using f64 to avoid overflow
        // or underflow in the test itself. Rounding must not collapse or flip it.
        if !orientation.is_finite() || orientation <= 0.0 {
            return Err(ErrorReason::InvalidAsset);
        }
        for index in indices {
            self.indices
                .push(u16::try_from(index).map_err(|_| ErrorReason::InvalidAsset)?);
        }
        Ok(())
    }

    pub(super) fn encode_positions(self) -> Result<Vec<u8>, ErrorReason> {
        let mut bytes = Vec::with_capacity(28 + self.vertices.len() * 12 + self.indices.len() * 2);
        bytes.extend(b"IPPM");
        for value in [
            3u32,
            self.vertices.len() as u32,
            self.indices.len() as u32,
            1,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0, 1, 0, 0]);
        bytes.extend((self.vertices.len() as u32 * 12).to_le_bytes());
        for vertex in self.vertices {
            for value in vertex.position {
                bytes.extend(value.to_le_bytes());
            }
        }
        for index in self.indices {
            bytes.extend(index.to_le_bytes());
        }
        Ok(bytes)
    }

    #[cfg(feature = "builtin-assets")]
    pub(super) fn encode_weighted(self, weights: &[u8]) -> Result<Vec<u8>, ErrorReason> {
        self.encode_streams(Some(weights))
    }

    #[cfg(feature = "builtin-assets")]
    pub(super) fn encode(self) -> Result<Vec<u8>, ErrorReason> {
        self.encode_streams(None)
    }

    #[cfg(feature = "builtin-assets")]
    pub(super) fn encode_streams(self, weights: Option<&[u8]>) -> Result<Vec<u8>, ErrorReason> {
        let count = 4 + usize::from(weights.is_some());
        let stride = 44 + usize::from(weights.is_some());
        let size = 20 + count * 8 + self.vertices.len() * stride + self.indices.len() * 2;
        if weights.is_some_and(|values| values.len() != self.vertices.len()) {
            return Err(ErrorReason::InvalidAsset);
        }

        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| ErrorReason::Capacity)?;
        bytes.extend(b"IPPM");
        for value in [
            3,
            self.vertices.len() as u32,
            self.indices.len() as u32,
            count as u32,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        for (semantic, format, width) in [(0, 1, 12), (1, 1, 12), (2, 2, 8), (3, 3, 1), (4, 1, 12)]
        {
            if semantic == 3 && weights.is_none() {
                continue;
            }
            bytes.extend([semantic, format, 0, 0]);
            bytes.extend((self.vertices.len() as u32 * width).to_le_bytes());
        }
        for vertex in &self.vertices {
            for value in vertex.position {
                bytes.extend(value.to_le_bytes());
            }
        }
        for vertex in &self.vertices {
            for value in vertex.color {
                bytes.extend(value.to_le_bytes());
            }
        }
        for vertex in &self.vertices {
            for value in vertex.uv {
                bytes.extend(value.to_le_bytes());
            }
        }
        if let Some(weights) = weights {
            bytes.extend(weights);
        }
        for vertex in &self.vertices {
            let length = vertex.outward.iter().map(|v| v * v).sum::<f64>().sqrt();
            if !length.is_finite() || length == 0.0 {
                return Err(ErrorReason::InvalidAsset);
            }
            for value in vertex.outward {
                bytes.extend(((value / length) as f32).to_le_bytes());
            }
        }
        for index in self.indices {
            bytes.extend(index.to_le_bytes());
        }
        Ok(bytes)
    }
}

fn solid(mesh: &mut BuiltinMesh, radius: f64, height: f64) -> Result<(), ErrorReason> {
    let body = height - 2.0 * radius;
    let half_body = body * 0.5;
    let meridian = PI * radius + body;
    let mut rings = Vec::with_capacity(LATITUDES);
    let mut previous_y = (height * 0.5) as f32;
    for latitude in 1..LATITUDES {
        let (sin, cos) = circle_sample(latitude, LATITUDES * 2);
        // The equator occurs twice only when a nonzero cylinder joins the caps.
        let centers: &[f64] = if latitude == LATITUDES / 2 && body > 0.0 {
            &[half_body, -half_body]
        } else if latitude <= LATITUDES / 2 {
            &[half_body]
        } else {
            &[-half_body]
        };
        for &center in centers {
            // Area alone cannot detect a hemisphere rounded into concentric
            // flat discs when height dwarfs radius. Preserve every latitude.
            let y = (radius * cos + center) as f32;
            if y >= previous_y || y <= (-height * 0.5) as f32 {
                return Err(ErrorReason::InvalidAsset);
            }
            previous_y = y;

            let v = (PI * radius * latitude as f64 / LATITUDES as f64
                + if center < 0.0 {
                    body
                } else {
                    0.0
                })
                / meridian;
            rings.push(mesh.vertices.len());
            for longitude in 0..=LONGITUDES {
                let (s, c) = circle_sample(longitude, LONGITUDES);
                let normal = [sin * c, cos, sin * s];
                let position = add(scale(normal, radius), [0.0, center, 0.0]);
                mesh.vertex(position, normal, [longitude as f64 / LONGITUDES as f64, v])?;
            }
        }
    }

    for rows in rings.windows(2) {
        for longitude in 0..LONGITUDES {
            let a = rows[0] + longitude;
            let b = rows[1] + longitude;
            mesh.triangle([a, a + 1, b + 1])?;
            mesh.triangle([a, b + 1, b])?;
        }
    }
    for (top, row) in [(true, rings[0]), (false, rings[rings.len() - 1])] {
        let sign = if top {
            1.0
        } else {
            -1.0
        };
        for longitude in 0..LONGITUDES {
            let pole = mesh.vertex(
                [0.0, sign * height * 0.5, 0.0],
                [0.0, sign, 0.0],
                [
                    (longitude as f64 + 0.5) / LONGITUDES as f64,
                    if top {
                        0.0
                    } else {
                        1.0
                    },
                ],
            )?;
            let a = row + longitude;
            mesh.triangle(if top {
                [pole, a + 1, a]
            } else {
                [pole, a, a + 1]
            })?;
        }
    }
    Ok(())
}

fn tube(
    mesh: &mut BuiltinMesh,
    path: &[BuiltinPathPoint],
    plane: BuiltinMeshPoint,
    stroke: f64,
) -> Result<usize, ErrorReason> {
    let base = mesh.vertices.len();
    for point in path {
        for side in 0..=TUBE_SIDES {
            let (sin, cos) = circle_sample(side, TUBE_SIDES);
            let normal = add(scale(point.radial, cos), scale(plane, sin));
            mesh.vertex(
                add(point.center, scale(normal, stroke * 0.5)),
                normal,
                [point.u, side as f64 / TUBE_SIDES as f64],
            )?;
        }
    }
    for segment in 0..path.len() - 1 {
        for side in 0..TUBE_SIDES {
            let a = base + segment * (TUBE_SIDES + 1) + side;
            let b = a + TUBE_SIDES + 1;
            mesh.triangle([a, b, b + 1])?;
            mesh.triangle([a, b + 1, a + 1])?;
        }
    }
    Ok(base)
}

// A planar contour supplies a consistent radial and plane normal, avoiding
// parallel-transport ambiguity or twisting at the capsule's straight sections.
fn ring(
    mesh: &mut BuiltinMesh,
    radius: f64,
    center: BuiltinMeshPoint,
    axes: [BuiltinMeshPoint; 2],
    stroke: f64,
) -> Result<(), ErrorReason> {
    ring_with_segments(mesh, radius, center, axes, stroke, CURVE_SEGMENTS)
}

pub(super) fn ring_with_segments(
    mesh: &mut BuiltinMesh,
    radius: f64,
    center: BuiltinMeshPoint,
    axes: [BuiltinMeshPoint; 2],
    stroke: f64,
    segments: usize,
) -> Result<(), ErrorReason> {
    let path: Vec<_> = (0..=segments)
        .map(|segment| {
            let (sin, cos) = circle_sample(segment, segments);
            let radial = add(scale(axes[0], cos), scale(axes[1], sin));
            BuiltinPathPoint {
                center: add(center, scale(radial, radius)),
                radial,
                u: segment as f64 / segments as f64,
            }
        })
        .collect();
    tube(mesh, &path, cross(axes[0], axes[1]), stroke)?;
    Ok(())
}

fn sphere_outline(mesh: &mut BuiltinMesh, radius: f64, stroke: f64) -> Result<(), ErrorReason> {
    for axes in [[X, Y], [X, Z], [Y, Z]] {
        ring(mesh, radius, [0.0; 3], axes, stroke)?;
    }
    Ok(())
}

fn pill_outline(
    mesh: &mut BuiltinMesh,
    radius: f64,
    height: f64,
    stroke: f64,
) -> Result<(), ErrorReason> {
    let half_body = height * 0.5 - radius;
    let perimeter = TAU * radius + 4.0 * half_body;
    for horizontal in [X, Z] {
        let mut path = Vec::with_capacity(CURVE_SEGMENTS + 3);
        for half in 0..2 {
            let offset = if half == 0 {
                half_body
            } else {
                -half_body
            };
            for segment in 0..=CURVE_SEGMENTS / 2 {
                let index = half * CURVE_SEGMENTS / 2 + segment;
                let (sin, cos) = circle_sample(index, CURVE_SEGMENTS);
                let radial = add(scale(horizontal, cos), scale(Y, sin));
                path.push(BuiltinPathPoint {
                    center: add(scale(radial, radius), scale(Y, offset)),
                    radial,
                    u: (TAU * radius * index as f64 / CURVE_SEGMENTS as f64
                        + half as f64 * 2.0 * half_body)
                        / perimeter,
                });
            }
        }
        path.push(BuiltinPathPoint {
            center: path[0].center,
            radial: path[0].radial,
            u: 1.0,
        });
        tube(mesh, &path, cross(horizontal, Y), stroke)?;
    }
    for y in [half_body, -half_body] {
        ring(mesh, radius, [0.0, y, 0.0], [X, Z], stroke)?;
    }
    Ok(())
}

fn cube_outline(
    mesh: &mut BuiltinMesh,
    extents: BuiltinMeshPoint,
    stroke: f64,
) -> Result<(), ErrorReason> {
    for axis in 0..3 {
        for a in [-1.0, 1.0] {
            for b in [-1.0, 1.0] {
                let mut start = scale(extents, 0.5);
                start[axis] *= -1.0;
                start[(axis + 1) % 3] *= a;
                start[(axis + 2) % 3] *= b;
                let mut end = start;
                end[axis] *= -1.0;
                cylinder(mesh, start, end, axis, stroke)?;
            }
        }
    }
    Ok(())
}

// All callers provide a segment along a positive coordinate axis. Shared caps
// keep debug edges and normal arrows closed without a renderer-specific path.
pub(super) fn cylinder(
    mesh: &mut BuiltinMesh,
    start: BuiltinMeshPoint,
    end: BuiltinMeshPoint,
    axis: usize,
    stroke: f64,
) -> Result<(), ErrorReason> {
    let direction = [X, Y, Z][axis];
    let radial = [X, Y, Z][(axis + 1) % 3];
    oriented_cylinder(mesh, start, end, direction, radial, stroke)
}

// The supplied unit direction and perpendicular radial axis fix the tube frame.
pub(super) fn oriented_cylinder(
    mesh: &mut BuiltinMesh,
    start: BuiltinMeshPoint,
    end: BuiltinMeshPoint,
    direction: BuiltinMeshPoint,
    radial: BuiltinMeshPoint,
    stroke: f64,
) -> Result<(), ErrorReason> {
    let plane = cross(radial, direction);
    let path = [
        BuiltinPathPoint {
            center: start,
            radial,
            u: 0.0,
        },
        BuiltinPathPoint {
            center: end,
            radial,
            u: 1.0,
        },
    ];
    let base = tube(mesh, &path, plane, stroke)?;
    for (end_cap, center) in [start, end].into_iter().enumerate() {
        let normal = scale(
            direction,
            if end_cap == 0 {
                -1.0
            } else {
                1.0
            },
        );
        let rim = base + end_cap * (TUBE_SIDES + 1);
        let cap_rim = if mesh.normals {
            let duplicate = mesh.vertices.len();
            for side in 0..=TUBE_SIDES {
                let vertex = &mesh.vertices[rim + side];
                mesh.vertex(
                    vertex.position.map(f64::from),
                    normal,
                    vertex.uv.map(f64::from),
                )?;
            }
            duplicate
        } else {
            rim
        };
        let cap = mesh.vertex(center, normal, [end_cap as f64, 0.5])?;
        for side in 0..TUBE_SIDES {
            let a = cap_rim + side;
            mesh.triangle(if end_cap == 0 {
                [cap, a, a + 1]
            } else {
                [cap, a + 1, a]
            })?;
        }
    }
    Ok(())
}
