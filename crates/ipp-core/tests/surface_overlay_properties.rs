//! Surface-owned overlay properties follow item declaration lifetimes.
#![cfg(feature = "surfaces")]

mod support;
use support::WorldTestDriver;

use ipp_core::{
    Batch, Command, ComponentOverlayMode, ComponentValue, DynamicValue, EntityOverlayMode,
    EntityRef, FieldValue, FieldWrite, HostRuntime, StateOverlayRef, Surface, SurfaceItemContent,
    SurfaceItemId, SurfaceItemStyle, WorldContext, WorldLimits,
};

fn run(world: &mut WorldContext<'_>, operations: Vec<Command>) -> ipp_core::BatchOutcome {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    let outcome = world.update_for_test(0.0).unwrap().outcomes.remove(0);
    assert!(outcome.result.is_ok(), "{outcome:?}");
    outcome
}

fn items_field(surface: &Surface) -> FieldWrite {
    ComponentValue::Surface(surface.clone())
        .fields()
        .into_iter()
        .find_map(|(offset, value)| match value {
            ipp_core::components::schema::FieldValue::Bytes(value) => Some(FieldWrite {
                offset,
                value: FieldValue::Bytes(value),
            }),
            _ => None,
        })
        .unwrap()
}

#[test]
fn owned_surface_only_deletes_properties_for_structurally_removed_items() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let mut surface = Surface::default();
    let item = surface
        .insert_item(0, SurfaceItemContent::Drawing, SurfaceItemStyle::default())
        .unwrap();
    let color = Surface::property_name(item, "color").unwrap();
    let properties = surface
        .properties
        .descriptors()
        .keys()
        .map(|name| (name.clone(), surface.properties.get(name).unwrap()))
        .collect();
    let outcome = run(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 1,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(1),
                alias: 2,
                symbolic_id: "surface-owned".into(),
                mode: EntityOverlayMode::Owned,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(1),
                binding: StateOverlayRef::Alias(2),
                alias: 3,
                component: ComponentValue::SURFACE,
                mode: ComponentOverlayMode::Owned,
                fields: vec![items_field(&surface)],
            },
            Command::UpdateDynamicComponentStateOverlay {
                owner: StateOverlayRef::Alias(1),
                overlay: StateOverlayRef::Alias(3),
                properties,
                clear: vec![],
            },
        ],
    );
    let entity = outcome.state_overlays[1].entity.unwrap();
    let owner = StateOverlayRef::Handle(outcome.state_overlays[0].id);
    let overlay = StateOverlayRef::Handle(outcome.state_overlays[2].id);
    let original_key = world
        .surface(entity)
        .unwrap()
        .properties
        .key(&color)
        .unwrap();

    run(
        &mut world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::SURFACE,
            name: color.clone(),
            value: DynamicValue::Vec4([0.0, 1.0, 0.0, 1.0]),
        }],
    );
    run(
        &mut world,
        vec![Command::UpdateDynamicComponentStateOverlay {
            owner,
            overlay,
            properties: vec![],
            clear: vec![color.clone()],
        }],
    );
    assert_eq!(
        world.surface(entity).unwrap().properties.get(&color),
        Some(DynamicValue::Vec4([0.0, 1.0, 0.0, 1.0])),
        "an active item's producer value must survive override withdrawal"
    );

    let names = [
        "position",
        "scale",
        "color",
        "opacity",
        "font_size",
        "asset",
    ]
    .iter()
    .map(|suffix| Surface::property_name(item, suffix).unwrap())
    .collect::<Vec<_>>();
    run(
        &mut world,
        vec![
            Command::UpdateComponentStateOverlay {
                owner,
                overlay,
                fields: vec![items_field(&Surface::default())],
                clear: vec![],
            },
            Command::UpdateDynamicComponentStateOverlay {
                owner,
                overlay,
                properties: vec![],
                clear: names,
            },
        ],
    );
    assert_eq!(world.surface(entity).unwrap().items(), []);
    assert_eq!(world.surface(entity).unwrap().properties.get(&color), None);

    run(
        &mut world,
        vec![Command::UpdateDynamicComponentStateOverlay {
            owner,
            overlay,
            properties: vec![(color.clone(), DynamicValue::Vec4([0.5; 4]))],
            clear: vec![],
        }],
    );
    assert_ne!(
        world.surface(entity).unwrap().properties.key(&color),
        Some(original_key),
        "a structurally removed item property identity must not be reused"
    );
    assert_eq!(item, SurfaceItemId(1));
}
