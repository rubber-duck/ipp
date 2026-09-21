//! Controlled Host-time particle evidence on the real GLES renderer.
use super::world::{HEIGHT, WIDTH, apply, coverage, fixture_world, render_frame, save};
use ipp_core::{components::*, *};
use ipp_render_gl::{RenderDevice, RenderService};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let mut world = fixture_world(&mut host)?;
    apply(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Transform(Transform::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::ParticleEmitter(ParticleEmitter {
                    burst: 2000,
                    rate: 0.0,
                    shape: 1,
                    extent_x: 1.5,
                    extent_y: 1.0,
                    extent_z: 0.0,
                    speed: 0.0,
                    size: 0.12,
                    lifetime: 2.0,
                    ..Default::default()
                }),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::ParticleSprite(ParticleSprite {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                    end_opacity: 1.0,
                    ..Default::default()
                }),
            },
        ],
    )?;
    let entity = world
        .entities()
        .iter()
        .find(|e| {
            e.effective
                .iter()
                .any(|c| c.type_id() == ComponentValue::PARTICLE_EMITTER)
        })
        .unwrap()
        .id;
    let stats = render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
    assert_eq!(world.particles(entity).unwrap().len(), 2000);
    assert_eq!(stats.draw_calls, 1);
    assert_eq!(stats.triangles, 4000);
    let initial = capture()?;
    save(output, "sprites", &initial)?;
    assert!(coverage(&initial).0 > 1000);
    let state = world.particles(entity).unwrap().to_vec();
    renderer.unload(&mut world);
    render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
    assert_eq!(world.particles(entity).unwrap(), state);
    let restored = capture()?;
    save(output, "restored", &restored)?;
    assert_eq!(initial, restored);
    let geometry = ipp_core::systems::geometry::GeometryDefinition::from(
        ipp_core::systems::geometry::GeometryShape::Box {
            min: [-4.0; 3],
            max: [4.0; 3],
        },
    )
    .encode()?;
    apply(
        &mut world,
        vec![
            Command::InsertComponentValue {
                entity: EntityRef::Handle(entity),
                value: ComponentValue::BoundingGeometry(BoundingGeometry {
                    geometry,
                    ..Default::default()
                }),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Handle(entity),
                value: ComponentValue::Transform(Transform {
                    x: 100.0,
                    ..Default::default()
                }),
            },
        ],
    )?;
    assert_eq!(
        render_frame(renderer, &mut world, WIDTH, HEIGHT)?.draw_calls,
        0,
        "explicit effect bounds should cull the batch"
    );
    apply(
        &mut world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::BOUNDING_GEOMETRY,
        }],
    )?;
    assert_eq!(
        render_frame(renderer, &mut world, WIDTH, HEIGHT)?.draw_calls,
        1,
        "missing bounds must keep the effect eligible"
    );
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::Transform(Transform::default()),
        }],
    )?;
    world.step(2.0)?;
    assert_eq!(
        render_frame(renderer, &mut world, WIDTH, HEIGHT)?.draw_calls,
        0
    );
    let drained = capture()?;
    save(output, "drained", &drained)?;
    assert_eq!(coverage(&drained).0, 0);
    // Exercise the same instance attributes with a real custom mesh program.
    use ipp_core::services::asset_management::{
        AssetSource,
        shader::{SHADER_TYPE, ShaderBackendSource, ShaderDefinition, ShaderRecipe},
    };
    let definition = ShaderDefinition {
        recipe: ShaderRecipe {
            features: 32,
            ..Default::default()
        },
        backends: std::collections::BTreeMap::from([(
            "glsl-es-300".into(),
            ShaderBackendSource {
                vertex: "void materialVertex() { ippDefaultVertex(); }".into(),
                fragment: "vec4 materialFragment() { return vec4(0,0,1,1); }".into(),
            },
        )]),
        ..Default::default()
    };
    let mut mesh = b"IPPM".to_vec();
    for n in [1u32, 3, 3] {
        mesh.extend_from_slice(&n.to_le_bytes());
    }
    for position in [[-0.5f32, -0.5, 0.0], [0.5, -0.5, 0.0], [0.0, 0.5, 0.0]] {
        for f in position.into_iter().chain([1.0, 1.0, 1.0]) {
            mesh.extend_from_slice(&f.to_le_bytes());
        }
    }
    for n in [0u16, 1, 2] {
        mesh.extend_from_slice(&n.to_le_bytes());
    }
    let world_id = world.id();
    for (kind, uri, bytes) in [
        (MESH_TYPE, "client://particles/mesh", mesh),
        (
            SHADER_TYPE,
            "client://particles/shader",
            definition.encode()?,
        ),
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
    let target = EntityRef::Handle(entity);
    apply(
        &mut world,
        vec![
            Command::RemoveComponent {
                entity: target,
                component: ComponentValue::PARTICLE_SPRITE,
            },
            Command::InsertComponentValue {
                entity: target,
                value: ComponentValue::ParticleEmitter(ParticleEmitter {
                    restart: 1,
                    burst: 1000,
                    rate: 0.0,
                    shape: 1,
                    speed: 0.0,
                    size: 0.2,
                    lifetime: 10.0,
                    ..Default::default()
                }),
            },
            Command::InsertComponentValue {
                entity: target,
                value: ComponentValue::ParticleMesh(ParticleMesh {
                    source: "client://particles/mesh".into(),
                    variant: 0,
                }),
            },
            Command::InsertComponentValue {
                entity: target,
                value: ComponentValue::CustomMaterial(CustomMaterial {
                    source: "client://particles/shader".into(),
                    ..Default::default()
                }),
            },
        ],
    )?;
    let stats = render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
    assert_eq!(stats.draw_calls, 1);
    assert_eq!(stats.triangles, 1000);
    assert!(
        renderer.custom_material_diagnostics().is_empty(),
        "{:?}",
        renderer.custom_material_diagnostics()
    );
    let blue = capture()?;
    save(output, "custom-mesh", &blue)?;
    assert!(
        blue.as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[2] > 200 && p[0] < 20)
            .count()
            > 100
    );

    Ok(())
}
