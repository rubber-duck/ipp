//! Paint-ordered solid drawing layers over shared quadratic contours.

use super::{Asset, AssetLoader, AssetTypeId, BufferedAssetLoader, quadratic::Decoder};
use crate::ErrorReason;

/// Compiled identity for portable IPPD drawing assets.
pub const DRAWING_TYPE: AssetTypeId = AssetTypeId(18);

/// Winding rule applied independently to one paint layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    /// Nonzero winding fill.
    NonZero,
    /// Even-odd winding fill.
    EvenOdd,
}

/// One solid layer in source painter's order.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawingLayer {
    /// sRGB color channels and linear alpha.
    pub color: [u8; 4],
    /// Winding rule for these contours.
    pub fill_rule: FillRule,
    /// Closed contours after offline stroke expansion and curve conversion.
    pub contours: Vec<super::quadratic::QuadraticContour>,
}

/// Immutable decoded drawing independent of renderer acceleration data.
#[derive(Debug)]
pub struct DrawingAsset {
    view_box: [f32; 4],
    bounds: [f32; 4],
    quadratic_tolerance: f32,
    layers: Vec<DrawingLayer>,
}

impl DrawingAsset {
    /// Normalized source view box `[min_x, min_y, max_x, max_y]`, Y up.
    pub fn view_box(&self) -> [f32; 4] {
        self.view_box
    }

    /// Converted control-point enclosure in Y-up coordinates.
    pub fn bounds(&self) -> [f32; 4] {
        self.bounds
    }

    /// Maximum source-space cubic/arc to quadratic conversion error.
    pub fn quadratic_tolerance(&self) -> f32 {
        self.quadratic_tolerance
    }

    /// Solid layers in painter's order.
    pub fn layers(&self) -> &[DrawingLayer] {
        &self.layers
    }

    /// Validate and decode one complete IPPD version 1 payload.
    pub fn decode(bytes: &[u8]) -> Result<Self, ErrorReason> {
        if bytes.len() < 48 || &bytes[..4] != b"IPPD" {
            return Err(ErrorReason::InvalidAsset);
        }
        let mut decoder = Decoder::new(bytes, 4);
        if decoder.u32()? != 1 {
            return Err(ErrorReason::InvalidAsset);
        }
        let view_box = decoder.bounds()?;
        let bounds = decoder.bounds()?;
        let quadratic_tolerance = decoder.f32()?;
        if quadratic_tolerance <= 0.0 {
            return Err(ErrorReason::InvalidAsset);
        }
        let layer_count = decoder.u32()?;
        if layer_count as usize > decoder.remaining() / 12 {
            return Err(ErrorReason::InvalidAsset);
        }
        let mut layers = Vec::new();
        layers
            .try_reserve_exact(layer_count as usize)
            .map_err(|_| ErrorReason::Capacity)?;
        for _ in 0..layer_count {
            let color: [u8; 4] = decoder.take(4)?.try_into().unwrap();
            let fill_rule = match decoder.u8()? {
                0 => FillRule::NonZero,
                1 => FillRule::EvenOdd,
                _ => return Err(ErrorReason::InvalidAsset),
            };
            if decoder.take(3)? != [0, 0, 0] {
                return Err(ErrorReason::InvalidAsset);
            }
            let contour_count = decoder.u32()?;
            layers.push(DrawingLayer {
                color,
                fill_rule,
                contours: decoder.contours(contour_count)?,
            });
        }
        if decoder.position() != bytes.len() {
            return Err(ErrorReason::InvalidAsset);
        }
        Ok(Self {
            view_box,
            bounds,
            quadratic_tolerance,
            layers,
        })
    }
}

impl Asset for DrawingAsset {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn decoded(&self) -> &dyn std::any::Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        std::mem::size_of_val(self.layers.as_slice())
            + self
                .layers
                .iter()
                .flat_map(|layer| &layer.contours)
                .map(|contour| std::mem::size_of_val(contour.segments.as_slice()))
                .sum::<usize>()
    }
}

/// Construct a streaming buffered CPU loader.
pub fn cpu_drawing_loader() -> impl AssetLoader<Data = DrawingAsset> {
    BufferedAssetLoader::new(|bytes| DrawingAsset::decode(bytes).map_err(|error| error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle() -> Vec<u8> {
        let mut bytes = b"IPPD".to_vec();
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        for value in [0.0_f32, 0.0, 10.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.05] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&[10, 20, 30, 128, 1, 0, 0, 0]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        for point in [[10.0_f32, 0.0], [0.0, 10.0], [0.0, 0.0]] {
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            for value in point {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn decodes_paint_order_and_closed_lines() {
        let drawing = DrawingAsset::decode(&triangle()).unwrap();

        assert_eq!(drawing.layers()[0].color, [10, 20, 30, 128]);
        assert_eq!(drawing.layers()[0].fill_rule, FillRule::EvenOdd);
        assert_eq!(drawing.layers()[0].contours[0].segments.len(), 3);
    }

    #[test]
    fn rejects_unknown_fill_rule_and_truncation() {
        let mut unknown = triangle();
        unknown[52] = 2;
        assert!(DrawingAsset::decode(&unknown).is_err());

        let mut truncated = triangle();
        truncated.pop();
        assert!(DrawingAsset::decode(&truncated).is_err());
    }
}
