//! Immutable rig sources and bounded joint-local pose decoding.

use crate::{
    ErrorReason, components::Transform, components::schema::ComponentLifecycle,
    services::asset_management::*,
};

pub use super::MAX_JOINTS;

/// Immutable hierarchy and rest-pose resource type.
pub const SKELETON_TYPE: AssetTypeId = AssetTypeId(3);

/// Immutable joint-local pose resource type.
pub const POSE_TYPE: AssetTypeId = AssetTypeId(4);

/// A stable ordinal joint mapping. Parents precede children; roots use None.
#[derive(Clone, Debug, PartialEq)]
pub struct SkeletonJoint {
    /// Parent in this asset's stable joint order.
    pub parent: Option<usize>,
    /// Rest transform relative to the parent (or skeleton for roots).
    pub rest: Transform,
}

/// Validated immutable joint hierarchy and rest pose.
#[derive(Debug)]
pub struct SkeletonAsset {
    joints: Vec<SkeletonJoint>,
}

impl SkeletonAsset {
    /// Stable authored joint order, shared by all instances.
    pub fn joints(&self) -> &[SkeletonJoint] {
        &self.joints
    }

    /// IPPS v1: magic, version/count u32, then parent u32 + ten TRS f32 per joint.
    pub fn decode(bytes: &[u8]) -> Result<Self, ErrorReason> {
        let count = header(bytes, b"IPPS", 44)?;
        let mut joints = Vec::with_capacity(count);
        for (index, bytes) in bytes[12..].as_chunks::<44>().0.iter().enumerate() {
            let parent = u32_at(bytes, 0);
            let parent = if parent == u32::MAX {
                None
            } else if (parent as usize) < index {
                Some(parent as usize)
            } else {
                return Err(ErrorReason::InvalidAsset);
            };
            joints.push(SkeletonJoint {
                parent,
                rest: transform(&bytes[4..])?,
            });
        }
        Ok(Self {
            joints,
        })
    }
}

/// Reusable joint-local TRS values in the skeleton's stable ordinal mapping.
#[derive(Debug)]
pub struct PoseAsset {
    joints: Vec<Transform>,
}

impl PoseAsset {
    /// Complete local pose. SkeletonJoint count must match its instance's skeleton.
    pub fn joints(&self) -> &[Transform] {
        &self.joints
    }

    /// IPPP v1: magic, version/count u32, then ten TRS f32 per joint.
    pub fn decode(bytes: &[u8]) -> Result<Self, ErrorReason> {
        header(bytes, b"IPPP", 40)?;
        let joints = bytes[12..]
            .as_chunks::<40>()
            .0
            .iter()
            .map(|v| transform(v))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            joints,
        })
    }
}

pub(crate) fn header(bytes: &[u8], magic: &[u8; 4], stride: usize) -> Result<usize, ErrorReason> {
    if bytes.len() < 12 || &bytes[..4] != magic || u32_at(bytes, 4) != 1 {
        return Err(ErrorReason::InvalidAsset);
    }
    let count = u32_at(bytes, 8) as usize;
    if !(1..=MAX_JOINTS).contains(&count) || bytes.len() != 12 + count * stride {
        return Err(ErrorReason::InvalidAsset);
    }
    Ok(count)
}

pub(crate) fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated extent"),
    )
}

pub(crate) fn transform(bytes: &[u8]) -> Result<Transform, ErrorReason> {
    let [x, y, z, qx, qy, qz, qw, sx, sy, sz] =
        std::array::from_fn(|i| f32::from_bits(u32_at(bytes, i * 4)));
    let value = Transform {
        x,
        y,
        z,
        qx,
        qy,
        qz,
        qw,
        sx,
        sy,
        sz,
    };
    value.validate().map_err(|_| ErrorReason::InvalidAsset)?;
    crate::systems::camera::model_matrix(&value).map_err(|_| ErrorReason::InvalidAsset)?;
    Ok(value)
}

macro_rules! source_asset {
    ($asset:ty, $field:ident, $loader:ident) => {
        impl Asset for $asset {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn decoded(&self) -> &dyn std::any::Any {
                self
            }

            fn resident_bytes(&self) -> usize {
                std::mem::size_of_val(self.$field.as_slice())
            }
        }

        /// Construct a CPU decoder without graphics allocation.
        pub fn $loader() -> impl AssetLoader<Data = $asset> {
            AsyncAssetLoader::decode(|mut reader| async move {
                <$asset>::decode_reader(&mut *reader).await
            })
        }
    };
}

source_asset!(SkeletonAsset, joints, skeleton_asset_loader);
source_asset!(PoseAsset, joints, pose_asset_loader);

pub(crate) use source_asset;

pub(crate) fn encode_transform(bytes: &mut Vec<u8>, joint: &Transform) {
    for v in [
        joint.x, joint.y, joint.z, joint.qx, joint.qy, joint.qz, joint.qw, joint.sx, joint.sy,
        joint.sz,
    ] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
}

impl super::writer::AssetEncoder for SkeletonAsset {
    fn encode_asset(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        let length = 12 + self.joints.len() * 44;
        if length > max_bytes {
            return Err("Skeleton output byte budget exhausted".into());
        }
        let mut bytes = Vec::with_capacity(length);
        bytes.extend_from_slice(b"IPPS");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(self.joints.len() as u32).to_le_bytes());
        for joint in &self.joints {
            bytes.extend_from_slice(
                &joint
                    .parent
                    .map_or(u32::MAX, |index| index as u32)
                    .to_le_bytes(),
            );
            encode_transform(&mut bytes, &joint.rest);
        }
        Ok(bytes)
    }
}

impl super::writer::AssetEncoder for PoseAsset {
    fn encode_asset(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        let length = 12 + self.joints.len() * 40;
        if length > max_bytes {
            return Err("Pose output byte budget exhausted".into());
        }
        let mut bytes = Vec::with_capacity(length);
        bytes.extend_from_slice(b"IPPP");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(self.joints.len() as u32).to_le_bytes());
        for joint in &self.joints {
            encode_transform(&mut bytes, joint);
        }
        Ok(bytes)
    }
}

pub(crate) async fn read_header(
    input: &mut super::decode::AssetReader<'_>,
    magic: &[u8; 4],
) -> Result<usize, String> {
    if &input.array::<4>().await? != magic || input.u32().await? != 1 {
        return Err("Invalid rig header".into());
    }
    let count = input.u32().await? as usize;
    if !(1..=MAX_JOINTS).contains(&count) {
        return Err("Invalid rig joint count".into());
    }
    Ok(count)
}

impl SkeletonAsset {
    /// Decode bounded joint records without retaining their source encoding.
    pub async fn decode_reader(reader: &mut dyn IoReader) -> Result<Self, String> {
        let mut input = super::decode::AssetReader::new(reader);
        let count = read_header(&mut input, b"IPPS").await?;
        let mut joints = Vec::with_capacity(count);
        for index in 0..count {
            let record = input.array::<44>().await?;
            let parent = u32_at(&record, 0);
            let parent = if parent == u32::MAX {
                None
            } else if (parent as usize) < index {
                Some(parent as usize)
            } else {
                return Err("Invalid skeleton parent".into());
            };
            joints.push(SkeletonJoint {
                parent,
                rest: transform(&record[4..]).map_err(|error| error.to_string())?,
            });
        }
        input.finish().await?;
        Ok(Self {
            joints,
        })
    }
}

impl PoseAsset {
    /// Decode bounded joint-local transforms through the common rig validator.
    pub async fn decode_reader(reader: &mut dyn IoReader) -> Result<Self, String> {
        let mut input = super::decode::AssetReader::new(reader);
        let count = read_header(&mut input, b"IPPP").await?;
        let mut joints = Vec::with_capacity(count);
        for _ in 0..count {
            joints.push(transform(&input.array::<40>().await?).map_err(|error| error.to_string())?);
        }
        input.finish().await?;
        Ok(Self {
            joints,
        })
    }
}
