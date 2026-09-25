//! Keyboard traversal: entry from no focus, reverse traversal, focus scopes
//! and eligibility skipping.

use super::test_support::*;
use super::*;
use crate::systems::surface::Surface;
use crate::{
    Batch, Command, ComponentValue, EntityMetadata, EntityRef, GuiCommand, GuiContainerKind,
    GuiNodeData, GuiNodeId, GuiNodePatch, GuiNodeStyle,
};

const OTHER: u64 = 8;

/// Insert this tree, each checkbox at its tree-order position:
///
/// ```text
/// 1 Column
/// ├── 2 Checkbox
/// ├── 3 Column (focus scope)
/// │   ├── 4 Checkbox
/// │   ├── 5 Column (focus scope)
/// │   │   └── 6 Checkbox
/// │   └── 7 Checkbox
/// ├── 8 Checkbox
/// └── 9 Checkbox
/// ```
fn insert_scoped_tree(fixture: &mut Fixture) {
    let panel = fixture.panel;
    insert_scoped_tree_on(fixture, panel);
}

fn insert_scoped_tree_on(fixture: &mut Fixture, entity: EntityId) {
    let root_incarnation = root_incarnation(fixture, entity);
    let column =
        |id: u32, parent: Option<u32>, index: u32, focus_scope: bool| GuiCommand::InsertNode {
            entity,
            root_incarnation,
            id: GuiNodeId(id),
            parent: parent.map(GuiNodeId),
            index,
            data: GuiNodeData::Container(GuiContainerKind::Column),
            values: crate::GuiNodeDataRow::default(),
            style: GuiNodeStyle {
                width: Some(10.0),
                focus_scope,
                ..Default::default()
            },
        };
    let checkbox = |id: u32, parent: u32, index: u32| GuiCommand::InsertNode {
        entity,
        root_incarnation,
        id: GuiNodeId(id),
        parent: Some(GuiNodeId(parent)),
        index,
        data: GuiNodeData::Checkbox,
        values: GuiNodeDataRow::checkbox(false),
        style: GuiNodeStyle {
            width: Some(0.5),
            height: Some(0.5),
            ..Default::default()
        },
    };
    let commands = [
        column(1, None, 0, false),
        checkbox(2, 1, 0),
        column(3, Some(1), 1, true),
        checkbox(4, 3, 0),
        column(5, Some(3), 1, true),
        checkbox(6, 5, 0),
        checkbox(7, 3, 2),
        checkbox(8, 1, 2),
        checkbox(9, 1, 3),
    ];
    let mut context = world(fixture);
    for command in commands {
        context.enqueue_gui_command(SESSION, command).unwrap();
    }
    context.step(0.0).unwrap();
}

fn root_incarnation(fixture: &mut Fixture, entity: EntityId) -> u64 {
    world(fixture)
        .inspect_gui(entity, None, 1, 1)
        .unwrap()
        .root_incarnation
}

/// A second empty panel, created after (and so ordered after) the first.
fn second_panel(fixture: &mut Fixture) -> EntityId {
    let mut context = world(fixture);
    context
        .enqueue(Batch {
            id: context.tick() + 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: EntityMetadata::default(),
                },
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::Surface({
                        let mut surface = Surface::default();
                        surface.width = 10.0;
                        surface.height = 10.0;
                        surface
                    }),
                ),
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::GuiRoot(GuiRoot::default()),
                ),
            ],
        })
        .unwrap();
    let report = context.step(0.0).unwrap();
    report.outcomes[0].result.as_ref().unwrap()[0].1
}

fn key(fixture: &mut Fixture, session: u64, key: GuiKey) -> crate::WorldUpdateReport {
    let mut context = world(fixture);
    context
        .enqueue_gui_input_command(
            session,
            GuiInputCommand::Key {
                key,
                pressed: true,
            },
        )
        .unwrap();
    context.step(0.0).unwrap()
}

fn focus_node(fixture: &mut Fixture, node: u32) {
    let panel = fixture.panel;
    focus_node_on(fixture, panel, node);
}

fn focus_node_on(fixture: &mut Fixture, panel: EntityId, node: u32) {
    let root_incarnation = root_incarnation(fixture, panel);
    let mut context = world(fixture);
    context
        .enqueue_gui_input_command(
            SESSION,
            GuiInputCommand::Focus {
                handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(node)),
            },
        )
        .unwrap();
    context.step(0.0).unwrap();
    context.step(0.0).unwrap();
}

fn focused(fixture: &mut Fixture) -> Option<u32> {
    world(fixture)
        .gui_input_focus()
        .map(|focus| focus.target.node.0)
}

fn focused_on(fixture: &mut Fixture) -> Option<(EntityId, u32)> {
    world(fixture)
        .gui_input_focus()
        .map(|focus| (focus.target.entity, focus.target.node.0))
}

/// Focused node after each key press, in order.
fn walk(fixture: &mut Fixture, keys: &[GuiKey]) -> Vec<Option<u32>> {
    keys.iter()
        .map(|&pressed| {
            key(fixture, SESSION, pressed);
            focused(fixture)
        })
        .collect()
}

fn patch_node(fixture: &mut Fixture, node: u32, patch: GuiNodePatch) {
    let panel = fixture.panel;
    let root_incarnation = incarnation(fixture);
    let mut context = world(fixture);
    context
        .enqueue_gui_command(
            SESSION,
            GuiCommand::UpdateNode {
                handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(node)),
                patch,
            },
        )
        .unwrap();
    context.step(0.0).unwrap();
}

#[test]
fn tab_without_focus_enters_first_control_and_acquires_the_context() {
    let mut fixture = setup();
    insert_scoped_tree(&mut fixture);

    let report = key(&mut fixture, SESSION, GuiKey::Tab);
    assert!(report.gui_unhandled_inputs.is_empty());
    assert_eq!(focused(&mut fixture), Some(2));

    // The routed focus commits at the next boundary like any other focus.
    let report = world(&mut fixture).step(0.0).unwrap();
    let focus_changes: Vec<_> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match effect.kind {
            GuiInputEffectKind::FocusChanged {
                focus,
            } => Some(focus.map(|focus| (focus.target.node.0, focus.session))),
            _ => None,
        })
        .collect();
    assert_eq!(focus_changes, vec![Some((2, SESSION))]);

    // Entry acquired the context: another session's keys do not reach it.
    let report = key(&mut fixture, OTHER, GuiKey::Tab);
    assert_eq!(
        report.gui_unhandled_inputs[0].reason,
        GuiUnhandledReason::NotOwner
    );
    assert_eq!(focused(&mut fixture), Some(2));
}

#[test]
fn back_tab_without_focus_enters_last_control() {
    let mut fixture = setup();
    insert_scoped_tree(&mut fixture);
    key(&mut fixture, SESSION, GuiKey::BackTab);
    assert_eq!(focused(&mut fixture), Some(9));
}

#[test]
fn entry_without_focusable_controls_is_unhandled_and_keeps_the_context_free() {
    let mut fixture = setup();
    let root_incarnation = incarnation(&mut fixture);
    let panel = fixture.panel;
    world(&mut fixture)
        .enqueue_gui_command(
            SESSION,
            GuiCommand::InsertNode {
                entity: panel,
                root_incarnation,
                id: GuiNodeId(1),
                parent: None,
                index: 0,
                data: GuiNodeData::Container(GuiContainerKind::Column),
                values: crate::GuiNodeDataRow::default(),
                style: GuiNodeStyle::default(),
            },
        )
        .unwrap();
    world(&mut fixture).step(0.0).unwrap();

    let report = key(&mut fixture, OTHER, GuiKey::Tab);
    assert_eq!(
        report.gui_unhandled_inputs[0].reason,
        GuiUnhandledReason::NoFocus
    );
    // Only focus-moving keys enter; others still report no focus.
    let report = key(&mut fixture, SESSION, GuiKey::Enter);
    assert_eq!(
        report.gui_unhandled_inputs[0].reason,
        GuiUnhandledReason::NoFocus
    );
    assert_eq!(focused(&mut fixture), None);
}

#[test]
fn unscoped_traversal_follows_tree_order_and_wraps_both_ways() {
    let mut fixture = setup();
    insert_scoped_tree(&mut fixture);
    focus_node(&mut fixture, 8);
    // Outside every scope, traversal follows the whole tree order,
    // including scoped descendants, and wraps at both ends.
    assert_eq!(
        walk(&mut fixture, &[GuiKey::Tab, GuiKey::Tab, GuiKey::Tab]),
        vec![Some(9), Some(2), Some(4)]
    );
    focus_node(&mut fixture, 2);
    assert_eq!(
        walk(&mut fixture, &[GuiKey::BackTab, GuiKey::BackTab]),
        vec![Some(9), Some(8)]
    );
}

#[test]
fn traversal_stays_within_the_innermost_focus_scope() {
    let mut fixture = setup();
    insert_scoped_tree(&mut fixture);
    // Scope 3 traverses 4, 6 (inside nested scope 5) and 7, wrapping.
    focus_node(&mut fixture, 7);
    assert_eq!(
        walk(
            &mut fixture,
            &[GuiKey::Tab, GuiKey::BackTab, GuiKey::BackTab]
        ),
        vec![Some(4), Some(7), Some(6)]
    );
    // Reaching nested scope 5 bounds traversal to its only control.
    assert_eq!(
        walk(&mut fixture, &[GuiKey::Tab, GuiKey::BackTab]),
        vec![Some(6), Some(6)]
    );
    focus_node(&mut fixture, 4);
    assert_eq!(
        walk(
            &mut fixture,
            &[GuiKey::BackTab, GuiKey::BackTab, GuiKey::Tab]
        ),
        vec![Some(7), Some(6), Some(6)]
    );
}

#[test]
fn escape_leaves_the_scope_and_tab_reenters_the_panel() {
    let mut fixture = setup();
    insert_scoped_tree(&mut fixture);
    focus_node(&mut fixture, 7);
    assert_eq!(
        walk(&mut fixture, &[GuiKey::Escape, GuiKey::Tab]),
        vec![None, Some(2)]
    );
    assert_eq!(
        walk(&mut fixture, &[GuiKey::Escape, GuiKey::BackTab]),
        vec![None, Some(9)]
    );
}

#[test]
fn traversal_and_entry_skip_disabled_hidden_and_transparent_controls() {
    let mut fixture = setup();
    insert_scoped_tree(&mut fixture);
    patch_node(
        &mut fixture,
        2,
        GuiNodePatch {
            enabled: Some(false),
            ..Default::default()
        },
    );
    patch_node(
        &mut fixture,
        9,
        GuiNodePatch {
            opacity: Some(0.0),
            ..Default::default()
        },
    );
    // A removed scope subtree leaves nothing behind to traverse.
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    world(&mut fixture)
        .enqueue_gui_command(
            SESSION,
            GuiCommand::RemoveNode {
                handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(3)),
            },
        )
        .unwrap();
    world(&mut fixture).step(0.0).unwrap();

    assert_eq!(walk(&mut fixture, &[GuiKey::Tab]), vec![Some(8)]);
    assert_eq!(
        walk(&mut fixture, &[GuiKey::Tab, GuiKey::BackTab]),
        vec![Some(8), Some(8)]
    );
    key(&mut fixture, SESSION, GuiKey::Escape);
    assert_eq!(walk(&mut fixture, &[GuiKey::BackTab]), vec![Some(8)]);
}

#[test]
fn traversal_crosses_panels_and_entry_returns_to_the_last_focused_panel() {
    let mut fixture = setup();
    insert_scoped_tree(&mut fixture);
    let first = fixture.panel;
    let second = second_panel(&mut fixture);
    insert_scoped_tree_on(&mut fixture, second);

    // With no history, entry takes the first panel in traversal order.
    key(&mut fixture, SESSION, GuiKey::Tab);
    assert_eq!(focused_on(&mut fixture), Some((first, 2)));

    // Unscoped traversal continues across panels in entity order.
    focus_node_on(&mut fixture, first, 9);
    key(&mut fixture, SESSION, GuiKey::Tab);
    assert_eq!(focused_on(&mut fixture), Some((second, 2)));
    key(&mut fixture, SESSION, GuiKey::BackTab);
    assert_eq!(focused_on(&mut fixture), Some((first, 9)));

    // A scope never extends into another panel.
    focus_node_on(&mut fixture, second, 7);
    key(&mut fixture, SESSION, GuiKey::Tab);
    assert_eq!(focused_on(&mut fixture), Some((second, 4)));

    // Entry returns to the panel that last held focus.
    key(&mut fixture, SESSION, GuiKey::Escape);
    key(&mut fixture, SESSION, GuiKey::BackTab);
    assert_eq!(focused_on(&mut fixture), Some((second, 9)));
    key(&mut fixture, SESSION, GuiKey::Escape);
    key(&mut fixture, SESSION, GuiKey::Tab);
    assert_eq!(focused_on(&mut fixture), Some((second, 2)));
}
