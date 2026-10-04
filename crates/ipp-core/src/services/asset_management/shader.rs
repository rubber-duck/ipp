//! Immutable backend-specific custom shader definitions (IPPH version 3).
//!
//! One definition is either a mesh material, with vertex and fragment bodies, a
//! recipe and required streams, or a canvas paint: one fill function body with
//! float scalar or vector parameters and nothing else. The renderer's material and
//! paint guides, `CUSTOM_MATERIALS.md` and `CANVAS_PAINTS.md` beside its render
//! service, own the backend interfaces.

use super::{Asset, AssetLoader, AssetTypeId, AsyncAssetLoader};
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

    /// Whether a canvas paint may declare this kind: a float scalar or vector.
    pub fn paint_parameter(self) -> bool {
        matches!(self, Self::F32 | Self::Vec2 | Self::Vec3 | Self::Vec4)
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
    /// The statements of one canvas paint function returning straight linear RGBA;
    /// nonblank only in a paint definition.
    pub paint: String,
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
    ///
    /// A definition with a nonblank paint body in any backend is a canvas paint: it
    /// has no material stages, recipe features or required streams, and declares
    /// only float scalar and vector parameters.
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
                || source.paint.contains('\0')
            {
                return Err("Invalid shader backend source".into());
            }
        }
        if self.is_paint() {
            let stages = self.backends.values().any(|source| {
                !source.vertex.trim().is_empty() || !source.fragment.trim().is_empty()
            });
            if stages || self.recipe.features != 0 || self.required_attributes != 0 {
                return Err(
                    "A paint definition has no material stages, recipe features or streams".into(),
                );
            }
            if !self.parameters.values().all(|kind| kind.paint_parameter()) {
                return Err("Paint parameters are float scalars or vectors".into());
            }
        }
        Ok(())
    }

    /// Whether this definition is a canvas paint: some backend has a paint body.
    pub fn is_paint(&self) -> bool {
        self.backends
            .values()
            .any(|source| !source.paint.trim().is_empty())
    }

    /// The paint body of `backend`, if it has a nonblank one.
    pub fn paint_body(&self, backend: &str) -> Option<&str> {
        self.backends
            .get(backend)
            .map(|source| source.paint.as_str())
            .filter(|body| !body.trim().is_empty())
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
        let mut bytes = b"IPPH\x03\0\0\0".to_vec();
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
            string(&mut bytes, &source.paint)?;
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
        let recipe = if header == b"IPPH\x03\0\0\0" {
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
        if count as usize > bytes.len() / 17 {
            return Err("Truncated shader backends".into());
        }
        for _ in 0..count {
            let backend = string(&mut bytes)?;
            let source = ShaderBackendSource {
                vertex: string(&mut bytes)?,
                fragment: string(&mut bytes)?,
                paint: string(&mut bytes)?,
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
                    key.capacity()
                        + value.vertex.capacity()
                        + value.fragment.capacity()
                        + value.paint.capacity()
                })
                .sum::<usize>()
    }
}

pub(crate) fn shader_asset_loader() -> impl AssetLoader<Data = ShaderDefinition> {
    AsyncAssetLoader::decode(|mut reader| async move {
        ShaderDefinition::decode_reader(&mut *reader).await?;
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

impl ShaderDefinition {
    /// Decode immutable strings directly into semantic fields, without encoded staging.
    pub async fn decode_reader(reader: &mut dyn super::IoReader) -> Result<Self, String> {
        use super::decode::AssetReader;
        async fn text(input: &mut AssetReader<'_>) -> Result<String, String> {
            let length = input.u32().await? as usize;
            input.text(length).await
        }
        let mut input = AssetReader::new(reader);
        if input.array::<8>().await? != *b"IPPH\x03\0\0\0" {
            return Err("Unsupported shader definition".into());
        }
        let recipe = ShaderRecipe {
            features: input.u32().await?,
            backend: text(&mut input).await?,
        };
        let required_attributes = input.u32().await?;
        let mut definition = Self {
            recipe,
            required_attributes,
            ..Default::default()
        };
        let count = input.u32().await?;
        for _ in 0..count {
            let name = text(&mut input).await?;
            let kind = ShaderParameterKind::from_tag(input.u8().await?)?;
            if definition.parameters.insert(name, kind).is_some() {
                return Err("Duplicate shader parameter".into());
            }
        }
        let count = input.u32().await?;
        for _ in 0..count {
            let backend = text(&mut input).await?;
            let source = ShaderBackendSource {
                vertex: text(&mut input).await?,
                fragment: text(&mut input).await?,
                paint: text(&mut input).await?,
            };
            if definition.backends.insert(backend, source).is_some() {
                return Err("Duplicate shader backend".into());
            }
        }
        input.finish().await?;
        definition.validate()?;
        Ok(definition)
    }
}
