//! Lowering of portable `ipp` authored-shader bodies to a renderer's native
//! shader source, without a shader-compiler dependency.
//!
//! An `ipp` body is a restricted GLSL-flavoured function body: it references
//! only generated `p_<name>` parameters, framework-provided names and the
//! intrinsics below, and declares no uniforms, samplers or entry points of its
//! own. The builder that wraps the body owns prologues, stage inputs/outputs,
//! parameter packing and binding assignment per backend; this module owns the
//! two mechanical steps between body and native source:
//!
//! 1. Rewriting the sampling/control intrinsics, whose native spellings are
//!    method calls or statements rather than functions, by locating each call,
//!    splitting its arguments at top-level commas and emitting the target form.
//! 2. Mapping the remaining type and function tokens (`vec4` to `float4`,
//!    `mix` to `lerp`, and similar) with word-boundary replacement.
//!
//! Canonical intrinsics:
//!
//! | Intrinsic | Meaning |
//! | --- | --- |
//! | `ippSample(tex, uv)` | Filtered sample, implicit LOD (fragment only) |
//! | `ippSampleLod(tex, uv, lod)` | Filtered sample at an explicit LOD |
//! | `ippSampleBias(tex, uv, bias)` | Filtered sample with an LOD bias |
//! | `ippLoad(tex, coord, lod)` | Unfiltered integer texel fetch |
//! | `ippTextureSize(tex, lod)` | Texture dimensions at `lod` as an int vector |
//! | `ippMul(a, b)` | Matrix/vector multiplication in the target's semantics |
//! | `ippDiscard()` | Fragment discard |
//! | `ippFragmentCoord()` | Fragment position in pixels as a 2-vector |
//!
//! A portable vertex body writes its clip-space position to the builder-provided
//! `ipp_position` instead of `gl_Position`; each builder binds that name to its
//! assignable native clip output (the GLSL builder aliases it to `gl_Position`),
//! so calling `ippDefaultVertex()` first and adjusting `ipp_position` after it
//! composes exactly as in a native body.
//!
//! Sampler state is deliberately not an intrinsic argument: the IPP sampler is
//! linear filtering with clamp-to-edge on every backend, declared by the
//! builder's prologue (`ippSamplerLinear`) or set on the texture object where
//! the API keeps sampler state there. HLSL and Metal builders must also bind
//! the name `ipp_fragment_coord` to their stage input's pixel position.
//!
//! Raw `texture`/`textureLod`/`texelFetch` calls and `gl_` names are rejected
//! rather than mistranslated; native source remains available through a
//! renderer-specific backend entry for anything outside this subset.

/// A renderer backend an `ipp` body can be lowered to. Only the GLSL target
/// has a device backend today; the HLSL and Metal lowerings are snapshot-tested
/// here and consumed by their future backends (ipp-fnir.15, ipp-fnir.16).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IppShaderTarget {
    /// WebGL 2 and GLES 3 (`glsl-es-300`).
    GlslEs300,
    /// Direct3D 11 HLSL.
    Hlsl,
    /// Metal Shading Language.
    Msl,
}

const INTRINSICS: [&str; 8] = [
    "ippSampleBias",
    "ippTextureSize",
    "ippSampleLod",
    "ippFragmentCoord",
    "ippSample",
    "ippDiscard",
    "ippLoad",
    "ippMul",
];

/// Lower one portable body to `target` source. The result is a body, not a
/// complete program: the caller's builder supplies declarations and entry
/// points exactly as it does for a renderer-specific body. Comments are
/// blanked (newlines preserved) before validation and rewriting, so comment
/// text is never parsed as shader code and generated line numbers stay put.
pub(crate) fn lower(body: &str, target: IppShaderTarget) -> Result<String, String> {
    let masked = mask_comments(body)?;
    validate_body(&masked)?;
    let rewritten = rewrite_intrinsics(&masked, target)?;
    Ok(map_tokens(&rewritten, target))
}

/// Replace `//` and `/* */` comment characters with spaces, preserving bytes
/// and newlines so positions in the lowered source still match the body.
fn mask_comments(body: &str) -> Result<String, String> {
    let mut masked = String::with_capacity(body.len());
    let mut rest = body;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("//") {
            let line = after.split('\n').next().expect("first line");
            masked.push_str(&" ".repeat(2 + line.len()));
            rest = &after[line.len()..];
        } else if let Some(after) = rest.strip_prefix("/*") {
            let end = after
                .find("*/")
                .ok_or("an ipp shader body has an unterminated comment")?;
            for character in after[..end].chars() {
                masked.push(if character == '\n' {
                    '\n'
                } else {
                    ' '
                });
            }
            masked.push_str("    ");
            rest = &after[end + 2..];
        } else {
            let character = rest.chars().next().expect("nonempty rest");
            masked.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }
    Ok(masked)
}

fn validate_body(body: &str) -> Result<(), String> {
    for word in identifiers(body) {
        if word.starts_with("gl_")
            || word.starts_with("texture")
            || word.starts_with("texel")
            || FORBIDDEN.contains(&word)
        {
            return Err(format!(
                "an ipp shader body cannot use {word}; use the ipp intrinsics and framework names"
            ));
        }
    }
    Ok(())
}

/// Native shader spellings and declarations a portable body must express
/// through the IPP intrinsics and the builder's generated declarations. The
/// whole native sampling family (`texture*`, `texel*`) is rejected by prefix
/// in [`validate_body`], so new native variants cannot slip through unnamed.
const FORBIDDEN: [&str; 8] = [
    "discard",
    "uniform",
    "layout",
    "sampler2D",
    "sampler3D",
    "samplerCube",
    "imageLoad",
    "imageStore",
];

fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty() && !word.starts_with(|c: char| c.is_ascii_digit()))
}

fn rewrite_intrinsics(body: &str, target: IppShaderTarget) -> Result<String, String> {
    let bytes = body.as_bytes();
    let mut output = String::with_capacity(body.len());
    let mut index = 0;
    while index < bytes.len() {
        let intrinsic = INTRINSICS.iter().find(|name| {
            body[index..].starts_with(**name)
                && (index == 0 || !is_identifier_byte(bytes[index - 1]))
                && body[index + name.len()..].trim_start().starts_with('(')
        });
        let Some(name) = intrinsic else {
            let character = body[index..].chars().next().expect("index at char");
            output.push(character);
            index += character.len_utf8();
            continue;
        };
        let open = index + name.len() + body[index + name.len()..].find('(').expect("checked");
        let (args, close) = call_arguments(body, open)?;
        let args: Vec<String> = args
            .iter()
            .map(|arg| rewrite_intrinsics(arg, target))
            .collect::<Result<_, _>>()?;
        output.push_str(&emit_intrinsic(name, &args, target)?);
        index = close + 1;
    }
    Ok(output)
}

/// The arguments of the call whose `(` is at `open`, and the index of its `)`.
fn call_arguments(text: &str, open: usize) -> Result<(Vec<String>, usize), String> {
    let bytes = text.as_bytes();
    let mut args = Vec::new();
    let mut depth = 0_i64;
    let mut start = open + 1;
    let mut index = open;
    while index < bytes.len() {
        match bytes[index] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    let last = text[start..index].trim();
                    if !last.is_empty() || !args.is_empty() {
                        args.push(last.to_string());
                    }
                    return Ok((args, index));
                }
            }
            b',' if depth == 1 => {
                args.push(text[start..index].trim().to_string());
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    Err("an ipp intrinsic call leaves a parenthesis open".into())
}

fn emit_intrinsic(name: &str, args: &[String], target: IppShaderTarget) -> Result<String, String> {
    let expect = |count: usize| -> Result<(), String> {
        if args.len() == count {
            Ok(())
        } else {
            Err(format!(
                "{name} takes {count} arguments, got {}",
                args.len()
            ))
        }
    };
    let out = match (name, target) {
        ("ippSample", _) => {
            expect(2)?;
            match target {
                IppShaderTarget::GlslEs300 => format!("texture({}, {})", args[0], args[1]),
                IppShaderTarget::Hlsl => {
                    format!("{}.Sample(ippSamplerLinear, {})", args[0], args[1])
                }
                IppShaderTarget::Msl => {
                    format!("{}.sample(ippSamplerLinear, {})", args[0], args[1])
                }
            }
        }
        ("ippSampleLod", _) => {
            expect(3)?;
            match target {
                IppShaderTarget::GlslEs300 => {
                    format!("textureLod({}, {}, {})", args[0], args[1], args[2])
                }
                IppShaderTarget::Hlsl => format!(
                    "{}.SampleLevel(ippSamplerLinear, {}, {})",
                    args[0], args[1], args[2]
                ),
                IppShaderTarget::Msl => format!(
                    "{}.sample(ippSamplerLinear, {}, level({}))",
                    args[0], args[1], args[2]
                ),
            }
        }
        ("ippSampleBias", _) => {
            expect(3)?;
            match target {
                IppShaderTarget::GlslEs300 => {
                    format!("texture({}, {}, {})", args[0], args[1], args[2])
                }
                IppShaderTarget::Hlsl => format!(
                    "{}.SampleBias(ippSamplerLinear, {}, {})",
                    args[0], args[1], args[2]
                ),
                IppShaderTarget::Msl => format!(
                    "{}.sample(ippSamplerLinear, {}, bias({}))",
                    args[0], args[1], args[2]
                ),
            }
        }
        ("ippLoad", _) => {
            expect(3)?;
            match target {
                IppShaderTarget::GlslEs300 => {
                    format!("texelFetch({}, {}, {})", args[0], args[1], args[2])
                }
                IppShaderTarget::Hlsl => {
                    format!("{}.Load(int3({}, {}))", args[0], args[1], args[2])
                }
                IppShaderTarget::Msl => {
                    format!("{}.read(uint2({}), {})", args[0], args[1], args[2])
                }
            }
        }
        ("ippTextureSize", _) => {
            expect(2)?;
            match target {
                IppShaderTarget::GlslEs300 => format!("textureSize({}, {})", args[0], args[1]),
                IppShaderTarget::Hlsl => {
                    format!("ipp_texture_size({}, {})", args[0], args[1])
                }
                IppShaderTarget::Msl => format!(
                    "int2({}.get_width({}), {}.get_height({}))",
                    args[0], args[1], args[0], args[1]
                ),
            }
        }
        ("ippMul", _) => {
            expect(2)?;
            match target {
                IppShaderTarget::Hlsl => format!("mul(({}), ({}))", args[0], args[1]),
                _ => format!("(({} ) * ({}))", args[0], args[1]),
            }
        }
        ("ippDiscard", _) => {
            expect(0)?;
            match target {
                IppShaderTarget::Msl => "discard_fragment()".into(),
                _ => "discard".into(),
            }
        }
        ("ippFragmentCoord", _) => {
            expect(0)?;
            match target {
                IppShaderTarget::GlslEs300 => "gl_FragCoord.xy".into(),
                _ => "ipp_fragment_coord".into(),
            }
        }
        _ => return Err(format!("unknown ipp intrinsic {name}")),
    };
    Ok(out)
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn map_tokens(text: &str, target: IppShaderTarget) -> String {
    if target == IppShaderTarget::GlslEs300 {
        return text.to_string();
    }
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let first = rest.chars().next().expect("nonempty rest");
        if first.is_ascii_alphabetic() || first == '_' {
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            output.push_str(map_word(&rest[..end], target));
            rest = &rest[end..];
        } else {
            output.push(first);
            rest = &rest[first.len_utf8()..];
        }
    }
    output
}

fn map_word(word: &str, target: IppShaderTarget) -> &str {
    let mapped = match word {
        "vec2" => "float2",
        "vec3" => "float3",
        "vec4" => "float4",
        "ivec2" => "int2",
        "ivec3" => "int3",
        "ivec4" => "int4",
        "uvec2" => "uint2",
        "uvec3" => "uint3",
        "uvec4" => "uint4",
        "bvec2" => "bool2",
        "bvec3" => "bool3",
        "bvec4" => "bool4",
        "mat2" => "float2x2",
        "mat3" => "float3x3",
        "mat4" => "float4x4",
        "inversesqrt" => "rsqrt",
        _ => word,
    };
    if mapped != word {
        return mapped;
    }
    match (word, target) {
        ("mix", IppShaderTarget::Hlsl) => "lerp",
        ("fract", IppShaderTarget::Hlsl) => "frac",
        ("dFdx", IppShaderTarget::Hlsl) => "ddx",
        ("dFdy", IppShaderTarget::Hlsl) => "ddy",
        ("dFdx", IppShaderTarget::Msl) => "dfdx",
        ("dFdy", IppShaderTarget::Msl) => "dfdy",
        _ => word,
    }
}

#[cfg(test)]
#[path = "ipp_shader_tests.rs"]
mod tests;
