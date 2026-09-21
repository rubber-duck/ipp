//! Custom material scenarios shared by native GL environment drivers.
use super::world::{HEIGHT, WIDTH, apply, fixture_world, save};
use ipp_core::{
    components::{CustomMaterial, MeshInstance, Transform, UnlitMaterial},
    services::asset_management::{
        AssetSource,
        shader::{SHADER_TYPE, ShaderBackendSource, ShaderDefinition, ShaderParameterKind},
    },
    *,
};
use ipp_render_gl::{RenderDevice, RenderService};
use std::{collections::BTreeMap, path::Path};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mesh: &[u8],
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    mut replacement: impl FnMut() -> Result<D>,
    output: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let mut world = fixture_world(&mut host)?;
    let mut material = CustomMaterial {
        source: "client://native/scene/shader#1".into(),
        ..CustomMaterial::default()
    };
    material
        .properties
        .set("tint", DynamicValue::Vec4([0.0, 1.0, 0.0, 1.0]))
        .unwrap();
    apply(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: EntityMetadata {
                    symbolic_id: Some("custom".into()),
                    classes: vec![],
                },
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Transform(Transform::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::MeshInstance(MeshInstance {
                    source: "client://native/scene/mesh#1".into(),
                    variant: 0,
                }),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::UnlitMaterial(UnlitMaterial {
                    r: 0.0,
                    g: 0.0,
                    b: 1.0,
                }),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::CustomMaterial(material),
            },
        ],
    )?;
    let entity = world
        .entities()
        .iter()
        .find(|e| e.metadata.symbolic_id.as_deref() == Some("custom"))
        .unwrap()
        .id;
    let definition = ShaderDefinition {
        parameters: BTreeMap::from([("tint".into(), ShaderParameterKind::Vec4)]),
        backends: BTreeMap::from([(
            "glsl-es-300".into(),
            ShaderBackendSource {
                vertex: String::new(),
                fragment: "vec4 materialFragment() { return p_tint; }".into(),
            },
        )]),
        required_attributes: 0,
        ..Default::default()
    };
    let shader = definition.encode()?;
    let world_id = world.id();

    for (kind, uri, bytes) in [
        (SHADER_TYPE, "client://native/scene/shader#1", shader),
        (MESH_TYPE, "client://native/scene/mesh#1", mesh.to_vec()),
    ] {
        world.asset_resources_mut().register_client_source(
            world_id,
            AssetSource {
                kind,
                uri: uri.into(),
                variant: 0,
            },
            bytes,
        )?;
    }
    for _ in 0..8 {
        super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    }
    super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    let green = capture()?;
    save(output, "custom-green", &green)?;
    assert!(
        renderer.custom_material_diagnostics().is_empty(),
        "{:?}",
        renderer.custom_material_diagnostics()
    );
    center(&green, [0, 255, 0]);
    let programs = renderer.cached_program_count();
    apply(
        &mut world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: "tint".into(),
            value: DynamicValue::Vec4([1.0, 0.0, 0.0, 1.0]),
        }],
    )?;
    super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    let red = capture()?;
    save(output, "custom-red", &red)?;
    center(&red, [255, 0, 0]);
    assert_eq!(
        renderer.cached_program_count(),
        programs,
        "uniform values must not create programs"
    );
    apply(
        &mut world,
        vec![Command::RemoveDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: "tint".into(),
        }],
    )?;
    super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    let fallback = capture()?;
    save(output, "custom-fallback-blue", &fallback)?;
    center(&fallback, [0, 0, 79]);
    assert!(renderer.custom_material_diagnostics().contains_key(&entity));
    apply(
        &mut world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::UNLIT_MATERIAL,
        }],
    )?;
    super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    let fallback = capture()?;
    save(output, "custom-fallback-red", &fallback)?;
    center(&fallback, [255, 0, 0]);
    let camera = world.active_camera().unwrap();
    let mut back = CustomMaterial {
        source: "client://native/scene/shader#1".into(),
        ..Default::default()
    };
    back.properties
        .set("tint", DynamicValue::Vec4([1.0, 0.0, 0.0, 1.0]))
        .unwrap();
    apply(
        &mut world,
        vec![
            Command::InsertComponentValue {
                entity: EntityRef::Handle(camera),
                value: ComponentValue::Transform(Transform {
                    z: 6.0,
                    ..Default::default()
                }),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Handle(entity),
                value: ComponentValue::Transform(Transform {
                    z: 0.5,
                    ..Default::default()
                }),
            },
            Command::SetDynamicProperty {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: "tint".into(),
                value: DynamicValue::Vec4([0.0, 1.0, 0.0, 0.5]),
            },
            Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CUSTOM_MATERIAL,
                field: FieldWrite {
                    offset: std::mem::offset_of!(CustomMaterial, alpha_mode) as u32,
                    value: FieldValue::U32(2),
                },
            },
            Command::Create {
                alias: 2,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(2),
                value: ComponentValue::Transform(Transform {
                    z: -0.5,
                    ..Default::default()
                }),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(2),
                value: ComponentValue::MeshInstance(MeshInstance {
                    source: "client://native/scene/mesh#1".into(),
                    variant: 0,
                }),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(2),
                value: ComponentValue::CustomMaterial(back),
            },
        ],
    )?;
    let stats = super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    assert_eq!(stats.draw_calls, 2);
    let blended = capture()?;
    save(output, "custom-linear-blend", &blended)?;
    center(&blended, [187, 188, 0]);

    let mut packed = CustomMaterial {
        source: "asset://13/992".into(),
        ..Default::default()
    };
    for (name, value) in [
        ("number", DynamicValue::F32(0.25)),
        ("signed", DynamicValue::I32(-7)),
        ("unsigned", DynamicValue::U32(19)),
        ("flag", DynamicValue::Bool(true)),
        ("pair", DynamicValue::Vec2([1.0, 2.0])),
        ("triple", DynamicValue::Vec3([1.0, 2.0, 3.0])),
        ("quad", DynamicValue::Vec4([1.0, 2.0, 3.0, 4.0])),
        ("small", DynamicValue::Mat2([1.0, 2.0, 3.0, 4.0])),
        ("medium", DynamicValue::Mat3([1.0; 9])),
        ("large", DynamicValue::Mat4([2.0; 16])),
        (
            "first",
            DynamicValue::Asset(AssetSource {
                kind: TEXTURE_TYPE,
                uri: "asset://2/993".into(),
                variant: 0,
            }),
        ),
        (
            "second",
            DynamicValue::Asset(AssetSource {
                kind: TEXTURE_TYPE,
                uri: "asset://2/994".into(),
                variant: 0,
            }),
        ),
    ] {
        packed.properties.set(name, value).unwrap();
    }
    let definition = ShaderDefinition {
        parameters: packed.properties.descriptors().keys().map(|name| {
            (name.clone(), ShaderParameterKind::from_value(&packed.properties.get(name).unwrap()).unwrap())
        }).collect(),
        backends: BTreeMap::from([("glsl-es-300".into(), ShaderBackendSource {
            vertex: String::new(),
            fragment: r#"vec4 materialFragment() {
    bool valid = p_number == 0.25 && p_signed == -7 && p_unsigned == 19u && p_flag
        && p_pair.y == 2.0 && p_triple.z == 3.0 && p_quad.w == 4.0
        && p_small[1][0] == 3.0 && p_medium[2][1] == 1.0 && p_large[3][2] == 2.0;
    return valid ? vec4(0.5*(texture(p_first,v_uv).rgb + texture(p_second,v_uv).rgb),1) : vec4(1,0,0,1);
}"#.into(),
        })]),
        ..Default::default()
    };
    use ipp_core::services::asset_management::{AssetUpload, AssetUploadIdentity};
    world.enqueue_asset(AssetUpload {
        id: 992,
        key: AssetUploadIdentity {
            kind: SHADER_TYPE,
            asset: 992,
            variant: 0,
        },
        bytes: definition.encode()?,
    })?;
    for (asset, rgb) in [(993, [255, 0, 0]), (994, [0, 255, 0])] {
        let mut bytes = b"IPPT".to_vec();
        for value in [3u32, 1, 1] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(rgb);
        bytes.push(255);
        world.enqueue_asset(AssetUpload {
            id: asset,
            key: AssetUploadIdentity {
                kind: TEXTURE_TYPE,
                asset,
                variant: 0,
            },
            bytes,
        })?;
    }
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::CustomMaterial(packed),
        }],
    )?;
    for _ in 0..8 {
        super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    }
    assert!(
        renderer.custom_material_diagnostics().is_empty(),
        "{:?}",
        renderer.custom_material_diagnostics()
    );
    let packed = capture()?;
    save(output, "custom-packing-textures", &packed)?;
    center(&packed, [188, 188, 0]);

    let world_id = world.id();
    drop(world);
    renderer.replace_device(&mut host, replacement()?)?;
    world = host.world_mut(world_id).unwrap();
    for _ in 0..8 {
        super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    }
    assert!(
        world.take_resource_requests().is_empty(),
        "Host-owned sources recover without producer I/O"
    );
    let restored = capture()?;
    save(output, "custom-packing-device-replacement", &restored)?;
    assert_eq!(
        packed, restored,
        "GPU programs, meshes and textures recover from retained sources"
    );

    let mut limited = CustomMaterial {
        source: "asset://13/995".into(),
        ..Default::default()
    };
    for index in 0..64 {
        limited
            .properties
            .set(
                &format!("texture{index}"),
                DynamicValue::Asset(AssetSource {
                    kind: TEXTURE_TYPE,
                    uri: "asset://2/993".into(),
                    variant: 0,
                }),
            )
            .unwrap();
    }
    let definition = ShaderDefinition {
        parameters: limited
            .properties
            .descriptors()
            .keys()
            .map(|name| {
                (
                    name.clone(),
                    ShaderParameterKind::from_value(&limited.properties.get(name).unwrap())
                        .unwrap(),
                )
            })
            .collect(),
        backends: BTreeMap::from([(
            "glsl-es-300".into(),
            ShaderBackendSource {
                vertex: String::new(),
                fragment: "vec4 materialFragment() { return vec4(0,1,0,1); }".into(),
            },
        )]),
        ..Default::default()
    };
    world.enqueue_asset(AssetUpload {
        id: 995,
        key: AssetUploadIdentity {
            kind: SHADER_TYPE,
            asset: 995,
            variant: 0,
        },
        bytes: definition.encode()?,
    })?;
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::CustomMaterial(limited),
        }],
    )?;
    for _ in 0..8 {
        super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    }
    assert!(renderer.custom_material_diagnostics()[&entity].contains("limits"));
    let limited = capture()?;
    save(output, "custom-device-limit-fallback", &limited)?;
    center(&limited, [255, 0, 0]);

    // Preserve near-black display levels through the intermediate color target.
    let gradient = ShaderDefinition {
        parameters: BTreeMap::new(),
        backends: BTreeMap::from([("glsl-es-300".into(), ShaderBackendSource {
            vertex: String::new(),
            fragment: "vec4 materialFragment() { float v = 0.001 + clamp((gl_FragCoord.x - 140.0) / 40.0, 0.0, 1.0) * 0.07; return vec4(vec3(v),1); }".into(),
        })]),
        required_attributes: 0,
        ..Default::default()
    };
    world.enqueue_asset(AssetUpload {
        id: 996,
        key: AssetUploadIdentity {
            kind: SHADER_TYPE,
            asset: 996,
            variant: 0,
        },
        bytes: gradient.encode()?,
    })?;
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::CustomMaterial(CustomMaterial {
                source: "asset://13/996".into(),
                ..Default::default()
            }),
        }],
    )?;
    for _ in 0..8 {
        super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    }

    let gradient = capture()?;
    save(output, "custom-dark-gradient", &gradient)?;

    let ramp: Vec<u8> = (145..175)
        .map(|x| gradient[((HEIGHT / 2 * WIDTH + x) * 4) as usize])
        .collect();
    let levels: std::collections::BTreeSet<_> = ramp.iter().copied().collect();
    assert!(
        levels.len() >= 22,
        "dark gradient lost display levels: {ramp:?}"
    );
    assert!(
        ramp.windows(2).all(|v| v[1] >= v[0] && v[1] - v[0] <= 3),
        "dark gradient is banded: {ramp:?}"
    );

    Ok(())
}

fn center(pixels: &[u8], expected: [u8; 3]) {
    for y in HEIGHT / 2 - 4..HEIGHT / 2 + 4 {
        for x in WIDTH / 2 - 4..WIDTH / 2 + 4 {
            let pixel = &pixels[((y * WIDTH + x) * 4) as usize..][..3];
            assert!(
                pixel.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                "custom center pixel {pixel:?}, expected {expected:?}"
            );
        }
    }
}
