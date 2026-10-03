//! Shared linear mapping; algorithms own automatic-bound selection over visible series.

use super::{PlotFrame2d, PlotFrame3d};
use crate::ErrorReason;

/// Prepared local 2D mapping with one common data range for every visible series.
#[derive(Clone, Copy, Debug)]
pub struct PlotFrameMapping2d {
    min: [f64; 2],
    scale: [f64; 2],
    origin: [f64; 2],
    /// Cartesian paint rectangle in top-left/Y-down logical coordinates.
    pub rect: [f32; 4],
}

impl PlotFrameMapping2d {
    /// Prepare fixed or algorithm-selected automatic bounds, in data units.
    pub fn new(frame: &PlotFrame2d, min: [f64; 2], max: [f64; 2]) -> Result<Self, ErrorReason> {
        validate_ranges(&min, &max)?;
        let left = frame.padding_left;
        let top = frame.padding_top;
        let right = frame.width - frame.padding_right;
        let bottom = frame.height - frame.padding_bottom;
        if left < 0.0 || top < 0.0 || right <= left || bottom <= top {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(Self {
            min,
            scale: [
                f64::from(right - left) / (max[0] - min[0]),
                -f64::from(bottom - top) / (max[1] - min[1]),
            ],
            origin: [f64::from(left), f64::from(bottom)],
            rect: [left, top, right, bottom],
        })
    }

    /// Map one finite data point; data +Y points upwards inside the Canvas frame.
    pub fn map(&self, point: [f64; 2]) -> [f32; 2] {
        std::array::from_fn(|axis| {
            (self.origin[axis] + (point[axis] - self.min[axis]) * self.scale[axis]) as f32
        })
    }
}

/// Prepared shared entity-local physical mapping, +Y up and +Z depth.
#[derive(Clone, Copy, Debug)]
pub struct PlotFrameMapping3d {
    min: [f64; 3],
    scale: [f64; 3],
}

impl PlotFrameMapping3d {
    /// Prepare one common data range mapped to [0,size] on all three axes.
    pub fn new(frame: &PlotFrame3d, min: [f64; 3], max: [f64; 3]) -> Result<Self, ErrorReason> {
        validate_ranges(&min, &max)?;
        let size = frame.size();
        Ok(Self {
            min,
            scale: std::array::from_fn(|axis| f64::from(size[axis]) / (max[axis] - min[axis])),
        })
    }

    /// Map one finite data point to entity-local metres.
    pub fn map(&self, point: [f64; 3]) -> [f32; 3] {
        std::array::from_fn(|axis| ((point[axis] - self.min[axis]) * self.scale[axis]) as f32)
    }
}

fn validate_ranges<const N: usize>(min: &[f64; N], max: &[f64; N]) -> Result<(), ErrorReason> {
    if min
        .iter()
        .zip(max)
        .all(|(min, max)| min.is_finite() && max.is_finite() && min < max)
    {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}
