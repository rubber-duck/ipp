use crate::{
    components::schema::FieldError,
    services::asset_management::{AssetSource, AssetTypeId},
};

/// Supported component parameter types; identities are independent of GPU types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum DynamicPropertyKind {
    /// One finite 32-bit floating point value.
    F32 = 1,
    /// One signed 32-bit integer.
    I32 = 2,
    /// One unsigned 32-bit integer.
    U32 = 3,
    /// One step-sampled boolean.
    Bool = 4,
    /// Two finite floating point lanes.
    Vec2 = 5,
    /// Three finite floating point lanes.
    Vec3 = 6,
    /// Four finite floating point lanes.
    Vec4 = 7,
    /// A column-major two by two float matrix.
    Mat2 = 8,
    /// A column-major three by three float matrix.
    Mat3 = 9,
    /// A column-major four by four float matrix.
    Mat4 = 10,
    /// A step-sampled owned typed asset reference.
    // Tag 11 belonged to the retired texture-only representation.
    Asset = 12,
}

/// Typed authored or sampled dynamic property value. Matrices are column-major.
#[derive(Clone, Debug, PartialEq)]
pub enum DynamicValue {
    /// One finite 32-bit floating point value.
    F32(f32),
    /// One signed 32-bit integer.
    I32(i32),
    /// One unsigned 32-bit integer.
    U32(u32),
    /// One step-sampled boolean.
    Bool(bool),
    /// Two finite floating point lanes.
    Vec2([f32; 2]),
    /// Three finite floating point lanes.
    Vec3([f32; 3]),
    /// Four finite floating point lanes.
    Vec4([f32; 4]),
    /// A column-major two by two float matrix.
    Mat2([f32; 4]),
    /// A column-major three by three float matrix.
    Mat3([f32; 9]),
    /// A column-major four by four float matrix.
    Mat4([f32; 16]),
    /// A step-sampled owned typed asset reference.
    Asset(AssetSource),
}

impl DynamicPropertyKind {
    /// Decode a checked canonical wire type tag.
    pub fn from_tag(tag: u8) -> Result<Self, FieldError> {
        Ok(match tag {
            1 => Self::F32,
            2 => Self::I32,
            3 => Self::U32,
            4 => Self::Bool,
            5 => Self::Vec2,
            6 => Self::Vec3,
            7 => Self::Vec4,
            8 => Self::Mat2,
            9 => Self::Mat3,
            10 => Self::Mat4,
            12 => Self::Asset,
            _ => return Err(FieldError::WrongType),
        })
    }

    /// Numeric storage size. Asset references are owned separately.
    pub fn byte_len(self) -> usize {
        match self {
            Self::F32 | Self::I32 | Self::U32 | Self::Bool => 4,
            Self::Vec2 => 8,
            Self::Vec3 => 12,
            Self::Vec4 | Self::Mat2 => 16,
            Self::Mat3 => 36,
            Self::Mat4 => 64,
            Self::Asset => 0,
        }
    }
}

impl DynamicValue {
    /// The exact component storage type of this value.
    pub fn kind(&self) -> DynamicPropertyKind {
        match self {
            Self::F32(_) => DynamicPropertyKind::F32,
            Self::I32(_) => DynamicPropertyKind::I32,
            Self::U32(_) => DynamicPropertyKind::U32,
            Self::Bool(_) => DynamicPropertyKind::Bool,
            Self::Vec2(_) => DynamicPropertyKind::Vec2,
            Self::Vec3(_) => DynamicPropertyKind::Vec3,
            Self::Vec4(_) => DynamicPropertyKind::Vec4,
            Self::Mat2(_) => DynamicPropertyKind::Mat2,
            Self::Mat3(_) => DynamicPropertyKind::Mat3,
            Self::Mat4(_) => DynamicPropertyKind::Mat4,
            Self::Asset(_) => DynamicPropertyKind::Asset,
        }
    }

    /// Borrow numeric float lanes without GPU padding.
    pub fn floats(&self) -> Option<&[f32]> {
        match self {
            Self::F32(v) => Some(std::slice::from_ref(v)),
            Self::Vec2(v) => Some(v),
            Self::Vec3(v) => Some(v),
            Self::Vec4(v) | Self::Mat2(v) => Some(v),
            Self::Mat3(v) => Some(v),
            Self::Mat4(v) => Some(v),
            _ => None,
        }
    }

    /// Reject nonfinite values and invalid asset source references.
    pub fn validate(&self) -> Result<(), FieldError> {
        if self
            .floats()
            .is_some_and(|values| values.iter().any(|v| !v.is_finite()))
        {
            return Err(FieldError::NonFinite);
        }
        if let Self::Asset(asset) = self {
            crate::services::asset_management::service::validate_source(&asset.uri)
                .map_err(|_| FieldError::WrongType)?;
        }
        Ok(())
    }

    /// Numeric weighted combination; matrix and vector lanes interpolate independently.
    pub(crate) fn weighted(values: &[&Self], weights: &[f64]) -> Result<Self, FieldError> {
        let first = values.first().ok_or(FieldError::WrongType)?;
        if values.len() != weights.len() || values.iter().any(|v| v.kind() != first.kind()) {
            return Err(FieldError::WrongType);
        }
        let result = if let Some(lanes) = first.floats() {
            if crate::allocation_optimizations_enabled() {
                let mut bytes = [0u8; 65];
                bytes[0] = first.kind() as u8;
                for i in 0..lanes.len() {
                    let value = values
                        .iter()
                        .zip(weights)
                        .map(|(v, w)| f64::from(v.floats().unwrap()[i]) * w)
                        .sum::<f64>() as f32;
                    bytes[1 + i * 4..5 + i * 4].copy_from_slice(&value.to_le_bytes());
                }
                return Self::decode(&bytes[..1 + lanes.len() * 4]);
            }
            let mut bytes = vec![first.kind() as u8];
            for i in 0..lanes.len() {
                let value = values
                    .iter()
                    .zip(weights)
                    .map(|(v, w)| f64::from(v.floats().unwrap()[i]) * w)
                    .sum::<f64>() as f32;
                bytes.extend(value.to_le_bytes());
            }
            Self::decode(&bytes)?
        } else {
            match first {
                Self::I32(_) => {
                    let value = values
                        .iter()
                        .zip(weights)
                        .map(|(v, w)| {
                            if let Self::I32(v) = v {
                                f64::from(*v) * w
                            } else {
                                unreachable!()
                            }
                        })
                        .sum::<f64>()
                        .round();
                    Self::I32(value.clamp(i32::MIN as f64, i32::MAX as f64) as i32)
                }
                Self::U32(_) => {
                    let value = values
                        .iter()
                        .zip(weights)
                        .map(|(v, w)| {
                            if let Self::U32(v) = v {
                                f64::from(*v) * w
                            } else {
                                unreachable!()
                            }
                        })
                        .sum::<f64>()
                        .round();
                    Self::U32(value.clamp(0.0, u32::MAX as f64) as u32)
                }
                _ => return Err(FieldError::WrongType),
            }
        };
        result.validate()?;
        Ok(result)
    }

    /// Canonical self-describing value encoding, shared by owned wire payloads and assets.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![self.kind() as u8];
        if let Some(values) = self.floats() {
            for value in values {
                bytes.extend(value.to_le_bytes());
            }
        } else {
            match self {
                Self::I32(v) => bytes.extend(v.to_le_bytes()),
                Self::U32(v) => bytes.extend(v.to_le_bytes()),
                Self::Bool(v) => bytes.extend(u32::from(*v).to_le_bytes()),
                Self::Asset(v) => {
                    bytes.extend(v.kind.0.to_le_bytes());
                    bytes.extend(v.variant.to_le_bytes());
                    bytes.extend(v.uri.as_bytes());
                }
                _ => unreachable!(),
            }
        }
        bytes
    }

    /// Decode one complete canonical typed payload.
    pub fn decode(bytes: &[u8]) -> Result<Self, FieldError> {
        let (&tag, data) = bytes.split_first().ok_or(FieldError::WrongType)?;
        let kind = DynamicPropertyKind::from_tag(tag)?;
        if kind != DynamicPropertyKind::Asset && data.len() != kind.byte_len() {
            return Err(FieldError::WrongType);
        }
        fn floats<const N: usize>(bytes: &[u8]) -> [f32; N] {
            std::array::from_fn(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
        }
        let value = match kind {
            DynamicPropertyKind::F32 => Self::F32(floats::<1>(data)[0]),
            DynamicPropertyKind::I32 => Self::I32(i32::from_le_bytes(data.try_into().unwrap())),
            DynamicPropertyKind::U32 => Self::U32(u32::from_le_bytes(data.try_into().unwrap())),
            DynamicPropertyKind::Bool => {
                Self::Bool(match u32::from_le_bytes(data.try_into().unwrap()) {
                    0 => false,
                    1 => true,
                    _ => return Err(FieldError::WrongType),
                })
            }
            DynamicPropertyKind::Vec2 => Self::Vec2(floats(data)),
            DynamicPropertyKind::Vec3 => Self::Vec3(floats(data)),
            DynamicPropertyKind::Vec4 => Self::Vec4(floats(data)),
            DynamicPropertyKind::Mat2 => Self::Mat2(floats(data)),
            DynamicPropertyKind::Mat3 => Self::Mat3(floats(data)),
            DynamicPropertyKind::Mat4 => Self::Mat4(floats(data)),
            DynamicPropertyKind::Asset => {
                if data.len() < 6 {
                    return Err(FieldError::WrongType);
                }
                Self::Asset(AssetSource {
                    kind: AssetTypeId(u16::from_le_bytes(data[..2].try_into().unwrap())),
                    variant: u32::from_le_bytes(data[2..6].try_into().unwrap()),
                    uri: std::str::from_utf8(&data[6..])
                        .map_err(|_| FieldError::WrongType)?
                        .into(),
                })
            }
        };
        value.validate()?;
        Ok(value)
    }
}
