//! Three independently colored arrows along the positive coordinate axes.

use super::{BuiltinMesh, ErrorReason, arrow};

pub(super) fn recipe(mesh: &mut BuiltinMesh, uri: &str) -> Result<(), ErrorReason> {
    let args = super::super::argument_values(
        uri,
        "ipp://mesh/axis?",
        ["length", "stroke", "xColor", "yColor", "zColor"],
        2,
    )?;
    let mut dimensions = [0.0; 2];
    for (dimension, value) in dimensions.iter_mut().zip(&args) {
        let parsed: f32 = value.parse().map_err(|_| ErrorReason::InvalidAsset)?;
        if !parsed.is_finite() || parsed <= 0.0 {
            return Err(ErrorReason::InvalidAsset);
        }
        *dimension = f64::from(parsed);
    }

    let mut colors = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for (color, value) in colors.iter_mut().zip(&args[2..]) {
        if !value.is_empty() {
            *color = parse_color(value)?;
        }
    }

    let [length, stroke] = dimensions;
    for (axis, color) in colors.into_iter().enumerate() {
        let base = mesh.vertices.len();
        arrow::generate(mesh, length, stroke, 0.0)?;
        for vertex in &mut mesh.vertices[base..] {
            // Cyclic permutations rotate +Z to +X/+Y without changing winding.
            vertex.position.rotate_right((axis + 1) % 3);
            vertex.outward.rotate_right((axis + 1) % 3);
            vertex.color = color;
        }
    }
    Ok(())
}

fn parse_color(value: &str) -> Result<[f32; 3], ErrorReason> {
    let mut channels = value.split(',');
    let mut color = [0.0; 3];
    for channel in &mut color {
        let parsed: f64 = channels
            .next()
            .ok_or(ErrorReason::InvalidAsset)?
            .parse()
            .map_err(|_| ErrorReason::InvalidAsset)?;
        if !parsed.is_finite() || !(0.0..=1.0).contains(&parsed) {
            return Err(ErrorReason::InvalidAsset);
        }
        *channel = parsed as f32;
    }
    if channels.next().is_some() {
        return Err(ErrorReason::InvalidAsset);
    }
    Ok(color)
}
