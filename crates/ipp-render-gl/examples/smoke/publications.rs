//! Publication composition scenarios independent of graphics context creation.

use ipp_core::components::{Camera, CustomMaterial, MeshInstance, Transform, UnlitMaterial};
use ipp_core::services::asset_management::{
    AssetSource,
    shader::{SHADER_TYPE, ShaderBackendSource, ShaderDefinition, ShaderParameterKind},
};
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, HostRuntime, OutputKind, OutputRef,
    WorldAttachment, WorldId, WorldViewport,
};
use ipp_render_gl::{RenderDevice, RenderService};
use std::{collections::BTreeMap, path::Path};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const SIZE: u32 = 256;

fn viewport() -> WorldViewport {
    WorldViewport {
        width: SIZE,
        height: SIZE,
        device_pixel_ratio: 1.0,
    }
}

pub(super) fn apply(
    host: &mut HostRuntime,
    world: WorldId,
    operations: Vec<Command>,
) -> Result<Vec<(u32, EntityId)>> {
    let outcome = apply_batch(
        host,
        world,
        Batch {
            id: 1,
            operations,
        },
    )?;

    Ok(outcome.result.map_err(|error| format!("{error:?}"))?)
}

/// Queue one batch, run a Host frame and return the batch's outcome.
pub(super) fn apply_batch(
    host: &mut HostRuntime,
    world: WorldId,
    batch: Batch,
) -> Result<ipp_core::BatchOutcome> {
    host.world_mut(world)
        .ok_or_else(|| format!("unknown World {world:?}"))?
        .enqueue(batch)?;

    let update = host
        .frame(0.0)?
        .worlds
        .remove(&world)
        .ok_or_else(|| format!("World {world:?} was not updated"))?
        .map_err(|reason| format!("World {world:?} update: {reason:?}"))?;

    Ok(update
        .outcomes
        .into_iter()
        .next()
        .ok_or_else(|| format!("World {world:?} reported no batch outcome"))?)
}

pub(super) fn create(
    host: &mut HostRuntime,
    world: WorldId,
    values: Vec<ComponentValue>,
) -> Result<EntityId> {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(0), value)),
    );
    Ok(apply(host, world, operations)?[0].1)
}

fn quad_bytes() -> Vec<u8> {
    let mut bytes = b"IPPM".to_vec();
    for value in [3_u32, 4, 6, 2] {
        bytes.extend(value.to_le_bytes());
    }
    for semantic in [0, 4] {
        bytes.extend([semantic, 1, 0, 0]);
        bytes.extend(48_u32.to_le_bytes());
    }
    for vertex in [
        [-0.5_f32, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [0.5, 0.5, 0.0],
        [-0.5, 0.5, 0.0],
    ] {
        for value in vertex {
            bytes.extend(value.to_le_bytes());
        }
    }
    for _ in 0..4 {
        for value in [0.0_f32, 0.0, 1.0] {
            bytes.extend(value.to_le_bytes());
        }
    }
    for index in [0_u16, 1, 2, 0, 2, 3] {
        bytes.extend(index.to_le_bytes());
    }
    bytes
}

pub(super) fn mesh(
    host: &mut HostRuntime,
    world: WorldId,
    transform: Transform,
    color: [f32; 3],
) -> Result<EntityId> {
    let source = AssetSource {
        kind: ipp_core::MESH_TYPE,
        uri: format!("producer://{}/1/10", world.0).into(),
        variant: 0,
    };
    if host.asset_resources().find(&source).is_none() {
        host.asset_resources_mut()
            .register_client_source(world, source.clone(), quad_bytes())?;
    }
    create(
        host,
        world,
        vec![
            ComponentValue::Transform(transform),
            ComponentValue::MeshInstance(MeshInstance {
                source: source.uri,
                variant: 0,
            }),
            ComponentValue::UnlitMaterial(UnlitMaterial {
                r: color[0],
                g: color[1],
                b: color[2],
            }),
        ],
    )
}

pub(super) fn camera(host: &mut HostRuntime, world: WorldId, height: f32) -> Result<OutputRef> {
    let entity = create(
        host,
        world,
        vec![
            ComponentValue::Transform(Transform {
                z: 5.0,
                ..Default::default()
            }),
            ComponentValue::Camera(Camera {
                projection: 1,
                ortho_height: height,
                ..Default::default()
            }),
        ],
    )?;
    Ok(host.bind_output(host.world_ref(world).unwrap(), entity, OutputKind::Camera)?)
}

fn attach(
    host: &mut HostRuntime,
    parent: WorldId,
    child: WorldId,
    transform: Transform,
) -> Result<EntityId> {
    let child = host.world_ref(child).unwrap();
    create(
        host,
        parent,
        vec![
            ComponentValue::Transform(transform),
            ComponentValue::WorldAttachment(WorldAttachment::spatial(child)),
        ],
    )
}

fn settle<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut HostRuntime,
    selection: OutputRef,
) -> Result<()> {
    for _ in 0..4 {
        let report = host.frame(0.0)?;
        assert!(
            report.worlds.values().all(std::result::Result::is_ok),
            "{:?}",
            report.worlds
        );
        assert!(
            report.publication_errors.is_empty(),
            "{:?}",
            report.publication_errors
        );
        renderer.prepare(
            host,
            host.root_output(selection.world().id())
                .map(|(output, _, publication)| (output, publication)),
        )?;
        host.progress_assets();
    }
    let publication = host.root_output(selection.world().id()).unwrap().2;
    let summary = renderer.draw(host, selection, publication, viewport(), 1.0)?;
    assert_eq!(summary.failed_draw_calls, 0);
    assert!(!summary.invalid_camera);
    Ok(())
}

pub(super) fn save(output: &Path, name: &str, pixels: &[u8]) -> Result<()> {
    std::fs::write(output.join(format!("{name}.rgba")), pixels)?;
    let mut ppm = format!("P6\n{SIZE} {SIZE}\n255\n").into_bytes();
    for pixel in pixels.as_chunks::<4>().0 {
        ppm.extend_from_slice(&pixel[..3]);
    }
    std::fs::write(output.join(format!("{name}.ppm")), ppm)?;
    Ok(())
}

pub(super) fn assert_color(pixels: &[u8], horizontal: usize, vertical: usize, linear: [f32; 3]) {
    let expected = linear.map(|value| {
        let value = if value <= 0.0031308 {
            12.92 * value
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        (value * 255.0).round() as i32
    });
    for row in vertical - 1..=vertical + 1 {
        for column in horizontal - 1..=horizontal + 1 {
            let offset = (row * SIZE as usize + column) * 4;
            for channel in 0..3 {
                assert!(
                    (i32::from(pixels[offset + channel]) - expected[channel]).abs() <= 3,
                    "pixel ({column},{row}) channel {channel}: {} != {}",
                    pixels[offset + channel],
                    expected[channel]
                );
            }
        }
    }
}

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    mut replacement: impl FnMut() -> Result<D>,
    output: &Path,
) -> Result<()> {
    selected_roots(renderer, &mut capture, &mut replacement, output)?;
    spatial_child_recovery(renderer, &mut capture, &mut replacement, output)?;
    containing_debug_policy(renderer, &mut capture, output)?;
    transparency(renderer, &mut capture, output)?;
    spatial_shadows(renderer, &mut capture, output)?;
    nested(renderer, &mut capture, output)?;
    Ok(())
}

fn private_programs(
    host: &HostRuntime,
) -> std::collections::BTreeSet<ipp_core::services::asset_management::AssetKey> {
    host.asset_resources()
        .iter()
        .filter(|resource| resource.source().uri.starts_with("ipp-render://program/"))
        .map(|resource| resource.key())
        .collect()
}

fn selected_roots<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    replacement: &mut impl FnMut() -> Result<D>,
    output: &Path,
) -> Result<()> {
    use ipp_core::services::asset_management::{AssetLoadStatus, shader::ShaderRecipe};
    use ipp_core::{
        components::BoundingGeometry,
        systems::geometry::{GeometryDefinition, GeometryShape},
    };

    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let first = host.create_world(Default::default(), &super::selection::scene())?;
    let second = host.create_world(Default::default(), &super::selection::scene())?;
    mesh(&mut host, first, Transform::default(), [0.0, 0.5, 0.0])?;
    let blue = mesh(&mut host, second, Transform::default(), [0.0, 0.0, 0.5])?;
    let first_output = camera(&mut host, first, 2.0)?;
    let second_output = camera(&mut host, second, 2.0)?;
    host.set_root_output(first_output, viewport())?;
    host.set_root_output(second_output, viewport())?;
    create(
        &mut host,
        second,
        vec![
            ComponentValue::Transform(Transform {
                x: -0.8,
                ..Default::default()
            }),
            ComponentValue::BoundingGeometry(BoundingGeometry {
                geometry: GeometryDefinition::from(GeometryShape::Box {
                    min: [-0.1; 3],
                    max: [0.1; 3],
                })
                .encode()?,
                is_rendered: true,
                ..Default::default()
            }),
        ],
    )?;
    let unsupported = AssetSource {
        kind: SHADER_TYPE,
        uri: format!("producer://{}/{}/99", second.0, SHADER_TYPE.0).into(),
        variant: 0,
    };
    let definition = ShaderDefinition {
        recipe: ShaderRecipe {
            backend: "unavailable-fixture-backend".into(),
            features: 0,
        },
        ..Default::default()
    };
    host.asset_resources_mut().register_client_source(
        second,
        unsupported.clone(),
        definition.encode()?,
    )?;
    apply(
        &mut host,
        second,
        vec![Command::insert_value(
            EntityRef::Handle(blue),
            ComponentValue::CustomMaterial(CustomMaterial {
                source: unsupported.uri.clone(),
                ..Default::default()
            }),
        )],
    )?;
    settle(renderer, &mut host, first_output)?;
    let unsupported_key = host.asset_resources().find(&unsupported).unwrap();
    let authored: Vec<_> = host
        .asset_resources()
        .iter()
        .filter(|resource| !resource.source().uri.starts_with("ipp-render://program/"))
        .map(|resource| resource.key())
        .collect();
    assert!(
        matches!(host.asset_resources().get(unsupported_key).unwrap().status(), AssetLoadStatus::Failed(error) if error.contains("Unsupported shader backend"))
    );
    assert!(renderer.custom_material_diagnostics().is_empty());
    let first_programs = private_programs(&host);
    assert_eq!(
        first_programs.len(),
        1,
        "unselected debug output must not demand a program"
    );
    let green = capture()?;
    save(output, "selected-root-green", &green)?;
    assert_color(&green, 128, 128, [0.0, 0.5, 0.0]);
    assert_color(&green, 26, 128, [0.003095975, 0.004400849, 0.007194409]);
    settle(renderer, &mut host, first_output)?;
    assert_eq!(private_programs(&host), first_programs);
    assert_eq!(renderer.statistics().uploaded_bytes, 0);
    assert_eq!(capture()?, green);

    settle(renderer, &mut host, second_output)?;
    assert_eq!(private_programs(&host).len(), 2);
    assert_eq!(renderer.custom_material_diagnostics().len(), 1);
    let blue = capture()?;
    save(output, "selected-root-blue-fallback", &blue)?;
    assert_color(&blue, 128, 128, [0.0, 0.0, 0.5]);
    assert_ne!(green, blue);
    settle(renderer, &mut host, first_output)?;
    host.flush_resource_lifecycle();
    assert_eq!(private_programs(&host), first_programs);
    assert!(renderer.custom_material_diagnostics().is_empty());
    assert_eq!(capture()?, green);

    let retained_key = *first_programs.first().unwrap();
    let retained_source = host
        .asset_resources()
        .get(retained_key)
        .unwrap()
        .source()
        .clone();
    host.asset_resources_mut()
        .retain_prepared_source(second, &retained_source)?;
    renderer.prepare(&mut host, None)?;
    host.flush_resource_lifecycle();
    assert_eq!(
        private_programs(&host),
        first_programs,
        "another World's explicit source ownership must survive deselection"
    );
    host.asset_resources_mut()
        .release_client_source(second, &retained_source);
    host.flush_resource_lifecycle();
    assert!(
        private_programs(&host).is_empty(),
        "both live roots must have no implicit renderer demand"
    );
    assert!(host.root_output(first).is_some() && host.root_output(second).is_some());
    for key in &authored {
        assert!(host.asset_resources().get(*key).is_some());
    }

    settle(renderer, &mut host, first_output)?;
    let recovered_programs = private_programs(&host);
    renderer.replace_device(&mut host, replacement()?)?;
    settle(renderer, &mut host, first_output)?;
    assert_eq!(private_programs(&host), recovered_programs);
    let recovered = capture()?;
    save(output, "selected-root-recovered", &recovered)?;
    assert_eq!(recovered, green);
    assert!(renderer.custom_material_diagnostics().is_empty());

    let stale = host.root_output(first).unwrap().2;
    host.clear_root_output(first);
    assert_eq!(
        renderer.prepare(&mut host, Some((first_output, stale))),
        Err(ipp_render_gl::RenderError::UnavailableOutput)
    );
    host.flush_resource_lifecycle();
    assert!(private_programs(&host).is_empty());
    host.set_root_output(first_output, viewport())?;
    settle(renderer, &mut host, first_output)?;
    assert!(host.destroy_world(first));
    renderer.prepare(&mut host, None)?;
    host.flush_resource_lifecycle();
    assert!(private_programs(&host).is_empty());
    assert!(host.root_output(second).is_some());
    assert_eq!(
        host.asset_resources().find(&unsupported),
        Some(unsupported_key)
    );
    std::fs::write(
        output.join("selected-preparation.txt"),
        "roots=2\nselected-program-counts=1,2,1\nwarm-upload-bytes=0\nunselected-unsupported-shader=isolated\nindependent-source-owner=preserved\ndeselected-program-count=0\nrecovery-pixels=identical\nselected-world-destruction=retired\n",
    )?;
    renderer.unload_host(&mut host)?;
    Ok(())
}

fn spatial_child_recovery<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    replacement: &mut impl FnMut() -> Result<D>,
    output: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let parent = host.create_world(Default::default(), &super::selection::scene())?;
    let child = host.create_world(Default::default(), &super::selection::scene())?;
    let parent_mesh = mesh(
        &mut host,
        parent,
        Transform {
            x: -0.75,
            ..Default::default()
        },
        [0.75, 0.0, 0.0],
    )?;
    let child_mesh = mesh(&mut host, child, Transform::default(), [0.25, 0.5, 0.75])?;
    assert_eq!(
        parent_mesh, child_mesh,
        "fixture must collide in World-local identity"
    );
    let selection = camera(&mut host, parent, 4.0)?;
    host.set_root_output(selection, viewport())?;
    let anchor = attach(
        &mut host,
        parent,
        child,
        Transform {
            x: 0.75,
            ..Default::default()
        },
    )?;
    settle(renderer, &mut host, selection)?;
    let initial = capture()?;
    save(output, "spatial", &initial)?;
    assert_color(&initial, 80, 128, [0.75, 0.0, 0.0]);
    assert_color(&initial, 176, 128, [0.25, 0.5, 0.75]);
    host.world_mut(parent).unwrap().enqueue(Batch {
        id: 3,
        operations: vec![Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::Transform(Transform {
                y: 0.75,
                ..Default::default()
            }),
        )],
    })?;
    settle(renderer, &mut host, selection)?;
    let moved = capture()?;
    save(output, "moved-parent", &moved)?;
    assert_color(&moved, 128, 80, [0.25, 0.5, 0.75]);

    renderer.replace_device(&mut host, replacement()?)?;
    settle(renderer, &mut host, selection)?;
    let restored = capture()?;
    save(output, "device-replacement", &restored)?;
    assert_eq!(
        moved, restored,
        "device recovery must reproduce the published child"
    );

    let retained = host.latest_publication(child).unwrap();
    let mesh_key = host
        .publication(retained)
        .unwrap()
        .chunk(ipp_core::systems::render::RenderSystem::ID)
        .unwrap()
        .data::<ipp_core::systems::render::RenderPublication>()
        .unwrap()
        .items[0]
        .item
        .mesh
        .asset;
    host.asset_resources_mut().revoke_resource(
        ipp_core::services::asset_management::AssetKey::from_u64(mesh_key),
    );
    host.flush_resource_lifecycle();
    settle(renderer, &mut host, selection)?;
    let revoked = capture()?;
    save(output, "lease-revoked", &revoked)?;
    assert_color(&revoked, 128, 80, [0.003095975, 0.004400849, 0.007194409]);
    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;
    Ok(())
}

fn containing_debug_policy<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    use ipp_core::components::BoundingGeometry;
    use ipp_core::systems::geometry::{GeometryDefinition, GeometryShape};

    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let parent = host.create_world(Default::default(), &super::selection::scene())?;
    let child = host.create_world(Default::default(), &super::selection::scene())?;
    let geometry = GeometryDefinition::from(GeometryShape::Box {
        min: [-0.3; 3],
        max: [0.3; 3],
    })
    .encode()?;
    let mut entities = Vec::new();
    for (index, horizontal) in [-1.0, 0.0, 1.0].into_iter().enumerate() {
        entities.push(create(
            &mut host,
            child,
            vec![
                ComponentValue::Transform(Transform {
                    x: horizontal,
                    ..Default::default()
                }),
                ComponentValue::BoundingGeometry(BoundingGeometry {
                    geometry: geometry.clone(),
                    is_rendered: index == 1,
                    outline: false,
                    has_color_override: index == 2,
                    r: 0.0,
                    g: 0.0,
                    b: 1.0,
                    ..Default::default()
                }),
            ],
        )?);
    }
    let selection = camera(&mut host, parent, 4.0)?;
    host.set_root_output(selection, viewport())?;
    attach(&mut host, parent, child, Transform::default())?;
    host.world_mut(child)
        .unwrap()
        .enqueue_render_state_update(ipp_core::RenderStatePatch {
            show_all_debug_geometries: Some(true),
            debug_geometry_color: Some([1.0, 0.0, 0.0]),
            ..Default::default()
        })?;
    host.world_mut(parent)
        .unwrap()
        .enqueue_render_state_update(ipp_core::RenderStatePatch {
            show_all_debug_geometries: Some(false),
            debug_geometry_color: Some([0.0, 0.5, 0.0]),
            ..Default::default()
        })?;
    settle(renderer, &mut host, selection)?;
    let initial = capture()?;
    save(output, "debug-containing-policy", &initial)?;
    let background = [0.003095975, 0.004400849, 0.007194409];
    assert_color(&initial, 64, 128, background);
    assert_color(&initial, 128, 128, [0.0, 0.5, 0.0]);
    assert_color(&initial, 192, 128, background);

    host.world_mut(parent)
        .unwrap()
        .enqueue_render_state_update(ipp_core::RenderStatePatch {
            show_all_debug_geometries: Some(true),
            debug_geometry_color: Some([0.25, 0.5, 0.75]),
            ..Default::default()
        })?;
    settle(renderer, &mut host, selection)?;
    let shown = capture()?;
    save(output, "debug-show-all", &shown)?;
    assert_color(&shown, 64, 128, [0.25, 0.5, 0.75]);
    assert_color(&shown, 128, 128, [0.25, 0.5, 0.75]);
    assert_color(&shown, 192, 128, [0.0, 0.0, 1.0]);

    host.world_mut(parent)
        .unwrap()
        .enqueue_render_state_update(ipp_core::RenderStatePatch {
            show_all_debug_geometries: Some(false),
            debug_geometry_color: Some([0.75, 0.25, 0.0]),
            ..Default::default()
        })?;
    settle(renderer, &mut host, selection)?;
    let hidden = capture()?;
    save(output, "debug-individual-visibility", &hidden)?;
    assert_color(&hidden, 64, 128, background);
    assert_color(&hidden, 128, 128, [0.75, 0.25, 0.0]);
    assert_color(&hidden, 192, 128, background);

    apply_batch(
        &mut host,
        child,
        Batch {
            id: 78,
            operations: entities
                .iter()
                .enumerate()
                .map(|(index, entity)| {
                    Command::insert_value(
                        EntityRef::Handle(*entity),
                        ComponentValue::BoundingGeometry(BoundingGeometry {
                            geometry: geometry.clone(),
                            is_rendered: index != 1,
                            outline: false,
                            has_color_override: true,
                            r: 1.0,
                            g: 0.0,
                            b: 0.0,
                            ..Default::default()
                        }),
                    )
                })
                .collect(),
        },
    )?
    .result
    .map_err(|error| format!("{error:?}"))?;
    settle(renderer, &mut host, selection)?;
    let edited = capture()?;
    save(output, "debug-edited-child-policy", &edited)?;
    assert_color(&edited, 64, 128, [1.0, 0.0, 0.0]);
    assert_color(&edited, 128, 128, background);
    assert_color(&edited, 192, 128, [1.0, 0.0, 0.0]);
    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;
    Ok(())
}

fn transparent(
    host: &mut HostRuntime,
    world: WorldId,
    depth: f32,
    tint: [f32; 4],
) -> Result<EntityId> {
    let entity = mesh(
        host,
        world,
        Transform {
            z: depth,
            ..Default::default()
        },
        [1.0; 3],
    )?;
    let source = AssetSource {
        kind: SHADER_TYPE,
        uri: format!("producer://{}/{}/11", world.0, SHADER_TYPE.0).into(),
        variant: 0,
    };
    let definition = ShaderDefinition {
        parameters: BTreeMap::from([("tint".into(), ShaderParameterKind::Vec4)]),
        backends: BTreeMap::from([(
            "glsl-es-300".into(),
            ShaderBackendSource {
                vertex: String::new(),
                fragment: "vec4 materialFragment() { return p_tint; }".into(),
                ..Default::default()
            },
        )]),
        required_attributes: 0,
        ..Default::default()
    };
    host.asset_resources_mut()
        .register_client_source(world, source.clone(), definition.encode()?)
        .map_err(|error| format!("shader registration: {error}"))?;
    let mut material = CustomMaterial {
        source: source.uri,
        alpha_mode: 2,
        ..Default::default()
    };
    material
        .properties
        .set("tint", ipp_core::DynamicValue::Vec4(tint))
        .map_err(|error| format!("{error:?}"))?;
    apply(
        host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::CustomMaterial(material),
        )],
    )?;
    Ok(entity)
}

fn transparency<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let parent = host.create_world(Default::default(), &super::selection::scene())?;
    let child = host.create_world(Default::default(), &super::selection::scene())?;
    transparent(&mut host, parent, 0.5, [1.0, 0.0, 0.0, 0.5])?;
    transparent(&mut host, child, -0.5, [0.0, 0.0, 1.0, 0.5])?;
    attach(&mut host, parent, child, Transform::default())?;
    let selection = camera(&mut host, parent, 4.0)?;
    host.set_root_output(selection, viewport())?;
    settle(renderer, &mut host, selection)?;
    assert!(
        renderer.custom_material_diagnostics().is_empty(),
        "{:?}",
        renderer.custom_material_diagnostics()
    );
    let pixels = capture()?;
    save(output, "merged-transparency", &pixels)?;
    assert_color(
        &pixels,
        128,
        128,
        [
            0.5 + 0.003095975 * 0.25,
            0.004400849 * 0.25,
            0.25 + 0.007194409 * 0.25,
        ],
    );
    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;
    Ok(())
}

fn spatial_shadows<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    use ipp_core::components::{Light, PbrMaterial};
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let parent = host.create_world(Default::default(), &super::selection::scene())?;
    let child = host.create_world(Default::default(), &super::selection::scene())?;
    let receiver = mesh(
        &mut host,
        parent,
        Transform {
            sx: 3.0,
            sy: 3.0,
            ..Default::default()
        },
        [1.0; 3],
    )?;
    let caster = mesh(
        &mut host,
        child,
        Transform {
            z: 1.0,
            sx: 0.4,
            sy: 0.4,
            ..Default::default()
        },
        [1.0; 3],
    )?;
    for (world, entity) in [(parent, receiver), (child, caster)] {
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::PbrMaterial(PbrMaterial {
                    roughness: 1.0,
                    ..Default::default()
                }),
            )],
        )?;
    }
    let light_value = Light {
        kind: 2,
        intensity: 40.0,
        range: 10.0,
        inner_cone: 0.5,
        outer_cone: 0.8,
        cast_shadows: true,
        ..Default::default()
    };
    let light = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Transform(Transform {
                x: 1.0,
                z: 4.0,
                ..Default::default()
            }),
            ComponentValue::Light(light_value),
        ],
    )?;
    attach(&mut host, parent, child, Transform::default())?;
    let selection = camera(&mut host, parent, 4.0)?;
    host.set_root_output(selection, viewport())?;
    settle(renderer, &mut host, selection)?;
    let shadowed = capture()?;
    save(output, "child-shadow-parent-receiver", &shadowed)?;
    assert!(renderer.statistics().shadow_draw_calls >= 2);
    apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(light),
            ComponentValue::Light(Light {
                cast_shadows: false,
                ..light_value
            }),
        )],
    )?;
    settle(renderer, &mut host, selection)?;
    let unshadowed = capture()?;
    save(output, "parent-light-child-mesh", &unshadowed)?;
    let offset = (128 * SIZE as usize + 98) * 4;
    assert!(
        unshadowed[offset] > shadowed[offset].saturating_add(60),
        "cross-World shadow missing: {} vs {}",
        shadowed[offset],
        unshadowed[offset]
    );
    assert!(
        unshadowed[(128 * SIZE as usize + 128) * 4] > 80,
        "parent light must illuminate the child caster"
    );
    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;
    Ok(())
}

fn nested<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    use ipp_core::systems::{
        System, SystemFactory, SystemId, SystemInitContext, SystemInitError, SystemUpdateContext,
        compiled_system_factories,
    };
    use std::sync::{Arc, Mutex};

    struct Placement(Arc<Mutex<Option<(EntityId, OutputRef)>>>);

    impl SystemFactory for Placement {
        fn id(&self) -> SystemId {
            SystemId("fixture.publication-placement")
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> std::result::Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(Self(self.0.clone())))
        }
    }

    impl System for Placement {
        fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

        fn attachment_placement(
            &self,
            world: &ipp_core::WorldContext<'_>,
            anchor: EntityId,
        ) -> ipp_core::AttachmentPlacement {
            self.0
                .lock()
                .unwrap()
                .filter(|(entity, owner)| *entity == anchor && owner.world().id() == world.id())
                .map_or(ipp_core::AttachmentPlacement::Unmanaged, |(_, owner)| {
                    ipp_core::AttachmentPlacement::Ready {
                        owner,
                        affine: ipp_core::systems::geometry::GeometryShapeTransform::default()
                            .matrix(),
                    }
                })
        }
    }

    let placement = Arc::new(Mutex::new(None));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Placement(placement.clone())));
    let mut host = HostRuntime::with_system_factories(factories)?;
    renderer.install(&mut host)?;
    let parent = host.create_world(
        Default::default(),
        &super::selection::with_fixtures(&host, super::selection::scene()),
    )?;
    let middle = host.create_world(
        Default::default(),
        &super::selection::with_fixtures(&host, super::selection::scene()),
    )?;
    let child = host.create_world(Default::default(), &super::selection::scene())?;
    mesh(
        &mut host,
        child,
        Transform {
            x: -0.5,
            y: 0.5,
            z: 0.5,
            sx: 0.6,
            sy: 0.6,
            ..Default::default()
        },
        [0.25, 0.5, 0.75],
    )?;
    mesh(
        &mut host,
        child,
        Transform {
            x: -0.5,
            y: 0.5,
            sx: 0.6,
            sy: 0.6,
            ..Default::default()
        },
        [1.0, 0.0, 0.0],
    )?;
    mesh(
        &mut host,
        child,
        Transform {
            x: 0.5,
            y: -0.5,
            sx: 0.6,
            sy: 0.6,
            ..Default::default()
        },
        [0.0, 0.75, 0.0],
    )?;
    let child_output = camera(&mut host, child, 2.0)?;
    let middle_output = camera(&mut host, middle, 2.0)?;
    let mut parent_anchor = None;
    for (world, selection) in [(middle, child_output), (parent, middle_output)] {
        let surface = ipp_core::components::FlatSurface {
            width: 2.0,
            height: 2.0,
            ..Default::default()
        };
        let anchor = create(
            &mut host,
            world,
            vec![
                ComponentValue::Transform(Transform::default()),
                ComponentValue::FlatSurface(surface),
                ComponentValue::WorldAttachment(WorldAttachment::surface(selection)),
            ],
        )?;
        if world == parent {
            parent_anchor = Some(anchor);
        }
    }
    let selection = camera(&mut host, parent, 4.0)?;
    host.set_root_output(selection, viewport())?;
    settle(renderer, &mut host, selection)?;
    let pixels = capture()?;
    save(output, "nested-linear-depth", &pixels)?;
    assert_color(&pixels, 96, 96, [0.25, 0.5, 0.75]);
    assert_color(&pixels, 160, 160, [0.0, 0.75, 0.0]);

    let other = camera(&mut host, parent, 4.0)?;
    let anchor = parent_anchor.unwrap();
    let branch = WorldAttachment::spatial(host.world_ref(middle).unwrap());
    apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::WorldAttachment(branch),
        )],
    )?;
    *placement.lock().unwrap() = Some((anchor, selection));
    for (name, view) in [
        ("scoped-spatial-other-before", other),
        ("scoped-spatial-owner", selection),
        ("scoped-spatial-owner-reuse", selection),
        ("scoped-spatial-other-after", other),
        ("scoped-spatial-owner-restored", selection),
    ] {
        host.set_root_output(view, viewport())?;
        settle(renderer, &mut host, view)?;
        let pixels = capture()?;
        save(output, name, &pixels)?;
        if view == selection {
            assert_color(&pixels, 96, 96, [0.25, 0.5, 0.75]);
            assert_color(&pixels, 160, 160, [0.0, 0.75, 0.0]);
        } else {
            let background = [0.003095975, 0.004400849, 0.007194409];
            assert_color(&pixels, 96, 96, background);
            assert_color(&pixels, 160, 160, background);
        }
    }
    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;
    Ok(())
}
