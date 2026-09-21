use super::*;
use ipp_core::{DynamicValue, MESH_TYPE, TEXTURE_TYPE, services::asset_management::AssetSource};
use std::collections::BTreeMap;

#[test]
fn packing_is_definition_ordered_padded_and_independent_of_component_offsets() {
    let mut values = DynamicProperties::default();
    values.set("extra", DynamicValue::Mat4([9.0; 16])).unwrap();
    values
        .set(
            "matrix",
            DynamicValue::Mat3([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]),
        )
        .unwrap();
    values.set("flag", DynamicValue::Bool(true)).unwrap();
    values
        .set(
            "texture",
            DynamicValue::Asset(AssetSource {
                kind: TEXTURE_TYPE,
                uri: "file:///a".into(),
                variant: 3,
            }),
        )
        .unwrap();
    let definition = ShaderDefinition {
        parameters: BTreeMap::from([
            ("matrix".into(), Kind::Mat3),
            ("flag".into(), Kind::Bool),
            ("texture".into(), Kind::Texture2D),
        ]),
        ..Default::default()
    };
    let (source, words, textures) = parameter_layout(&definition, &values).unwrap();
    assert_eq!(words.len(), 16);
    assert_eq!(words[0], 1);
    assert_eq!(
        &words[4..8],
        &[1f32.to_bits(), 2f32.to_bits(), 3f32.to_bits(), 0]
    );
    assert_eq!(words[14], 9f32.to_bits());
    assert_eq!(textures, ["texture"]);
    values.remove("extra");
    values.set("unrelated", DynamicValue::I32(-8)).unwrap();
    assert_eq!(
        parameter_layout(&definition, &values).unwrap(),
        (source, words, textures)
    );
    values.set("flag", DynamicValue::U32(1)).unwrap();
    assert!(parameter_layout(&definition, &values).is_err());
}

#[test]
fn pose_and_skin_variants_keep_custom_vertex_and_shadow_entries() {
    let definition = ShaderDefinition {
        backends: BTreeMap::from([(
            "glsl-es-300".into(),
            ipp_core::services::asset_management::shader::ShaderBackendSource {
                vertex: "void materialVertex() { ippDefaultVertex(); gl_Position.x += 0.1; }"
                    .into(),
                fragment: "vec4 materialFragment() { if(v_uv.x < 0.5) discard; return vec4(1); }"
                    .into(),
            },
        )]),
        ..Default::default()
    };
    for shadow in [false, true] {
        let config = RenderShaderConfig::default().with_lighting(false, true);
        #[cfg(feature = "mesh-poses")]
        let config = config.with_mesh_pose(true);
        #[cfg(feature = "skeletal-animation")]
        let config = config.with_skinning(true);
        let (vertex, fragment) = sources(config, &definition, "", true, shadow).unwrap();
        assert!(vertex.starts_with("#version 300 es"));
        assert!(vertex.contains("void main() { materialVertex(); }"));
        assert!(fragment.contains("discard"));
        assert!(fragment.contains(&format!("#define IPP_PASS_SHADOW {}", u8::from(shadow))));
        #[cfg(feature = "mesh-poses")]
        assert!(vertex.contains("mix(a_position, a_pose_position, u_pose_weight)"));
        #[cfg(feature = "skeletal-animation")]
        assert!(vertex.contains("skinned_position(local_position)"));
    }
}

#[test]
fn shader_requirements_validate_asset_types_without_retyping_properties() {
    let mut properties = DynamicProperties::default();
    let mesh = DynamicValue::Asset(AssetSource {
        kind: MESH_TYPE,
        uri: "file:///model.mesh".into(),
        variant: 3,
    });
    let key = properties.set("input", mesh.clone()).unwrap();
    let mut definition = ShaderDefinition::default();
    assert!(definition.accepts(&properties));
    definition
        .parameters
        .insert("input".into(), Kind::Texture2D);
    assert!(!definition.accepts(&properties));
    assert!(parameter_layout(&definition, &properties).is_err());
    assert_eq!(properties.get("input"), Some(mesh));

    assert_eq!(
        properties
            .set(
                "input",
                DynamicValue::Asset(AssetSource {
                    kind: TEXTURE_TYPE,
                    uri: "file:///image.png".into(),
                    variant: 4,
                })
            )
            .unwrap(),
        key
    );
    assert!(definition.accepts(&properties));
    let (source, words, textures) = parameter_layout(&definition, &properties).unwrap();
    assert!(source.contains("uniform sampler2D p_input;"));
    assert!(words.is_empty());
    assert_eq!(textures, ["input"]);
    let restored = ShaderDefinition::decode(&definition.encode().unwrap()).unwrap();
    assert_eq!(restored.parameters["input"], Kind::Texture2D);
}

#[test]
fn reusable_parameter_words_match_all_numeric_std140_layouts() {
    let cases = [
        (Kind::F32, DynamicValue::F32(-1.25)),
        (Kind::I32, DynamicValue::I32(-37)),
        (Kind::U32, DynamicValue::U32(u32::MAX)),
        (Kind::Bool, DynamicValue::Bool(true)),
        (Kind::Vec2, DynamicValue::Vec2([1.0, 2.0])),
        (Kind::Vec3, DynamicValue::Vec3([1.0, 2.0, 3.0])),
        (Kind::Vec4, DynamicValue::Vec4([1.0, 2.0, 3.0, 4.0])),
        (Kind::Mat2, DynamicValue::Mat2([1.0, 2.0, 3.0, 4.0])),
        (
            Kind::Mat3,
            DynamicValue::Mat3([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]),
        ),
        (
            Kind::Mat4,
            DynamicValue::Mat4(std::array::from_fn(|i| i as f32 + 0.25)),
        ),
    ];
    let mut definition = ShaderDefinition::default();
    let mut properties = DynamicProperties::default();
    let mut words = Vec::with_capacity(256);
    let pointer = words.as_ptr();
    for (index, (kind, value)) in cases.into_iter().enumerate() {
        let name = format!("p{index}");
        definition.parameters.insert(name.clone(), kind);
        properties.set(&name, value.clone()).unwrap();
        assert_eq!(properties.get(&name), Some(value));
        parameter_words(&definition, &properties, &mut words).unwrap();
        assert_eq!(words, parameter_layout(&definition, &properties).unwrap().1);
        assert_eq!(words.as_ptr(), pointer);
    }
    properties.set("p0", DynamicValue::F32(8.0)).unwrap();
    parameter_words(&definition, &properties, &mut words).unwrap();
    assert_eq!(words, parameter_layout(&definition, &properties).unwrap().1);
    properties.set("p0", DynamicValue::U32(8)).unwrap();
    assert!(parameter_words(&definition, &properties, &mut words).is_err());
}
