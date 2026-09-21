//! A closed cone centred on the origin, with its apex along +Y.

use super::{
    BuiltinMesh, ErrorReason, LONGITUDES, X, Z, circle_sample, diameter, oriented_cylinder,
    ring_with_segments, validate_stroke,
};

pub(super) fn generate(
    mesh: &mut BuiltinMesh,
    radius: f64,
    height: f64,
) -> Result<(), ErrorReason> {
    let half = height * 0.5;
    if (half as f32) == 0.0 {
        return Err(ErrorReason::InvalidAsset);
    }

    let rim = mesh.vertices.len();
    for side in 0..=LONGITUDES {
        let (sin, cos) = circle_sample(side, LONGITUDES);
        mesh.vertex(
            [radius * cos, -half, radius * sin],
            [height * cos, radius, height * sin],
            [side as f64 / LONGITUDES as f64, 1.0],
        )?;
    }

    // The apex has no unique normal or longitude. Give each wedge its own
    // midpoint sample, keeping interpolation within that wedge's UV interval.
    let apex = mesh.vertices.len();
    for side in 0..LONGITUDES {
        let (sin, cos) = circle_sample(side * 2 + 1, LONGITUDES * 2);
        mesh.vertex(
            [0.0, half, 0.0],
            [height * cos, radius, height * sin],
            [(side as f64 + 0.5) / LONGITUDES as f64, 0.0],
        )?;
    }

    // Separate cap vertices preserve the hard edge and planar disk UVs.
    let cap_rim = mesh.vertices.len();
    for side in 0..=LONGITUDES {
        let (sin, cos) = circle_sample(side, LONGITUDES);
        mesh.vertex(
            [radius * cos, -half, radius * sin],
            [0.0, -1.0, 0.0],
            [0.5 + 0.5 * cos, 0.5 - 0.5 * sin],
        )?;
    }
    let cap = mesh.vertex([0.0, -half, 0.0], [0.0, -1.0, 0.0], [0.5, 0.5])?;

    for side in 0..LONGITUDES {
        mesh.triangle([apex + side, rim + side + 1, rim + side])?;
        mesh.triangle([cap, cap_rim + side, cap_rim + side + 1])?;
    }
    Ok(())
}

pub(super) fn outline_recipe(mesh: &mut BuiltinMesh, uri: &str) -> Result<(), ErrorReason> {
    let args = super::super::arguments(
        uri,
        "ipp://mesh/cone-outline?",
        ["radius", "height", "stroke", "rings"],
    )?;
    let mut dimensions = [0.0; 3];
    for (dimension, value) in dimensions.iter_mut().zip(&args) {
        let parsed: f32 = value.parse().map_err(|_| ErrorReason::InvalidAsset)?;
        if !parsed.is_finite() || parsed <= 0.0 {
            return Err(ErrorReason::InvalidAsset);
        }
        *dimension = f64::from(parsed);
    }
    if !args[3].bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ErrorReason::InvalidAsset);
    }
    let rings: usize = args[3].parse().map_err(|_| ErrorReason::InvalidAsset)?;
    // Fixed tessellation defines the supported ring detail.
    if rings > 16 {
        return Err(ErrorReason::InvalidAsset);
    }
    let [radius, height, stroke] = dimensions;
    validate_stroke([diameter(radius)?, height, diameter(radius)?], stroke)?;
    // The smallest ring must retain an open interior; neighboring rings must
    // remain separate even when the caller requests the maximum subdivision.
    let section = 1.0 / (rings + 1) as f64;
    if rings > 0 && (stroke > radius * section || stroke > height * section * 0.5) {
        return Err(ErrorReason::InvalidAsset);
    }

    let half = height * 0.5;
    let slant = radius.hypot(height);
    for side in 0..4 {
        let (sin, cos) = circle_sample(side, 4);
        oriented_cylinder(
            mesh,
            [radius * cos, -half, radius * sin],
            [0.0, half, 0.0],
            [-radius * cos / slant, height / slant, -radius * sin / slant],
            [sin, 0.0, -cos],
            stroke,
        )?;
    }

    // Interior rings retain their spacing; the final cross-section is always
    // the base rim, including when no interior subdivisions are requested.
    for ring in 1..=rings + 1 {
        let fraction = ring as f64 * section;
        ring_with_segments(
            mesh,
            radius * fraction,
            [0.0, half - height * fraction, 0.0],
            [X, Z],
            stroke,
            LONGITUDES,
        )?;
    }
    Ok(())
}
