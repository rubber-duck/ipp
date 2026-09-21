use super::world::{HEIGHT, WIDTH, apply, float, save};
use ipp_core::{
    components::{Camera, MeshInstance, Skeleton, Skin, Transform},
    services::asset_management::{AssetUpload, AssetUploadIdentity},
    *,
};
use ipp_render_gl::{RenderDevice, RenderService};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn insert(alias: u32, component: u16, fields: Vec<FieldWrite>) -> Command {
    Command::InsertComponent {
        entity: EntityRef::Alias(alias),
        component,
        fields,
    }
}

fn source(offset: usize, uri: &str) -> FieldWrite {
    FieldWrite {
        offset: offset as u32,
        value: FieldValue::String(uri.into()),
    }
}

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    fixtures: &Path,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    mut replacement: impl FnMut() -> Result<D>,
    output: &Path,
) -> Result<()> {
    let mut world_host = ipp_core::HostRuntime::new();
    renderer.install(&mut world_host)?;
    let world_id = world_host.create_world(Default::default())?;
    let mut world = world_host.world_mut(world_id).unwrap();
    for (kind, id, name) in [
        (SKELETON_TYPE, 1, "rig.skeleton"),
        (SKIN_TYPE, 2, "rig.skin"),
        (MESH_TYPE, 3, "rig.mesh"),
        (POSE_TYPE, 4, "bent.pose"),
    ] {
        world.enqueue_asset(AssetUpload {
            id,
            key: AssetUploadIdentity {
                kind,
                asset: id,
                variant: 0,
            },
            bytes: std::fs::read(fixtures.join(name))?,
        })?;
    }
    let mut ops = vec![
        Command::Create {
            alias: 0,
            metadata: Default::default(),
        },
        insert(
            0,
            ComponentValue::TRANSFORM,
            vec![
                float(std::mem::offset_of!(Transform, y), 1.0),
                float(std::mem::offset_of!(Transform, z), 5.0),
            ],
        ),
        insert(
            0,
            ComponentValue::CAMERA,
            vec![
                FieldWrite {
                    offset: std::mem::offset_of!(Camera, projection) as u32,
                    value: FieldValue::U32(1),
                },
                float(std::mem::offset_of!(Camera, ortho_height), 3.0),
            ],
        ),
    ];
    for (alias, x) in [(1, -0.8), (2, 0.8)] {
        ops.extend([
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
                ComponentValue::SKELETON,
                vec![source(
                    std::mem::offset_of!(Skeleton, source),
                    "asset://3/1",
                )],
            ),
            insert(
                alias,
                ComponentValue::SKIN,
                vec![
                    source(std::mem::offset_of!(Skin, source), "asset://5/2"),
                    FieldWrite {
                        offset: std::mem::offset_of!(Skin, skeleton) as u32,
                        value: FieldValue::Entity(EntityRef::Alias(alias)),
                    },
                ],
            ),
            insert(
                alias,
                ComponentValue::MESH_INSTANCE,
                vec![source(
                    std::mem::offset_of!(MeshInstance, source),
                    "asset://1/3",
                )],
            ),
            insert(alias, ComponentValue::UNLIT_MATERIAL, vec![]),
        ]);
    }
    {
        for alias in [1, 2] {
            ops.push(insert(alias, ComponentValue::PBR_MATERIAL, vec![]));
        }
        ops.extend([
            Command::Create {
                alias: 3,
                metadata: Default::default(),
            },
            insert(
                3,
                ComponentValue::TRANSFORM,
                vec![
                    float(std::mem::offset_of!(Transform, y), 1.0),
                    float(std::mem::offset_of!(Transform, z), 4.0),
                ],
            ),
            Command::InsertComponentValue {
                entity: EntityRef::Alias(3),
                value: ComponentValue::Light(ipp_core::components::Light {
                    kind: 2,
                    intensity: 40.0,
                    inner_cone: 0.5,
                    outer_cone: 0.9,
                    cast_shadows: cfg!(feature = "shadows"),
                    ..Default::default()
                }),
            },
        ]);
    }
    apply(&mut world, ops)?;
    let entities = world.entities();
    let camera = entities[0].id;
    let a = entities[1].id;
    let b = entities[2].id;
    world.enqueue_camera_activate(camera)?;
    drop(world);
    ready(renderer, &mut world_host, world_id)?;
    world = world_host.world_mut(world_id).unwrap();
    let rest = capture()?;
    save(output, "skin-rest", &rest)?;
    apply(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(a),
            component: ComponentValue::SKELETON,
            field: source(std::mem::offset_of!(Skeleton, pose_source), "asset://4/4"),
        }],
    )?;
    drop(world);
    ready(renderer, &mut world_host, world_id)?;
    world = world_host.world_mut(world_id).unwrap();
    let bent = capture()?;
    save(output, "skin-bent", &bent)?;
    assert!(difference(&rest, &bent, 0, WIDTH / 2) > 1000);
    assert_eq!(difference(&rest, &bent, WIDTH / 2, WIDTH), 0);
    drop(world);
    renderer.replace_device(&mut world_host, replacement()?)?;
    world = world_host.world_mut(world_id).unwrap();
    drop(world);
    ready(renderer, &mut world_host, world_id)?;
    world = world_host.world_mut(world_id).unwrap();
    let recovered = capture()?;
    save(output, "skin-recovered", &recovered)?;
    assert_eq!(recovered, bent);

    // Exercise all four nonzero slots with the same analytic deformation as two.
    let mut two = std::fs::read(fixtures.join("rig.mesh"))?;
    let start = 52 + 432 + 72;
    for row in two[start..start + 18 * 16].as_chunks_mut::<16>().0 {
        for (slot, v) in [0.5f32, 0.5, 0.0, 0.0].into_iter().enumerate() {
            row[slot * 4..slot * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    let mut four = two.clone();
    for row in four[52 + 432..start].as_chunks_mut::<4>().0 {
        *row = [1, 0, 1, 0];
    }
    for row in four[start..start + 18 * 16].as_chunks_mut::<16>().0 {
        for (slot, v) in [0.125f32, 0.25, 0.375, 0.25].into_iter().enumerate() {
            row[slot * 4..slot * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    for (id, bytes) in [(5, two), (6, four)] {
        world.enqueue_asset(AssetUpload {
            id,
            key: AssetUploadIdentity {
                kind: MESH_TYPE,
                asset: id,
                variant: 0,
            },
            bytes,
        })?;
    }
    apply(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(b),
            component: ComponentValue::SKELETON,
            field: source(std::mem::offset_of!(Skeleton, pose_source), "asset://4/4"),
        }],
    )?;
    let set_mesh = |id| {
        [a, b]
            .map(|entity| Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::MESH_INSTANCE,
                field: source(
                    std::mem::offset_of!(MeshInstance, source),
                    &format!("asset://1/{id}"),
                ),
            })
            .to_vec()
    };
    apply(&mut world, set_mesh(5))?;
    drop(world);
    ready(renderer, &mut world_host, world_id)?;
    world = world_host.world_mut(world_id).unwrap();
    let two = capture()?;
    save(output, "skin-two-slots", &two)?;
    apply(&mut world, set_mesh(6))?;
    drop(world);
    ready(renderer, &mut world_host, world_id)?;
    let four = capture()?;
    save(output, "skin-four-slots", &four)?;
    assert_eq!(difference(&two, &four, 0, WIDTH), 0);
    Ok(())
}

fn ready<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut HostRuntime,
    id: WorldId,
) -> Result<()> {
    for _ in 0..12 {
        let stats = super::world::render_host_frame(renderer, host, id, WIDTH, HEIGHT)?;
        if stats.draw_calls == 2 {
            #[cfg(feature = "shadows")]
            assert_eq!(stats.shadow_draw_calls, 2);
            return Ok(());
        }
    }
    Err(format!(
        "skin frame unavailable: {:?}",
        host.world_mut(id).unwrap().render_diagnostics()
    )
    .into())
}

fn difference(a: &[u8], b: &[u8], left: u32, right: u32) -> usize {
    (0..HEIGHT)
        .flat_map(|y| (left..right).map(move |x| (y * WIDTH + x) as usize * 4))
        .filter(|&i| (0..3).any(|c| a[i + c].abs_diff(b[i + c]) > 5))
        .count()
}
