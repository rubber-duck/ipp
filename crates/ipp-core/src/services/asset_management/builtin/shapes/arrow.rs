//! A closed +Z arrow shared by standalone, plane-normal and axis recipes.

use super::{BuiltinMesh, ErrorReason, TUBE_SIDES, Z, circle_sample, cylinder};

#[cfg(feature = "builtin-assets")]
pub(super) fn recipe(mesh: &mut BuiltinMesh, uri: &str) -> Result<(), ErrorReason> {
    let [length, stroke] = super::dimensions(uri, "ipp://mesh/arrow?", ["length", "stroke"])?;
    generate(mesh, length, stroke, 0.0)
}

pub(super) fn generate(
    mesh: &mut BuiltinMesh,
    length: f64,
    stroke: f64,
    offset: f64,
) -> Result<(), ErrorReason> {
    if stroke > length / 8.0 {
        return Err(ErrorReason::InvalidAsset);
    }

    let shoulder = offset
        + crate::systems::geometry::arrow_shoulder(length, stroke)
            .ok_or(ErrorReason::InvalidAsset)?;
    let tip = offset + length;
    if tip > f64::from(f32::MAX)
        || offset as f32 >= shoulder as f32
        || shoulder as f32 >= tip as f32
    {
        return Err(ErrorReason::InvalidAsset);
    }

    cylinder(mesh, [0.0, 0.0, offset], [0.0, 0.0, shoulder], 2, stroke)?;
    cone(mesh, shoulder, tip, stroke * 2.0)
}

fn cone(mesh: &mut BuiltinMesh, shoulder: f64, tip: f64, radius: f64) -> Result<(), ErrorReason> {
    let base = mesh.vertices.len();
    for side in 0..=TUBE_SIDES {
        let (sin, cos) = circle_sample(side, TUBE_SIDES);
        mesh.vertex(
            [radius * cos, radius * sin, shoulder],
            if mesh.normals {
                [(tip - shoulder) * cos, (tip - shoulder) * sin, radius]
            } else {
                [cos, sin, 0.0]
            },
            [0.5 + 0.5 * cos, 0.5 - 0.5 * sin],
        )?;
    }
    let apex = mesh.vertices.len();
    if mesh.normals {
        for side in 0..TUBE_SIDES {
            let (sin, cos) = circle_sample(side * 2 + 1, TUBE_SIDES * 2);
            mesh.vertex(
                [0.0, 0.0, tip],
                [(tip - shoulder) * cos, (tip - shoulder) * sin, radius],
                [0.5, 0.5],
            )?;
        }
    } else {
        mesh.vertex([0.0, 0.0, tip], Z, [0.5, 0.5])?;
    }
    let cap_rim = if mesh.normals {
        let duplicate = mesh.vertices.len();
        for side in 0..=TUBE_SIDES {
            let vertex = &mesh.vertices[base + side];
            mesh.vertex(
                vertex.position.map(f64::from),
                [0.0, 0.0, -1.0],
                vertex.uv.map(f64::from),
            )?;
        }
        duplicate
    } else {
        base
    };
    let cap = mesh.vertex([0.0, 0.0, shoulder], [0.0, 0.0, -1.0], [0.5, 0.5])?;

    for side in 0..TUBE_SIDES {
        mesh.triangle([
            apex + if mesh.normals {
                side
            } else {
                0
            },
            base + side,
            base + side + 1,
        ])?;
        mesh.triangle([cap, cap_rim + side + 1, cap_rim + side])?;
    }
    Ok(())
}
