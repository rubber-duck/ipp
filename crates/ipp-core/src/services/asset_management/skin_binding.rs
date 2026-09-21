//! Mesh-specific inverse binds, separate from reusable skeleton assets.

use crate::{
    ErrorReason,
    services::asset_management::skeleton::{MAX_JOINTS, header, source_asset, u32_at},
    services::asset_management::*,
};

/// Immutable skin binding resource type.
pub const SKIN_TYPE: AssetTypeId = AssetTypeId(5);

/// One palette entry mapping mesh indices to skeleton joint order.
#[derive(Clone, Debug)]
pub struct SkinJoint {
    /// SkeletonJoint ordinal in the selected skeleton.
    pub joint: usize,
    /// Column-major mesh bind-space to joint bind-space transformation.
    pub inverse_bind: [f32; 16],
}

/// Immutable mesh-to-skeleton joint mapping and inverse-bind matrices.
#[derive(Debug)]
pub struct SkinAsset {
    joints: Vec<SkinJoint>,
}

impl SkinAsset {
    /// Palette order addressed by the mesh's joint-index stream.
    pub fn joints(&self) -> &[SkinJoint] {
        &self.joints
    }

    /// IPPB v1: magic, version/count u32, then joint u32 + 16 column-major f32.
    pub fn decode(bytes: &[u8]) -> Result<Self, ErrorReason> {
        let count = header(bytes, b"IPPB", 68)?;
        let mut joints = Vec::with_capacity(count);
        for bytes in bytes[12..].as_chunks::<68>().0 {
            let joint = u32_at(bytes, 0) as usize;
            let inverse_bind = std::array::from_fn(|i| f32::from_bits(u32_at(bytes, 4 + i * 4)));
            if joint >= MAX_JOINTS
                || !inverse_bind.iter().all(|v| v.is_finite())
                || [
                    inverse_bind[3],
                    inverse_bind[7],
                    inverse_bind[11],
                    inverse_bind[15],
                ] != [0.0, 0.0, 0.0, 1.0]
                || !crate::systems::camera::invertible(inverse_bind)
            {
                return Err(ErrorReason::InvalidAsset);
            }
            joints.push(SkinJoint {
                joint,
                inverse_bind,
            });
        }
        Ok(Self {
            joints,
        })
    }
}

source_asset!(SkinAsset, joints, skin_asset_loader);

impl super::writer::AssetEncoder for SkinAsset {
    fn encode_asset(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        let length = 12 + self.joints.len() * 68;
        if length > max_bytes {
            return Err("Skin output byte budget exhausted".into());
        }
        let mut bytes = Vec::with_capacity(length);
        bytes.extend_from_slice(b"IPPB");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(self.joints.len() as u32).to_le_bytes());
        for joint in &self.joints {
            bytes.extend_from_slice(&(joint.joint as u32).to_le_bytes());
            for value in joint.inverse_bind {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        Ok(bytes)
    }
}
