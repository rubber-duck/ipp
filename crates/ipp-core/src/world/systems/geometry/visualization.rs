use crate::{ErrorReason, components::schema::ComponentLifecycle};

/// Private tessellation parameters; geometry semantics remain in GeometryBounds.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryPrimitiveVisual {
    /// Shape selector: 0 cube, 1 sphere, 2 pill, 3 plane.
    pub shape: u32,
    /// Select the corresponding built-in contour instead of the solid form.
    pub outline: bool,
    /// Cube width in local metres.
    pub width: f32,
    /// Cube or total pill height in local metres.
    pub height: f32,
    /// Cube length in local metres.
    pub length: f32,
    /// Sphere or pill radius in local metres.
    pub radius: f32,
    /// GeometryPlane side length in local metres.
    pub size: f32,
    /// GeometryPlane normal arrow length in local metres.
    pub normal_length: f32,
    /// Outline tube diameter or plane arrow stroke in local metres.
    pub stroke: f32,
}

impl Default for GeometryPrimitiveVisual {
    fn default() -> Self {
        Self {
            shape: 0,
            outline: false,
            width: 2.0,
            height: 2.0,
            length: 2.0,
            radius: 1.0,
            size: 2.0,
            normal_length: 1.25,
            stroke: 0.04,
        }
    }
}

impl GeometryPrimitiveVisual {
    /// Prepare private mesh parameters and the exact evaluated placement.
    pub fn from_shape(
        part: &super::TransformedGeometryShape,
        outline: bool,
        stroke: f32,
    ) -> Option<(Self, [f32; 16])> {
        use super::{GeometryShape, GeometryShapeTransform, dot, scale, subtract};
        let mut geometry = Self {
            outline,
            stroke,
            ..Self::default()
        };
        let mut local = GeometryShapeTransform::default().matrix();
        match part.shape {
            GeometryShape::Box {
                min,
                max,
            } => {
                for axis in 0..3 {
                    local[axis * 4 + axis] = (max[axis] - min[axis]) * 0.5;
                    local[12 + axis] = (max[axis] + min[axis]) * 0.5;
                }
            }
            GeometryShape::Sphere {
                center,
                radius,
            } => {
                geometry.shape = 1;
                for axis in 0..3 {
                    local[axis * 4 + axis] = radius;
                    local[12 + axis] = center[axis];
                }
            }
            GeometryShape::Pill {
                start,
                end,
                radius,
            } => {
                let axis = subtract(end, start);
                let length = dot(axis, axis).sqrt();
                geometry.shape = if length == 0.0 {
                    1
                } else {
                    2
                };
                geometry.radius = radius as f32;
                geometry.height = (length + radius * 2.0) as f32;
                if length != 0.0 {
                    let y = scale(axis, 1.0 / length);
                    let reference = if y[0].abs() < 0.9 {
                        [1.0, 0.0, 0.0]
                    } else {
                        [0.0, 0.0, 1.0]
                    };
                    let mut x = subtract(reference, scale(y, dot(reference, y)));
                    let norm = dot(x, x).sqrt();
                    x = scale(x, 1.0 / norm);
                    let z = [
                        x[1] * y[2] - x[2] * y[1],
                        x[2] * y[0] - x[0] * y[2],
                        x[0] * y[1] - x[1] * y[0],
                    ];
                    for (col, values) in [x, y, z].into_iter().enumerate() {
                        local[col * 4..col * 4 + 3].copy_from_slice(&values);
                    }
                }
                for axis in 0..3 {
                    local[12 + axis] = (start[axis] + end[axis]) * 0.5;
                }
            }
        }
        // Visual meshes can be flat even though the entity's affine is invertible.
        let outer = part.transform.matrix();
        let combined = super::transform::multiply(outer, local);
        let mut model = [0.0; 16];
        for i in 0..16 {
            model[i] = combined[i] as f32;
            if !model[i].is_finite() {
                return None;
            }
        }
        geometry.validate_dimensions().ok()?;
        Some((geometry, model))
    }

    /// Validate shape dimensions without generating or allocating asset data.
    pub fn validate_dimensions(&self) -> Result<(), ErrorReason> {
        let positive = |value: f32| value.is_finite() && value > 0.0;
        let diameter = f64::from(self.radius) * 2.0;
        let extents = match self.shape {
            0 if [self.width, self.height, self.length]
                .into_iter()
                .all(|value| positive(value) && value * 0.5 > 0.0) =>
            {
                [
                    f64::from(self.width),
                    f64::from(self.height),
                    f64::from(self.length),
                ]
            }
            1 if positive(self.radius) && diameter <= f64::from(f32::MAX) => [diameter; 3],
            2 if positive(self.radius)
                && positive(self.height)
                && f64::from(self.height) >= diameter =>
            {
                [diameter, f64::from(self.height), diameter]
            }
            3 if positive(self.size) && positive(self.normal_length) => [
                f64::from(self.size),
                f64::from(self.normal_length),
                f64::from(self.size),
            ],
            _ => return Err(ErrorReason::InvalidValue),
        };
        if self.outline || self.shape == 3 {
            let divisor = if self.shape == 3 {
                8.0
            } else {
                4.0
            };
            if !positive(self.stroke)
                || extents.into_iter().any(|extent| {
                    f64::from(self.stroke) > extent / divisor
                        || extent + f64::from(self.stroke) > f64::from(f32::MAX)
                })
            {
                return Err(ErrorReason::InvalidValue);
            }
        }
        if self.shape == 3
            && arrow_shoulder(f64::from(self.normal_length), f64::from(self.stroke)).is_none()
        {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }

    /// Stable shape/parameter identity, excluding pose, color, and visibility.
    pub fn mesh_key(&self) -> [u32; 6] {
        let mut key = [
            self.shape,
            u32::from(self.outline),
            0,
            0,
            0,
            if self.outline || self.shape == 3 {
                self.stroke.to_bits()
            } else {
                0
            },
        ];
        match self.shape {
            0 => {
                key[2..5].copy_from_slice(&[
                    self.width.to_bits(),
                    self.height.to_bits(),
                    self.length.to_bits(),
                ]);
            }
            1 => {
                key[2] = self.radius.to_bits();
            }
            2 => {
                key[2] = self.radius.to_bits();
                key[3] = self.height.to_bits();
            }
            3 => {
                key[2] = self.size.to_bits();
                key[3] = self.normal_length.to_bits();
            }
            _ => {}
        }
        key
    }
}

impl ComponentLifecycle for GeometryPrimitiveVisual {
    fn validate(&self) -> Result<(), ErrorReason> {
        if [
            self.width,
            self.height,
            self.length,
            self.radius,
            self.size,
            self.normal_length,
            self.stroke,
        ]
        .iter()
        .any(|value| !value.is_finite())
        {
            return Err(ErrorReason::InvalidValue);
        }
        self.validate_dimensions()
    }
}

/// Scalar arrowhead construction shared by validation and actual tessellation.
/// The positive inputs have already passed their shape-specific range checks.
pub(crate) fn arrow_shoulder(length: f64, stroke: f64) -> Option<f64> {
    let head_length = (length * 0.25).min(stroke * 4.0);
    let shoulder = length - head_length;
    ((shoulder as f32) < length as f32).then_some(shoulder)
}
