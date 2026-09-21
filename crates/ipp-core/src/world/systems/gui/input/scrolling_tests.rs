//! Behavioural tests for GUI scroll routing and scroll-owned offsets.

use super::test_support::*;
use super::*;
use crate::{
    GuiCommand, GuiContainerKind, GuiEvaluatedContent, GuiNodeContent, GuiNodeId, GuiNodeStyle,
};

#[test]
fn scroll_then_pointer_shares_one_snapshot_without_reflow() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, true, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let panel = fixture.panel;
    // Scroll alone never reflows: offsets stay input-owned while routing
    // shares the retained snapshot.
    let before = world(&mut fixture).gui_layout_view(panel).unwrap();
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Scroll {
                    panel: None,
                    position: at,
                    delta: [0.0, 2.0],
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    let after = world(&mut fixture).gui_layout_view(panel).unwrap();
    assert_eq!(before.layout_revision, after.layout_revision);
    assert_eq!(before.reflow_count, after.reflow_count);
    let scroll = report
        .gui_input_effects
        .iter()
        .find_map(|effect| match &effect.kind {
            GuiInputEffectKind::ScrollChanged {
                offset,
                ..
            } => Some(*offset),
            _ => None,
        })
        .unwrap();
    assert_eq!(scroll, [0.0, 2.0]);
    // Scroll offsets stay input-owned: layout never observes them.
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(2)),
        [0.0, 2.0]
    );
    // A routed toggle refreshes retained paint/state without geometry work.
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(
        report
            .gui_input_effects
            .iter()
            .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
    );
    let toggled = world(&mut fixture).gui_layout_view(panel).unwrap();
    assert_eq!(toggled.layout_revision, after.layout_revision);
    assert_eq!(toggled.reflow_count, after.reflow_count);
    assert!(toggled.paint_revision > after.paint_revision);
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(true), 2)
    );
    match evaluated_content(&mut fixture, GuiNodeId(2)) {
        GuiEvaluatedContent::Checkbox {
            checked,
            revision,
        } => {
            assert!(checked);
            assert_eq!(revision, 2);
        }
        other => panic!("expected checkbox, got {other:?}"),
    }
    // The input-owned scroll offset survives the commit reflow.
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(2)),
        [0.0, 2.0]
    );
}

/// Nested ScrollViews without fonts: outer viewport 10x6 over 10x10 content,
/// inner viewport 10x4 over 10x8 content, plus one checkbox riding the inner
/// content at local [0, 4].
fn insert_nested_scroll(fixture: &mut Fixture) {
    let root_incarnation = incarnation(fixture);
    let panel = fixture.panel;
    let style = |width: f32, height: f32| GuiNodeStyle {
        width: Some(width),
        height: Some(height),
        ..Default::default()
    };
    let commands = vec![
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(1),
            parent: None,
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::Column),
            style: style(10.0, 10.0),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(2),
            parent: Some(GuiNodeId(1)),
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::ScrollView),
            style: style(10.0, 6.0),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(3),
            parent: Some(GuiNodeId(2)),
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::Column),
            style: GuiNodeStyle::default(),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(4),
            parent: Some(GuiNodeId(3)),
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::ScrollView),
            style: style(10.0, 4.0),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(5),
            parent: Some(GuiNodeId(4)),
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::Column),
            style: GuiNodeStyle::default(),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(6),
            parent: Some(GuiNodeId(5)),
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::SizedBox),
            style: style(10.0, 4.0),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(7),
            parent: Some(GuiNodeId(5)),
            index: 1,
            content: GuiNodeContent::Container(GuiContainerKind::SizedBox),
            style: style(10.0, 4.0),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(8),
            parent: Some(GuiNodeId(3)),
            index: 1,
            content: GuiNodeContent::Container(GuiContainerKind::SizedBox),
            style: style(10.0, 6.0),
        },
        // Node identities allocate sequentially: id 9 must come last.
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(9),
            parent: Some(GuiNodeId(7)),
            index: 0,
            content: GuiNodeContent::Checkbox {
                checked: false,
            },
            // Explicit size: layout evaluation drops unsized controls.
            style: style(1.0, 1.0),
        },
    ];
    let mut context = world(fixture);
    for command in commands {
        context.enqueue_gui_command(SESSION, command).unwrap();
    }
    context.step(0.0).unwrap();
}

/// Scroll offsets reported by one apply step, keyed by consuming node.
fn scroll_offsets_of(report: &crate::WorldUpdateReport) -> Vec<(GuiNodeId, [f32; 2])> {
    let mut offsets: Vec<(GuiNodeId, [f32; 2])> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ScrollChanged {
                node,
                offset,
                ..
            } => Some((*node, *offset)),
            _ => None,
        })
        .collect();
    offsets.sort_by_key(|(node, _)| *node);
    offsets
}

fn scroll_command(position: [f32; 2], delta: [f32; 2]) -> GuiInputCommand {
    GuiInputCommand::Scroll {
        panel: None,
        position,
        delta,
        blockers: Vec::new(),
        panel_distance: None,
    }
}

/// Route one scroll and apply it, returning the apply-step report.
fn scroll_and_apply(
    fixture: &mut Fixture,
    position: [f32; 2],
    delta: [f32; 2],
) -> crate::WorldUpdateReport {
    {
        let mut context = world(fixture);
        context
            .enqueue_gui_input_command(SESSION, scroll_command(position, delta))
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(fixture).step(0.0).unwrap()
}

#[test]
fn nested_scroll_consumes_innermost_first_clamps_and_propagates() {
    let mut fixture = setup();
    insert_nested_scroll(&mut fixture);
    let panel = fixture.panel;
    // Inner capacity [0, 4], outer capacity [0, 4].
    let report = scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 6.0]);
    assert_eq!(
        scroll_offsets_of(&report),
        vec![(GuiNodeId(2), [0.0, 2.0]), (GuiNodeId(4), [0.0, 4.0])]
    );
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(4)),
        [0.0, 4.0]
    );
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(2)),
        [0.0, 2.0]
    );
    // Scrolling never reflows layout: offsets stay input-owned.
    let before = world(&mut fixture).gui_layout_view(panel).unwrap();
    let revision = before.layout_revision;
    // Drive both views to their edges: inner holds at 4 while outer climbs
    // to 4, and the leftover remainder drops instead of accumulating.
    let report = scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 10.0]);
    assert_eq!(scroll_offsets_of(&report), vec![(GuiNodeId(2), [0.0, 4.0])]);
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(4)),
        [0.0, 4.0]
    );
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(2)),
        [0.0, 4.0]
    );
    let after = world(&mut fixture).gui_layout_view(panel).unwrap();
    assert_eq!(after.layout_revision, revision);
    // Clamped at both edges, a further scroll consumes nothing and reports
    // no scroll effect.
    let report = scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 10.0]);
    assert!(scroll_offsets_of(&report).is_empty());
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(4)),
        [0.0, 4.0]
    );
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(2)),
        [0.0, 4.0]
    );
}

#[test]
fn scrolled_content_moves_and_reveals_clipped_targets() {
    let mut fixture = setup();
    insert_nested_scroll(&mut fixture);
    let panel = fixture.panel;
    assert_eq!(
        world(&mut fixture).gui_scrolled_rect(panel, GuiNodeId(6)),
        Some([0.0, 0.0, 10.0, 4.0])
    );
    // Before scrolling, the point sits over the plain container: no control
    // activates there.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(7, [0.07, 0.07])[0].clone())
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_eq!(report.gui_unhandled_inputs.len(), 1);
        assert_eq!(
            report.gui_unhandled_inputs[0].reason,
            GuiUnhandledReason::NotFocusable
        );
        assert_eq!(context.gui_input_pressed(7), None);
    }
    // Scroll the inner view by its full capacity: content moves up by 4 and
    // the checkbox rides from local y=4 to the viewport top.
    scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 4.0]);
    assert_eq!(
        world(&mut fixture).gui_scrolled_rect(panel, GuiNodeId(6)),
        Some([0.0, -4.0, 10.0, 4.0])
    );
    let scrolled = world(&mut fixture)
        .gui_scrolled_rect(panel, GuiNodeId(9))
        .unwrap();
    assert!((scrolled[0] - 0.0).abs() < 1e-5);
    assert!((scrolled[1] - 0.0).abs() < 1e-5);
    // The same viewport point now presses the revealed checkbox: the tap
    // completes on release with exactly one commit.
    {
        let mut context = world(&mut fixture);
        for command in down_up(7, [0.07, 0.07]) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
        let pressed = context.gui_input_pressed(7);
        assert!(pressed.is_none());
        let hover = context.gui_input_hover(7).unwrap();
        assert_eq!((hover.entity, hover.node), (panel, GuiNodeId(9)));
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    let commits: Vec<(GuiControlValue, u32)> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ControlCommitted {
                value,
                revision,
                ..
            } => Some((value.clone(), *revision)),
            _ => None,
        })
        .collect();
    assert_eq!(commits, vec![(GuiControlValue::Bool(true), 2)]);
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(9)),
        (GuiControlValue::Bool(true), 2)
    );
}

#[test]
fn scroll_start_cancels_held_checkbox_tap() {
    let mut fixture = setup();
    insert_nested_scroll(&mut fixture);
    let panel = fixture.panel;
    scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 4.0]);
    // Hold a press on the revealed checkbox.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, [0.07, 0.07])[0].clone())
            .unwrap();
        context.step(0.0).unwrap();
        assert!(context.gui_input_pressed(1).is_some());
    }
    world(&mut fixture).step(0.0).unwrap();
    // A scroll that moves content disarms the held tap: the cancellation
    // reports once and the later release finds no capture.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, scroll_command([0.07, 0.07], [0.0, -2.0]))
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(report.gui_input_cancellations.iter().any(|cancellation| {
            cancellation.reason == GuiInputCancelReason::GestureCancelled
                && cancellation
                    .target
                    .is_some_and(|target| target.node == GuiNodeId(9))
        }));
        assert_eq!(context.gui_input_pressed(1), None);
    }
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, [0.07, 0.07])[1].clone())
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(
            report
                .gui_unhandled_inputs
                .iter()
                .any(|unhandled| unhandled.reason == GuiUnhandledReason::NoCapture)
        );
    }
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
    // The scroll itself still consumed against the inner view.
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(4)),
        [0.0, 2.0]
    );
}

#[test]
fn scroll_helpers_resolve_ancestry_and_capacity() {
    let mut root = GuiRoot::default();
    root.nodes_mut()
        .insert_node(
            GuiNodeId(1),
            None,
            0,
            GuiNodeContent::Container(GuiContainerKind::Column),
        )
        .unwrap();
    root.nodes_mut()
        .insert_node(
            GuiNodeId(2),
            Some(GuiNodeId(1)),
            0,
            GuiNodeContent::Container(GuiContainerKind::ScrollView),
        )
        .unwrap();
    root.nodes_mut()
        .insert_node(
            GuiNodeId(3),
            Some(GuiNodeId(2)),
            0,
            GuiNodeContent::Container(GuiContainerKind::Column),
        )
        .unwrap();
    root.nodes_mut()
        .insert_node(
            GuiNodeId(4),
            Some(GuiNodeId(3)),
            0,
            GuiNodeContent::Checkbox {
                checked: false,
            },
        )
        .unwrap();
    // Innermost-first ancestry, including a viewport resolving itself.
    assert_eq!(
        super::GuiInputSystem::scroll_chain(&root, GuiNodeId(4)),
        vec![GuiNodeId(2)]
    );
    assert_eq!(
        super::GuiInputSystem::scroll_chain(&root, GuiNodeId(2)),
        vec![GuiNodeId(2)]
    );
    assert_eq!(
        super::GuiInputSystem::scroll_chain(&root, GuiNodeId(1)),
        Vec::<GuiNodeId>::new()
    );
    // Capacity clamps content extents over the retained viewport; unknown
    // nodes hold still.
    let record = super::super::super::GuiEvaluatedNode {
        node: GuiNodeId(2),
        lifetime: 1,
        depth: 1,
        rect: [0.0, 0.0, 10.0, 6.0],
        clip: None,
        content: super::super::super::GuiEvaluatedContent::Container,
        enabled: true,
        visible: true,
        available: true,
        paint_suppressed: false,
        visual_offset: [0.0, 0.0],
        visual_scale: [1.0, 1.0],
        acc_scale: [1.0, 1.0],
        content_extents: Some([10.0, 10.0]),
        content_origin: [0.0, 0.0],
        color: [1.0; 4],
        background: None,
        opacity: 1.0,
    };
    let view = super::super::super::GuiEvaluatedView {
        entity: EntityId::from_bits(0x51),
        root_incarnation: 1,
        layout_revision: 0,
        paint_revision: 0,
        evaluation_tick: 0,
        root_bounds: [0.0, 0.0, 10.0, 10.0],
        units_per_metre: 1.0,
        nodes: vec![record],
        diagnostics: Vec::new(),
        remeasure_count: 0,
        reflow_count: 0,
        available: true,
    };
    assert_eq!(
        super::GuiInputSystem::scroll_max(&view, GuiNodeId(2)),
        [0.0, 4.0]
    );
    assert_eq!(
        super::GuiInputSystem::scroll_max(&view, GuiNodeId(9)),
        [0.0, 0.0]
    );
}

/// Nested-scroll fixture with backgrounded boxes, so retained paint carries
/// one Box per viewport and content node: outer viewport 10x6 (node 2),
/// inner viewport 10x4 (node 4), inner content 10x4 (node 6) and outer
/// content 10x6 (node 8). Geometry matches `insert_nested_scroll`.
fn insert_nested_scroll_paint(fixture: &mut Fixture) {
    let root_incarnation = incarnation(fixture);
    let panel = fixture.panel;
    let style = |width: f32, height: f32| GuiNodeStyle {
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
    let commands = vec![
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(1),
            parent: None,
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::Column),
            style: style(10.0, 10.0),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(2),
            parent: Some(GuiNodeId(1)),
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::ScrollView),
            style: background(10.0, 6.0, [1.0, 0.0, 0.0, 1.0]),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(3),
            parent: Some(GuiNodeId(2)),
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::Column),
            style: GuiNodeStyle::default(),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(4),
            parent: Some(GuiNodeId(3)),
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::ScrollView),
            style: background(10.0, 4.0, [0.0, 1.0, 0.0, 1.0]),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(5),
            parent: Some(GuiNodeId(4)),
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::Column),
            style: GuiNodeStyle::default(),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(6),
            parent: Some(GuiNodeId(5)),
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::SizedBox),
            style: background(10.0, 4.0, [0.0, 0.0, 1.0, 1.0]),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(7),
            parent: Some(GuiNodeId(5)),
            index: 1,
            content: GuiNodeContent::Container(GuiContainerKind::SizedBox),
            style: style(10.0, 4.0),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(8),
            parent: Some(GuiNodeId(3)),
            index: 1,
            content: GuiNodeContent::Container(GuiContainerKind::SizedBox),
            style: background(10.0, 6.0, [1.0, 1.0, 1.0, 1.0]),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(9),
            parent: Some(GuiNodeId(7)),
            index: 0,
            content: GuiNodeContent::Checkbox {
                checked: false,
            },
            style: style(1.0, 1.0),
        },
    ];
    let mut context = world(fixture);
    for command in commands {
        context.enqueue_gui_command(SESSION, command).unwrap();
    }
    context.step(0.0).unwrap();
}

/// Prepared Box position and clip for one GUI node on the fixture panel.
fn paint_box_of(
    primitives: &[crate::SurfaceRenderPrimitive],
    node: GuiNodeId,
) -> ([f32; 2], Option<[f32; 4]>) {
    primitives
        .iter()
        .find_map(|primitive| match primitive {
            crate::SurfaceRenderPrimitive::Box {
                style,
                ..
            } if matches!(
                style.identity,
                crate::SurfacePrimitiveIdentity::Gui(id) if id.node == node
            ) =>
            {
                Some((style.position, style.clip))
            }
            _ => None,
        })
        .unwrap()
}

/// Surface preparation counters and retained panel primitives.
fn paint_observation(fixture: &mut Fixture) -> (u64, u64, Vec<crate::SurfaceRenderPrimitive>) {
    let panel = fixture.panel;
    let primitives = world(fixture)
        .surface_render_items()
        .iter()
        .find(|item| item.entity == panel)
        .map(|item| item.primitives.clone())
        .unwrap();
    let (model, primitive) = world(fixture)
        .with_system::<crate::systems::render::RenderSystem, _>(
            crate::systems::render::RenderSystem::ID,
            |system, _| {
                (
                    system.state.surface_layout_cache.model_preparations,
                    system.state.surface_layout_cache.primitive_preparations,
                )
            },
        )
        .unwrap();
    (model, primitive, primitives)
}

#[test]
fn scroll_revision_bumps_only_when_offsets_move() {
    let mut fixture = setup();
    insert_nested_scroll(&mut fixture);
    let panel = fixture.panel;
    assert_eq!(world(&mut fixture).gui_scroll_revision(), 0);

    scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 2.0]);
    assert_eq!(world(&mut fixture).gui_scroll_revision(), 1);
    scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 2.0]);
    assert_eq!(world(&mut fixture).gui_scroll_revision(), 2);
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(4)),
        [0.0, 4.0]
    );
    // Inner holds at its edge while the outer climbs; leftover drops.
    scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 10.0]);
    assert_eq!(world(&mut fixture).gui_scroll_revision(), 3);
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(2)),
        [0.0, 4.0]
    );
    // Clamped at both edges: no envelope, no offset change, no bump.
    let report = scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 10.0]);
    assert!(scroll_offsets_of(&report).is_empty());
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(4)),
        [0.0, 4.0]
    );
    assert_eq!(world(&mut fixture).gui_scroll_revision(), 3);
}

#[test]
fn pure_scroll_refreshes_scrolled_paint_without_reflow() {
    let mut fixture = setup();
    insert_nested_scroll_paint(&mut fixture);
    let panel = fixture.panel;
    world(&mut fixture).step(0.0).unwrap();
    world(&mut fixture).step(0.0).unwrap();
    let view = world(&mut fixture).gui_layout_view(panel).unwrap();
    assert_eq!(view.units_per_metre, 1.0);
    let (layout_revision, paint_revision, reflow_count, remeasure_count) = (
        view.layout_revision,
        view.paint_revision,
        view.reflow_count,
        view.remeasure_count,
    );
    let (model_before, primitive_before, items_before) = paint_observation(&mut fixture);
    assert_eq!(items_before.len(), 4);

    scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 4.0]);
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(4)),
        [0.0, 4.0]
    );
    assert_eq!(world(&mut fixture).gui_scroll_revision(), 1);

    // Layout never reflows on scroll: revisions and work counts hold still.
    let view = world(&mut fixture).gui_layout_view(panel).unwrap();
    assert_eq!(view.layout_revision, layout_revision);
    assert_eq!(view.paint_revision, paint_revision);
    assert_eq!(view.reflow_count, reflow_count);
    assert_eq!(view.remeasure_count, remeasure_count);

    // Preparation still refreshes exactly once with translated paint.
    let (model_after, primitive_after, items_after) = paint_observation(&mut fixture);
    assert_eq!(model_after, model_before + 1);
    assert_eq!(primitive_after, primitive_before + 1);
    assert_eq!(items_after.len(), items_before.len());
    // Inner content rides up by 4 under the fixed inner viewport clip.
    assert_eq!(
        paint_box_of(&items_after, GuiNodeId(6)),
        ([0.0, -4.0], Some([0.0, 0.0, 10.0, 4.0]))
    );
    // Viewports and outer content keep their retained bytes.
    for node in [GuiNodeId(2), GuiNodeId(4), GuiNodeId(8)] {
        assert_eq!(
            paint_box_of(&items_after, node),
            paint_box_of(&items_before, node)
        );
    }

    // Drive the outer view to its edge, then prove the clamped no-op
    // prepares nothing and keeps bytes identical.
    scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 10.0]);
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(2)),
        [0.0, 4.0]
    );
    let (model_edge, primitive_edge, items_edge) = paint_observation(&mut fixture);
    let report = scroll_and_apply(&mut fixture, [5.0, 1.0], [0.0, 10.0]);
    assert!(scroll_offsets_of(&report).is_empty());
    assert_eq!(world(&mut fixture).gui_scroll_revision(), 2);
    let (model_quiet, primitive_quiet, items_quiet) = paint_observation(&mut fixture);
    assert_eq!((model_quiet, primitive_quiet), (model_edge, primitive_edge));
    assert_eq!(items_quiet, items_edge);
    assert_eq!(
        (model_edge, primitive_edge),
        (model_after + 1, primitive_after + 1)
    );
}
