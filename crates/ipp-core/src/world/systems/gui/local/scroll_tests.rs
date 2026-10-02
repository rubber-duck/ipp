use super::local_tests::{GuiTestHost, action, apply, frame, outcomes, snapshot};
use super::*;
use crate::ErrorReason;
use crate::systems::gui::layout::GuiLayout;
use crate::systems::gui::presentation::GuiControlRecord;
use crate::{Command, ComponentValue, EntityPlacementRef, EntityRef};

fn fixture(count: u32) -> (GuiTestHost, crate::WorldId, crate::EntityId) {
    let mut host = GuiTestHost::default();
    let world = host
        .create_world(
            Default::default(),
            // Scroll content animates through clips, which need asset dependencies.
            &[
                crate::systems::animation::AnimationSystem::ID,
                crate::systems::asset_dependencies::AssetDependencySystem::ID,
                crate::systems::canvas::CanvasSystem::ID,
                crate::systems::gui::GuiSystem::ID,
                crate::systems::gui::GuiLayoutSystem::ID,
            ],
        )
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue_canvas_state_update(crate::CanvasStateUpdate {
            extent: Some([100.0, 50.0]),
            units_per_metre: Some(1.0),
        })
        .unwrap();
    let entity = apply(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::GuiLayout(GuiLayout {
                    width: 100.0,
                    height: 50.0,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::GuiVirtualList(GuiVirtualList {
                    item_count: count,
                    item_extent: 10.0,
                    overscan: 1,
                    axis: 1,
                    ..Default::default()
                }),
            ),
        ],
    )[0]
    .1;
    for _ in 0..3 {
        frame(&mut host);
    }
    (host, world, entity)
}

/// The scroll fields of a ScrollView or VirtualList.
#[derive(Clone, Debug, PartialEq)]
struct ScrollFields {
    offset: [f32; 2],
    anchor: (u32, f32),
    viewport: [f32; 2],
    content: [f32; 2],
    capacity: [f32; 2],
    range: (u32, u32),
    item_count: Option<u32>,
}

/// Read the scroll fields of `entity`, if it scrolls.
fn scroll(
    host: &mut crate::HostRuntime,
    world: crate::WorldId,
    entity: crate::EntityId,
) -> Option<ScrollFields> {
    host.world_mut(world)
        .unwrap()
        .inspect(entity)?
        .components
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::GuiVirtualList(list) => Some(ScrollFields {
                offset: [list.offset_x, list.offset_y],
                anchor: (list.anchor_index, list.anchor_offset),
                viewport: [list.viewport_x, list.viewport_y],
                content: [list.content_x, list.content_y],
                capacity: [list.capacity_x, list.capacity_y],
                range: (list.range_first, list.range_last),
                item_count: Some(list.item_count),
            }),
            ComponentValue::GuiScrollView(view) => Some(ScrollFields {
                offset: [view.offset_x, view.offset_y],
                anchor: (0, 0.0),
                viewport: [view.viewport_x, view.viewport_y],
                content: [view.content_x, view.content_y],
                capacity: [view.capacity_x, view.capacity_y],
                range: (0, 0),
                item_count: None,
            }),
            _ => None,
        })
}

fn fields(
    host: &mut crate::HostRuntime,
    world: crate::WorldId,
    entity: crate::EntityId,
) -> ScrollFields {
    scroll(host, world, entity).expect("scrolling control")
}

/// Whether a published control record carries exactly these scroll fields.
fn publishes(record: &GuiControlRecord, fields: &ScrollFields) -> bool {
    record.scroll() == Some((fields.offset, fields.capacity))
}

#[test]
fn ordinary_virtual_ranges_settle_without_repeated_normalization() {
    let (mut host, world, entity) = fixture(100_000);
    let before = snapshot(&mut host, world, entity);
    assert_eq!(fields(&mut host, world, entity).range, (0, 6));
    action(
        &mut host,
        world,
        before.target,
        GuiLocalAction::ScrollToIndex {
            index: 40,
            offset: 2.0,
        },
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    let moved = fields(&mut host, world, entity);
    assert_eq!(moved.offset, [0.0, 402.0]);
    assert_eq!(moved.range, (39, 47));
    for _ in 0..4 {
        frame(&mut host);
    }
    assert_eq!(fields(&mut host, world, entity), moved);
}

#[test]
fn shrinking_and_regrowing_keeps_the_clamped_surviving_anchor() {
    let (mut host, world, entity) = fixture(100);
    let initial = snapshot(&mut host, world, entity);
    action(
        &mut host,
        world,
        initial.target,
        GuiLocalAction::ScrollToIndex {
            index: 90,
            offset: 2.0,
        },
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    for (count, expected) in [(8, 30.0), (100, 30.0), (0, 0.0), (100, 0.0)] {
        apply(
            &mut host,
            world,
            vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_VIRTUAL_LIST,
                field: crate::FieldWrite {
                    offset: std::mem::offset_of!(GuiVirtualList, item_count) as u32,
                    value: crate::FieldValue::U32(count),
                },
            }],
        );
        for _ in 0..3 {
            frame(&mut host);
        }
        assert_eq!(fields(&mut host, world, entity).offset[1], expected);
    }
}

#[test]
fn realized_measurements_preserve_the_visible_anchor_and_use_explicit_indices() {
    let (mut host, world, entity) = fixture(100);
    let initial = snapshot(&mut host, world, entity);
    action(
        &mut host,
        world,
        initial.target,
        GuiLocalAction::ScrollToIndex {
            index: 40,
            offset: 2.0,
        },
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    apply(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::GuiVirtualItem(GuiVirtualItem {
                    index: 2,
                }),
            ),
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::GuiLayout(GuiLayout {
                    width: 100.0,
                    height: 30.0,
                    ..Default::default()
                }),
            ),
            Command::PlaceEntity {
                entity: EntityRef::Alias(1),
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Handle(entity)),
                    before: None,
                },
            },
        ],
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    let measured = fields(&mut host, world, entity);
    assert_eq!(
        (measured.offset, measured.anchor),
        ([0.0, 422.0], (40, 2.0))
    );
}

/// Commands a client sends to declare the items `indices` of `list`, each
/// measuring `extent` along the main axis.
fn declare_items(
    list: crate::EntityId,
    indices: std::ops::Range<u32>,
    extent: f32,
) -> Vec<Command> {
    indices
        .enumerate()
        .flat_map(|(slot, index)| {
            let alias = slot as u32 + 1;
            [
                Command::Create {
                    alias,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(alias),
                    ComponentValue::GuiVirtualItem(GuiVirtualItem {
                        index,
                    }),
                ),
                Command::insert_value(
                    EntityRef::Alias(alias),
                    ComponentValue::GuiLayout(GuiLayout {
                        width: 90.0,
                        height: extent,
                        ..Default::default()
                    }),
                ),
                Command::PlaceEntity {
                    entity: EntityRef::Alias(alias),
                    placement: EntityPlacementRef {
                        parent: Some(EntityRef::Handle(list)),
                        before: None,
                    },
                },
            ]
        })
        .collect()
}

/// Mutation boundary at which a client's item change lands, relative to the
/// boundary that applies the last thumb move.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Arrival {
    Before,
    WithTheMove,
    After,
}

/// The order in which the client deletes the measured top items and declares
/// the measured end items around the last thumb move.
#[derive(Clone, Copy, Debug)]
struct ItemOrdering {
    top_deleted: Arrival,
    end_declared: Arrival,
}

/// The last move of a thumb drag past the track end writes the capacity the
/// router saw, which is an offset in the previous layout's item placement.
/// Item changes landing at the same mutation boundary move that placement;
/// read in the new placement the offset falls short of the end inside an
/// earlier item, and the anchor then keeps that short in-item offset while
/// later measurements arrive.
#[test]
fn a_thumb_dragged_to_the_end_settles_at_the_end_in_every_measurement_order() {
    use super::input_test_support::GuiRoutedHost;
    use crate::services::gui_input::router::{GuiPhysicalButton, GuiPhysicalInput};

    use Arrival::*;

    for ordering in [
        (Before, Before),
        (Before, WithTheMove),
        (Before, After),
        (WithTheMove, WithTheMove),
        (WithTheMove, After),
        (After, After),
    ]
    .map(|(top_deleted, end_declared)| ItemOrdering {
        top_deleted,
        end_declared,
    }) {
        // Twenty items estimated at 10 in a 30-high viewport, with a 1.5-wide
        // bar flush with its right side and ends. Measured items are shorter,
        // so each declared window shortens the content.
        let mut host = GuiRoutedHost::new();
        let world = host.world.id();
        let list = host.create(vec![
            ComponentValue::GuiLayout(GuiLayout {
                width: 100.0,
                height: 30.0,
                ..Default::default()
            }),
            ComponentValue::GuiVirtualList(GuiVirtualList {
                item_count: 20,
                item_extent: 10.0,
                overscan: 1,
                axis: 1,
                bar_thickness: 1.5,
                bar_inset: 0.0,
                bar_end_inset: 0.0,
                ..Default::default()
            }),
        ]);

        // The client has measured the seven top items: 28 shorter.
        let top = apply(&mut host.gui, world, declare_items(list, 0..7, 6.0));
        for _ in 0..3 {
            host.frame();
        }
        assert_eq!(fields(&mut host.gui, world, list).capacity[1], 142.0);
        let delete_top: Vec<_> = top
            .iter()
            .map(|&(_, entity)| Command::Delete {
                entity: EntityRef::Handle(entity),
            })
            .collect();

        // The seven end items measure 7: 21 shorter.
        let declare_end = declare_items(list, 13..20, 7.0);
        let end_capacity = 149.0;

        // The thumb is pressed at the track start on the right edge; the last
        // move passes the track end, so the router asks for the capacity the
        // previous layout published.
        host.route(GuiPhysicalInput::PointerDown {
            pointer: 1,
            point: host.point([99.25, 1.0]),
            button: GuiPhysicalButton::Primary,
        })
        .unwrap();
        let changes = |arrival| {
            let mut operations = Vec::new();
            if ordering.top_deleted == arrival {
                operations.extend(delete_top.iter().cloned());
            }
            if ordering.end_declared == arrival {
                operations.extend(declare_end.iter().cloned());
            }
            operations
        };
        let before = changes(Before);
        if !before.is_empty() {
            apply(&mut host.gui, world, before);
        }
        host.send(GuiPhysicalInput::PointerMove {
            pointer: 1,
            point: host.point([99.25, 60.0]),
        })
        .unwrap();
        let with_the_move = changes(WithTheMove);
        if with_the_move.is_empty() {
            host.frame();
        } else {
            apply(&mut host.gui, world, with_the_move);
        }
        let after = changes(After);
        if !after.is_empty() {
            apply(&mut host.gui, world, after);
        }
        host.route(GuiPhysicalInput::PointerUp {
            pointer: 1,
            point: host.point([99.25, 60.0]),
            button: GuiPhysicalButton::Primary,
        })
        .unwrap();
        for _ in 0..3 {
            host.frame();
        }

        let settled = fields(&mut host.gui, world, list);
        assert_eq!(settled.capacity[1], end_capacity, "{ordering:?}");
        assert_eq!(settled.range.1, 20, "{ordering:?}");
        assert_eq!(
            settled.offset[1], settled.capacity[1],
            "{ordering:?}: the drag to the end stopped short: {settled:?}"
        );
    }
}

fn published(
    host: &crate::HostRuntime,
    world: crate::WorldId,
) -> (crate::systems::canvas::CanvasPublication, GuiControlRecord) {
    use crate::systems::canvas::CanvasSystem;
    use crate::systems::gui::presentation::GuiCanvasPublication;
    let publication = host
        .publication(host.latest_publication(world).unwrap())
        .unwrap();
    let gui = publication
        .chunk(CanvasSystem::ID)
        .unwrap()
        .data::<GuiCanvasPublication>()
        .unwrap();
    let view = gui.views.values().next().unwrap();
    let canvas = publication
        .output(view.selection)
        .unwrap()
        .data::<crate::systems::canvas::CanvasPublication>()
        .unwrap();
    (canvas.clone(), view.controls[0].record.clone())
}

#[test]
fn measurement_clamp_commits_completed_paint_and_semantics_in_the_same_pass() {
    let (mut host, world, entity) = fixture(100);
    let initial = snapshot(&mut host, world, entity);
    action(
        &mut host,
        world,
        initial.target,
        GuiLocalAction::ScrollToIndex {
            index: 90,
            offset: 2.0,
        },
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    let before = published(&host, world);
    apply(
        &mut host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_VIRTUAL_LIST,
            field: crate::FieldWrite {
                offset: std::mem::offset_of!(GuiVirtualList, item_count) as u32,
                value: crate::FieldValue::U32(8),
            },
        }],
    );
    frame(&mut host);
    let settled = published(&host, world);
    let clamped = fields(&mut host, world, entity);
    assert_eq!(clamped.offset, [0.0, 30.0]);
    assert_eq!(clamped.item_count, Some(8));
    assert_ne!(settled.1, before.1);
    assert!(publishes(&settled.1, &clamped), "{:?}", settled.1);
    for _ in 0..4 {
        frame(&mut host);
    }
    assert_eq!(published(&host, world), settled);
}

#[test]
fn restored_scroll_keeps_clamped_position_without_old_intent() {
    let (mut host, world, entity) = fixture(100);
    let initial = snapshot(&mut host, world, entity);
    action(
        &mut host,
        world,
        initial.target,
        GuiLocalAction::ScrollToIndex {
            index: 90,
            offset: 2.0,
        },
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    apply(
        &mut host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_VIRTUAL_LIST,
            field: crate::FieldWrite {
                offset: std::mem::offset_of!(GuiVirtualList, item_count) as u32,
                value: crate::FieldValue::U32(8),
            },
        }],
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    let saved = host.save_world(world, 71, Default::default()).unwrap();
    let loaded = host
        .load_world(
            &saved,
            71,
            crate::services::world_serialization::WorldLoadOptions {
                symbolic_id: Some("restored-scroll".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let restored = host.world_mut(loaded.root.id()).unwrap().entities()[0].id;
    apply(
        &mut host,
        loaded.root.id(),
        vec![Command::SetField {
            entity: EntityRef::Handle(restored),
            component: ComponentValue::GUI_VIRTUAL_LIST,
            field: crate::FieldWrite {
                offset: std::mem::offset_of!(GuiVirtualList, item_count) as u32,
                value: crate::FieldValue::U32(100),
            },
        }],
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    let state = fields(&mut host, loaded.root.id(), restored);
    assert_eq!((state.offset, state.anchor), ([0.0, 30.0], (3, 0.0)));
    assert_eq!(state.item_count, Some(100));
}

#[test]
fn raw_leaf_measurement_changes_normalize_without_oscillation() {
    let (mut host, world, entity) = fixture(100);
    let initial = snapshot(&mut host, world, entity);
    action(
        &mut host,
        world,
        initial.target,
        GuiLocalAction::ScrollToIndex {
            index: 40,
            offset: 2.0,
        },
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    let child = apply(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::GuiVirtualItem(GuiVirtualItem {
                    index: 2,
                }),
            ),
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::CanvasBox(crate::systems::canvas::CanvasBox {
                    width: 100.0,
                    height: 30.0,
                    ..Default::default()
                }),
            ),
            Command::PlaceEntity {
                entity: EntityRef::Alias(1),
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Handle(entity)),
                    before: None,
                },
            },
        ],
    )[0]
    .1;
    for _ in 0..3 {
        frame(&mut host);
    }
    assert_eq!(fields(&mut host, world, entity).offset[1], 422.0);
    apply(
        &mut host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(child),
            component: ComponentValue::CANVAS_BOX,
            field: crate::FieldWrite {
                offset: std::mem::offset_of!(crate::systems::canvas::CanvasBox, height) as u32,
                value: crate::FieldValue::F32(50.0),
            },
        }],
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    let stable = fields(&mut host, world, entity);
    assert_eq!(stable.offset[1], 442.0);
    for _ in 0..5 {
        frame(&mut host);
        assert_eq!(fields(&mut host, world, entity), stable);
    }
}

#[test]
fn continuous_actions_publish_each_committed_offset_and_range_without_settlement_starvation() {
    let (mut host, world, entity) = fixture(100);
    for index in [10, 20, 30, 40] {
        let previous = snapshot(&mut host, world, entity);
        action(
            &mut host,
            world,
            previous.target,
            GuiLocalAction::ScrollToIndex {
                index,
                offset: 0.0,
            },
        );
        frame(&mut host);
        let (_, current) = published(&host, world);
        let stored = fields(&mut host, world, entity);
        assert!(publishes(&current, &stored), "{current:?}");
        assert_eq!(stored.anchor.0, index);
        assert_eq!(stored.range.0, index - 1);
    }
}

#[test]
fn continuously_animated_measurement_publishes_current_geometry_offset_and_range() {
    use crate::services::asset_management::{AssetUpload, AssetUploadIdentity};
    use crate::systems::animation::*;
    use crate::systems::canvas::CanvasBox;

    let (mut host, world, entity) = fixture(100);
    let child = apply(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::GuiVirtualItem(GuiVirtualItem {
                    index: 2,
                }),
            ),
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::CanvasBox(CanvasBox {
                    width: 100.0,
                    height: 10.0,
                    ..Default::default()
                }),
            ),
            Command::PlaceEntity {
                entity: EntityRef::Alias(1),
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Handle(entity)),
                    before: None,
                },
            },
        ],
    )[0]
    .1;
    frame(&mut host);
    let initial = snapshot(&mut host, world, entity);
    action(
        &mut host,
        world,
        initial.target,
        GuiLocalAction::ScrollToIndex {
            index: 40,
            offset: 2.0,
        },
    );
    frame(&mut host);
    let target = AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: ComponentValue::CANVAS_BOX,
        offsets: vec![std::mem::offset_of!(CanvasBox, height) as u32],
    });
    let clip = AnimationClip::new(
        10.0,
        vec![AnimationTrack {
            target: target.clone(),
            keys: vec![
                AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(crate::components::schema::FieldValue::F32(10.0)),
                    interpolation: AnimationInterpolation::Linear,
                },
                AnimationKeyframe {
                    time: 10.0,
                    value: AnimationValue::Field(crate::components::schema::FieldValue::F32(90.0)),
                    interpolation: AnimationInterpolation::Step,
                },
            ],
        }],
    )
    .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue_asset(AssetUpload {
            id: 1,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 1,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    for _ in 0..512 {
        let report = host.frame(0.0).unwrap();
        if report.worlds[&world]
            .as_ref()
            .unwrap()
            .assets
            .iter()
            .any(|asset| asset.id == 1)
        {
            break;
        }
    }
    {
        let mut context = host.world_mut(world).unwrap();
        let controller = context
            .create_animation_controller(AnimationControllerDescription {
                drivers: vec![AnimationDriverDescription {
                    source: "asset://10/1".into(),
                    variant: 0,
                    track: 0,
                    target: child,
                    property: target,
                    entity_bindings: Vec::new(),
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                }],
                ..Default::default()
            })
            .unwrap();
        context
            .control_animation_controller(controller, AnimationPlaybackControl::Play)
            .unwrap();
    }
    let mut previous_height = 10.0;
    for _ in 0..8 {
        frame(&mut host);
        let height = host
            .world_mut(world)
            .unwrap()
            .world
            .components
            .canvas_box(child.index() as usize)
            .unwrap()
            .height;
        assert!(
            height > previous_height,
            "real numeric animation must advance every measured frame"
        );
        previous_height = height;
        let (_, observed) = published(&host, world);
        let current = fields(&mut host, world, entity);
        assert!(
            publishes(&observed, &current),
            "Canvas must not retain a prior scroll position while geometry keeps changing"
        );
        assert_eq!(current.offset, [0.0, 392.0 + height]);
        assert_eq!(current.anchor.0, 40);
        assert_eq!(current.content[1], 990.0 + height);
        let placement = host
            .world_mut(world)
            .unwrap()
            .gui_entity_layout(child)
            .unwrap();
        assert_eq!(placement.origin[1], 20.0 - (392.0 + height));
        assert_eq!(placement.size[1], height);
    }
}

/// A 100 x 50 canvas whose root is a vertical ScrollView over a
/// 100 x 200 box, beside a checkbox.
fn scroll_view_fixture() -> (
    GuiTestHost,
    crate::WorldId,
    crate::EntityId,
    crate::EntityId,
) {
    let (mut host, world, root) = fixture(0);
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(root),
                component: ComponentValue::GUI_VIRTUAL_LIST,
            },
            Command::insert_value(
                EntityRef::Handle(root),
                ComponentValue::GuiScrollView(GuiScrollView {
                    axis: 1,
                    ..Default::default()
                }),
            ),
        ],
    );
    let mut child = |value: ComponentValue| {
        apply(
            &mut host,
            world,
            vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(EntityRef::Alias(1), value),
                Command::PlaceEntity {
                    entity: EntityRef::Alias(1),
                    placement: EntityPlacementRef {
                        parent: Some(EntityRef::Handle(root)),
                        before: None,
                    },
                },
            ],
        )[0]
        .1
    };
    child(ComponentValue::CanvasBox(
        crate::systems::canvas::CanvasBox {
            width: 100.0,
            height: 200.0,
            ..Default::default()
        },
    ));
    let checkbox = child(ComponentValue::GuiCheckbox(Default::default()));
    for _ in 0..3 {
        frame(&mut host);
    }
    (host, world, root, checkbox)
}

#[test]
fn a_scroll_that_moves_nothing_writes_and_repaints_nothing() {
    let (mut host, world, entity) = fixture(100);
    let at_start = fields(&mut host, world, entity);
    let (canvas, _) = published(&host, world);
    let target = snapshot(&mut host, world, entity).target;

    // At the start edge a backward scroll is clamped to the same position: it
    // settles as an unchanged value without a write, repaint or layout pass.
    for request in [
        GuiLocalAction::ScrollBy([0.0, -5.0]),
        GuiLocalAction::ScrollBy([0.0, 0.0]),
        GuiLocalAction::ScrollTo([0.0, 0.0]),
        GuiLocalAction::ScrollToIndex {
            index: 0,
            offset: 0.0,
        },
    ] {
        action(&mut host, world, target, request.clone());
        frame(&mut host);
        assert_eq!(
            outcomes(&mut host, world).remove(0).result,
            Ok(None),
            "{request:?}"
        );
        assert_eq!(fields(&mut host, world, entity), at_start);
        let (unchanged, _) = published(&host, world);
        assert_eq!(unchanged.layout_revision, canvas.layout_revision);
        assert_eq!(unchanged.paint_revision, canvas.paint_revision);
    }

    // A scroll that moves writes the offset once; the same request at the
    // far edge is again unchanged. Value effects are never published.
    action(
        &mut host,
        world,
        target,
        GuiLocalAction::ScrollBy([0.0, 5000.0]),
    );
    frame(&mut host);
    let moved = fields(&mut host, world, entity);
    assert_eq!(moved.offset, [0.0, 950.0]);
    assert_eq!(outcomes(&mut host, world).remove(0).result, Ok(None));
    assert!(host.effects.is_empty());
    action(
        &mut host,
        world,
        target,
        GuiLocalAction::ScrollBy([0.0, 10.0]),
    );
    frame(&mut host);
    assert_eq!(outcomes(&mut host, world)[0].result, Ok(None));
    assert_eq!(fields(&mut host, world, entity), moved);
}

#[test]
fn scroll_to_index_refuses_out_of_range_requests_and_non_list_targets() {
    let (mut host, world, entity) = fixture(100);
    let list = snapshot(&mut host, world, entity);
    let unscrolled = fields(&mut host, world, entity);
    for (index, offset) in [
        (100, 0.0),
        (u32::MAX, 0.0),
        (0, -1.0),
        (0, f32::NAN),
        (0, f32::INFINITY),
    ] {
        action(
            &mut host,
            world,
            list.target,
            GuiLocalAction::ScrollToIndex {
                index,
                offset,
            },
        );
        frame(&mut host);
        assert_eq!(
            outcomes(&mut host, world)[0].result,
            Err(ErrorReason::InvalidValue),
            "{index} {offset}"
        );
    }
    assert_eq!(fields(&mut host, world, entity), unscrolled);

    // An ordinary ScrollView and a non-scrolling control have no items.
    let (mut host, world, view, checkbox) = scroll_view_fixture();
    for entity in [view, checkbox] {
        let observed = snapshot(&mut host, world, entity);
        let unchanged = scroll(&mut host, world, entity);
        action(
            &mut host,
            world,
            observed.target,
            GuiLocalAction::ScrollToIndex {
                index: 0,
                offset: 0.0,
            },
        );
        frame(&mut host);
        assert_eq!(
            outcomes(&mut host, world)[0].result,
            Err(ErrorReason::UnsupportedAction)
        );
        assert_eq!(snapshot(&mut host, world, entity), observed);
        assert_eq!(scroll(&mut host, world, entity), unchanged);
    }
}

#[test]
fn configuration_edits_keep_the_virtual_anchor() {
    let (mut host, world, entity) = fixture(100);
    let initial = snapshot(&mut host, world, entity);
    action(
        &mut host,
        world,
        initial.target,
        GuiLocalAction::ScrollToIndex {
            index: 6,
            offset: 5.0,
        },
    );
    frame(&mut host);
    assert_eq!(fields(&mut host, world, entity).offset, [0.0, 65.0]);

    // Changing the item estimate and overscan re-anchors the offset
    // at the same item instead of replaying or resetting scroll intent.
    apply(
        &mut host,
        world,
        vec![
            Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_VIRTUAL_LIST,
                field: crate::FieldWrite {
                    offset: std::mem::offset_of!(GuiVirtualList, item_extent) as u32,
                    value: crate::FieldValue::F32(20.0),
                },
            },
            Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_VIRTUAL_LIST,
                field: crate::FieldWrite {
                    offset: std::mem::offset_of!(GuiVirtualList, overscan) as u32,
                    value: crate::FieldValue::U32(2),
                },
            },
        ],
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    assert_eq!(snapshot(&mut host, world, entity).target, initial.target);
    let edited = fields(&mut host, world, entity);
    assert_eq!((edited.offset, edited.anchor), ([0.0, 125.0], (6, 5.0)));
    assert_eq!(edited.content, [100.0, 2000.0]);
    assert_eq!(edited.range, (4, 11));
}

#[test]
fn restored_virtual_list_publishes_its_wanted_range_before_items_are_declared() {
    let (mut host, world, entity) = fixture(100_000);
    let initial = snapshot(&mut host, world, entity);
    action(
        &mut host,
        world,
        initial.target,
        GuiLocalAction::ScrollToIndex {
            index: 70_000,
            offset: 0.0,
        },
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    let saved_state = snapshot(&mut host, world, entity);
    let saved = fields(&mut host, world, entity);
    assert_eq!(saved.range, (69_999, 70_006));
    let saved_fields = saved;
    let saved = host.save_world(world, 72, Default::default()).unwrap();
    let loaded = host
        .load_world(
            &saved,
            72,
            crate::services::world_serialization::WorldLoadOptions {
                symbolic_id: Some("restored-range".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        )
        .unwrap();
    frame(&mut host);
    let restored = host.world_mut(loaded.root.id()).unwrap().entities()[0].id;
    let state = snapshot(&mut host, loaded.root.id(), restored);
    let restored_fields = fields(&mut host, loaded.root.id(), restored);
    assert_eq!(
        (restored_fields.offset, restored_fields.anchor),
        (saved_fields.offset, saved_fields.anchor)
    );
    assert_eq!(restored_fields.range, saved_fields.range);
    assert_ne!(state.target, saved_state.target);
}

#[test]
fn scroll_fields_expose_offsets_and_capacity_and_a_new_scroll_view_starts_at_zero() {
    let (mut host, world, view, checkbox) = scroll_view_fixture();
    let initial = snapshot(&mut host, world, view);
    assert_eq!(initial.kind, GuiControlKind::ScrollView);
    let geometry = fields(&mut host, world, view);
    assert_eq!(geometry.capacity, [0.0, 150.0]);
    assert_eq!(geometry.item_count, None);
    assert_eq!(scroll(&mut host, world, checkbox), None);
    action(
        &mut host,
        world,
        initial.target,
        GuiLocalAction::ScrollTo([0.0, 30.0]),
    );
    frame(&mut host);
    assert!(outcomes(&mut host, world)[0].result.is_ok());
    let scrolled = fields(&mut host, world, view);
    assert_eq!(scrolled.offset, [0.0, 30.0]);
    assert_eq!(scrolled.capacity, [0.0, 150.0]);

    // Removing the ScrollView retires its position with its
    // incarnation; a new ScrollView on the same entity starts at the origin.
    apply(
        &mut host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(view),
            component: ComponentValue::GUI_SCROLL_VIEW,
        }],
    );
    frame(&mut host);
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(view),
            ComponentValue::GuiScrollView(GuiScrollView {
                axis: 1,
                ..Default::default()
            }),
        )],
    );
    for _ in 0..3 {
        frame(&mut host);
    }
    let fresh = snapshot(&mut host, world, view);
    assert_ne!(fresh.target, initial.target);
    assert_eq!(fields(&mut host, world, view).offset, [0.0, 0.0]);
    action(
        &mut host,
        world,
        initial.target,
        GuiLocalAction::ScrollTo([0.0, 10.0]),
    );
    frame(&mut host);
    assert_eq!(
        outcomes(&mut host, world)[0].result,
        Err(ErrorReason::StaleTarget)
    );
}

#[test]
fn a_routed_wheel_at_the_scroll_limit_writes_and_repaints_nothing() {
    use super::input_test_support::GuiRoutedHost;
    use crate::services::gui_input::GuiDeliveryTerminal;
    use crate::services::gui_input::router::GuiPhysicalInput;

    let mut host = GuiRoutedHost::new();
    let view = host.create(vec![
        ComponentValue::GuiScrollView(GuiScrollView {
            axis: 1,
            ..Default::default()
        }),
        ComponentValue::GuiLayout(GuiLayout {
            width: 100.0,
            height: 50.0,
            ..Default::default()
        }),
    ]);
    host.apply(vec![
        Command::Create {
            alias: 1,
            metadata: Default::default(),
            adopt: false,
        },
        Command::insert_value(
            EntityRef::Alias(1),
            ComponentValue::CanvasBox(crate::systems::canvas::CanvasBox {
                width: 100.0,
                height: 200.0,
                ..Default::default()
            }),
        ),
        Command::PlaceEntity {
            entity: EntityRef::Alias(1),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(view)),
                before: None,
            },
        },
    ]);
    for _ in 0..3 {
        host.frame();
    }
    let point = host.point([50.0, 25.0]);
    let wheel = |delta| GuiPhysicalInput::Wheel {
        point,
        delta: [0.0, delta],
        shift: false,
    };
    let changed = |terminals: &[GuiDeliveryTerminal]| {
        terminals
            .iter()
            .filter(|terminal| {
                matches!(
                    terminal,
                    GuiDeliveryTerminal::Written {
                        changed: true,
                        ..
                    }
                )
            })
            .count()
    };
    let world = host.world.id();

    // Wheeling past the end writes the clamped end position once.
    host.route(wheel(1000.0)).unwrap();
    let at_limit = fields(&mut host.gui, world, view);
    assert_eq!(at_limit.offset, [0.0, 150.0]);
    assert_eq!(changed(&host.terminals()), 1);
    let canvas = host.canvas();

    // Further wheels toward the limit consume nothing: no write, repaint or
    // reflow, and the movement is left for scene controls.
    for _ in 0..2 {
        host.route(wheel(10.0)).unwrap();
        assert_eq!(changed(&host.terminals()), 0);
        assert_eq!(fields(&mut host.gui, world, view), at_limit);
        let unchanged = host.canvas();
        assert_eq!(unchanged.layout_revision, canvas.layout_revision);
        assert_eq!(unchanged.paint_revision, canvas.paint_revision);
        assert_eq!(
            host.router
                .scroll_remainder(&host.context)
                .map(|chain| chain.remaining().unwrap()),
            Some([0.0, 10.0])
        );
    }
}
