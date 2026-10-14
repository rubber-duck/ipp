use super::*;

#[test]
fn glsl_sampling_intrinsics_lower_to_native_calls() {
    let lowered = lower(
        "return ippSample(p_image, v_uv) + ippSampleLod(p_image, v_uv, 2.0);",
        IppShaderTarget::GlslEs300,
    )
    .unwrap();
    assert_eq!(
        lowered,
        "return texture(p_image, v_uv) + textureLod(p_image, v_uv, 2.0);"
    );
}

#[test]
fn intrinsic_arguments_split_at_top_level_commas_only() {
    let lowered = lower(
        "return ippSample(p_image, vec2(min(x, 1.0), y));",
        IppShaderTarget::GlslEs300,
    )
    .unwrap();
    assert_eq!(lowered, "return texture(p_image, vec2(min(x, 1.0), y));");
}

#[test]
fn hlsl_lowering_maps_types_functions_and_sampling() {
    let lowered = lower(
        "vec4 c = ippSample(p_image, vec2(0.5)); return mix(c, vec4(1.0), fract(t));",
        IppShaderTarget::Hlsl,
    )
    .unwrap();
    assert_eq!(
        lowered,
        "float4 c = p_image.Sample(ippSamplerLinear, float2(0.5)); return lerp(c, float4(1.0), frac(t));"
    );
}

#[test]
fn msl_lowering_maps_sampling_load_size_and_discard() {
    let lowered = lower(
        "ivec2 s = ippTextureSize(p_image, 0); vec4 v = ippLoad(p_image, ivec2(1, 2), 0); if (v.x < 0.0) ippDiscard(); return v;",
        IppShaderTarget::Msl,
    )
    .unwrap();
    assert_eq!(
        lowered,
        "int2 s = int2(p_image.get_width(0), p_image.get_height(0)); float4 v = p_image.read(uint2(int2(1, 2)), 0); if (v.x < 0.0) discard_fragment(); return v;"
    );
}

#[test]
fn mul_uses_the_targets_multiplication_form() {
    assert_eq!(
        lower("return ippMul(m, v);", IppShaderTarget::Hlsl).unwrap(),
        "return mul((m), (v));"
    );
    assert_eq!(
        lower("return ippMul(m, v);", IppShaderTarget::GlslEs300).unwrap(),
        "return ((m ) * (v));"
    );
}

#[test]
fn mul_parenthesizes_compound_operands() {
    assert_eq!(
        lower("return ippMul(m, a + b);", IppShaderTarget::GlslEs300).unwrap(),
        "return ((m ) * (a + b));"
    );
}

#[test]
fn nested_intrinsics_lower_inside_arguments() {
    assert_eq!(
        lower(
            "return ippSample(p_image, ippFragmentCoord() / p_size);",
            IppShaderTarget::GlslEs300,
        )
        .unwrap(),
        "return texture(p_image, gl_FragCoord.xy / p_size);"
    );
}

#[test]
fn comments_are_not_parsed_as_code() {
    assert!(
        lower(
            "// mentions gl_FragCoord and texture(p_x)\nreturn vec4(1.0);",
            IppShaderTarget::GlslEs300,
        )
        .is_ok()
    );
    assert!(
        lower(
            "return ippSample(p_image, /* normalized, UV */ v_uv);",
            IppShaderTarget::GlslEs300,
        )
        .unwrap()
        .contains("texture(p_image,")
    );
}

#[test]
fn native_spellings_and_declarations_are_rejected() {
    for body in [
        "return textureGrad(p_image, v_uv, ddx, ddy);",
        "ivec2 s = textureSize(p_image, 0); return vec4(1.0);",
        "if (x) discard; return vec4(1.0);",
        "uniform float extra; return vec4(extra);",
        "return textureLod(p_image, v_uv, 0.0);",
        "return textureOffset(p_image, v_uv, ivec2(1, 0));",
        "return texelFetchOffset(p_image, ivec2(1), 0, ivec2(1, 0));",
        "return textureProj(p_image, v_proj);",
    ] {
        assert!(lower(body, IppShaderTarget::GlslEs300).is_err(), "{body}");
    }
}

#[test]
fn derivatives_map_per_target() {
    let body = "float w = fwidth(edge) + inversesqrt(dFdx(edge) + dFdy(edge));";
    assert!(
        lower(body, IppShaderTarget::Hlsl)
            .unwrap()
            .contains("rsqrt(ddx(edge) + ddy(edge))")
    );
    assert!(
        lower(body, IppShaderTarget::Msl)
            .unwrap()
            .contains("rsqrt(dfdx(edge) + dfdy(edge))")
    );
}

#[test]
fn raw_gl_calls_and_names_are_rejected() {
    assert!(lower("return texture(p_image, v_uv);", IppShaderTarget::GlslEs300).is_err());
    assert!(
        lower(
            "return vec4(gl_FragCoord.xy, 0.0, 1.0);",
            IppShaderTarget::Msl
        )
        .is_err()
    );
}
