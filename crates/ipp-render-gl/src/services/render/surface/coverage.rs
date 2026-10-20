//! Projected screen fraction for allocation priority, independent of raster
//! magnification. Clip before division by W; neither depth occlusion nor camera
//! distance affects the score. Separated shells contribute their largest area,
//! rather than multiplying a Surface's priority by its image count.

use crate::RenderError;
use ipp_core::systems::surface::Surface;

pub(in crate::services::render) fn surface(
    geometry: &dyn Surface,
    offsets: &[f64],
    mvp: [f32; 16],
) -> Result<f64, RenderError> {
    let extent = geometry.physical_extent();
    let mut coverage = 0.0_f64;
    for &offset in offsets.iter().take(32) {
        if let Some(affine) = geometry.exact_affine(offset) {
            coverage = coverage.max(plane(
                [0.0, 0.0, extent[0] as f32, extent[1] as f32],
                ipp_core::math::multiply(mvp, affine.map(|v| v as f32)),
            ));
            continue;
        }
        const CELLS: usize = 16;
        let mut points = Vec::with_capacity((CELLS + 1).pow(2));
        for y in 0..=CELLS {
            for x in 0..=CELLS {
                let position = geometry
                    .sample(
                        [
                            extent[0] * x as f64 / CELLS as f64,
                            extent[1] * y as f64 / CELLS as f64,
                        ],
                        offset,
                    )
                    .map_err(|_| RenderError::UnavailableOutput)?
                    .position;
                points.push(project(position, mvp));
            }
        }
        let mut area = 0.0;
        for y in 0..CELLS {
            for x in 0..CELLS {
                let top = y * (CELLS + 1) + x;
                let bottom = top + CELLS + 1;
                area += triangle([points[top], points[top + 1], points[bottom]]);
                area += triangle([points[bottom + 1], points[bottom], points[top + 1]]);
            }
        }
        coverage = coverage.max(area);
    }
    Ok(coverage.clamp(0.0, 1.0))
}

/// A Canvas slot's local clip, including ancestor clipping, under its composed
/// content-to-clip transform. Small overlays count only their occupied rectangle.
pub(in crate::services::render) fn plane(clip: [f32; 4], mvp: [f32; 16]) -> f64 {
    let [left, top, right, bottom] = clip.map(f64::from);
    if left >= right || top >= bottom {
        return 0.0;
    }
    let points = [
        [left, top, 0.0],
        [right, top, 0.0],
        [left, bottom, 0.0],
        [right, bottom, 0.0],
    ]
    .map(|p| project(p, mvp));
    (triangle([points[0], points[1], points[2]]) + triangle([points[3], points[2], points[1]]))
        .clamp(0.0, 1.0)
}

fn project(position: [f64; 3], mvp: [f32; 16]) -> [f64; 4] {
    std::array::from_fn(|row| {
        (0..3)
            .map(|col| f64::from(mvp[col * 4 + row]) * position[col])
            .sum::<f64>()
            + f64::from(mvp[12 + row])
    })
}

fn triangle(points: [[f64; 4]; 3]) -> f64 {
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return 0.0;
    }
    let (polygon, count) = super::quality::visible_triangle(points);
    if count < 3 || polygon[..count].iter().any(|p| p[3] <= 1e-8) {
        return 0.0;
    }
    let mut twice_area = 0.0;
    for i in 0..count {
        let a = polygon[i];
        let b = polygon[(i + 1) % count];
        twice_area += (a[0] * b[1] - b[0] * a[1]) / (a[3] * b[3]);
    }
    // NDC spans [-1, 1] in both axes: screen area is four square units.
    twice_area.abs() / 8.0
}

#[cfg(test)]
#[path = "coverage_tests.rs"]
mod tests;
