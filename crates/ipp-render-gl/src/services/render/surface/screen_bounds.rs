//! Constant-size projection of the provider's analytic enclosure. This estimates
//! screen footprint, not the maximum local derivative of a curved mapping.
//! Occlusion and tessellation do not participate in image-quality selection.

use crate::RenderError;
use ipp_core::{systems::geometry::GeometryBounds, systems::surface::Surface};

pub(super) enum SurfaceScreenBounds {
    Invisible,
    Rect {
        min: [f64; 2],
        max: [f64; 2],
    },
    /// A visible enclosure crosses the eye plane. Avoid unstable division;
    /// request bounded maximum quality until its projection is well defined.
    EyePlane,
}

impl SurfaceScreenBounds {
    pub(super) fn coverage(&self) -> f64 {
        match self {
            Self::Invisible => 0.0,
            Self::EyePlane => 1.0,
            Self::Rect {
                min,
                max,
            } => {
                (max[0].min(1.0) - min[0].max(-1.0)).max(0.0)
                    * (max[1].min(1.0) - min[1].max(-1.0)).max(0.0)
                    * 0.25
            }
        }
    }
}

pub(super) fn surface(
    geometry: &dyn Surface,
    offsets: &[f64],
    mvp: [f32; 16],
) -> Result<SurfaceScreenBounds, RenderError> {
    let mut range = [0.0_f64; 2];
    if let Some(&first) = offsets.first() {
        range = [first; 2];
        for &offset in offsets {
            if !offset.is_finite() {
                return Err(RenderError::UnavailableOutput);
            }
            range = [range[0].min(offset), range[1].max(offset)];
        }
    }
    let bounds = geometry
        .bounds(range)
        .map_err(|_| RenderError::UnavailableOutput)?
        .bounds()
        .ok_or(RenderError::UnavailableOutput)?;
    project(bounds, mvp)
}

pub(super) fn plane(rect: [f32; 4], mvp: [f32; 16]) -> Result<SurfaceScreenBounds, RenderError> {
    let [left, top, right, bottom] = rect.map(f64::from);
    if left >= right || top >= bottom {
        return Ok(SurfaceScreenBounds::Invisible);
    }
    project([[left, top, 0.0], [right, bottom, 0.0]], mvp)
}

fn project(bounds: [[f64; 3]; 2], mvp: [f32; 16]) -> Result<SurfaceScreenBounds, RenderError> {
    #[cfg(feature = "instrumentation")]
    let _trace = ipp_core::profiling::CpuTraceScope::new("Surface.screen_bounds");

    let points: [[f64; 4]; 8] = std::array::from_fn(|corner| {
        let position: [f64; 3] = std::array::from_fn(|axis| bounds[(corner >> axis) & 1][axis]);
        std::array::from_fn(|row| {
            (0..3)
                .map(|col| f64::from(mvp[col * 4 + row]) * position[col])
                .sum::<f64>()
                + f64::from(mvp[12 + row])
        })
    });
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(RenderError::UnavailableOutput);
    }
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            if points.iter().all(|p| p[3] + sign * p[axis] < 0.0) {
                return Ok(SurfaceScreenBounds::Invisible);
            }
        }
    }
    if points.iter().any(|p| p[3] <= 1e-8) {
        return Ok(SurfaceScreenBounds::EyePlane);
    }

    // For positive W, each projected coordinate is linear-fractional on the
    // box, so its extrema occur at corners. Keep the full rectangle for density:
    // moving half a pane offscreen must not halve its visible texel density.
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for p in points {
        for axis in 0..2 {
            let value = p[axis] / p[3];
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    let projected = SurfaceScreenBounds::Rect {
        min,
        max,
    };
    Ok(if projected.coverage() == 0.0 {
        SurfaceScreenBounds::Invisible
    } else {
        projected
    })
}
