//! The composed query and keyboard traversal are bounded by the composition
//! they read, not by a fixed whole-view work budget.

use crate::components::{Camera, GuiCheckbox, PickingGeometry, Transform};
use crate::services::gui_input::query::{GuiPickingBlocker, GuiQueryUnavailable, GuiQueryWorlds};
use crate::services::gui_input::router::*;
use crate::services::gui_input::routing_test_support::*;
use crate::services::gui_input::test_support::*;
use crate::systems::geometry::{GeometryDefinition, GeometryShape};
use crate::{
    Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef, HostRuntime, WorldRef,
};

/// The former fixed budget for one whole composed view.
const FORMER_WORK_LIMIT: usize = 16_384;

#[test]
fn a_world_reached_twice_fails_the_walk_instead_of_looping() {
    let (mut host, _) = host();
    let (first, second) = (world(&mut host), world(&mut host));
    let mut worlds = GuiQueryWorlds::default();
    assert_eq!(worlds.enter(first), Ok(()));
    assert_eq!(worlds.enter(second), Ok(()));
    assert_eq!(worlds.enter(first), Err(GuiQueryUnavailable::RepeatedWorld));
}

fn picking_box(center: [f32; 3], half: f64) -> Vec<ComponentValue> {
    vec![
        ComponentValue::Transform(Transform {
            x: center[0],
            y: center[1],
            z: center[2],
            ..Default::default()
        }),
        ComponentValue::PickingGeometry(PickingGeometry {
            geometry: GeometryDefinition::from(GeometryShape::Box {
                min: [-half; 3],
                max: [half; 3],
            })
            .encode()
            .unwrap(),
            ..Default::default()
        }),
    ]
}

/// Create `count` entities in one batch, each from `values(index)`, placed
/// under `parent` when given.
fn create_many(
    host: &mut HostRuntime,
    world: WorldRef,
    count: usize,
    parent: Option<EntityId>,
    values: impl Fn(usize) -> Vec<ComponentValue>,
) -> Vec<EntityId> {
    let mut commands = Vec::new();
    for index in 0..count {
        let alias = EntityRef::Alias(index as u32 + 1);
        commands.push(Command::Create {
            alias: index as u32 + 1,
            metadata: Default::default(),
            adopt: false,
        });
        commands.extend(
            values(index)
                .into_iter()
                .map(|value| Command::insert_value(alias.clone(), value)),
        );
        if let Some(parent) = parent {
            commands.push(Command::PlaceEntity {
                entity: alias,
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Handle(parent)),
                    before: None,
                },
            });
        }
    }
    apply(host, world, commands)
        .into_iter()
        .map(|(_, entity)| entity)
        .collect()
}

/// A `4 x 3` panel of `count` checkboxes: a tall first one, then thin ones,
/// all within the Canvas extent.
fn dense_panel(
    host: &mut HostRuntime,
    parent: WorldRef,
    transform: Transform,
    count: usize,
) -> Panel {
    let child = world(host);
    let (output, output_entity) = canvas_root(host, child, 4.0, 3.0);
    let controls = create_many(host, child, count, Some(output_entity), |index| {
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            sized(
                0,
                4.0,
                if index == 0 {
                    0.5
                } else {
                    0.005
                },
            ),
        ]
    });
    let anchor = attach(host, parent, output, [4.0, 3.0], transform);
    Panel {
        output,
        anchor,
        controls,
    }
}

#[test]
fn pointer_routing_and_tab_traversal_scale_past_the_former_whole_view_budget() {
    const PANELS: usize = 16;
    const CONTROLS: usize = 500;
    const GEOMETRY: usize = FORMER_WORK_LIMIT + 116;
    let (mut host, _) = host();
    let world = scene_world(&mut host);
    let root = camera_root(&mut host, world, Camera::default());
    // Unmarked picking geometry far off every ray, more than the former
    // budget on its own. Only explicit blockers may cost query work.
    create_many(&mut host, world, GEOMETRY, None, |index| {
        picking_box([1000.0 + index as f32, 0.0, 0.0], 0.1)
    });
    // The nearest panel faces the camera on its axis; the rest sit behind it.
    let panels: Vec<_> = (0..PANELS)
        .map(|index| {
            let placement = if index == 0 {
                at(0.0, 0.0, 1.0)
            } else {
                at(index as f32 * 5.0 - 40.0, 0.0, -5.0)
            };
            dense_panel(&mut host, world, placement, CONTROLS)
        })
        .collect();
    // The former traversal spent a unit per control record, per hit record
    // and per collected control, so this view alone exceeded the budget.
    const _: () = assert!(PANELS * CONTROLS * 3 > FORMER_WORK_LIMIT);
    // One explicit blocker in front of the nearest panel's lower half.
    let blocker = create(&mut host, world, picking_box([0.0, -0.5, 5.0], 0.2), None);
    let mut rig = Rig::new(host, root, viewport(800, 800));
    let blocked = [0.5, 0.620_71];
    let identity = rig
        .host
        .pick_view(rig.query(), blocked, false)
        .unwrap()
        .1
        .unwrap()
        .identity;
    assert_eq!(identity.entity, blocker);
    rig.blockers = vec![GuiPickingBlocker {
        world: identity.world,
        entity: blocker,
        incarnation: identity.incarnation,
    }];
    rig.rebind();

    // The first control spans panel rows 0..0.5, world y 1.0..1.5 at z = 1,
    // nine metres from the camera: y 1.25 is normalized
    // (1 - 1.25 / (9 * tan 22.5 deg)) / 2.
    let first = [0.5, 0.332_4];
    rig.send(press(1, first));
    rig.send(release(1, first));
    let nearest = &panels[0];
    assert_eq!(
        rig.value(nearest.world(), nearest.controls[0]),
        GuiTestValue::Bool(true)
    );
    // The explicit blocker still occludes the panel behind it.
    assert_eq!(rig.send(press(2, blocked)), GuiRoutingDisposition::Blocked);

    // Tab enters the nearest panel; BackTab from there wraps to the last
    // control of the last panel, across every control in the view.
    rig.send(key(GuiPhysicalKey::Escape));
    assert_eq!(
        rig.send(key(GuiPhysicalKey::Tab)),
        GuiRoutingDisposition::Routed {
            target: rig.snapshot(nearest.world(), nearest.controls[0]).target
        }
    );
    let last = panels
        .iter()
        .map(|panel| (panel.world(), *panel.controls.last().unwrap()))
        .collect::<Vec<_>>();
    let GuiRoutingDisposition::Routed {
        target,
    } = rig.send(key(GuiPhysicalKey::BackTab))
    else {
        panic!("BackTab must route");
    };
    assert!(last.contains(&(target.world, target.entity)));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}
