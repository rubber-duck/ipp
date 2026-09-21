//! The same endpoint data and baked comparisons, independent of context creation.

use super::world::{HEIGHT, WIDTH, apply, float, save};
use ipp_core::{
    components::{Camera, MeshPose, Transform},
    *,
};
use ipp_render_gl::{RenderDevice, RenderService};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn source(value: &str) -> FieldWrite {
    FieldWrite {
        offset: 0,
        value: FieldValue::String(value.into()),
    }
}

fn insert(alias: u32, component: u16, fields: Vec<FieldWrite>) -> Command {
    Command::InsertComponent {
        entity: EntityRef::Alias(alias),
        component,
        fields,
    }
}

fn set(entity: EntityId, component: u16, field: FieldWrite) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component,
        field,
    }
}

fn compare(output: &Path, name: &str, actual: &[u8], expected: &[u8]) -> Result<()> {
    save(output, name, actual)?;
    save(output, &format!("{name}-expected"), expected)?;
    let diff: Vec<_> = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| a.abs_diff(*b))
        .collect();
    save(output, &format!("{name}-diff"), &diff)?;
    let changed = diff
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[..3].iter().any(|&v| v > 6))
        .count();
    if changed > (WIDTH * HEIGHT / 2000) as usize {
        return Err(format!("{name}: {changed} pixels differ from baked geometry").into());
    }
    Ok(())
}

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    fixtures: &Path,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    mut replacement: impl FnMut() -> Result<D>,
    output: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let world_id = host.create_world(Default::default())?;
    let mut world = host.world_mut(world_id).unwrap();
    for (asset, name) in [
        (1, "base"),
        (2, "target"),
        (3, "half"),
        (4, "affine-half"),
        (5, "aimed-affine-half"),
    ] {
        world.enqueue_mesh(MeshUpload {
            id: asset,
            key: MeshKey {
                asset,
                variant: 0,
            },
            bytes: std::fs::read(fixtures.join(format!("{name}.mesh")))?,
        })?;
    }
    let mut operations = vec![
        Command::Create {
            alias: 0,
            metadata: Default::default(),
        },
        insert(
            0,
            ComponentValue::TRANSFORM,
            vec![float(std::mem::offset_of!(Transform, z), 6.0)],
        ),
        insert(
            0,
            ComponentValue::CAMERA,
            vec![
                FieldWrite {
                    offset: std::mem::offset_of!(Camera, projection) as u32,
                    value: FieldValue::U32(1),
                },
                float(std::mem::offset_of!(Camera, ortho_height), 4.5),
            ],
        ),
    ];
    for (alias, x) in [(1, -1.25), (2, 1.25)] {
        operations.extend([
            Command::Create {
                alias,
                metadata: Default::default(),
            },
            insert(
                alias,
                ComponentValue::TRANSFORM,
                vec![float(std::mem::offset_of!(Transform, x), x)],
            ),
            insert(
                alias,
                ComponentValue::MESH_INSTANCE,
                vec![source("asset://1/1")],
            ),
            insert(
                alias,
                ComponentValue::MESH_POSE,
                vec![
                    source("asset://1/2"),
                    float(std::mem::offset_of!(MeshPose, weight), 0.5),
                ],
            ),
            insert(alias, ComponentValue::UNLIT_MATERIAL, vec![]),
            insert(alias, ComponentValue::BOUNDING_GEOMETRY, vec![]),
        ]);
        operations.push(insert(alias, ComponentValue::PBR_MATERIAL, vec![]));
    }
    {
        use ipp_core::components::Light;
        operations.extend([
            Command::Create {
                alias: 3,
                metadata: Default::default(),
            },
            insert(
                3,
                ComponentValue::TRANSFORM,
                vec![float(std::mem::offset_of!(Transform, z), 4.0)],
            ),
            insert(
                3,
                ComponentValue::LIGHT,
                vec![
                    FieldWrite {
                        offset: std::mem::offset_of!(Light, kind) as u32,
                        value: FieldValue::U32(2),
                    },
                    float(std::mem::offset_of!(Light, intensity), 60.0),
                    float(std::mem::offset_of!(Light, outer_cone), 1.0),
                    FieldWrite {
                        offset: std::mem::offset_of!(Light, cast_shadows) as u32,
                        value: FieldValue::Bool(true),
                    },
                ],
            ),
        ]);
    }
    world.enqueue(Batch {
        id: 1,
        operations,
    })?;
    let report = world.step(0.0)?;
    let aliases = report.outcomes[0]
        .result
        .as_ref()
        .map_err(|error| format!("{error:?}"))?;
    let camera = aliases.iter().find(|(alias, _)| *alias == 0).unwrap().1;
    let a = aliases.iter().find(|(alias, _)| *alias == 1).unwrap().1;
    world.enqueue_camera_activate(camera)?;
    world.step(0.0)?;
    let mut endpoints = Vec::new();
    for (weight, baked) in [
        (0.0, "asset://1/1"),
        (0.5, "asset://1/3"),
        (1.0, "asset://1/2"),
    ] {
        apply(
            &mut world,
            vec![
                set(a, ComponentValue::MESH_INSTANCE, source("asset://1/1")),
                set(a, ComponentValue::MESH_POSE, source("asset://1/2")),
                set(
                    a,
                    ComponentValue::MESH_POSE,
                    float(std::mem::offset_of!(MeshPose, weight), weight),
                ),
            ],
        )?;
        let stats = ready(renderer, &mut world)?;
        if stats.draw_calls != 2 {
            return Err(format!("incomplete native pose scene: {stats:?}").into());
        }
        let posed = capture()?;
        endpoints.push(posed.clone());
        apply(
            &mut world,
            vec![
                set(a, ComponentValue::MESH_POSE, source("")),
                set(a, ComponentValue::MESH_INSTANCE, source(baked)),
            ],
        )?;
        ready(renderer, &mut world)?;
        compare(output, &format!("pose-{weight}"), &posed, &capture()?)?;
    }
    if endpoints[0] == endpoints[2] {
        return Err("endpoint geometry did not change".into());
    }
    // The shared TypeScript fixture independently bakes the affine matrix and normals.
    let affine = Transform {
        y: 0.15,
        qy: (std::f32::consts::PI / 12.0).sin(),
        qw: (std::f32::consts::PI / 12.0).cos(),
        sx: 0.8,
        sy: 1.2,
        sz: 0.6,
        ..Default::default()
    };
    let authored = |entity, value| Command::InsertComponentValue {
        entity: EntityRef::Handle(entity),
        value,
    };
    apply(
        &mut world,
        vec![
            authored(a, ComponentValue::Transform(affine)),
            set(a, ComponentValue::MESH_INSTANCE, source("asset://1/1")),
            set(a, ComponentValue::MESH_POSE, source("asset://1/2")),
            set(
                a,
                ComponentValue::MESH_POSE,
                float(std::mem::offset_of!(MeshPose, weight), 0.5),
            ),
        ],
    )?;
    ready(renderer, &mut world)?;
    let transformed = capture()?;
    apply(
        &mut world,
        vec![
            authored(a, ComponentValue::Transform(Transform::default())),
            set(a, ComponentValue::MESH_POSE, source("")),
            set(a, ComponentValue::MESH_INSTANCE, source("asset://1/4")),
        ],
    )?;
    ready(renderer, &mut world)?;
    compare(output, "affine-object", &transformed, &capture()?)?;
    apply(
        &mut world,
        vec![
            Command::Create {
                alias: 50,
                metadata: EntityMetadata {
                    symbolic_id: Some("affine-parent".into()),
                    ..Default::default()
                },
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(50),
                value: ComponentValue::Transform(affine),
            },
            Command::Create {
                alias: 51,
                metadata: EntityMetadata {
                    symbolic_id: Some("aim-target".into()),
                    ..Default::default()
                },
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(51),
                value: ComponentValue::Transform(Transform {
                    x: 0.8 * 3.0f32.sqrt() / 2.0 - 0.3,
                    y: 0.15,
                    z: -0.4 - 0.6 * 3.0f32.sqrt() / 2.0,
                    ..Default::default()
                }),
            },
        ],
    )?;
    let parent = world.lookup_id("affine-parent").unwrap();
    let target = world.lookup_id("aim-target").unwrap();
    apply(
        &mut world,
        vec![
            authored(
                a,
                ComponentValue::Hierarchy(components::Hierarchy {
                    parent,
                    ..Default::default()
                }),
            ),
            set(a, ComponentValue::MESH_INSTANCE, source("asset://1/1")),
            set(a, ComponentValue::MESH_POSE, source("asset://1/2")),
        ],
    )?;
    ready(renderer, &mut world)?;
    compare(output, "affine-hierarchy", &capture()?, &transformed)?;
    apply(
        &mut world,
        vec![authored(
            a,
            ComponentValue::LookAt(components::LookAt {
                target,
                ..Default::default()
            }),
        )],
    )?;
    ready(renderer, &mut world)?;
    let aimed = capture()?;
    apply(
        &mut world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(a),
                component: ComponentValue::HIERARCHY,
            },
            Command::RemoveComponent {
                entity: EntityRef::Handle(a),
                component: ComponentValue::LOOK_AT,
            },
            set(a, ComponentValue::MESH_POSE, source("")),
            set(a, ComponentValue::MESH_INSTANCE, source("asset://1/5")),
        ],
    )?;
    ready(renderer, &mut world)?;
    compare(output, "affine-look-at", &aimed, &capture()?)?;
    apply(
        &mut world,
        vec![
            set(a, ComponentValue::MESH_INSTANCE, source("asset://1/1")),
            set(a, ComponentValue::MESH_POSE, source("asset://1/2")),
            set(
                a,
                ComponentValue::MESH_POSE,
                float(std::mem::offset_of!(MeshPose, weight), 0.5),
            ),
        ],
    )?;
    ready(renderer, &mut world)?;
    let before = capture()?;
    drop(world);
    renderer.replace_device(&mut host, replacement()?)?;
    world = host.world_mut(world_id).unwrap();
    ready(renderer, &mut world)?;
    compare(output, "recovered", &capture()?, &before)?;
    Ok(())
}

fn ready<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    world: &mut WorldContext<'_>,
) -> Result<ipp_render_gl::RenderStats> {
    for _ in 0..512 {
        let stats = super::world::render_frame(renderer, world, WIDTH, HEIGHT)?;
        if stats.draw_calls == 2 {
            return Ok(stats);
        }
    }
    Err("mesh pose resources did not become ready".into())
}
