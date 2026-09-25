//! VirtualList scrolling through the World: anchoring, wanted ranges,
//! scroll-to-index and a new incarnation from a persisted anchor.

use super::test_support::*;
use crate::{
    Batch, Command, ComponentValue, EntityRef, GuiCommand, GuiContainerKind, GuiInputEffectKind,
    GuiNodeData, GuiNodeDataRow, GuiNodeHandle, GuiNodeId, GuiNodeStyle,
};

const LIST: GuiNodeId = GuiNodeId(2);
const COUNT: u32 = 100_000;

fn sized(width: f32, height: f32) -> GuiNodeStyle {
    GuiNodeStyle {
        width: Some(width),
        height: Some(height),
        ..Default::default()
    }
}

/// A 10 x 10 column holding a 10 x 6 VirtualList of `COUNT` unit items with
/// two items of overscan.
fn insert_list(fixture: &mut Fixture) -> crate::WorldUpdateReport {
    let root_incarnation = incarnation(fixture);
    let panel = fixture.panel;
    let mut context = world(fixture);
    for command in [
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(1),
            parent: None,
            index: 0,
            data: GuiNodeData::Container(GuiContainerKind::Column),
            values: GuiNodeDataRow::default(),
            style: sized(10.0, 10.0),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: LIST,
            parent: Some(GuiNodeId(1)),
            index: 0,
            data: GuiNodeData::Container(GuiContainerKind::VirtualList),
            values: GuiNodeDataRow::virtual_list(COUNT, 1.0, 2, 1),
            style: sized(10.0, 6.0),
        },
    ] {
        context.enqueue_gui_command(SESSION, command).unwrap();
    }
    context.step(0.0).unwrap()
}

/// Declare item children of `height` at `indices` in one step, returning
/// their nodes.
fn declare(
    fixture: &mut Fixture,
    indices: impl IntoIterator<Item = u32>,
    height: f32,
) -> (Vec<GuiNodeId>, crate::WorldUpdateReport) {
    let panel = fixture.panel;
    let root_incarnation = incarnation(fixture);
    let first = world(fixture).gui_root(panel).unwrap().next_node_id();
    let mut context = world(fixture);
    let mut nodes = Vec::new();
    for (id, index) in (first..).zip(indices) {
        let id = GuiNodeId(id);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::InsertNode {
                    entity: panel,
                    root_incarnation,
                    id,
                    parent: Some(LIST),
                    index,
                    data: GuiNodeData::Container(GuiContainerKind::SizedBox),
                    values: GuiNodeDataRow::default(),
                    style: sized(10.0, height),
                },
            )
            .unwrap();
        nodes.push(id);
    }
    (nodes, context.step(0.0).unwrap())
}

fn handle(fixture: &mut Fixture, node: GuiNodeId) -> GuiNodeHandle {
    let root_incarnation = incarnation(fixture);
    GuiNodeHandle::new(SESSION, fixture.panel, root_incarnation, node)
}

fn ranges(report: &crate::WorldUpdateReport) -> Vec<(u32, u32, u32)> {
    report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match effect.kind {
            GuiInputEffectKind::VirtualRangeChanged {
                node,
                first,
                last,
                revision,
                ..
            } if node == LIST => {
                assert_eq!(effect.session, 0);
                Some((first, last, revision))
            }
            _ => None,
        })
        .collect()
}

fn anchor(fixture: &mut Fixture) -> (u32, f32) {
    let panel = fixture.panel;
    let context = world(fixture);
    let row = context.gui_root(panel).unwrap().data_row(LIST).unwrap();
    (row.anchor_index.unwrap(), row.anchor_offset.unwrap())
}

fn step(fixture: &mut Fixture) -> crate::WorldUpdateReport {
    world(fixture).step(0.0).unwrap()
}

#[test]
fn a_virtual_list_scrolls_by_count_and_anchors_measurements_above_the_viewport() {
    let mut fixture = setup();
    let panel = fixture.panel;

    // Attach publishes the visible six items plus two of overscan, and the
    // content extent comes from the count alone.
    let attached = insert_list(&mut fixture);
    assert_eq!(ranges(&attached), vec![(0, 8, 1)]);
    let view = world(&mut fixture).gui_layout_view(panel).unwrap();
    let record = view.nodes.iter().find(|node| node.node == LIST).unwrap();
    assert_eq!(record.content_extents, Some([10.0, COUNT as f32]));
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, LIST),
        [0.0, 0.0]
    );

    // A wheel over the list scrolls it without any declared child, persists
    // the anchor and moves the wanted range.
    let applied = scroll_and_apply(&mut fixture, [5.0, 3.0], [0.0, 10.5]);
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, LIST),
        [0.0, 10.5]
    );
    assert_eq!(anchor(&mut fixture), (10, 0.5));
    assert_eq!(ranges(&applied), vec![(8, 19, 2)]);
    assert!(ranges(&step(&mut fixture)).is_empty());

    // Declaring the range with items twice the estimate, two of them above
    // the viewport, keeps item 10 where it was: the offset follows by +2.
    let before = world(&mut fixture).gui_scroll_revision();
    let (children, declared) = declare(&mut fixture, 8..19, 2.0);
    let item_ten = children[2];
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, LIST),
        [0.0, 12.5]
    );
    assert!(world(&mut fixture).gui_scroll_revision() > before);
    let list_top = world(&mut fixture).gui_scrolled_rect(panel, LIST).unwrap()[1];
    let item_rect = world(&mut fixture)
        .gui_scrolled_rect(panel, item_ten)
        .unwrap();
    assert!((item_rect[1] - (list_top - 0.5)).abs() < 1.0e-4);
    assert_eq!(item_rect[3], 2.0);
    assert_eq!(anchor(&mut fixture), (10, 0.5));

    // Taller items shrink the wanted range, and the extent and thumb follow.
    assert_eq!(ranges(&declared), vec![(8, 16, 3)]);
    let view = world(&mut fixture).gui_layout_view(panel).unwrap();
    let record = view.nodes.iter().find(|node| node.node == LIST).unwrap();
    assert_eq!(record.content_extents, Some([10.0, COUNT as f32 + 11.0]));

    // Removing the items the range no longer wants converges: no further
    // range effect while the declared window covers it.
    for &child in &children[8..] {
        let handle = handle(&mut fixture, child);
        world(&mut fixture)
            .enqueue_gui_command(
                SESSION,
                GuiCommand::RemoveNode {
                    handle,
                },
            )
            .unwrap();
    }
    let converged = step(&mut fixture);
    assert!(ranges(&converged).is_empty());
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, LIST),
        [0.0, 12.5]
    );

    // The semantic tree reports count, loaded range, anchor and scroll.
    let tree = world(&mut fixture)
        .gui_semantic_snapshot(panel, 32, 256)
        .unwrap();
    let node = tree.node(LIST).unwrap();
    assert_eq!(node.role, crate::GuiSemanticRole::VirtualList);
    assert_eq!(
        node.virtual_list,
        Some(crate::GuiSemanticVirtualList {
            item_count: COUNT,
            loaded_first: 8,
            loaded_last: 16,
            anchor_index: 10,
            anchor_offset: 0.5,
        })
    );
    let scroll = node.scroll.unwrap();
    assert_eq!(scroll.offset, [0.0, 12.5]);
    assert_eq!(scroll.max_offset, [0.0, COUNT as f32 + 8.0 - 6.0]);

    // Declared children outside the viewport stay clipped by it.
    let first = world(&mut fixture)
        .gui_scrolled_rect(panel, children[0])
        .unwrap();
    assert!(first[1] + first[3] <= list_top + 1.0e-4);
}

#[test]
fn scroll_to_index_moves_the_anchor_and_a_new_incarnation_restores_it() {
    let mut fixture = setup();
    let panel = fixture.panel;
    insert_list(&mut fixture);
    let above = declare(&mut fixture, [3], 4.0).0[0];

    let list = handle(&mut fixture, LIST);
    world(&mut fixture)
        .enqueue_gui_command(
            SESSION,
            GuiCommand::ScrollToIndex {
                node: list,
                index: 50_000,
                offset: 0.25,
            },
        )
        .unwrap();
    let scrolled = step(&mut fixture);
    assert_eq!(anchor(&mut fixture), (50_000, 0.25));
    // Item 3 measured 4.0 instead of 1.0, moving everything after it by 3.
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, LIST),
        [0.0, 50_003.25]
    );
    assert_eq!(ranges(&scrolled), vec![(49_998, 50_009, 3)]);
    assert!(
        world(&mut fixture)
            .gui_scrolled_rect(panel, above)
            .is_some()
    );

    // An index past the end clamps to the last item and the offset to the
    // capacity.
    world(&mut fixture)
        .enqueue_gui_command(
            SESSION,
            GuiCommand::ScrollToIndex {
                node: list,
                index: u32::MAX,
                offset: 0.0,
            },
        )
        .unwrap();
    step(&mut fixture);
    assert_eq!(anchor(&mut fixture), (COUNT - 1, 0.0));
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, LIST),
        [0.0, COUNT as f32 + 3.0 - 6.0]
    );

    // A negative offset or a non-list target is refused.
    for (node, offset) in [(LIST, -1.0), (GuiNodeId(1), 0.0)] {
        let node = handle(&mut fixture, node);
        world(&mut fixture)
            .enqueue_gui_command_with_reply(
                SESSION,
                5,
                GuiCommand::ScrollToIndex {
                    node,
                    index: 0,
                    offset,
                },
            )
            .unwrap();
        let report = step(&mut fixture);
        let outcome = report
            .system_command_outcomes
            .iter()
            .find(|outcome| outcome.request_id == 5)
            .unwrap();
        assert!(outcome.result.is_err());
    }

    // A new incarnation holding the persisted anchor (as a restore does)
    // publishes its range again from that anchor, before any child is
    // re-declared.
    world(&mut fixture)
        .enqueue_gui_command(
            SESSION,
            GuiCommand::ScrollToIndex {
                node: list,
                index: 70_000,
                offset: 0.0,
            },
        )
        .unwrap();
    step(&mut fixture);
    let mut restored = world(&mut fixture).gui_root(panel).unwrap().clone();
    restored
        .remove_node(above)
        .expect("restored roots re-declare their items");
    world(&mut fixture)
        .enqueue(Batch {
            id: 90,
            operations: vec![Command::RemoveComponent {
                entity: EntityRef::Handle(panel),
                component: ComponentValue::GUI_ROOT,
            }],
        })
        .unwrap();
    step(&mut fixture);
    world(&mut fixture)
        .enqueue(Batch {
            id: 91,
            operations: vec![Command::insert_value(
                EntityRef::Handle(panel),
                ComponentValue::GuiRoot(restored),
            )],
        })
        .unwrap();
    let report = step(&mut fixture);
    for outcome in &report.outcomes {
        outcome.result.as_ref().expect("restored root inserts");
    }
    assert_eq!(ranges(&report), vec![(69_998, 70_008, 1)]);
    assert_eq!(anchor(&mut fixture), (70_000, 0.0));
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, LIST),
        [0.0, 70_000.0]
    );
}

/// Route one primary pointer step in its own tick, then apply it.
fn pointer(fixture: &mut Fixture, kind: &str, position: [f32; 2]) {
    let command = match kind {
        "down" => crate::GuiInputCommand::PointerDown {
            pointer: 1,
            panel: None,
            position,
            button: crate::GuiPointerButton::Primary,
            blockers: Vec::new(),
            panel_distance: None,
        },
        "move" => crate::GuiInputCommand::PointerMove {
            pointer: 1,
            panel: None,
            position,
            blockers: Vec::new(),
            panel_distance: None,
        },
        _ => crate::GuiInputCommand::PointerUp {
            pointer: 1,
            panel: None,
            position,
            button: crate::GuiPointerButton::Primary,
            blockers: Vec::new(),
            panel_distance: None,
        },
    };
    world(fixture)
        .enqueue_gui_input_command(SESSION, command)
        .unwrap();
    step(fixture);
    step(fixture);
}

#[test]
fn scroll_bars_and_drags_scroll_a_virtual_list_like_a_scroll_view() {
    let mut fixture = setup();
    let panel = fixture.panel;
    insert_list(&mut fixture);

    // The 10 x 6 viewport shows a vertical bar 0.3 wide at x 9.7..10; over
    // 100000 items its thumb keeps the two-thickness minimum of 0.6 and
    // travels 5.4 over the capacity.
    let capacity = COUNT as f32 - 6.0;
    pointer(&mut fixture, "down", [9.85, 0.3]);
    pointer(&mut fixture, "move", [9.85, 0.3 + 2.7]);
    let offset = world(&mut fixture).gui_input_scroll(panel, LIST);
    assert!(
        offset[0] == 0.0 && (offset[1] - capacity / 2.0).abs() < 1.0,
        "{offset:?}"
    );
    pointer(&mut fixture, "up", [9.85, 3.0]);
    let (index, within) = anchor(&mut fixture);
    assert!((index as f32 + within - offset[1]).abs() < 1.0e-2);

    // A primary drag over content past the slop scrolls the list by the
    // dragged distance, and its anchor follows.
    let before = world(&mut fixture).gui_input_scroll(panel, LIST)[1];
    pointer(&mut fixture, "down", [5.0, 4.0]);
    pointer(&mut fixture, "move", [5.0, 3.0]);
    pointer(&mut fixture, "move", [5.0, 2.0]);
    pointer(&mut fixture, "up", [5.0, 2.0]);
    let after = world(&mut fixture).gui_input_scroll(panel, LIST)[1];
    assert!((after - before - 2.0).abs() < 1.0e-2, "{before} -> {after}");
    let (index, within) = anchor(&mut fixture);
    assert!((index as f32 + within - after).abs() < 1.0e-2);
}
