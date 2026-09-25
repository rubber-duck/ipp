//! Cost of GUI commands on a live World root: every command kind is a
//! bounded number of row writes independent of the root's size, and the
//! GUI System's derived child order follows each commit, including chunks,
//! failed commands, whole insertions and restores.
//!
//! The maintained measurement builds 1,000- and 4,000-node roots, asserts
//! equal write counts for each command kind on both, and prints the median
//! World time per command (`cargo test -p ipp-core --features gui --lib
//! command_cost -- --nocapture`). Test builds enable `checked-invariants`,
//! whose oracle validates the whole committed root at every commit, so the
//! table also prints that oracle's time on the same root: production
//! commands cost the World time less the oracle, of which the ordinary
//! per-batch staging copy of the component remains proportional to the
//! root. Timings are reported, not asserted.

use super::*;
use crate::systems::gui::{GuiContainerKind, GuiNodeDataRow, GuiNodeStyle, GuiTreeIndex};
use crate::{Batch, ComponentValue, EntityMetadata, EntityRef, HostRuntime, WorldLimits};
use std::time::{Duration, Instant};

const SESSION: u64 = 4;

struct Fixture {
    host: HostRuntime,
    world: crate::WorldId,
    panel: EntityId,
}

fn world<'a>(fixture: &'a mut Fixture) -> crate::WorldContext<'a> {
    fixture.host.world_mut(fixture.world).unwrap()
}

fn column() -> GuiNodeData {
    GuiNodeData::Container(GuiContainerKind::Column)
}

/// Flat root of `count` nodes: a Column root holding Text leaves, one
/// Column subtree of seven Text leaves, a Slider and a TextInput, inserted
/// whole as a new incarnation.
fn flat_root(count: u32) -> GuiRoot {
    let mut root = GuiRoot::default();
    let style = GuiNodeStyle::default();
    let mut insert = |id: u32, parent: Option<u32>, data: GuiNodeData, values: GuiNodeDataRow| {
        root.insert_node_at(
            GuiNodeId(id),
            parent.map(GuiNodeId),
            id << 16,
            data,
            values,
            &style,
        )
        .unwrap();
    };
    insert(1, None, column(), Default::default());
    let subtree = count - 9;
    for id in 2..subtree {
        insert(
            id,
            Some(1),
            GuiNodeData::Text(format!("Label {id}")),
            Default::default(),
        );
    }
    insert(subtree, Some(1), column(), Default::default());
    for id in subtree + 1..=subtree + 7 {
        insert(
            id,
            Some(subtree),
            GuiNodeData::Text(format!("Item {id}")),
            Default::default(),
        );
    }
    insert(
        count - 1,
        Some(1),
        GuiNodeData::Slider,
        GuiNodeDataRow::slider(0.0, 0.0, 100.0, 0.0),
    );
    insert(
        count,
        Some(1),
        GuiNodeData::TextInput {
            text: "abc".into(),
            placeholder: "type".into(),
        },
        Default::default(),
    );
    root.validate_complete().unwrap();
    root
}

fn setup(root: GuiRoot) -> Fixture {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    let panel = {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: 1,
                operations: vec![
                    Command::Create {
                        alias: 1,
                        metadata: EntityMetadata::default(),
                    },
                    Command::insert_value(
                        EntityRef::Alias(1),
                        ComponentValue::Surface({
                            let mut surface = crate::systems::surface::Surface::default();
                            surface.width = 10.0;
                            surface.height = 10.0;
                            surface
                        }),
                    ),
                    Command::insert_value(EntityRef::Alias(1), ComponentValue::GuiRoot(root)),
                ],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        report.outcomes[0].result.as_ref().unwrap()[0].1
    };
    Fixture {
        host,
        world,
        panel,
    }
}

fn incarnation(fixture: &mut Fixture) -> u64 {
    let panel = fixture.panel;
    world(fixture)
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation
}

fn handle(fixture: &mut Fixture, id: u32) -> GuiNodeHandle {
    let incarnation = incarnation(fixture);
    GuiNodeHandle::new(SESSION, fixture.panel, incarnation, GuiNodeId(id))
}

/// The System's derived child order equals a rebuild from committed rows.
fn assert_index_current(fixture: &mut Fixture) {
    let panel = fixture.panel;
    let incarnation = incarnation(fixture);
    let context = world(fixture);
    let root = context.gui_root(panel).unwrap();
    let tree = context
        .system::<GuiSystem>(GuiSystem::ID)
        .unwrap()
        .tree(panel)
        .expect("the System indexes every live root");
    assert_eq!(tree, &GuiTreeIndex::new(root, incarnation));
}

/// Row writes one command produces against the committed root.
fn writes(fixture: &mut Fixture, command: &GuiCommand) -> usize {
    let panel = fixture.panel;
    let incarnation = incarnation(fixture);
    let context = world(fixture);
    let root = context.gui_root(panel).unwrap();
    let tree = context
        .system::<GuiSystem>(GuiSystem::ID)
        .unwrap()
        .tree(panel)
        .unwrap();
    edit_commands(panel, root, tree, incarnation, SESSION, command)
        .unwrap()
        .len()
}

/// Apply one command at the World's mutation boundary and time it.
fn apply(fixture: &mut Fixture, command: GuiCommand) -> Duration {
    let mut context = world(fixture);
    let start = Instant::now();
    let outcome = context
        .apply_gui_command_chunk(SESSION, 1, vec![command])
        .unwrap();
    let elapsed = start.elapsed();
    outcome.result.unwrap();
    elapsed
}

fn text(id: u32, entity: EntityId, incarnation: u64, parent: u32) -> GuiCommand {
    GuiCommand::InsertNode {
        entity,
        root_incarnation: incarnation,
        id: GuiNodeId(id),
        parent: Some(GuiNodeId(parent)),
        index: u32::MAX,
        data: GuiNodeData::Text("new".into()),
        values: Default::default(),
        style: GuiNodeStyle::default(),
    }
}

/// Write count and median World time of each command kind on a root of
/// `count` nodes.
fn measure(count: u32, repetitions: u32) -> Vec<(&'static str, usize, Duration)> {
    let mut fixture = setup(flat_root(count));
    let panel = fixture.panel;
    let inc = incarnation(&mut fixture);
    let median = |mut values: Vec<Duration>| {
        values.sort();
        values[values.len() / 2]
    };
    let mut results = Vec::new();
    let mut next = count + 1;

    let insert = text(next, panel, inc, 1);
    let insert_writes = writes(&mut fixture, &insert);
    let mut times = Vec::new();
    let mut inserted = Vec::new();
    for _ in 0..repetitions {
        times.push(apply(&mut fixture, text(next, panel, inc, 1)));
        inserted.push(next);
        next += 1;
    }
    results.push(("InsertNode", insert_writes, median(times)));

    let one_property: fn(f32) -> GuiNodePatch = |opacity| GuiNodePatch {
        opacity: Some(opacity),
        ..Default::default()
    };
    let three_properties: fn(f32) -> GuiNodePatch = |opacity| GuiNodePatch {
        opacity: Some(opacity),
        width: Some(Some(opacity)),
        color: Some([opacity, 0.0, 0.0, 1.0]),
        ..Default::default()
    };
    for (name, node, patch) in [
        ("UpdateNode, one property", count / 2, one_property),
        (
            "UpdateNode, three properties",
            count / 2 + 1,
            three_properties,
        ),
    ] {
        let node = handle(&mut fixture, node);
        let edit = |rep: u32| GuiCommand::UpdateNode {
            handle: node,
            patch: patch(if rep % 2 == 0 {
                0.5
            } else {
                0.25
            }),
        };
        let count = writes(&mut fixture, &edit(0));
        let times = (0..repetitions)
            .map(|rep| apply(&mut fixture, edit(rep)))
            .collect();
        results.push((name, count, median(times)));
    }

    let node = handle(&mut fixture, count / 2);
    let reorder = |rep: u32| GuiCommand::MoveNode {
        handle: node,
        parent: Some(GuiNodeId(1)),
        index: if rep % 2 == 0 {
            0
        } else {
            count / 3
        },
    };
    let move_writes = writes(&mut fixture, &reorder(0));
    let times = (0..repetitions)
        .map(|rep| apply(&mut fixture, reorder(rep)))
        .collect();
    results.push(("MoveNode", move_writes, median(times)));

    let mut times = Vec::new();
    let mut leaf_writes = 0;
    for id in inserted {
        let remove = GuiCommand::RemoveNode {
            handle: handle(&mut fixture, id),
        };
        leaf_writes = writes(&mut fixture, &remove);
        times.push(apply(&mut fixture, remove));
    }
    results.push(("RemoveNode, leaf", leaf_writes, median(times)));

    let mut times = Vec::new();
    let mut subtree = count - 9;
    let mut subtree_writes = 0;
    for _ in 0..repetitions {
        let remove = GuiCommand::RemoveNode {
            handle: handle(&mut fixture, subtree),
        };
        subtree_writes = writes(&mut fixture, &remove);
        times.push(apply(&mut fixture, remove));
        subtree = next;
        let mut rebuild = vec![GuiCommand::InsertNode {
            entity: panel,
            root_incarnation: inc,
            id: GuiNodeId(next),
            parent: Some(GuiNodeId(1)),
            index: u32::MAX,
            data: column(),
            values: Default::default(),
            style: GuiNodeStyle::default(),
        }];
        next += 1;
        for _ in 0..7 {
            rebuild.push(text(next, panel, inc, subtree));
            next += 1;
        }
        world(&mut fixture)
            .apply_gui_command_chunk(SESSION, 1, rebuild)
            .unwrap()
            .result
            .unwrap();
    }
    results.push(("RemoveNode, 8-node subtree", subtree_writes, median(times)));

    for (name, node, value) in [
        (
            "SetControlValue, slider",
            count - 1,
            (|rep: u32| GuiControlValue::Scalar(rep as f32)) as fn(u32) -> GuiControlValue,
        ),
        ("SetControlValue, text keystroke", count, |rep: u32| {
            GuiControlValue::Text(format!("abc{}", "x".repeat(rep as usize + 1)))
        }),
    ] {
        let target = handle(&mut fixture, node);
        let mut count = 0;
        let mut times = Vec::new();
        for rep in 0..repetitions {
            let revision = world(&mut fixture)
                .gui_root(panel)
                .unwrap()
                .control_revision(GuiNodeId(node));
            let commit = GuiCommand::SetControlValue {
                handle: target,
                expected_revision: revision,
                value: value(rep),
            };
            count = writes(&mut fixture, &commit);
            times.push(apply(&mut fixture, commit));
        }
        results.push((name, count, median(times)));
    }

    world(&mut fixture).finish_command_stream();
    assert_index_current(&mut fixture);
    let root = world(&mut fixture).gui_root(panel).unwrap().clone();
    let times = (0..repetitions)
        .map(|_| {
            let start = Instant::now();
            root.validate_complete().unwrap();
            start.elapsed()
        })
        .collect();
    results.push(("whole-root check (test oracle)", 0, median(times)));
    results
}

#[test]
fn command_row_writes_do_not_grow_with_the_root() {
    let small = measure(1_000, 9);
    let large = measure(4_000, 3);
    eprintln!("GUI command cost (debug build unless built with --release):");
    eprintln!(
        "{:<32} {:>7} {:>14} {:>14}",
        "command", "writes", "1,000 nodes", "4,000 nodes"
    );
    for ((name, writes, small), (_, large_writes, large)) in small.iter().zip(&large) {
        eprintln!("{name:<32} {writes:>7} {small:>14.2?} {large:>14.2?}");
        assert_eq!(
            writes, large_writes,
            "{name} writes depend on the root size"
        );
        assert!(*writes <= 8, "{name} writes {writes} rows");
    }
}

#[test]
fn derived_child_order_follows_every_commit_path() {
    let mut fixture = setup(flat_root(40));
    let panel = fixture.panel;
    let inc = incarnation(&mut fixture);
    assert_index_current(&mut fixture);

    // A chunk mixing insertions, moves, a failing command and removals.
    let first = handle(&mut fixture, 5);
    let subtree = handle(&mut fixture, 31);
    let removed = handle(&mut fixture, 7);
    let root_node = handle(&mut fixture, 1);
    let outcome = world(&mut fixture)
        .apply_gui_command_chunk(
            SESSION,
            1,
            vec![
                text(41, panel, inc, 31),
                GuiCommand::MoveNode {
                    handle: first,
                    parent: Some(GuiNodeId(31)),
                    index: 0,
                },
                GuiCommand::MoveNode {
                    handle: subtree,
                    parent: Some(GuiNodeId(1)),
                    index: 0,
                },
                GuiCommand::RemoveNode {
                    handle: removed,
                },
                // A cycle fails and stops the chunk.
                GuiCommand::MoveNode {
                    handle: root_node,
                    parent: Some(GuiNodeId(31)),
                    index: 0,
                },
                text(42, panel, inc, 1),
            ],
        )
        .unwrap();
    assert_eq!(outcome.applied, 4);
    assert!(outcome.result.is_err());
    world(&mut fixture).finish_command_stream();
    assert_index_current(&mut fixture);
    let children = world(&mut fixture)
        .inspect_gui(panel, Some(GuiNodeId(31)), 2, 64)
        .unwrap()
        .nodes[0]
        .children
        .clone();
    assert_eq!(children[0], GuiNodeId(5));
    assert!(children.contains(&GuiNodeId(41)));

    // Queued commands between frames, then a subtree removal.
    for command in [
        GuiCommand::RemoveNode {
            handle: subtree,
        },
        text(42, panel, inc, 1),
    ] {
        world(&mut fixture)
            .enqueue_gui_command(SESSION, command)
            .unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    assert_index_current(&mut fixture);
    assert!(
        world(&mut fixture)
            .gui_root(panel)
            .unwrap()
            .nodes()
            .node(GuiNodeId(41))
            .is_none()
    );

    // A whole insertion starts a new incarnation with its own order.
    let replacement = flat_root(20);
    world(&mut fixture)
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::insert_value(
                EntityRef::Handle(panel),
                ComponentValue::GuiRoot(replacement),
            )],
        })
        .unwrap();
    world(&mut fixture).step(0.0).unwrap();
    assert_index_current(&mut fixture);
    assert_ne!(incarnation(&mut fixture), inc);

    // Removing the component drops the index.
    world(&mut fixture)
        .enqueue(Batch {
            id: 3,
            operations: vec![Command::RemoveComponent {
                entity: EntityRef::Handle(panel),
                component: ComponentValue::GUI_ROOT,
            }],
        })
        .unwrap();
    world(&mut fixture).step(0.0).unwrap();
    assert!(
        world(&mut fixture)
            .system::<GuiSystem>(GuiSystem::ID)
            .unwrap()
            .tree(panel)
            .is_none()
    );
}
