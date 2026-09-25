//! Behavioural tests for pointer drags scrolling ScrollView content and
//! tap-versus-drag arbitration.

use super::test_support::*;
use super::*;
use crate::{GuiCommand, GuiContainerKind, GuiNodeData, GuiNodeDataRow, GuiNodeId, GuiNodeStyle};

fn pointer_command(kind: &str, pointer: u32, position: [f32; 2]) -> GuiInputCommand {
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

/// Route one pointer step in its own tick and return that routing report.
fn pointer_step(
    fixture: &mut Fixture,
    kind: &str,
    pointer: u32,
    position: [f32; 2],
) -> crate::WorldUpdateReport {
    let mut context = world(fixture);
    context
        .enqueue_gui_input_command(SESSION, pointer_command(kind, pointer, position))
        .unwrap();
    context.step(0.0).unwrap()
}

fn offsets(fixture: &mut Fixture) -> ([f32; 2], [f32; 2]) {
    let panel = fixture.panel;
    let inner = world(fixture).gui_input_scroll(panel, GuiNodeId(4));
    let outer = world(fixture).gui_input_scroll(panel, GuiNodeId(2));
    (inner, outer)
}

#[test]
fn touch_drag_scrolls_content_and_passes_unused_travel_outward() {
    let mut fixture = setup();
    insert_nested_scroll(&mut fixture);

    // A press on plain inner content arms a drag instead of reporting the
    // content unfocusable.
    let report = pointer_step(&mut fixture, "down", 5, [5.0, 3.0]);
    assert!(report.gui_unhandled_inputs.is_empty());
    for position in [[5.0, 2.0], [5.0, 1.0]] {
        let report = pointer_step(&mut fixture, "move", 5, position);
        assert!(report.gui_unhandled_inputs.is_empty());
    }
    world(&mut fixture).step(0.0).unwrap();
    // Two units of upward travel scroll the inner view by two.
    assert_eq!(offsets(&mut fixture), ([0.0, 2.0], [0.0, 0.0]));

    // Further travel fills the inner capacity (4) and passes the unused
    // remainder to the outer view (capacity 4); the rest clamps away.
    pointer_step(&mut fixture, "move", 5, [5.0, -3.0]);
    pointer_step(&mut fixture, "move", 5, [5.0, -9.0]);
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(offsets(&mut fixture), ([0.0, 4.0], [0.0, 4.0]));

    // Reversing scrolls back innermost-first; release completes nothing and
    // finds the drag rather than a missing capture.
    pointer_step(&mut fixture, "move", 5, [5.0, -8.0]);
    let report = pointer_step(&mut fixture, "up", 5, [5.0, -8.0]);
    assert!(report.gui_unhandled_inputs.is_empty());
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(offsets(&mut fixture), ([0.0, 3.0], [0.0, 4.0]));

    // The drag ended: later moves of the same pointer only hover.
    pointer_step(&mut fixture, "move", 5, [5.0, 0.0]);
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(offsets(&mut fixture), ([0.0, 3.0], [0.0, 4.0]));
}

#[test]
fn travel_within_the_slop_keeps_the_tap() {
    let mut fixture = setup();
    insert_nested_scroll(&mut fixture);
    scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 4.0]);
    // The checkbox now sits at the inner viewport top; a press that wobbles
    // within the slop (1% of the 10-unit panel) still completes the tap.
    pointer_step(&mut fixture, "down", 1, [0.5, 0.5]);
    pointer_step(&mut fixture, "move", 1, [0.5, 0.55]);
    pointer_step(&mut fixture, "up", 1, [0.5, 0.55]);
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(9)),
        (GuiControlValue::Bool(true), 2)
    );
    assert_eq!(offsets(&mut fixture), ([0.0, 4.0], [0.0, 0.0]));
}

#[test]
fn drag_starting_on_checkbox_scrolls_and_commits_nothing() {
    let mut fixture = setup();
    insert_nested_scroll(&mut fixture);
    scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 4.0]);

    pointer_step(&mut fixture, "down", 1, [0.5, 0.5]);
    assert!(world(&mut fixture).gui_input_pressed(1).is_some());
    // Dragging down past the slop wins the gesture: the tap cancels once
    // and content follows the pointer back toward the start.
    let report = pointer_step(&mut fixture, "move", 1, [0.5, 2.5]);
    let cancelled: Vec<_> = report
        .gui_input_cancellations
        .iter()
        .filter(|cancellation| cancellation.reason == GuiInputCancelReason::GestureCancelled)
        .map(|cancellation| cancellation.target.map(|target| target.node))
        .collect();
    assert_eq!(cancelled, vec![Some(GuiNodeId(9))]);
    assert_eq!(world(&mut fixture).gui_input_pressed(1), None);
    let report = pointer_step(&mut fixture, "up", 1, [0.5, 2.5]);
    assert!(report.gui_unhandled_inputs.is_empty());
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(
        !report
            .gui_input_effects
            .iter()
            .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(9)),
        (GuiControlValue::Bool(false), 1)
    );
    assert_eq!(offsets(&mut fixture), ([0.0, 2.0], [0.0, 0.0]));
}

/// ScrollView 10x4 over a column holding a full-width slider and filler.
fn insert_slider_scroll(fixture: &mut Fixture) {
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
    let column = || GuiNodeData::Container(GuiContainerKind::Column);
    let commands = vec![
        node(
            1,
            None,
            0,
            column(),
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
            column(),
            GuiNodeDataRow::default(),
            GuiNodeStyle::default(),
        ),
        node(
            4,
            Some(3),
            0,
            GuiNodeData::Slider,
            GuiNodeDataRow::slider(0.0, 0.0, 1.0, 0.0),
            style(10.0, 1.0),
        ),
        node(
            5,
            Some(3),
            1,
            GuiNodeData::Container(GuiContainerKind::SizedBox),
            GuiNodeDataRow::default(),
            style(10.0, 8.0),
        ),
    ];
    let mut context = world(fixture);
    for command in commands {
        context.enqueue_gui_command(SESSION, command).unwrap();
    }
    context.step(0.0).unwrap();
}

#[test]
fn slider_drags_keep_capture_while_other_pointers_scroll() {
    let mut fixture = setup();
    insert_slider_scroll(&mut fixture);
    let panel = fixture.panel;

    // The slider keeps its capture across vertical travel: no scroll.
    pointer_step(&mut fixture, "down", 1, [1.0, 0.5]);
    pointer_step(&mut fixture, "move", 1, [5.0, 2.5]);
    // A second pointer drags plain content at the same time.
    pointer_step(&mut fixture, "down", 2, [5.0, 3.5]);
    pointer_step(&mut fixture, "move", 2, [5.0, 1.5]);
    pointer_step(&mut fixture, "up", 1, [5.0, 2.5]);
    pointer_step(&mut fixture, "up", 2, [5.0, 1.5]);
    world(&mut fixture).step(0.0).unwrap();

    let inspected = world(&mut fixture)
        .inspect_gui(panel, Some(GuiNodeId(4)), 1, 4)
        .unwrap();
    let slider = inspected
        .nodes
        .iter()
        .find(|node| node.id == GuiNodeId(4))
        .unwrap();
    assert_eq!(slider.control_value, GuiControlValue::Scalar(0.5));
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(2)),
        [0.0, 2.0]
    );
}
