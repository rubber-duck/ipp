//! A finite square and a three-dimensional arrow showing its positive normal.

use super::{BuiltinMesh, ErrorReason, Z, arrow, cylinder};

#[cfg(feature = "builtin-assets")]
pub(super) fn recipe(mesh: &mut BuiltinMesh, uri: &str, outline: bool) -> Result<(), ErrorReason> {
    let args = super::super::argument_values(
        uri,
        if outline {
            "ipp://mesh/plane-outline?"
        } else {
            "ipp://mesh/plane?"
        },
        ["size", "normalLength", "stroke", "normalOffset"],
        3,
    )?;
    let mut dimensions = [0.0; 3];
    for (dimension, value) in dimensions.iter_mut().zip(&args) {
        let parsed: f32 = value.parse().map_err(|_| ErrorReason::InvalidAsset)?;
        if !parsed.is_finite() || parsed <= 0.0 {
            return Err(ErrorReason::InvalidAsset);
        }
        *dimension = f64::from(parsed);
    }

    let [size, length, stroke] = dimensions;
    let offset = if args[3].is_empty() {
        stroke * 2.0
    } else {
        let parsed: f64 = args[3].parse().map_err(|_| ErrorReason::InvalidAsset)?;
        if !parsed.is_finite()
            || parsed < 0.0
            || parsed > f64::from(f32::MAX)
            || (parsed > 0.0 && (parsed as f32) == 0.0)
        {
            return Err(ErrorReason::InvalidAsset);
        }
        f64::from(parsed as f32)
    };

    generate(mesh, size, length, stroke, offset, outline)
}

pub(super) fn generate(
    mesh: &mut BuiltinMesh,
    size: f64,
    length: f64,
    stroke: f64,
    offset: f64,
    outline: bool,
) -> Result<(), ErrorReason> {
    if stroke > size.min(length) / 8.0 || size + stroke > f64::from(f32::MAX) {
        return Err(ErrorReason::InvalidAsset);
    }

    let half = size * 0.5;
    if outline {
        for side in [-half, half] {
            cylinder(mesh, [-half, side, 0.0], [half, side, 0.0], 0, stroke)?;
            cylinder(mesh, [side, -half, 0.0], [side, half, 0.0], 1, stroke)?;
        }
    } else {
        let base = mesh.vertices.len();
        for (position, uv) in [
            ([-half, -half, 0.0], [0.0, 1.0]),
            ([half, -half, 0.0], [1.0, 1.0]),
            ([half, half, 0.0], [1.0, 0.0]),
            ([-half, half, 0.0], [0.0, 0.0]),
        ] {
            let vertex = mesh.vertex(position, Z, uv)?;
            mesh.vertices[vertex].color = [0.25; 3];
        }
        mesh.triangle([base, base + 1, base + 2])?;
        mesh.triangle([base, base + 2, base + 3])?;
    }

    arrow::generate(mesh, length, stroke, offset)
}
