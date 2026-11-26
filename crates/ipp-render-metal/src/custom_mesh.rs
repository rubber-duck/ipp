use std::{collections::BTreeMap, fmt::Write};

use ipp_core::{
    HostRuntime, WorldPublicationId,
    services::asset_management::formats::{
        ipp_shader::{IppShaderTarget, lower},
        shader::{ShaderDefinition, ShaderParameterKind as Kind},
    },
    systems::render::{PublishedCustomMaterial, PublishedMaterialProperty},
};

use crate::{
    assets::MetalTexture,
    device::{Blend, Device, Material, MeshParameters, Object},
    service::asset,
    shaders::{BACKEND, MetalShader},
};

const MAX_TEXTURES: usize = 8;
const MAX_NUMERIC_RECORDS: usize = 64;
const MAX_VARYINGS: usize = 4;

pub(super) fn compile(device: &Device, definition: &ShaderDefinition) -> Result<Object, String> {
    let stage = definition
        .backend_source(BACKEND)
        .ok_or("Material requires an msl or ipp stage")?;
    if stage.fragment.trim().is_empty() {
        return Err("Material has no fragment body".into());
    }

    let (vertex, outgoing) = varying_declarations(&stage.vertex, "out")?;
    let (fragment, incoming) = varying_declarations(&stage.fragment, "in")?;
    if outgoing != incoming || outgoing.len() > MAX_VARYINGS {
        return Err("Metal material varying declarations must match within four lanes".into());
    }
    let lowered = |body: &str| {
        if definition.selects_portable(BACKEND) {
            lower(body, IppShaderTarget::Msl)
        } else {
            Ok(body.to_string())
        }
    };

    let mut fields = String::new();
    let mut arguments = String::new();
    let mut parameters = String::new();
    let mut store = String::new();
    let mut load = String::new();
    let mut texture_count = 0;
    let mut numeric_records = 0;
    for (name, kind) in &definition.parameters {
        if *kind == Kind::Texture2D {
            if texture_count >= MAX_TEXTURES {
                return Err("Metal material exceeds the texture limit".into());
            }
            writeln!(fields, "texture2d<float> p_{name};").unwrap();
            write!(
                arguments,
                ", texture2d<float> p_{name} [[texture({})]]",
                texture_count + 2
            )
            .unwrap();
            writeln!(parameters, "scope.p_{name} = p_{name};").unwrap();
            texture_count += 1;
        } else {
            let (ty, expression) = numeric(*kind, numeric_records);
            numeric_records += record_count(*kind);
            if numeric_records > MAX_NUMERIC_RECORDS {
                return Err("Metal material exceeds the numeric parameter limit".into());
            }
            writeln!(fields, "{ty} p_{name};").unwrap();
            writeln!(parameters, "scope.p_{name} = {expression};").unwrap();
        }
    }
    for (index, (name, ty)) in outgoing.into_iter().enumerate() {
        let (ty, pack, swizzle) = match ty {
            "float" => (
                "float",
                format!("float4(scope.{name}, 0.0, 0.0, 0.0)"),
                ".x",
            ),
            "vec2" | "float2" => ("float2", format!("float4(scope.{name}, 0.0, 0.0)"), ".xy"),
            "vec3" | "float3" => ("float3", format!("float4(scope.{name}, 0.0)"), ".xyz"),
            "vec4" | "float4" => ("float4", format!("scope.{name}"), ""),
            _ => return Err("Unsupported Metal material varying type".into()),
        };
        writeln!(fields, "{ty} {name};").unwrap();
        writeln!(store, "result.extra{index} = {pack};").unwrap();
        writeln!(load, "scope.{name} = raster.extra{index}{swizzle};").unwrap();
    }

    let vertex = if vertex.trim().is_empty() {
        String::from("void materialVertex() {}")
    } else {
        lowered(&vertex)?
    };
    let fragment = lowered(&fragment)?;
    let recipe = definition.recipe.features;
    let macro_definitions = format!(
        "#define IPP_SHADER_INTERFACE_VERSION 1\n#define IPP_RECEIVES_LIGHT {}\n",
        u8::from(recipe & 8 != 0),
    );
    let body = include_str!("custom_mesh.metal")
        .replace("/* IPP_FIELDS */", &fields)
        .replace("/* IPP_TEXTURE_ARGUMENTS */", &arguments)
        .replace("/* IPP_PARAMETERS */", &parameters)
        .replace("/* IPP_STORE_VARYINGS */", &store)
        .replace("/* IPP_LOAD_VARYINGS */", &load)
        .replace("/* IPP_VERTEX */", &vertex)
        .replace("/* IPP_FRAGMENT */", &fragment);
    device.material_program(&(macro_definitions + &body), recipe & 16 != 0)
}

fn varying_declarations<'a>(
    source: &'a str,
    qualifier: &str,
) -> Result<(String, BTreeMap<&'a str, &'a str>), String> {
    let mut result = String::new();
    let mut varyings = BTreeMap::new();
    for line in source.lines() {
        let mut words = line.split_ascii_whitespace();
        if words.next() == Some(qualifier) {
            let ty = words.next().ok_or("Missing material varying type")?;
            let name = words
                .next()
                .and_then(|name| name.strip_suffix(';'))
                .ok_or("Invalid material varying declaration")?;
            let mut characters = name.chars();
            if !characters
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
                || !characters.all(|value| value.is_ascii_alphanumeric() || value == '_')
                || varyings.insert(name, ty).is_some()
            {
                return Err("Invalid or repeated material varying name".into());
            }
        } else {
            writeln!(result, "{line}").unwrap();
        }
    }
    Ok((result, varyings))
}

fn record_count(kind: Kind) -> usize {
    match kind {
        Kind::Mat2 => 2,
        Kind::Mat3 => 3,
        Kind::Mat4 => 4,
        _ => 1,
    }
}

fn numeric(kind: Kind, slot: usize) -> (&'static str, String) {
    let floats = format!("as_type<float4>(parameters[{slot}])");
    match kind {
        Kind::F32 => ("float", format!("{floats}.x")),
        Kind::I32 => ("int", format!("as_type<int>(parameters[{slot}].x)")),
        Kind::U32 => ("uint", format!("parameters[{slot}].x")),
        Kind::Bool => ("bool", format!("parameters[{slot}].x != 0")),
        Kind::Vec2 => ("float2", format!("{floats}.xy")),
        Kind::Vec3 => ("float3", format!("{floats}.xyz")),
        Kind::Vec4 => ("float4", floats),
        Kind::Mat2 | Kind::Mat3 | Kind::Mat4 => {
            let dimension = record_count(kind);
            let (ty, swizzle) =
                [("float2x2", ".xy"), ("float3x3", ".xyz"), ("float4x4", "")][dimension - 2];
            let args = (slot..slot + dimension)
                .map(|i| format!("as_type<float4>(parameters[{i}]){swizzle}"))
                .collect::<Vec<_>>()
                .join(", ");
            (ty, format!("{ty}({args})"))
        }
        Kind::Texture2D => unreachable!(),
    }
}

pub(super) struct CustomDraw<'a> {
    program: &'a Object,
    values: Vec<[u32; 4]>,
    textures: Vec<&'a Object>,
    effects: [f32; 4],
    pub(super) blend: Blend,
    pub(super) casts_shadows: bool,
}

impl<'a> CustomDraw<'a> {
    pub(super) fn resolve(
        host: &'a HostRuntime,
        publication: WorldPublicationId,
        material: &PublishedCustomMaterial,
        attributes: u32,
    ) -> Option<Self> {
        let shader = asset::<MetalShader>(host, publication, material.shader?.to_u64())?;
        let definition = shader.definition();
        if definition.is_paint()
            || definition.required_attributes & attributes != definition.required_attributes
        {
            return None;
        }
        let features = definition.recipe.features;
        let casts_shadows = material.casts_shadows && material.alpha_mode != 2;
        if (casts_shadows && features & 16 == 0) || material.receives_light != (features & 8 != 0) {
            return None;
        }
        let program = shader.program()?;
        let mut values = Vec::new();
        let mut textures = Vec::new();
        for (name, kind) in &definition.parameters {
            match material.properties.get(name.as_str())? {
                PublishedMaterialProperty::Resource(Some(key)) if *kind == Kind::Texture2D => {
                    textures.push(
                        asset::<MetalTexture>(host, publication, key.to_u64())?
                            .texture
                            .as_ref()?,
                    );
                }
                PublishedMaterialProperty::Value(value)
                    if Kind::from_value(value).ok()? == *kind =>
                {
                    if let Some(floats) = value.floats() {
                        let count = record_count(*kind);
                        for column in floats.chunks(floats.len() / count) {
                            let mut record = [0; 4];
                            for (output, input) in record.iter_mut().zip(column) {
                                *output = input.to_bits();
                            }
                            values.push(record);
                        }
                    } else {
                        let word = match value {
                            ipp_core::DynamicValue::I32(value) => *value as u32,
                            ipp_core::DynamicValue::U32(value) => *value,
                            ipp_core::DynamicValue::Bool(value) => u32::from(*value),
                            _ => return None,
                        };
                        values.push([word, 0, 0, 0]);
                    }
                }
                _ => return None,
            }
        }
        let effects = [
            f32::from(material.receives_shadows),
            f32::from(material.receives_light),
            material.alpha_mode as f32,
            material.alpha_cutoff,
        ];
        let blend = if material.alpha_mode == 2 {
            Blend::Alpha
        } else {
            Blend::Opaque
        };
        Some(Self {
            program,
            values,
            textures,
            blend,
            casts_shadows,
            effects,
        })
    }

    pub(super) fn matches(&self, other: &Self) -> bool {
        std::ptr::eq(self.program, other.program)
            && self.values == other.values
            && self.textures.len() == other.textures.len()
            && self
                .textures
                .iter()
                .zip(&other.textures)
                .all(|(a, b)| std::ptr::eq(*a, *b))
    }

    pub(super) fn configure(&self, material: &mut Material) {
        material.effects = self.effects;
    }

    pub(super) fn parameters(&self) -> MeshParameters<'_> {
        MeshParameters {
            program: self.program,
            values: &self.values,
            textures: &self.textures,
        }
    }
}
