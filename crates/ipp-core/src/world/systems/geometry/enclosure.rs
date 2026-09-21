//! Compact conservative results shared by spatial queries and lighting.

/// Prepared world-space box and its enclosing sphere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryEnclosure {
    /// Axis-aligned minimum and maximum corners.
    pub bounds: [[f64; 3]; 2],
    /// Center of the enclosing sphere.
    pub center: [f64; 3],
    /// Unsquared radius; sphere overlaps use the squared sum of radii.
    pub radius: f64,
}

impl GeometryEnclosure {
    /// Transform a local box directly, preserving affine shear and reflections.
    pub(super) fn transformed_box(
        min: [f64; 3],
        max: [f64; 3],
        matrix: &[f64; 16],
    ) -> Option<Self> {
        let mut bounds = [[0.0; 3]; 2];
        for axis in 0..3 {
            let mut lower = matrix[12 + axis];
            let mut upper = lower;
            for local in 0..3 {
                let a = min[local] * matrix[local * 4 + axis];
                let b = max[local] * matrix[local * 4 + axis];
                lower += a.min(b);
                upper += a.max(b);
            }
            bounds[0][axis] = lower;
            bounds[1][axis] = upper;
        }
        Self::new(bounds)
    }

    pub(super) fn new(bounds: [[f64; 3]; 2]) -> Option<Self> {
        if !(0..3).all(|i| {
            bounds[0][i].is_finite() && bounds[1][i].is_finite() && bounds[0][i] <= bounds[1][i]
        }) {
            return None;
        }
        let center = std::array::from_fn(|i| bounds[0][i] * 0.5 + bounds[1][i] * 0.5);
        let half: [f64; 3] = std::array::from_fn(|i| bounds[1][i] * 0.5 - bounds[0][i] * 0.5);
        let radius = half.iter().map(|value| value * value).sum::<f64>().sqrt();
        radius.is_finite().then_some(Self {
            bounds,
            center,
            radius,
        })
    }
}
