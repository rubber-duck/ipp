//! Behavioural tests for ScrollView scroll bar input: thumb drags, track
//! paging, bar hover and press state, and semantic scroll positions.

use super::test_support::*;
use super::*;
use crate::{GuiCommand, GuiContainerKind, GuiNodeData, GuiNodeDataRow, GuiNodeId, GuiNodeStyle};

fn pointer(kind: &str, pointer: u32, position: [f32; 2]) -> GuiInputCommand {
    match kind {
        "down" => GuiInputCommand::PointerDown {
            pointer,
            panel: None,
            position,
            button: GuiPointerButton::Primary,
            blockers: Vec::new(),
            panel_distance: None,
        },
        "move" => GuiInputCommand::PointerMove {
            pointer,
            panel: None,
            position,
            blockers: Vec::new(),
            panel_distance: None,
        },
        _ => GuiInputCommand::PointerUp {
            pointer,
            panel: None,
            position,
            button: GuiPointerButton::Primary,
            blockers: Vec::new(),
            panel_distance: None,
        },
    }
}

/// Route one pointer step in its own tick, then apply it.
fn step(
    fixture: &mut Fixture,
    kind: &str,
    id: u32,
    position: [f32; 2],
) -> crate::WorldUpdateReport {
    let report = {
        let mut context = world(fixture);
        context
            .enqueue_gui_input_command(SESSION, pointer(kind, id, position))
            .unwrap();
        context.step(0.0).unwrap()
    };
    world(fixture).step(0.0).unwrap();
    report
}

fn outer(fixture: &mut Fixture) -> [f32; 2] {
    let panel = fixture.panel;
    world(fixture).gui_input_scroll(panel, GuiNodeId(2))
}

fn bar_cursor(fixture: &mut Fixture) -> Option<crate::systems::gui::GuiScrollBarCursor> {
    let panel = fixture.panel;
    let root_incarnation = incarnation(fixture);
    world(fixture)
        .system::<GuiInputSystem>(GuiInputSystem::ID)
        .unwrap()
        .skin_cursors()
        .scroll_bars
        .get(&GuiInputTarget {
            entity: panel,
            root_incarnation,
            node: GuiNodeId(2),
        })
        .copied()
}

// The nested fixture's outer 10x6 viewport over 10 units shows a vertical
// bar 0.3 wide at x 9.7..10 whose thumb is 3.6 long and travels 2.4 over
// the outer capacity of 4. It paints above the inner view's own bar.

#[test]
fn thumb_drag_scrolls_with_capture_and_press_state() {
    let mut fixture = setup();
    insert_nested_scroll(&mut fixture);

    // Hovering the thumb names it for skin state without hovering content.
    step(&mut fixture, "move", 1, [9.85, 1.0]);
    assert_eq!(
        bar_cursor(&mut fixture).and_then(|cursor| cursor.hovered),
        Some(crate::systems::surface::GuiPrimitivePart::ScrollThumbY)
    );
    assert_eq!(world(&mut fixture).gui_input_hover(1), None);

    let report = step(&mut fixture, "down", 1, [9.85, 1.0]);
    assert!(report.gui_unhandled_inputs.is_empty());
    assert_eq!(
        bar_cursor(&mut fixture).and_then(|cursor| cursor.pressed),
        Some(crate::systems::surface::GuiPrimitivePart::ScrollThumbY)
    );
    assert_eq!(outer(&mut fixture), [0.0, 0.0]);

    // Half the thumb travel scrolls half the capacity; the capture holds
    // while the pointer wanders off the bar.
    step(&mut fixture, "move", 1, [5.0, 2.2]);
    let offset = outer(&mut fixture);
    assert!(
        offset[0] == 0.0 && (offset[1] - 2.0).abs() < 1e-4,
        "{offset:?}"
    );
    let panel = fixture.panel;
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(4)),
        [0.0, 0.0]
    );
    // Past the track end the thumb clamps at the end instead of passing
    // movement outward.
    step(&mut fixture, "move", 1, [5.0, 40.0]);
    assert_eq!(outer(&mut fixture), [0.0, 4.0]);
    let report = step(&mut fixture, "up", 1, [5.0, 40.0]);
    assert!(report.gui_unhandled_inputs.is_empty());
    assert_eq!(
        bar_cursor(&mut fixture).and_then(|cursor| cursor.pressed),
        None
    );
    // Content under the released pointer toggled nothing.
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(9)),
        (GuiControlValue::Bool(false), 1)
    );
}

#[test]
fn track_presses_page_by_the_viewport_toward_the_pressed_side() {
    let mut fixture = setup();
    insert_nested_scroll(&mut fixture);

    // Below the thumb: one page forward, clamped to the capacity of 4.
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, pointer("down", 1, [9.85, 5.0]))
            .unwrap();
        context.step(0.0).unwrap();
        context.step(0.0).unwrap()
    };
    assert!(report.gui_input_effects.iter().any(|effect| matches!(
        effect.kind,
        GuiInputEffectKind::ScrollChanged {
            node: GuiNodeId(2),
            offset: [0.0, 4.0],
            ..
        }
    )));
    assert_eq!(
        bar_cursor(&mut fixture).and_then(|cursor| cursor.pressed),
        Some(crate::systems::surface::GuiPrimitivePart::ScrollTrackY)
    );
    step(&mut fixture, "up", 1, [9.85, 5.0]);

    // The thumb now spans y 2.4..6: a press above it pages back.
    step(&mut fixture, "down", 2, [9.85, 1.0]);
    step(&mut fixture, "up", 2, [9.85, 1.0]);
    assert_eq!(outer(&mut fixture), [0.0, 0.0]);
}

/// ScrollView 10x4 holding a column of a full-width checkbox and `filler`
/// units of spacer.
fn insert_single_scroll(fixture: &mut Fixture, filler: f32) {
    let root_incarnation = incarnation(fixture);
    let panel = fixture.panel;
    let style = |width: f32, height: f32| GuiNodeStyle {
        width: Some(width),
        height: Some(height),
        ..Default::default()
    };
    let node =
        |id: u32, parent: Option<u32>, index: u32, data, values, style| GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(id),
            parent: parent.map(GuiNodeId),
            index,
            data,
            values,
            style,
        };
    let commands = vec![
        node(
            1,
            None,
            0,
            GuiNodeData::Container(GuiContainerKind::Column),
            GuiNodeDataRow::default(),
            style(10.0, 10.0),
        ),
        node(
            2,
            Some(1),
            0,
            GuiNodeData::Container(GuiContainerKind::ScrollView),
            GuiNodeDataRow::default(),
            style(10.0, 4.0),
        ),
        node(
            3,
            Some(2),
            0,
            GuiNodeData::Container(GuiContainerKind::Column),
            GuiNodeDataRow::default(),
            GuiNodeStyle::default(),
        ),
        node(
            4,
            Some(3),
            0,
            GuiNodeData::Checkbox,
            GuiNodeDataRow::checkbox(false),
            style(10.0, 1.0),
        ),
        node(
            5,
            Some(3),
            1,
            GuiNodeData::Container(GuiContainerKind::SizedBox),
            GuiNodeDataRow::default(),
            style(10.0, filler),
        ),
    ];
    let mut context = world(fixture);
    for command in commands {
        context.enqueue_gui_command(SESSION, command).unwrap();
    }
    context.step(0.0).unwrap();
}

#[test]
fn bars_appear_only_with_overflow_and_cover_content_beneath_them() {
    // Content that fits shows no bar: the checkbox's full width stays
    // hittable at the viewport edge.
    let mut fitting = setup();
    insert_single_scroll(&mut fitting, 1.0);
    step(&mut fitting, "down", 1, [9.9, 0.5]);
    step(&mut fitting, "up", 1, [9.9, 0.5]);
    assert_eq!(
        committed_bool(&mut fitting, GuiNodeId(4)),
        (GuiControlValue::Bool(true), 2)
    );

    // Overflowing content shows the bar above the checkbox's right edge:
    // pressing there pages instead of toggling, while the rest of the
    // checkbox keeps its hit target.
    let mut overflowing = setup();
    insert_single_scroll(&mut overflowing, 9.0);
    step(&mut overflowing, "down", 1, [9.9, 0.5]);
    step(&mut overflowing, "up", 1, [9.9, 0.5]);
    assert_eq!(
        committed_bool(&mut overflowing, GuiNodeId(4)),
        (GuiControlValue::Bool(false), 1)
    );
    let panel = overflowing.panel;
    assert_eq!(
        world(&mut overflowing).gui_input_scroll(panel, GuiNodeId(2)),
        [0.0, 0.0]
    );
    step(&mut overflowing, "down", 1, [5.0, 0.5]);
    step(&mut overflowing, "up", 1, [5.0, 0.5]);
    assert_eq!(
        committed_bool(&mut overflowing, GuiNodeId(4)),
        (GuiControlValue::Bool(true), 2)
    );
}

#[test]
fn removing_the_scroll_view_mid_drag_drops_the_bar_press() {
    let mut fixture = setup();
    insert_nested_scroll(&mut fixture);
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    step(&mut fixture, "down", 1, [9.85, 1.0]);
    assert!(bar_cursor(&mut fixture).is_some_and(|cursor| cursor.pressed.is_some()));
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::RemoveNode {
                    handle: crate::GuiNodeHandle::new(
                        SESSION,
                        panel,
                        root_incarnation,
                        GuiNodeId(2),
                    ),
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let cursors = world(&mut fixture)
        .system::<GuiInputSystem>(GuiInputSystem::ID)
        .unwrap()
        .skin_cursors();
    assert!(cursors.scroll_bars.is_empty());
    assert!(
        world(&mut fixture)
            .system::<GuiInputSystem>(GuiInputSystem::ID)
            .unwrap()
            .interaction_roots()
            .is_empty()
    );
    // The release finds nothing to complete.
    let report = step(&mut fixture, "up", 1, [5.0, 2.0]);
    assert_eq!(report.gui_input_effects.len(), 0);
}

#[test]
fn semantic_snapshots_expose_scroll_positions() {
    let mut fixture = setup();
    insert_nested_scroll(&mut fixture);
    scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 6.0]);
    let panel = fixture.panel;
    let tree = world(&mut fixture)
        .gui_semantic_snapshot(panel, 32, 256)
        .unwrap();
    let scroll = |id: u32| {
        let node = tree.node(GuiNodeId(id)).unwrap();
        (node.role, node.scroll)
    };
    assert_eq!(
        scroll(2),
        (
            crate::GuiSemanticRole::ScrollView,
            Some(crate::GuiSemanticScroll {
                offset: [0.0, 2.0],
                max_offset: [0.0, 4.0],
            })
        )
    );
    assert_eq!(
        scroll(4),
        (
            crate::GuiSemanticRole::ScrollView,
            Some(crate::GuiSemanticScroll {
                offset: [0.0, 4.0],
                max_offset: [0.0, 4.0],
            })
        )
    );
    assert_eq!(scroll(3), (crate::GuiSemanticRole::Container, None));
}
