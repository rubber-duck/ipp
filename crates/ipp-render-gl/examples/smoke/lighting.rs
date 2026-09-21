//! Direct-light and shadow assertions shared by real native GL environments.
use super::world::{HEIGHT, WIDTH, apply, deliver, save};
use ipp_core::{
    Command, ComponentValue, EntityId, EntityRef, WorldContext,
    components::{Light, MeshInstance, PbrMaterial, Transform},
};
use ipp_render_gl::{RenderDevice, RenderService};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    fixture: &[u8],
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let mut world_host = ipp_core::HostRuntime::new();
    renderer.install(&mut world_host)?;
    let mut world = super::world::fixture_world(&mut world_host)?;
    let floor = add(
        &mut world,
        Transform {
            y: -0.05,
            sx: 3.5,
            sy: 0.05,
            sz: 3.0,
            ..Transform::default()
        },
        vec![
            ComponentValue::MeshInstance(MeshInstance {
                source: "fixture:///lighting.mesh".into(),
                variant: 0,
            }),
            ComponentValue::PbrMaterial(PbrMaterial {
                roughness: 0.9,
                cast_shadows: false,
                ..PbrMaterial::default()
            }),
        ],
    )?;
    let caster = add(
        &mut world,
        Transform {
            y: 0.65,
            sx: 0.65,
            sy: 0.65,
            sz: 0.65,
            ..Transform::default()
        },
        vec![
            ComponentValue::MeshInstance(MeshInstance {
                source: "fixture:///lighting.mesh".into(),
                variant: 0,
            }),
            ComponentValue::PbrMaterial(PbrMaterial::default()),
        ],
    )?;
    let mut light = Light {
        kind: 2,
        intensity: 55.0,
        inner_cone: 0.45,
        outer_cone: 0.85,
        cast_shadows: cfg!(feature = "shadows"),
        ..Light::default()
    };
    let spot = add(
        &mut world,
        aim(-2.0, 4.0, 2.0),
        vec![ComponentValue::Light(light)],
    )?;
    add(
        &mut world,
        aim(2.0, 4.0, -2.0),
        vec![ComponentValue::Light(Light {
            intensity: 0.2,
            ..Light::default()
        })],
    )?;
    deliver!(renderer, world_host, world, fixture, None)?;
    let stats = super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
    assert_eq!(stats.draw_calls, 2);
    #[cfg(feature = "shadows")]
    {
        assert_eq!(stats.shadow_draw_calls, 1);
        assert_eq!(stats.shadow_resident_bytes, 4 * 1024 * 1024);
    }
    let shadowed = capture()?;
    save(output, "lighting-shadowed", &shadowed)?;
    light.cast_shadows = false;
    replace(&mut world, spot, ComponentValue::Light(light))?;
    super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
    let lit = capture()?;
    save(output, "lighting-unshadowed", &lit)?;
    #[cfg(feature = "shadows")]
    {
        let darkened = darkened(&lit, &shadowed);
        assert!(
            darkened > 100,
            "spotlight must cast a measurable shadow: {darkened}"
        );
        light.cast_shadows = true;
        light.shadow_radius = 0.35;
        replace(&mut world, spot, ComponentValue::Light(light))?;
        super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
        let soft = capture()?;
        save(output, "lighting-soft-shadow", &soft)?;
        let softened = soft
            .as_chunks::<4>()
            .0
            .iter()
            .zip(shadowed.as_chunks::<4>().0.iter())
            .filter(|(soft, hard)| {
                (0..3)
                    .map(|i| i32::from(soft[i]) - i32::from(hard[i]))
                    .sum::<i32>()
                    > 18
            })
            .count();
        assert!(
            softened > 30,
            "finite emitter must soften the shadow: {softened}"
        );
        light.shadow_radius = 0.0;
        light.cast_shadows = true;
        replace(&mut world, spot, ComponentValue::Light(light))?;
        replace(
            &mut world,
            floor,
            ComponentValue::PbrMaterial(PbrMaterial {
                roughness: 0.9,
                cast_shadows: false,
                receive_shadows: false,
                ..PbrMaterial::default()
            }),
        )?;
        super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
        let unreceived = capture()?;
        save(output, "lighting-receiver-disabled", &unreceived)?;
        assert!(changed(&lit, &unreceived) < 20);
        replace(
            &mut world,
            floor,
            ComponentValue::PbrMaterial(PbrMaterial {
                roughness: 0.9,
                cast_shadows: false,
                ..PbrMaterial::default()
            }),
        )?;
        replace(
            &mut world,
            caster,
            ComponentValue::Transform(Transform {
                x: 1.1,
                y: 0.65,
                sx: 0.65,
                sy: 0.65,
                sz: 0.65,
                ..Transform::default()
            }),
        )?;
        super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
        let moved = capture()?;
        save(output, "lighting-caster-moved", &moved)?;
        assert!(changed(&shadowed, &moved) > 500);
        let world_id = world.id();
        drop(world);
        renderer.unload_host(&mut world_host);
        world_host.flush_resource_lifecycle();
        world = world_host.world_mut(world_id).unwrap();
        deliver!(renderer, world_host, world, fixture, None)?;
        super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
        let recovered = capture()?;
        save(output, "lighting-recovered", &recovered)?;
        assert_eq!(
            moved, recovered,
            "recovery restores depth pass and lit programs"
        );
    }
    #[cfg(feature = "shadows")]
    {
        replace(
            &mut world,
            caster,
            ComponentValue::Transform(Transform {
                y: 0.65,
                sx: 0.65,
                sy: 0.65,
                sz: 0.65,
                ..Transform::default()
            }),
        )?;
        light.kind = 2;
        light.intensity = 18.0;
        light.shadow_radius = 0.0;
        light.cast_shadows = true;
        replace(&mut world, spot, ComponentValue::Light(light))?;
        let mut sources = vec![spot];
        for (x, z) in [(2.0, 2.0), (-2.0, -2.0), (2.0, -2.0)] {
            sources.push(add(
                &mut world,
                aim(x, 4.0, z),
                vec![ComponentValue::Light(light)],
            )?);
        }
        let stats = super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
        assert_eq!(stats.shadow_draw_calls, 4);
        assert_eq!(stats.shadow_resident_bytes, 4 * 2048 * 2048);
        let all = capture()?;
        save(output, "four-shadows", &all)?;
        for (index, source) in sources.iter().enumerate() {
            replace(
                &mut world,
                *source,
                ComponentValue::Light(Light {
                    cast_shadows: false,
                    ..light
                }),
            )?;
            super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
            let without = capture()?;
            save(output, &format!("without-shadow-{index}"), &without)?;
            assert!(
                darkened(&without, &all) > 40,
                "Shadow source {index} must independently darken the receiver"
            );
            replace(&mut world, *source, ComponentValue::Light(light))?;
        }
        // Release the additional sources before the original point/directional checks.
        for source in sources.into_iter().skip(1) {
            apply(
                &mut world,
                vec![Command::Delete {
                    entity: EntityRef::Handle(source),
                }],
            )?;
        }
    }
    let _ = (floor, caster, shadowed);
    for (kind, label, intensity) in [(1, "point", 55.0), (0, "directional", 2.0)] {
        light.kind = kind;
        light.cast_shadows = false;
        light.intensity = intensity;
        replace(&mut world, spot, ComponentValue::Light(light))?;
        super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
        let pixels = capture()?;
        save(output, &format!("lighting-{label}"), &pixels)?;
        assert!(changed(&lit, &pixels) > 100);
    }
    let baseline = capture()?;
    let mut extra_lights = Vec::new();
    for _ in 0..12 {
        extra_lights.push(add(
            &mut world,
            Transform::default(),
            vec![ComponentValue::Light(Light {
                kind: 0,
                r: 0.0,
                g: 1.0,
                b: 0.0,
                intensity: 3.0,
                cast_shadows: false,
                ..Light::default()
            })],
        )?);
    }
    let stats = super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
    assert_eq!(stats.draw_calls, 2);
    let many = capture()?;
    save(output, "lighting-fourteen-candidates", &many)?;
    assert!(
        changed(&baseline, &many) > 100,
        "more than eight lights must continue illuminating"
    );
    for entity in extra_lights {
        replace(
            &mut world,
            entity,
            ComponentValue::Light(Light {
                intensity: 0.0,
                ..Light::default()
            }),
        )?;
    }
    super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
    drop(world);
    let restored = capture()?;
    save(output, "lighting-zeroed-candidates", &restored)?;
    assert!(
        changed(&restored, &baseline) < 5,
        "zero-contribution lights leave selections immediately"
    );
    Ok(())
}

fn add(
    world: &mut WorldContext<'_>,
    transform: Transform,
    values: Vec<ComponentValue>,
) -> Result<EntityId> {
    let mut commands = vec![
        Command::Create {
            alias: 99,
            metadata: Default::default(),
        },
        Command::InsertComponentValue {
            entity: EntityRef::Alias(99),
            value: ComponentValue::Transform(transform),
        },
    ];
    commands.extend(
        values
            .into_iter()
            .map(|value| Command::InsertComponentValue {
                entity: EntityRef::Alias(99),
                value,
            }),
    );
    world.enqueue(ipp_core::Batch {
        id: world.tick() + 1,
        operations: commands,
    })?;
    let report = world.step(0.0)?;
    let outcome = report
        .outcomes
        .into_iter()
        .next()
        .ok_or("missing fixture outcome")?;
    let aliases = outcome
        .result
        .map_err(|error| format!("fixture mutation: {error:?}"))?;
    Ok(aliases
        .into_iter()
        .find(|(alias, _)| *alias == 99)
        .ok_or("missing fixture alias")?
        .1)
}

fn replace(world: &mut WorldContext<'_>, id: EntityId, value: ComponentValue) -> Result<()> {
    apply(
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(id),
                component: value.type_id(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Handle(id),
                value,
            },
        ],
    )
}

fn aim(x: f32, y: f32, z: f32) -> Transform {
    let yaw = x.atan2(z) * 0.5;
    let pitch = -y.atan2(x.hypot(z)) * 0.5;
    Transform {
        x,
        y,
        z,
        qx: pitch.sin() * yaw.cos(),
        qy: pitch.cos() * yaw.sin(),
        qz: -pitch.sin() * yaw.sin(),
        qw: pitch.cos() * yaw.cos(),
        ..Transform::default()
    }
}

fn changed(a: &[u8], b: &[u8]) -> usize {
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .filter(|(a, b)| (0..3).any(|i| a[i].abs_diff(b[i]) > 4))
        .count()
}

#[cfg(feature = "shadows")]
fn darkened(a: &[u8], b: &[u8]) -> usize {
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .filter(|(a, b)| {
            (0..3)
                .map(|i| i32::from(a[i]) - i32::from(b[i]))
                .sum::<i32>()
                > 36
        })
        .count()
}

/// The same Rust-exported normal corpus is exercised by the browser harness.
pub fn normals<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    fixtures: &Path,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let mut frames = Vec::new();
    for (name, scaled) in [
        ("sphere", false),
        ("sphere-flat", false),
        ("sphere", true),
        ("sphere-baked", false),
    ] {
        let bytes = std::fs::read(fixtures.join(format!("{name}.mesh")))?;
        let (mesh, _) = ipp_core::MeshAsset::decode(&bytes)?;
        let mut world_host = ipp_core::HostRuntime::new();
        renderer.install(&mut world_host)?;
        let mut world = super::world::fixture_world(&mut world_host)?;
        let transform = if scaled {
            Transform {
                sx: 1.6,
                sy: 0.65,
                sz: 1.1,
                qy: 0.3f32.sin(),
                qw: 0.3f32.cos(),
                ..Transform::default()
            }
        } else {
            Transform::default()
        };
        add(
            &mut world,
            transform,
            vec![
                ComponentValue::MeshInstance(MeshInstance {
                    source: "fixture:///normal.mesh".into(),
                    variant: 0,
                }),
                ComponentValue::PbrMaterial(PbrMaterial {
                    r: 0.6,
                    g: 0.3,
                    b: 0.15,
                    roughness: 0.2,
                    metallic: 0.25,
                    ..PbrMaterial::default()
                }),
            ],
        )?;
        add(
            &mut world,
            aim(-2.0, 4.0, 2.0),
            vec![ComponentValue::Light(Light {
                intensity: 2.0,
                ..Light::default()
            })],
        )?;
        let stats = deliver!(renderer, world_host, world, &bytes, None)?;
        assert_eq!(stats.draw_calls, 1);
        assert_eq!(stats.failed_draw_calls, 0);
        assert_eq!(
            stats.uploaded_bytes,
            (mesh.vertex_bytes() + std::mem::size_of_val(mesh.indices())) as u32
        );
        let label = if scaled {
            "sphere-scaled"
        } else {
            name
        };
        let pixels = capture()?;
        save(output, &format!("normals-{label}"), &pixels)?;
        let world_id = world.id();
        drop(world);
        renderer.unload_host(&mut world_host);
        world_host.flush_resource_lifecycle();
        world = world_host.world_mut(world_id).unwrap();
        deliver!(renderer, world_host, world, &bytes, None)?;
        assert_eq!(
            pixels,
            capture()?,
            "Normal streams and shader variants must recover"
        );
        drop(world);
        frames.push(pixels);
    }
    let facets = changed(&frames[0], &frames[1]);
    let transformed = changed(&frames[2], &frames[3]);
    assert!(
        facets > 300,
        "Smooth normals must change the same sphere triangles: {facets}"
    );
    assert!(
        transformed < 10,
        "Independent baked transform must agree: {transformed}"
    );
    std::fs::write(
        output.join("normal-samples.csv"),
        format!("comparison,changed_pixels\nsmooth-flat,{facets}\nscaled-baked,{transformed}\n"),
    )?;
    Ok(())
}

/// Texture modulation and light response through native GPU resources and recovery.
pub fn textures<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mesh: &[u8],
    texture: &[u8],
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    use ipp_core::components::BaseColorTexture;

    let mut host = ipp_core::HostRuntime::new();
    renderer.install(&mut host)?;
    let mut world = super::world::fixture_world(&mut host)?;
    let cube = add(
        &mut world,
        Transform::default(),
        vec![
            ComponentValue::MeshInstance(MeshInstance {
                source: "fixture:///lit.mesh".into(),
                variant: 0,
            }),
            ComponentValue::PbrMaterial(PbrMaterial {
                r: 0.7,
                g: 0.4,
                b: 0.2,
                roughness: 0.8,
                ..PbrMaterial::default()
            }),
        ],
    )?;
    let light = Light {
        intensity: 2.0,
        ..Light::default()
    };
    let sun = add(
        &mut world,
        aim(2.0, 4.0, 3.0),
        vec![ComponentValue::Light(light)],
    )?;
    assert_eq!(
        deliver!(renderer, host, world, mesh, Some(texture))?.draw_calls,
        1
    );
    let solid = capture()?;
    save(output, "pbr-solid", &solid)?;
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(cube),
            value: ComponentValue::BaseColorTexture(BaseColorTexture {
                source: "fixture:///lit.texture".into(),
                variant: 0,
            }),
        }],
    )?;
    let stats = deliver!(renderer, host, world, mesh, Some(texture))?;
    assert_eq!(
        stats.draw_calls,
        1,
        "stats={stats:?} items={:?} resources={:?}",
        world.render_items(),
        world
            .asset_resources()
            .iter()
            .map(|r| (r.source().clone(), r.status().clone()))
            .collect::<Vec<_>>()
    );
    let textured = capture()?;
    save(output, "pbr-texture", &textured)?;
    assert!(
        changed(&solid, &textured) > 500,
        "Texture must modulate PBR base color"
    );
    replace(
        &mut world,
        sun,
        ComponentValue::Light(Light {
            intensity: 0.0,
            ..light
        }),
    )?;
    super::world::render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
    let dark = capture()?;
    save(output, "pbr-texture-dark", &dark)?;
    assert!(
        changed(&textured, &dark) > 1000,
        "Textured surfaces must respond to light"
    );
    let ambient = ipp_core::RenderStatePatch {
        ambient_light: Some([0.25, 0.5, 0.75]),
        ..Default::default()
    };
    world.enqueue_render_state_update(ambient)?;
    super::world::render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
    let filled = capture()?;
    save(output, "pbr-ambient", &filled)?;
    assert!(
        changed(&filled, &dark) > 1000,
        "Ambient state must illuminate textured PBR without direct lights"
    );
    world.enqueue_render_state_update(ipp_core::RenderStatePatch {
        ambient_light: Some([0.0; 3]),
        ..Default::default()
    })?;
    super::world::render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
    assert_eq!(dark, capture()?, "Zero ambient must restore the dark frame");
    replace(&mut world, sun, ComponentValue::Light(light))?;
    let id = world.id();
    drop(world);
    renderer.unload_host(&mut host);
    host.flush_resource_lifecycle();
    let mut world = host.world_mut(id).unwrap();
    deliver!(renderer, host, world, mesh, Some(texture))?;
    assert_eq!(
        textured,
        capture()?,
        "Textured PBR restores its immutable GPU inputs"
    );

    #[cfg(feature = "shadows")]
    {
        replace(
            &mut world,
            sun,
            ComponentValue::Light(Light {
                kind: 2,
                intensity: 55.0,
                inner_cone: 0.45,
                outer_cone: 0.85,
                cast_shadows: true,
                ..light
            }),
        )?;
        let stats = super::world::render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
        assert_eq!(stats.shadow_draw_calls, 1);
        assert_eq!(stats.failed_draw_calls, 0);
        let spotlight = capture()?;
        save(output, "pbr-texture-spotlight", &spotlight)?;
        assert!(
            changed(&spotlight, &dark) > 1000,
            "Native lit texture and shadow samplers must bind together"
        );
    }
    drop(world);
    Ok(())
}

/// Exercise the same authored discard in both custom surface and shadow programs.
#[cfg(feature = "shadows")]
#[allow(dead_code)] // Shared fixture module is also compiled by the built-in smoke runner.
pub fn run_custom<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    fixture: &[u8],
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    use ipp_core::{
        DynamicValue, FieldValue, FieldWrite,
        services::asset_management::{
            AssetUpload, AssetUploadIdentity,
            shader::{SHADER_TYPE, ShaderBackendSource, ShaderDefinition, ShaderParameterKind},
        },
    };
    use std::collections::BTreeMap;
    let mut host = ipp_core::HostRuntime::new();
    renderer.install(&mut host)?;
    let mut world = super::world::fixture_world(&mut host)?;
    let mesh = || {
        ComponentValue::MeshInstance(MeshInstance {
            source: "fixture:///custom-shadow.mesh".into(),
            variant: 0,
        })
    };
    add(
        &mut world,
        Transform {
            y: -0.05,
            sx: 3.5,
            sy: 0.05,
            sz: 3.0,
            ..Default::default()
        },
        vec![
            mesh(),
            ComponentValue::PbrMaterial(PbrMaterial {
                cast_shadows: false,
                roughness: 0.9,
                ..Default::default()
            }),
        ],
    )?;
    let mut material = ipp_core::components::CustomMaterial {
        source: "asset://13/991".into(),
        alpha_mode: 1,
        casts_shadows: true,
        ..Default::default()
    };
    material
        .properties
        .set("coverage", DynamicValue::F32(1.0))
        .unwrap();
    let caster = add(
        &mut world,
        Transform {
            y: 0.65,
            sx: 0.65,
            sy: 0.65,
            sz: 0.65,
            ..Default::default()
        },
        vec![mesh(), ComponentValue::CustomMaterial(material)],
    )?;
    add(
        &mut world,
        aim(-2.0, 4.0, 2.0),
        vec![ComponentValue::Light(Light {
            kind: 2,
            intensity: 55.0,
            inner_cone: 0.45,
            outer_cone: 0.85,
            cast_shadows: true,
            ..Default::default()
        })],
    )?;
    let definition = ShaderDefinition {
        recipe: ipp_core::services::asset_management::shader::ShaderRecipe {
            features: 16,
            ..Default::default()
        },
        parameters: BTreeMap::from([("coverage".into(), ShaderParameterKind::F32)]),
        backends: BTreeMap::from([(
            "glsl-es-300".into(),
            ShaderBackendSource {
                vertex: "void materialVertex() { ippDefaultVertex(); }".into(),
                fragment: "vec4 materialFragment() { return vec4(0,1,0,p_coverage); }".into(),
            },
        )]),
        ..Default::default()
    };
    world.enqueue_asset(AssetUpload {
        id: 991,
        key: AssetUploadIdentity {
            kind: SHADER_TYPE,
            asset: 991,
            variant: 0,
        },
        bytes: definition.encode()?,
    })?;
    deliver!(renderer, host, world, fixture, None)?;
    for _ in 0..8 {
        super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    }
    assert!(
        renderer.custom_material_diagnostics().is_empty(),
        "{:?}",
        renderer.custom_material_diagnostics()
    );
    let full = capture()?;
    save(output, "custom-shadow-full", &full)?;
    apply(
        &mut world,
        vec![ipp_core::Command::SetDynamicProperty {
            entity: EntityRef::Handle(caster),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: "coverage".into(),
            value: DynamicValue::F32(0.0),
        }],
    )?;
    super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    let cutout = capture()?;
    save(output, "custom-shadow-cutout", &cutout)?;
    apply(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(caster),
            component: ComponentValue::CUSTOM_MATERIAL,
            field: FieldWrite {
                offset: std::mem::offset_of!(ipp_core::components::CustomMaterial, casts_shadows)
                    as u32,
                value: FieldValue::Bool(false),
            },
        }],
    )?;
    super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    drop(world);
    let absent = capture()?;
    save(output, "custom-shadow-disabled", &absent)?;
    assert_eq!(
        cutout, absent,
        "discarded fragments must never write shadow depth"
    );
    assert!(
        darkened(&cutout, &full) > 100,
        "custom opaque caster must produce a visible shadow"
    );
    Ok(())
}
