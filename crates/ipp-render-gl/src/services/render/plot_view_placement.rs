//! Selected-camera stations over retained Cartesian paint. Bases and increasing
//! data coordinates never reflect; composed chart depth chooses enclosure sides.

use super::RenderError;
use ipp_core::systems::plot::PlotPlanePlacement;

pub(super) fn point(model: [f32; 16], local: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| {
        model[12 + row]
            + (0..3)
                .map(|col| model[col * 4 + row] * local[col])
                .sum::<f32>()
    })
}

pub(super) fn far_bounds(chart: [f32; 16], camera: [f32; 16], extent: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|axis| {
        let depth = (0..3)
            .map(|row| chart[axis * 4 + row] * camera[8 + row])
            .sum::<f32>();
        if depth < 0.0 {
            extent[axis]
        } else {
            0.0
        }
    })
}

pub(super) fn model(
    mut plane: [f32; 16],
    chart: [f32; 16],
    camera: [f32; 16],
    placement: PlotPlanePlacement,
) -> Result<[f32; 16], RenderError> {
    let (extent, moved): ([f32; 3], [bool; 3]) = match placement {
        PlotPlanePlacement::Grid {
            extent,
            normal,
        } => (
            extent,
            std::array::from_fn(|axis| axis == usize::from(normal)),
        ),
        PlotPlanePlacement::Axis {
            extent,
            axis,
        } => (
            extent,
            std::array::from_fn(|other| other != usize::from(axis)),
        ),
        _ => return Ok(plane),
    };
    let mut far = far_bounds(chart, camera, extent);
    // Grids form the far enclosure. Floor axes use the visible outer edges;
    // their ticks move with them. The upright axis and all its text share the
    // left projected enclosure edge rather than separating a rear axis from
    // its outward label lane.
    if let PlotPlanePlacement::Axis {
        axis,
        ..
    } = placement
    {
        match axis {
            0 => far[2] = extent[2] - far[2],
            1 => {
                // A support direction perpendicular to projected +Y selects one
                // common vertical edge, including composed chart rotations and
                // camera roll. Camera columns are right/up in world space.
                let dot = |axis: usize, column: usize| {
                    (0..3)
                        .map(|row| chart[axis * 4 + row] * camera[column + row])
                        .sum::<f32>()
                };
                let right_y = dot(1, 0);
                let up_y = dot(1, 4);
                for other in [0, 2] {
                    let side = dot(other, 0) * up_y - dot(other, 4) * right_y;
                    far[other] = if side < 0.0 {
                        extent[other]
                    } else {
                        0.0
                    };
                }
            }
            2 => far[0] = extent[0] - far[0],
            _ => {}
        }
    }
    for row in 0..3 {
        // All retained movable stations start at zero on their movable axes.
        plane[12 + row] += (0..3)
            .filter(|&axis| moved[axis])
            .map(|axis| chart[axis * 4 + row] * far[axis])
            .sum::<f32>();
    }
    if plane.iter().all(|value| value.is_finite()) {
        Ok(plane)
    } else {
        Err(RenderError::InvalidTransform)
    }
}

#[cfg(test)]
#[path = "plot_view_placement_tests.rs"]
mod tests;
