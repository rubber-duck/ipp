//! End-to-end GUI constraint evaluation through a real World.
//!
//! These tests drive [`GuiLayoutSystem`](ipp_core::GuiLayoutSystem) through
//! ordinary command ingress and World updates, then read back retained
//! geometry and prepared Surface paint. Expectations are hand-computed
//! from the documented layout rules on a 4x2 metre Surface at the default
//! units factor.
#![cfg(feature = "gui")]

mod support;
use support::WorldTestDriver;

use ipp_core::systems::gui::{
    GuiCommand, GuiContainerKind, GuiNodeContent, GuiNodeHandle, GuiNodeId, GuiNodePatch,
    GuiNodeStyle, GuiRoot,
};
use ipp_core::{
    Batch, Command, ComponentValue, DynamicValue, EntityId, EntityRef, ErrorReason,
    GuiInputCommand, HostRuntime, Surface, SurfacePrimitiveIdentity, SurfaceRenderPrimitive,
    WorldContext, WorldId, WorldLimits,
};

const SESSION: u64 = 1;

fn host_world() -> (HostRuntime, WorldId) {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    (host, world)
}

fn submit(
    world: &mut WorldContext<'_>,
    operations: Vec<Command>,
) -> Result<Vec<(u32, EntityId)>, ErrorReason> {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    world
        .update_for_test(0.0)
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .map_err(|error| error.reason)
}

fn create_panel(world: &mut WorldContext<'_>) -> EntityId {
    let mut surface = Surface::default();
    surface.width = 4.0;
    surface.height = 2.0;
    submit(
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Surface(surface),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::GuiRoot(GuiRoot::default()),
            },
        ],
    )
    .unwrap()[0]
        .1
}

fn incarnation(world: &WorldContext<'_>, entity: EntityId) -> u64 {
    world
        .inspect_gui(entity, None, 1, 1)
        .unwrap()
        .root_incarnation
}

fn edit(world: &mut WorldContext<'_>, command: GuiCommand) -> Result<(), ErrorReason> {
    world
        .enqueue_gui_command_with_reply(SESSION, 7, command)
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(report.system_command_outcomes.len(), 1);
    report.system_command_outcomes[0].result
}

fn insert(
    world: &mut WorldContext<'_>,
    entity: EntityId,
    id: u32,
    parent: Option<u32>,
    content: GuiNodeContent,
    style: GuiNodeStyle,
) -> GuiNodeHandle {
    let root_incarnation = incarnation(world, entity);
    edit(
        world,
        GuiCommand::InsertNode {
            entity,
            root_incarnation,
            id: GuiNodeId(id),
            parent: parent.map(GuiNodeId),
            index: u32::MAX,
            content,
            style,
        },
    )
    .unwrap();
    GuiNodeHandle::new(SESSION, entity, root_incarnation, GuiNodeId(id), 1)
}

fn backgrounded(w: f32, h: f32, color: [f32; 4]) -> GuiNodeStyle {
    GuiNodeStyle {
        width: Some(w),
        height: Some(h),
        background_color: Some(color),
        ..Default::default()
    }
}

fn layout_rect(world: &WorldContext<'_>, entity: EntityId, id: u32) -> [f32; 4] {
    world
        .gui_layout_view(entity)
        .unwrap()
        .nodes
        .iter()
        .find(|node| node.node == GuiNodeId(id))
        .unwrap()
        .rect
}

fn assert_rect(actual: [f32; 4], expected: [f32; 4]) {
    for (actual, expected) in actual.iter().zip(expected.iter()) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "rect {actual:?} != expected {expected:?}"
        );
    }
}

#[test]
fn layout_evaluates_through_world_and_feeds_preparation() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_panel(&mut world);

    insert(
        &mut world,
        entity,
        1,
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        GuiNodeStyle::default(),
    );
    insert(
        &mut world,
        entity,
        2,
        Some(1),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        backgrounded(1.0, 1.0, [1.0, 0.0, 0.0, 1.0]),
    );
    insert(
        &mut world,
        entity,
        3,
        Some(1),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        backgrounded(2.0, 0.5, [0.0, 1.0, 0.0, 1.0]),
    );

    // Retained geometry observes the real scheduled pass: panel creation
    // evaluates the empty tree once, then each insertion reflows.
    let view = world.gui_layout_view(entity).unwrap();
    assert_eq!(view.root_bounds, [0.0, 0.0, 4.0, 2.0]);
    assert_eq!(view.layout_revision, 4);
    assert!(view.available);
    assert_rect(layout_rect(&world, entity, 2), [0.0, 0.0, 1.0, 1.0]);
    assert_rect(layout_rect(&world, entity, 3), [0.0, 1.0, 2.0, 0.5]);

    // Prepared Surface paint carries the retained boxes with Gui identity
    // and the root clip, and no authored primitives.
    let item = world
        .surface_render_items()
        .iter()
        .find(|item| item.entity == entity)
        .unwrap();
    let boxes: Vec<&SurfaceRenderPrimitive> = item
        .primitives
        .iter()
        .filter(|primitive| matches!(primitive.style().identity, SurfacePrimitiveIdentity::Gui(_)))
        .collect();
    assert_eq!(boxes.len(), 2);
    match boxes[0] {
        SurfaceRenderPrimitive::Box {
            style,
            size,
            ..
        } => {
            assert_eq!(style.position, [0.0, 0.0]);
            assert_eq!(*size, [1.0, 1.0]);
            assert_eq!(style.color, [1.0, 0.0, 0.0, 1.0]);
            assert_eq!(style.clip, Some([0.0, 0.0, 4.0, 2.0]));
        }
        other => panic!("expected box, got {other:?}"),
    }
    match boxes[1] {
        SurfaceRenderPrimitive::Box {
            style,
            size,
            ..
        } => {
            assert_eq!(style.position, [0.0, 1.0]);
            assert_eq!(*size, [2.0, 0.5]);
            assert_eq!(style.color, [0.0, 1.0, 0.0, 1.0]);
        }
        other => panic!("expected box, got {other:?}"),
    }

    // A paint-only recolour advances paint without reflowing layout.
    let paint_before = view.paint_revision;
    let handle = GuiNodeHandle::new(
        SESSION,
        entity,
        incarnation(&world, entity),
        GuiNodeId(2),
        1,
    );
    edit(
        &mut world,
        GuiCommand::UpdateNode {
            handle,
            patch: GuiNodePatch {
                background_color: Some(Some([0.0, 0.0, 1.0, 1.0])),
                ..Default::default()
            },
        },
    )
    .unwrap();
    let view = world.gui_layout_view(entity).unwrap();
    assert_eq!(view.layout_revision, 4);
    assert_eq!(view.paint_revision, paint_before + 1);
    assert_eq!(view.remeasure_count, 0);
    assert_rect(layout_rect(&world, entity, 2), [0.0, 0.0, 1.0, 1.0]);

    // An unchanged frame refreshes the tick and nothing else.
    world.update_for_test(0.0).unwrap();
    let quiet = world.gui_layout_view(entity).unwrap();
    assert_eq!(quiet.layout_revision, 4);
    assert_eq!(quiet.paint_revision, paint_before + 1);
    assert_eq!(quiet.reflow_count, 4);
}

#[test]
fn visual_lanes_move_paint_and_hit_without_reflow() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_panel(&mut world);

    insert(
        &mut world,
        entity,
        1,
        None,
        GuiNodeContent::Container(GuiContainerKind::Stack),
        GuiNodeStyle {
            width: Some(4.0),
            height: Some(2.0),
            ..Default::default()
        },
    );
    insert(
        &mut world,
        entity,
        2,
        Some(1),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        backgrounded(1.0, 1.0, [1.0, 1.0, 1.0, 1.0]),
    );

    // Visual translation arrives through the ordinary dynamic-property
    // path, alongside authored style lanes.
    submit(
        &mut world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_ROOT,
            name: GuiRoot::property_name(GuiNodeId(2), "position").unwrap(),
            value: DynamicValue::Vec2([2.0, 0.5]),
        }],
    )
    .unwrap();

    // Panel creation and two insertions each reflow; the visual edit moves
    // rectangles and bumps paint without reflowing or remeasuring text.
    let view = world.gui_layout_view(entity).unwrap();
    assert_eq!(view.reflow_count, 3);
    assert_eq!(view.remeasure_count, 0);
    assert_rect(layout_rect(&world, entity, 2), [2.0, 0.5, 1.0, 1.0]);
    assert_eq!(view.hit_test([2.5, 1.0]).unwrap().node, GuiNodeId(2));

    let item = world
        .surface_render_items()
        .iter()
        .find(|item| item.entity == entity)
        .unwrap();
    let moved = item
        .primitives
        .iter()
        .find(|primitive| {
            matches!(
                primitive.style().identity,
                SurfacePrimitiveIdentity::Gui(id) if id.node == GuiNodeId(2)
            )
        })
        .unwrap();
    match moved {
        SurfaceRenderPrimitive::Box {
            style,
            ..
        } => assert_eq!(style.position, [2.0, 0.5]),
        other => panic!("expected box, got {other:?}"),
    }
}

fn create_scroll_panel(world: &mut WorldContext<'_>) -> EntityId {
    let mut surface = Surface::default();
    surface.width = 10.0;
    surface.height = 10.0;
    submit(
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Surface(surface),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::GuiRoot(GuiRoot::default()),
            },
        ],
    )
    .unwrap()[0]
        .1
}

fn scrolled_box_of(
    primitives: &[SurfaceRenderPrimitive],
    node: u32,
) -> ([f32; 2], Option<[f32; 4]>) {
    primitives
        .iter()
        .find_map(|primitive| match primitive {
            SurfaceRenderPrimitive::Box {
                style,
                ..
            } if matches!(
                style.identity,
                SurfacePrimitiveIdentity::Gui(id) if id.node == GuiNodeId(node)
            ) =>
            {
                Some((style.position, style.clip))
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn nested_scroll_moves_paint_without_reflow() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_scroll_panel(&mut world);

    // Outer viewport 10x6 (node 2) over 10x10 content, inner viewport 10x4
    // (node 4) over 10x8 content. Backgrounded viewports and content boxes
    // give retained paint one Box each.
    let sized = |width: f32, height: f32| GuiNodeStyle {
        width: Some(width),
        height: Some(height),
        ..Default::default()
    };
    let background = |width: f32, height: f32, color: [f32; 4]| GuiNodeStyle {
        width: Some(width),
        height: Some(height),
        background_color: Some(color),
        ..Default::default()
    };
    insert(
        &mut world,
        entity,
        1,
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        sized(10.0, 10.0),
    );
    insert(
        &mut world,
        entity,
        2,
        Some(1),
        GuiNodeContent::Container(GuiContainerKind::ScrollView),
        background(10.0, 6.0, [1.0, 0.0, 0.0, 1.0]),
    );
    insert(
        &mut world,
        entity,
        3,
        Some(2),
        GuiNodeContent::Container(GuiContainerKind::Column),
        GuiNodeStyle::default(),
    );
    insert(
        &mut world,
        entity,
        4,
        Some(3),
        GuiNodeContent::Container(GuiContainerKind::ScrollView),
        background(10.0, 4.0, [0.0, 1.0, 0.0, 1.0]),
    );
    insert(
        &mut world,
        entity,
        5,
        Some(4),
        GuiNodeContent::Container(GuiContainerKind::Column),
        GuiNodeStyle::default(),
    );
    insert(
        &mut world,
        entity,
        6,
        Some(5),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        background(10.0, 4.0, [0.0, 0.0, 1.0, 1.0]),
    );
    insert(
        &mut world,
        entity,
        7,
        Some(5),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(10.0, 4.0),
    );
    insert(
        &mut world,
        entity,
        8,
        Some(3),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        background(10.0, 6.0, [1.0, 1.0, 1.0, 1.0]),
    );
    insert(
        &mut world,
        entity,
        9,
        Some(7),
        GuiNodeContent::Checkbox {
            checked: false,
        },
        sized(1.0, 1.0),
    );
    world.update_for_test(0.0).unwrap();

    let view = world.gui_layout_view(entity).unwrap();
    assert_eq!(view.units_per_metre, 1.0);
    let (layout_revision, paint_revision, reflow_count, remeasure_count) = (
        view.layout_revision,
        view.paint_revision,
        view.reflow_count,
        view.remeasure_count,
    );
    let before: Vec<SurfaceRenderPrimitive> = world
        .surface_render_items()
        .iter()
        .find(|item| item.entity == entity)
        .map(|item| item.primitives.clone())
        .unwrap();
    assert_eq!(before.len(), 4);
    assert_eq!(world.gui_scroll_revision(), 0);

    // Route the inner scroll, then apply it: content moves up by 4.
    world
        .enqueue_gui_input_command(
            SESSION,
            GuiInputCommand::Scroll {
                panel: None,
                position: [5.0, 1.0],
                delta: [0.0, 4.0],
                blockers: Vec::new(),
                panel_distance: None,
            },
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(world.gui_input_scroll(entity, GuiNodeId(4)), [0.0, 4.0]);
    assert_eq!(world.gui_scroll_revision(), 1);

    // Scrolling never reflows layout: revisions and work counts hold still.
    let view = world.gui_layout_view(entity).unwrap();
    assert_eq!(view.layout_revision, layout_revision);
    assert_eq!(view.paint_revision, paint_revision);
    assert_eq!(view.reflow_count, reflow_count);
    assert_eq!(view.remeasure_count, remeasure_count);

    // Prepared paint shows translated content under fixed viewport clips.
    let after: Vec<SurfaceRenderPrimitive> = world
        .surface_render_items()
        .iter()
        .find(|item| item.entity == entity)
        .map(|item| item.primitives.clone())
        .unwrap();
    assert_eq!(after.len(), before.len());
    assert_eq!(
        scrolled_box_of(&after, 6),
        ([0.0, -4.0], Some([0.0, 0.0, 10.0, 4.0]))
    );
    for node in [2, 4, 8] {
        assert_eq!(
            scrolled_box_of(&after, node),
            scrolled_box_of(&before, node)
        );
    }
}
