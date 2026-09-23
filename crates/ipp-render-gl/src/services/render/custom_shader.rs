//! GLSL ES custom stage interface and renderer-owned std140 parameter packing.

use super::shader::RenderShaderConfig;
use crate::RenderError;
use ipp_core::{
    DynamicProperties,
    services::asset_management::shader::{ShaderDefinition, ShaderParameterKind as Kind},
};

pub(crate) fn parameter_layout(
    definition: &ShaderDefinition,
    properties: &DynamicProperties,
) -> Result<(String, Vec<u32>, Vec<String>), RenderError> {
    if !definition.accepts(properties) {
        return Err(RenderError::RenderDevice(
            "custom material parameter requirements do not match".into(),
        ));
    }
    let mut words = Vec::new();
    let mut macros = String::new();
    let mut textures = Vec::new();
    for (name, kind) in &definition.parameters {
        if *kind == Kind::Texture2D {
            macros.push_str(&format!("uniform sampler2D p_{name};\n"));
            textures.push(name.clone());
            continue;
        }
        let value = properties.get(name).expect("validated parameter");
        let bytes = value.encode();
        let lanes: Vec<u32> = bytes[1..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        let first = words.len() / 4;
        let dimension = match kind {
            Kind::Mat2 => 2,
            Kind::Mat3 => 3,
            Kind::Mat4 => 4,
            _ => 0,
        };
        let access = if dimension > 0 {
            for column in lanes.chunks_exact(dimension) {
                words.extend_from_slice(column);
                words.resize(words.len().next_multiple_of(4), 0);
            }
            let columns = (0..dimension)
                .map(|i| {
                    format!(
                        "uintBitsToFloat(u_parameter_words[{}]).{}",
                        first + i,
                        &"xyzw"[..dimension]
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("mat{dimension}({columns})")
        } else {
            words.extend(lanes);
            words.resize(words.len().next_multiple_of(4), 0);
            match kind {
                Kind::F32 => format!("uintBitsToFloat(u_parameter_words[{first}].x)"),
                Kind::I32 => format!("int(u_parameter_words[{first}].x)"),
                Kind::U32 => format!("u_parameter_words[{first}].x"),
                Kind::Bool => format!("(u_parameter_words[{first}].x != 0u)"),
                Kind::Vec2 => format!("uintBitsToFloat(u_parameter_words[{first}].xy)"),
                Kind::Vec3 => format!("uintBitsToFloat(u_parameter_words[{first}].xyz)"),
                Kind::Vec4 => format!("uintBitsToFloat(u_parameter_words[{first}])"),
                _ => unreachable!(),
            }
        };
        macros.push_str(&format!("#define p_{name} ({access})\n"));
    }
    if !words.is_empty() {
        macros = format!(
            "layout(std140) uniform IppParameters {{ uvec4 u_parameter_words[{}]; }};\n{macros}",
            words.len() / 4
        );
    }
    Ok((macros, words, textures))
}

pub(crate) fn sources(
    config: RenderShaderConfig,
    definition: &ShaderDefinition,
    declarations: &str,
    lit: bool,
    shadow: bool,
) -> Result<(String, String), RenderError> {
    let backend = definition
        .backends
        .get("glsl-es-300")
        .filter(|source| !source.fragment.trim().is_empty())
        .ok_or_else(|| {
            RenderError::RenderDevice("custom material has no GLSL ES fragment body".into())
        })?;
    let pass = if shadow {
        1
    } else {
        0
    };
    let common = format!(
        "#version 300 es\nprecision highp float;\nprecision highp int;\n#define IPP_SHADER_INTERFACE_VERSION 1\n#define IPP_PASS_SHADOW {pass}\n#define IPP_RECEIVES_LIGHT {}\n{declarations}\n",
        u8::from(lit)
    );
    let template = crate::services::render::embedded_shader!("shaders/custom.vert");
    let mut vertex = config.custom_vertex_source(template)?;
    vertex = vertex.replace("// CUSTOM_DECLARATIONS", &common);
    vertex.push_str(&backend.vertex);
    vertex.push_str(if backend.vertex.trim().is_empty() {
        "\nvoid main() { ippDefaultVertex(); }\n"
    } else {
        "\nvoid main() { materialVertex(); }\n"
    });
    let mut fragment = format!(
        "{common}\nin vec3 v_position;\nin vec3 v_normal;\nin vec3 v_color;\nin vec2 v_uv;\nin float v_weight;\nuniform int u_alpha_mode;\nuniform float u_alpha_cutoff;\nout vec4 out_color;\n"
    );
    if lit {
        fragment.push_str(crate::services::render::embedded_shader!(
            "shaders/custom-lighting.glsl"
        ));
    }
    #[cfg(feature = "shadows")]
    if lit {
        fragment.push_str(crate::services::render::embedded_shader!(
            "shaders/shadow-sampling.glsl"
        ));
    }
    if lit {
        fragment.push_str("uniform vec3 u_surface;\n");
        #[cfg(feature = "shadows")]
        fragment.push_str("float ippShadowVisibility(int index, float nl) { return u_surface.z > 0.5 ? visibility_from_shadow(index, nl, ippSurfaceNormal()) : 1.0; }\nfloat ippShadowVisibility(float nl) { for (int i = 0; i < u_light_count; ++i) { if (u_shadow_settings[i].x >= 0.0) return ippShadowVisibility(i, nl); } return 1.0; }\n");
        #[cfg(not(feature = "shadows"))]
        fragment.push_str("float ippShadowVisibility(int index, float nl) { return 1.0; }\nfloat ippShadowVisibility(float nl) { return 1.0; }\n");
    }
    fragment.push_str(&backend.fragment);
    fragment.push_str("\nvoid main() { vec4 color = materialFragment(); if (u_alpha_mode == 1 && color.a < u_alpha_cutoff) discard; out_color = vec4(clamp(color.rgb, 0.0, 1.0), u_alpha_mode == 2 ? clamp(color.a, 0.0, 1.0) : 1.0); }\n");
    Ok((vertex, fragment))
}

#[cfg(test)]
#[path = "custom_shader_tests.rs"]
mod tests;

/// Update numeric std140 words without rebuilding the immutable GLSL declarations.
pub(super) fn parameter_words(
    definition: &ShaderDefinition,
    properties: &DynamicProperties,
    words: &mut Vec<u32>,
) -> Result<(), RenderError> {
    if !definition.accepts(properties) {
        return Err(RenderError::RenderDevice(
            "custom material parameter requirements do not match".into(),
        ));
    }
    words.clear();
    for (name, kind) in &definition.parameters {
        if *kind == Kind::Texture2D {
            continue;
        }
        let value = properties.get(name).expect("validated parameter");
        let dimension = match kind {
            Kind::Mat2 => 2,
            Kind::Mat3 => 3,
            Kind::Mat4 => 4,
            _ => 0,
        };
        if let Some(lanes) = value.floats() {
            if dimension > 0 {
                for column in lanes.chunks_exact(dimension) {
                    words.extend(column.iter().map(|value| value.to_bits()));
                    words.resize(words.len().next_multiple_of(4), 0);
                }
            } else {
                words.extend(lanes.iter().map(|value| value.to_bits()));
            }
        } else {
            words.push(match value {
                ipp_core::DynamicValue::I32(v) => v as u32,
                ipp_core::DynamicValue::U32(v) => v,
                ipp_core::DynamicValue::Bool(v) => u32::from(v),
                _ => unreachable!("validated numeric parameter"),
            });
        }
        words.resize(words.len().next_multiple_of(4), 0);
    }
    Ok(())
}
