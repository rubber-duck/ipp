//! Immutable geometry definitions and their bounded portable encoding.

use super::{GeometryShape, GeometryShapeTransform};
use crate::{
    ErrorReason, components::Transform, components::schema::ComponentLifecycle,
    services::asset_management::*,
};

/// Immutable geometry resource type, independent of mesh GPU residency.
pub const GEOMETRY_TYPE: AssetTypeId = AssetTypeId(6);

/// Authored shape placement; joint pairs apply only to pill endpoints.
#[derive(Clone, Debug, PartialEq)]
pub struct GeometryShapePart {
    /// Primitive coordinates in the part's local space.
    pub shape: GeometryShape,
    /// Local placement composed with the entity or skeleton placement.
    pub transform: Transform,
    /// SkeletonJoint ordinals whose origins define a pill's central segment.
    pub joints: Option<[u32; 2]>,
}

impl From<GeometryShape> for GeometryShapePart {
    fn from(shape: GeometryShape) -> Self {
        Self {
            shape,
            transform: Transform::default(),
            joints: None,
        }
    }
}

/// Shared definition; one part is basic geometry, several form a compound union.
#[derive(Clone, Debug, PartialEq)]
pub struct GeometryDefinition {
    /// Stable part order is also the picking result's part identity.
    pub parts: Vec<GeometryShapePart>,
}

impl From<GeometryShape> for GeometryDefinition {
    fn from(shape: GeometryShape) -> Self {
        Self {
            parts: vec![shape.into()],
        }
    }
}

impl GeometryDefinition {
    /// IPPG v1: header and fixed 80-byte records; floats are little-endian f32.
    pub fn encode(&self) -> Result<Vec<u8>, ErrorReason> {
        self.validate()?;
        let length = self
            .parts
            .len()
            .checked_mul(80)
            .and_then(|bytes| bytes.checked_add(12))
            .ok_or(ErrorReason::Capacity)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| ErrorReason::Capacity)?;
        bytes.extend_from_slice(b"IPPG");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(self.parts.len() as u32).to_le_bytes());
        for part in &self.parts {
            let (tag, values) = match part.shape {
                GeometryShape::Box {
                    min,
                    max,
                } => (0u32, [min[0], min[1], min[2], max[0], max[1], max[2], 0.0]),
                GeometryShape::Sphere {
                    center,
                    radius,
                } => (1, [center[0], center[1], center[2], radius, 0.0, 0.0, 0.0]),
                GeometryShape::Pill {
                    start,
                    end,
                    radius,
                } => (
                    2,
                    [start[0], start[1], start[2], end[0], end[1], end[2], radius],
                ),
            };
            bytes.extend_from_slice(&tag.to_le_bytes());
            for value in values {
                let value = value as f32;
                if !value.is_finite() {
                    return Err(ErrorReason::InvalidGeometry);
                }
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            let t = part.transform;
            for value in [t.x, t.y, t.z, t.qx, t.qy, t.qz, t.qw, t.sx, t.sy, t.sz] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            for joint in part.joints.unwrap_or([u32::MAX; 2]) {
                bytes.extend_from_slice(&joint.to_le_bytes());
            }
        }
        // Validate after f32 conversion too: tiny positive radii may underflow.
        Self::decode(&bytes)?;
        Ok(bytes)
    }

    /// Decode a complete definition; truncated, oversized and invalid parts fail.
    pub fn decode(bytes: &[u8]) -> Result<Self, ErrorReason> {
        let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        if bytes.len() < 12 || &bytes[..4] != b"IPPG" || word(4) != 1 {
            return Err(ErrorReason::InvalidGeometry);
        }
        let count = word(8) as usize;
        if count == 0
            || count.checked_mul(80).and_then(|size| size.checked_add(12)) != Some(bytes.len())
        {
            return Err(ErrorReason::InvalidGeometry);
        }
        let mut parts = Vec::new();
        parts
            .try_reserve_exact(count)
            .map_err(|_| ErrorReason::Capacity)?;
        for index in 0..count {
            let base = 12 + index * 80;
            let v: [f64; 7] =
                std::array::from_fn(|i| f64::from(f32::from_bits(word(base + 4 + i * 4))));
            let shape = match word(base) {
                0 if v[6] == 0.0 => GeometryShape::Box {
                    min: [v[0], v[1], v[2]],
                    max: [v[3], v[4], v[5]],
                },
                1 if v[4..].iter().all(|&v| v == 0.0) => GeometryShape::Sphere {
                    center: [v[0], v[1], v[2]],
                    radius: v[3],
                },
                2 => GeometryShape::Pill {
                    start: [v[0], v[1], v[2]],
                    end: [v[3], v[4], v[5]],
                    radius: v[6],
                },
                _ => return Err(ErrorReason::InvalidGeometry),
            };
            let t: [f32; 10] = std::array::from_fn(|i| f32::from_bits(word(base + 32 + i * 4)));
            let joints = [word(base + 72), word(base + 76)];
            parts.push(GeometryShapePart {
                shape,
                transform: Transform {
                    x: t[0],
                    y: t[1],
                    z: t[2],
                    qx: t[3],
                    qy: t[4],
                    qz: t[5],
                    qw: t[6],
                    sx: t[7],
                    sy: t[8],
                    sz: t[9],
                },
                joints: (joints != [u32::MAX; 2]).then_some(joints),
            });
        }
        let definition = Self {
            parts,
        };
        definition.validate()?;
        Ok(definition)
    }

    /// Check authored values independently of resource or skeleton readiness.
    pub fn validate(&self) -> Result<(), ErrorReason> {
        if self.parts.is_empty() || u32::try_from(self.parts.len()).is_err() {
            return Err(ErrorReason::InvalidGeometry);
        }
        for part in &self.parts {
            part.shape.validate()?;
            part.transform.validate()?;
            GeometryShapeTransform::from_matrix(crate::systems::camera::model_matrix(
                &part.transform,
            )?)?;
            if let Some(joints) = part.joints
                && (!cfg!(feature = "skeletal-animation")
                    || joints[0] >= 32
                    || joints[1] >= 32
                    || !matches!(part.shape, GeometryShape::Pill { start, end, .. } if start == [0.0; 3] && end == [0.0; 3]))
            {
                return Err(ErrorReason::InvalidGeometry);
            }
        }
        Ok(())
    }
}

impl Asset for GeometryDefinition {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn decoded(&self) -> &dyn std::any::Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        std::mem::size_of_val(self.parts.as_slice())
    }
}

pub(crate) fn geometry_asset_loader() -> impl AssetLoader<Data = GeometryDefinition> {
    BufferedAssetLoader::new(|bytes| {
        GeometryDefinition::decode(bytes).map_err(|error| error.to_string())
    })
}

impl crate::services::asset_management::writer::AssetEncoder for GeometryDefinition {
    fn encode_asset(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        if 12 + self.parts.len() * 80 > max_bytes {
            return Err("Asset output byte budget exhausted".into());
        }
        self.encode().map_err(|error| error.to_string())
    }
}
