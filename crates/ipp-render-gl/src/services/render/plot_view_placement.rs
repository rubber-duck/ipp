//! Selected-camera stations over retained Cartesian paint. Bases and increasing
//! data coordinates never reflect; composed chart depth chooses enclosure sides.

use super::RenderError;
use ipp_core::systems::plot::PlotPlanePlacement;

// Composed f32 rotations can put the same edge-on view on either side of zero.
// Classify directions, not world distances: this angular tie band is invariant
// under chart/camera scale and comfortably covers composition roundoff.
const SUPPORT_TIE: f64 = 64.0 * f32::EPSILON as f64;

fn direction(matrix: [f32; 16], column: usize) -> [f64; 3] {
    let vector = std::array::from_fn(|row| f64::from(matrix[column + row]));
    let length = vector[0].hypot(vector[1]).hypot(vector[2]);
    if length > 0.0 {
        vector.map(|value| value / length)
    } else {
        [0.0; 3]
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|axis| a[axis] * b[axis]).sum()
}

pub(super) fn point(model: [f32; 16], local: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| {
        model[12 + row]
            + (0..3)
                .map(|col| model[col * 4 + row] * local[col])
                .sum::<f32>()
    })
}

pub(super) fn far_bounds(chart: [f32; 16], camera: [f32; 16], extent: [f32; 3]) -> [f32; 3] {
    let back = direction(camera, 8);
    std::array::from_fn(|axis| {
        // Indistinguishable far faces use local zero deterministically. The
        // floor axes take the opposite face, so their labels cannot jump to
        // the other side of the plot under an edge-on camera perturbation.
        if dot(direction(chart, axis * 4), back) < -SUPPORT_TIE {
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
                let right = direction(camera, 0);
                let up = direction(camera, 4);
                let back = direction(camera, 8);
                let y = direction(chart, 4);
                let right_y = dot(y, right);
                let up_y = dot(y, up);
                for other in [0, 2] {
                    let vector = direction(chart, other * 4);
                    let side = dot(vector, right) * up_y - dot(vector, up) * right_y;
                    // When both uprights have the same projected support, put
                    // the axis and its text on the near edge, where the data
                    // cannot hide the line behind its outward label lane. A
                    // depth tie (including a collapsed basis) uses local zero.
                    let high = side < -SUPPORT_TIE
                        || (side.abs() <= SUPPORT_TIE && dot(vector, back) > SUPPORT_TIE);
                    far[other] = if high {
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
