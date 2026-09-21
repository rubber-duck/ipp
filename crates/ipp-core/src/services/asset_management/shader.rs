//! Immutable backend-specific custom shader definitions (IPPH version 2).

use super::{Asset, AssetLoader, AssetTypeId, BufferedAssetLoader};
use crate::{DynamicProperties, DynamicPropertyKind, DynamicValue};
use std::collections::BTreeMap;

/// Shader-definition resource identity.
pub const SHADER_TYPE: AssetTypeId = AssetTypeId(13);

/// Shader-specific interpretation of component values. Tags belong to IPPH, not property storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ShaderParameterKind {
    /// One float lane.
    F32 = 1,
    /// One signed integer.
    I32 = 2,
    /// One unsigned integer.
    U32 = 3,
    /// One boolean.
    Bool = 4,
    /// Two float lanes.
    Vec2 = 5,
    /// Three float lanes.
    Vec3 = 6,
    /// Four float lanes.
    Vec4 = 7,
    /// A column-major two by two matrix.
    Mat2 = 8,
    /// A column-major three by three matrix.
    Mat3 = 9,
    /// A column-major four by four matrix.
    Mat4 = 10,
    /// A general asset reference whose expected payload is a 2D texture.
    Texture2D = 11,
}

impl ShaderParameterKind {
    fn from_tag(tag: u8) -> Result<Self, String> {
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
            11 => Self::Texture2D,
            _ => return Err("Invalid shader parameter type".into()),
        })
    }

    /// Interpret a value for a shader requirement; unsupported asset types remain valid properties.
    pub fn from_value(value: &DynamicValue) -> Result<Self, String> {
        match value {
            DynamicValue::Asset(asset) if asset.kind == crate::TEXTURE_TYPE => Ok(Self::Texture2D),
            DynamicValue::Asset(_) => Err("Shader parameter requires a 2D texture asset".into()),
            _ => Self::from_tag(value.kind() as u8),
        }
    }

    fn accepts(self, properties: &DynamicProperties, name: &str) -> bool {
        let Some(descriptor) = properties.descriptors().get(name) else {
            return false;
        };
        if self == Self::Texture2D {
            descriptor.kind == DynamicPropertyKind::Asset
                && properties
                    .asset(name)
                    .is_some_and(|asset| asset.kind == crate::TEXTURE_TYPE)
        } else {
            self as u8 == descriptor.kind as u8
        }
    }
}

/// One backend's source entries. Empty fragment code selects material fallback.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShaderBackendSource {
    /// Empty uses the standard vertex stage; otherwise implements `materialVertex`.
    pub vertex: String,
    /// Implements `vec4 materialFragment()` returning linear RGBA.
    pub fragment: String,
}

/// Explicit compilation features; material values never participate in this recipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderRecipe {
    /// The backend to compile in the receiving rendering Host.
    pub backend: String,
    /// Bit 0 normals, 1 skinning, 2 mesh poses, 3 lighting, 4 shadow pass, 5 instancing.
    pub features: u32,
}

impl Default for ShaderRecipe {
    fn default() -> Self {
        Self {
            backend: "glsl-es-300".into(),
            features: 0,
        }
    }
}

/// Immutable parameter requirements and backend-specific source; no instance values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShaderDefinition {
    /// Explicit provider compilation recipe.
    pub recipe: ShaderRecipe,
    /// Parameter name to exact required type. Extra component properties are permitted.
    pub parameters: BTreeMap<String, ShaderParameterKind>,
    /// Backend identifiers; `glsl-es-300` is shared by WebGL 2 and GLES 3.
    pub backends: BTreeMap<String, ShaderBackendSource>,
    /// Required authored streams: bit 0 color, bit 1 UV, bit 2 normal, bit 3 weight.
    pub required_attributes: u32,
}

impl ShaderDefinition {
    /// Validate source-independent definition structure. Compilation belongs to the renderer.
    pub fn validate(&self) -> Result<(), String> {
        if self.recipe.features & !63 != 0 || self.recipe.backend.is_empty() {
            return Err("Invalid shader recipe".into());
        }
        if self.required_attributes & !15 != 0 {
            return Err("Unknown required shader attribute".into());
        }
        for name in self.parameters.keys() {
            DynamicProperties::validate_name(name).map_err(|_| "Invalid shader parameter name")?;
        }
        for (backend, source) in &self.backends {
            if backend.is_empty()
                || backend.contains('\0')
                || source.vertex.contains('\0')
                || source.fragment.contains('\0')
            {
                return Err("Invalid shader backend source".into());
            }
        }
        Ok(())
    }

    /// Compare only required properties; shader and component layouts are independent.
    pub fn accepts(&self, properties: &DynamicProperties) -> bool {
        self.parameters
            .iter()
            .all(|(name, kind)| kind.accepts(properties, name))
    }

    /// Canonical owned asset encoding. Changed content must use a new resource name.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        fn string(bytes: &mut Vec<u8>, value: &str) -> Result<(), String> {
            bytes.extend(
                u32::try_from(value.len())
                    .map_err(|_| "Shader string too large")?
                    .to_le_bytes(),
            );
            bytes.extend(value.as_bytes());
            Ok(())
        }
        let mut bytes = b"IPPH\x02\0\0\0".to_vec();
        bytes.extend(self.recipe.features.to_le_bytes());
        string(&mut bytes, &self.recipe.backend)?;
        bytes.extend(self.required_attributes.to_le_bytes());
        bytes.extend(
            u32::try_from(self.parameters.len())
                .map_err(|_| "Too many shader parameters")?
                .to_le_bytes(),
        );
        for (name, kind) in &self.parameters {
            string(&mut bytes, name)?;
            bytes.push(*kind as u8);
        }
        bytes.extend(
            u32::try_from(self.backends.len())
                .map_err(|_| "Too many shader backends")?
                .to_le_bytes(),
        );
        for (backend, source) in &self.backends {
            string(&mut bytes, backend)?;
            string(&mut bytes, &source.vertex)?;
            string(&mut bytes, &source.fragment)?;
        }
        Ok(bytes)
    }

    /// Decode a complete owned data-plane payload with checked lengths and unique names.
    pub fn decode(mut bytes: &[u8]) -> Result<Self, String> {
        fn take<'a>(bytes: &mut &'a [u8], n: usize) -> Result<&'a [u8], String> {
            let value = bytes.get(..n).ok_or("Truncated shader definition")?;
            *bytes = &bytes[n..];
            Ok(value)
        }
        fn u32(bytes: &mut &[u8]) -> Result<u32, String> {
            Ok(u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()))
        }
        fn string(bytes: &mut &[u8]) -> Result<String, String> {
            let length = u32(bytes)? as usize;
            Ok(std::str::from_utf8(take(bytes, length)?)
                .map_err(|_| "Invalid shader UTF-8")?
                .into())
        }
        let header = take(&mut bytes, 8)?;
        let recipe = if header == b"IPPH\x02\0\0\0" {
            ShaderRecipe {
                features: u32(&mut bytes)?,
                backend: string(&mut bytes)?,
            }
        } else {
            return Err("Unsupported shader definition".into());
        };
        let mut definition = Self {
            recipe,
            required_attributes: u32(&mut bytes)?,
            ..Self::default()
        };
        let count = u32(&mut bytes)?;
        if count as usize > bytes.len() / 6 {
            return Err("Truncated shader parameters".into());
        }
        for _ in 0..count {
            let name = string(&mut bytes)?;
            let kind = ShaderParameterKind::from_tag(take(&mut bytes, 1)?[0])?;
            if definition.parameters.insert(name, kind).is_some() {
                return Err("Duplicate shader parameter".into());
            }
        }
        let count = u32(&mut bytes)?;
        if count as usize > bytes.len() / 13 {
            return Err("Truncated shader backends".into());
        }
        for _ in 0..count {
            let backend = string(&mut bytes)?;
            let source = ShaderBackendSource {
                vertex: string(&mut bytes)?,
                fragment: string(&mut bytes)?,
            };
            if definition.backends.insert(backend, source).is_some() {
                return Err("Duplicate shader backend".into());
            }
        }
        if !bytes.is_empty() {
            return Err("Trailing shader definition bytes".into());
        }
        definition.validate()?;
        Ok(definition)
    }
}

impl Asset for ShaderDefinition {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn decoded(&self) -> &dyn std::any::Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.parameters.keys().map(|s| s.capacity()).sum::<usize>()
            + self
                .backends
                .iter()
                .map(|(key, value)| {
                    key.capacity() + value.vertex.capacity() + value.fragment.capacity()
                })
                .sum::<usize>()
    }
}

pub(crate) fn shader_asset_loader() -> impl AssetLoader<Data = ShaderDefinition> {
    BufferedAssetLoader::new(|bytes| {
        ShaderDefinition::decode(bytes)?;
        Err("Shader programs require a rendering Host".into())
    })
}

impl super::writer::AssetEncoder for ShaderDefinition {
    fn encode_asset(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        let bytes = self.encode()?;
        if bytes.len() > max_bytes {
            return Err("Shader output byte budget exhausted".into());
        }
        Ok(bytes)
    }
}
