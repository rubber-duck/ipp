//! Renderer-independent closed quadratic contours used by fonts and drawings.

/// A finite line or quadratic Bezier ending at `to`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuadraticSegment {
    /// A straight edge from the previous endpoint.
    Line {
        /// Segment endpoint.
        to: [f32; 2],
    },
    /// A quadratic Bezier from the previous endpoint.
    Quadratic {
        /// Quadratic control point.
        control: [f32; 2],
        /// Segment endpoint.
        to: [f32; 2],
    },
}

/// One closed contour. The final segment ends at `start` implicitly when needed.
#[derive(Clone, Debug, PartialEq)]
pub struct QuadraticContour {
    /// First point in the contour.
    pub start: [f32; 2],
    /// Ordered edges; closure to `start` is implicit.
    pub segments: Vec<QuadraticSegment>,
}

pub(super) struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    pub(super) fn new(bytes: &'a [u8], offset: usize) -> Self {
        Self {
            bytes,
            offset,
        }
    }

    pub(super) fn position(&self) -> usize {
        self.offset
    }

    pub(super) fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    pub(super) fn take(&mut self, length: usize) -> Result<&'a [u8], crate::ErrorReason> {
        let end = self
            .offset
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(crate::ErrorReason::InvalidAsset)?;
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    pub(super) fn u8(&mut self) -> Result<u8, crate::ErrorReason> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u32(&mut self) -> Result<u32, crate::ErrorReason> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(super) fn f32(&mut self) -> Result<f32, crate::ErrorReason> {
        let value = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        if !value.is_finite() {
            return Err(crate::ErrorReason::InvalidAsset);
        }
        Ok(value)
    }

    pub(super) fn point(&mut self) -> Result<[f32; 2], crate::ErrorReason> {
        Ok([self.f32()?, self.f32()?])
    }

    pub(super) fn bounds(&mut self) -> Result<[f32; 4], crate::ErrorReason> {
        let bounds = [self.f32()?, self.f32()?, self.f32()?, self.f32()?];
        if bounds[0] > bounds[2] || bounds[1] > bounds[3] {
            return Err(crate::ErrorReason::InvalidAsset);
        }
        Ok(bounds)
    }

    pub(super) fn contours(
        &mut self,
        count: u32,
    ) -> Result<Vec<QuadraticContour>, crate::ErrorReason> {
        let mut contours = Vec::new();
        contours
            .try_reserve_exact(count as usize)
            .map_err(|_| crate::ErrorReason::Capacity)?;
        for _ in 0..count {
            let start = self.point()?;
            let segment_count = self.u32()?;
            if segment_count == 0 || segment_count as usize > self.remaining() / 12 {
                return Err(crate::ErrorReason::InvalidAsset);
            }
            let mut segments = Vec::new();
            segments
                .try_reserve_exact(segment_count as usize)
                .map_err(|_| crate::ErrorReason::Capacity)?;
            for _ in 0..segment_count {
                let kind = self.u8()?;
                if self.take(3)? != [0, 0, 0] {
                    return Err(crate::ErrorReason::InvalidAsset);
                }
                let segment = match kind {
                    0 => QuadraticSegment::Line {
                        to: self.point()?,
                    },
                    1 => QuadraticSegment::Quadratic {
                        control: self.point()?,
                        to: self.point()?,
                    },
                    _ => return Err(crate::ErrorReason::InvalidAsset),
                };
                segments.push(segment);
            }
            contours.push(QuadraticContour {
                start,
                segments,
            });
        }
        Ok(contours)
    }
}
