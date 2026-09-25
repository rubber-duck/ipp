//! Commit-time preparation of heap-owning components: one stable effective
//! value per component, released through ordinary ownership, with overlays
//! retaining only their hidden producer fields.

use crate::world::*;
use crate::{
    ComponentOverlayMode, EntityOverlayMode, StateOverlayRef,
    components::{CustomMaterial, dynamic_properties::clone_count},
};
use std::mem::offset_of;

fn run(world: &mut crate::WorldContext<'_>, operations: Vec<Command>) -> WorldUpdateReport {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    world.step(0.0).unwrap()
}

fn insert(entity: EntityId, source: &str) -> Command {
    Command::insert_value(
        EntityRef::Handle(entity),
        ComponentValue::CustomMaterial(CustomMaterial {
            source: source.into(),
            ..CustomMaterial::default()
        }),
    )
}

fn cutoff(value: f32) -> FieldWrite {
    FieldWrite {
        offset: offset_of!(CustomMaterial, alpha_cutoff) as u32,
        value: FieldValue::F32(value),
    }
}

fn material<'a>(
    world: &'a crate::WorldContext<'_>,
    entity: EntityId,
) -> Option<&'a CustomMaterial> {
    world
        .world
        .components
        .custom_material(entity.index() as usize)
}

#[test]
fn heap_owning_component_keeps_one_stable_value_through_failures_overlays_and_removal() {
    let mut world_host = crate::HostRuntime::new();
    let world_id = world_host
        .create_world(crate::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let report = run(
        &mut world,
        vec![Command::Create {
            alias: 0,
            metadata: EntityMetadata {
                symbolic_id: Some("material".into()),
                classes: vec![],
            },
        }],
    );
    let entity = report.outcomes[0].result.as_ref().unwrap()[0].1;
    assert!(
        run(
            &mut world,
            vec![insert(entity, "file:///materials/a.shader")]
        )
        .outcomes[0]
            .result
            .is_ok()
    );
    let address = material(&world, entity).unwrap() as *const CustomMaterial;

    // A failed later operation retains the applied replacement in the same slot.
    let report = run(
        &mut world,
        vec![
            insert(entity, "file:///materials/b.shader"),
            Command::Delete {
                entity: EntityRef::Alias(99),
            },
        ],
    );
    assert!(report.outcomes[0].result.is_err());
    assert_eq!(address, material(&world, entity).unwrap() as *const _);
    assert_eq!(
        material(&world, entity).unwrap().source,
        "file:///materials/b.shader"
    );

    let report = run(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: "material".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(0),
                binding: StateOverlayRef::Alias(1),
                alias: 2,
                component: ComponentValue::CUSTOM_MATERIAL,
                mode: ComponentOverlayMode::Bound,
                fields: vec![cutoff(0.25)],
            },
        ],
    );
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    assert_eq!(address, material(&world, entity).unwrap() as *const _);
    assert_eq!(material(&world, entity).unwrap().alpha_cutoff, 0.25);
    let inputs =
        &world.world.state.entities[&entity].layers[&ComponentValue::CUSTOM_MATERIAL].inputs;
    assert_eq!(
        inputs.hidden_fields.len(),
        1,
        "only the overridden cutoff is retained"
    );
    assert!(inputs.retained_inputs().next().is_none());

    let resources = &report.outcomes[0].state_overlays;
    let owner = StateOverlayRef::Handle(resources[0].id);
    let overlay = StateOverlayRef::Handle(resources[2].id);
    assert!(
        run(
            &mut world,
            vec![Command::UpdateComponentStateOverlay {
                owner,
                overlay,
                fields: vec![cutoff(0.75)],
                clear: vec![]
            }]
        )
        .outcomes[0]
            .result
            .is_ok()
    );
    assert_eq!(material(&world, entity).unwrap().alpha_cutoff, 0.75);
    assert!(
        run(
            &mut world,
            vec![Command::ReleaseStateOverlayOwner {
                owner
            }]
        )
        .outcomes[0]
            .result
            .is_ok()
    );
    let released = material(&world, entity).unwrap();
    assert_eq!(address, released as *const _);
    assert_eq!(
        (released.source.as_str(), released.alpha_cutoff),
        (
            "file:///materials/b.shader",
            CustomMaterial::default().alpha_cutoff
        )
    );
    assert!(
        run(
            &mut world,
            vec![Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CUSTOM_MATERIAL
            }]
        )
        .outcomes[0]
            .result
            .is_ok()
    );
    assert!(material(&world, entity).is_none());
}

#[test]
fn scalar_batches_do_not_clone_or_replace_unrelated_payloads() {
    let mut world_host = crate::HostRuntime::new();
    let world_id = world_host
        .create_world(crate::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let report = run(
        &mut world,
        (0..128)
            .map(|alias| Command::Create {
                alias,
                metadata: Default::default(),
            })
            .collect(),
    );
    let entities: Vec<_> = report.outcomes[0]
        .result
        .as_ref()
        .unwrap()
        .iter()
        .map(|(_, entity)| *entity)
        .collect();
    for &entity in &entities {
        assert!(
            run(
                &mut world,
                vec![
                    insert(
                        entity,
                        &format!("file:///materials/{}.shader", entity.index())
                    ),
                    Command::SetDynamicProperty {
                        entity: EntityRef::Handle(entity),
                        component: ComponentValue::CUSTOM_MATERIAL,
                        name: "seed".into(),
                        value: crate::DynamicValue::F32(1.0),
                    },
                    Command::InsertComponent {
                        entity: EntityRef::Handle(entity),
                        component: ComponentValue::SCALAR,
                        fields: vec![]
                    }
                ]
            )
            .outcomes[0]
                .result
                .is_ok()
        );
    }
    let addresses = |world: &crate::WorldContext<'_>| -> Vec<_> {
        entities
            .iter()
            .map(|&entity| {
                let value = material(world, entity).unwrap();
                (value as *const CustomMaterial, value.source.as_ptr())
            })
            .collect()
    };
    let before = addresses(&world);
    clone_count::take();
    for _ in 0..2 {
        let operations = (0..256)
            .map(|i| Command::SetField {
                entity: EntityRef::Handle(entities[i % entities.len()]),
                component: ComponentValue::SCALAR,
                field: FieldWrite {
                    offset: offset_of!(Scalar, value) as u32,
                    value: FieldValue::F32(i as f32),
                },
            })
            .collect();
        assert!(run(&mut world, operations).outcomes[0].result.is_ok());
    }
    world.step(0.0).unwrap();
    assert_eq!(
        clone_count::take(),
        0,
        "unrelated authored/effective values must not be cloned"
    );
    assert_eq!(addresses(&world), before);
}

#[test]
fn native_insertion_checks_field_policy_even_when_an_overlay_hides_the_base() {
    use crate::components::MeshInstance;

    let mut world_host = crate::HostRuntime::new();
    let world_id = world_host
        .create_world(crate::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let report = run(
        &mut world,
        vec![
            Command::Create {
                alias: 0,
                metadata: EntityMetadata {
                    symbolic_id: Some("mesh".into()),
                    classes: vec![],
                },
            },
            Command::insert_value(
                EntityRef::Alias(0),
                ComponentValue::MeshInstance(MeshInstance {
                    source: "bundle!mesh?flavour=opaque text".into(),
                    variant: 0,
                }),
            ),
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: "mesh".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(0),
                binding: StateOverlayRef::Alias(1),
                alias: 2,
                component: ComponentValue::MESH_INSTANCE,
                mode: ComponentOverlayMode::Auto,
                fields: vec![FieldWrite {
                    offset: offset_of!(MeshInstance, source) as u32,
                    value: FieldValue::String("archive!/mesh?part=opaque text".into()),
                }],
            },
        ],
    );
    let entity = report.outcomes[0].result.as_ref().unwrap()[0].1;
    let report = run(
        &mut world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::MeshInstance(MeshInstance {
                source: "x".repeat(4097),
                variant: 0,
            }),
        )],
    );
    assert!(report.outcomes[0].result.is_ok());
    let ComponentValue::MeshInstance(base) = world
        .read()
        .producer_component(entity, ComponentValue::MESH_INSTANCE)
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(base.source, "x".repeat(4097));
    assert_eq!(
        world
            .world
            .components
            .mesh_instance(entity.index() as usize)
            .unwrap()
            .source,
        "archive!/mesh?part=opaque text"
    );
}
