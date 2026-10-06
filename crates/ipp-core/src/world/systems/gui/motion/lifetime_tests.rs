use crate::systems::gui::GuiSystem;
use crate::systems::gui::motion::GuiThemeMotion;
use crate::systems::gui::presentation::{GuiSkin, GuiTheme};
use crate::{Batch, Command, ComponentValue, EntityId, EntityRef, HostRuntime, WorldId};

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> Vec<(u32, EntityId)> {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();

    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()
}

fn create(
    host: &mut HostRuntime,
    world: WorldId,
    values: Vec<ComponentValue>,
    parent: Option<EntityId>,
) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 1,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(1), value)),
    );
    if let Some(parent) = parent {
        operations.push(Command::PlaceEntity {
            entity: EntityRef::Alias(1),
            placement: crate::EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        });
    }
    apply(host, world, operations)[0].1
}

fn counts(host: &mut HostRuntime, world: WorldId) -> [usize; 5] {
    host.world_mut(world)
        .unwrap()
        .system::<GuiSystem>(GuiSystem::ID)
        .unwrap()
        .motion
        .index_counts()
}

fn select(host: &mut HostRuntime, world: WorldId, entity: EntityId, selected: bool) {
    apply(
        host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_BUTTON,
            field: crate::FieldWrite {
                offset: std::mem::offset_of!(crate::components::GuiButton, selected) as u32,
                value: crate::FieldValue::Bool(selected),
            },
        }],
    );
}

#[test]
fn motion_indexes_stay_bounded_through_control_churn_and_mid_transition_deletion() {
    let mut host = HostRuntime::new();
    let world = host
        .create_world(
            Default::default(),
            &[
                crate::systems::animation::AnimationSystem::ID,
                crate::systems::asset_dependencies::AssetDependencySystem::ID,
                crate::systems::canvas::CanvasSystem::ID,
                crate::systems::gui::GuiSystem::ID,
            ],
        )
        .unwrap();
    let parent = create(&mut host, world, Vec::new(), None);
    let themes = std::array::from_fn::<_, 2, _>(|_| {
        create(
            &mut host,
            world,
            vec![
                ComponentValue::GuiTheme(GuiTheme::default()),
                ComponentValue::GuiThemeMotion(GuiThemeMotion::default()),
            ],
            None,
        )
    });
    host.frame(0.0).unwrap();
    let mut control = None;
    for turn in 0..256 {
        if let Some(entity) = control.take() {
            apply(
                &mut host,
                world,
                vec![Command::Delete {
                    entity: EntityRef::Handle(entity),
                }],
            );
            assert_eq!(counts(&mut host, world), [0; 5]);
        }
        let entity = create(
            &mut host,
            world,
            vec![
                ComponentValue::GuiButton(Default::default()),
                ComponentValue::GuiSkin(GuiSkin {
                    theme: themes[turn % 2],
                    ..Default::default()
                }),
            ],
            Some(parent),
        );
        control = Some(entity);

        // Every eighth control is deleted halfway through a selection fill.
        if turn % 8 == 0 {
            select(&mut host, world, entity, true);
            let frame = host.frame(0.01).unwrap();
            assert!(frame.worlds.values().all(Result::is_ok));
            assert_eq!(counts(&mut host, world), [0, 0, 1, 0, 0]);
        }
    }
    host.frame(0.0).unwrap();
    assert_eq!(counts(&mut host, world), [0; 5]);
    apply(
        &mut host,
        world,
        vec![Command::Delete {
            entity: EntityRef::Handle(control.unwrap()),
        }],
    );
    host.frame(0.0).unwrap();
    assert_eq!(counts(&mut host, world), [0; 5]);
}
