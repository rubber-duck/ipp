//! Ported legacy keyboard traversal inside one Canvas and across Canvas
//! panels: entry, wrapping, focus scopes, Escape and eligibility.

use super::router_test_support::*;
use super::*;
use crate::components::{CanvasStyle, GuiBehavior, GuiCheckbox, GuiOverlay};
use crate::services::gui_input::router::*;

/// Canvas root `r` of one World:
///
/// ```text
/// r Column
/// ├── c2 Checkbox
/// ├── s3 Column (focus scope)
/// │   ├── c4 Checkbox
/// │   ├── s5 Column (focus scope)
/// │   │   └── c6 Checkbox
/// │   └── c7 Checkbox
/// ├── c8 Checkbox
/// └── c9 Checkbox
/// ```
struct Scoped {
    rig: Rig,
    world: WorldRef,
    /// Index = the legacy node number; scopes and unused slots are present
    /// so each test reads like the tree above.
    nodes: [EntityId; 10],
}

impl Scoped {
    fn new() -> Self {
        let (mut host, _) = host();
        let world = world(&mut host);
        let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
        let mut nodes = [root_entity; 10];
        let checkbox = |host: &mut HostRuntime, parent: EntityId| {
            create(
                host,
                world,
                vec![
                    ComponentValue::GuiCheckbox(GuiCheckbox::default()),
                    sized(0, 4.0, 1.0),
                ],
                Some(parent),
            )
        };
        let scope = |host: &mut HostRuntime, parent: EntityId, height: f32| {
            create(
                host,
                world,
                vec![
                    ComponentValue::GuiBehavior(GuiBehavior {
                        focus_scope: true,
                        ..Default::default()
                    }),
                    sized(2, 4.0, height),
                ],
                Some(parent),
            )
        };
        nodes[2] = checkbox(&mut host, root_entity);
        nodes[3] = scope(&mut host, root_entity, 3.0);
        nodes[4] = checkbox(&mut host, nodes[3]);
        nodes[5] = scope(&mut host, nodes[3], 1.0);
        nodes[6] = checkbox(&mut host, nodes[5]);
        nodes[7] = checkbox(&mut host, nodes[3]);
        nodes[8] = checkbox(&mut host, root_entity);
        nodes[9] = checkbox(&mut host, root_entity);
        Self {
            rig: Rig::new(host, root, viewport(100, 100)),
            world,
            nodes,
        }
    }

    /// Legacy node number of the focused checkbox.
    fn focused(&mut self) -> Option<usize> {
        let controls: Vec<_> = [2, 4, 6, 7, 8, 9]
            .into_iter()
            .map(|node| (self.world, self.nodes[node]))
            .collect();
        let (_, entity) = self.rig.focused(&controls)?;
        self.nodes.iter().position(|node| *node == entity)
    }

    /// Focus one checkbox with a pointer tap, as a user would.
    fn tap(&mut self, node: usize) {
        let point = self.rig.point_in(self.nodes[node], [0.5, 0.5]);
        self.rig.send(press(1, point));
        self.rig.send(release(1, point));
        assert_eq!(self.focused(), Some(node));
    }

    /// Focused node after each key, in order.
    fn walk(&mut self, keys: &[GuiPhysicalKey]) -> Vec<Option<usize>> {
        keys.iter()
            .map(|&pressed| {
                self.rig.send(key(pressed));
                self.focused()
            })
            .collect()
    }

    fn finish(self) {
        assert!(self.rig.rejected().is_empty(), "{:?}", self.rig.rejected());
        self.rig.finish();
    }
}

use GuiPhysicalKey::{BackTab, Escape, Tab};

#[test]
fn back_tab_without_focus_enters_the_last_control() {
    let mut scoped = Scoped::new();
    assert_eq!(scoped.walk(&[BackTab]), [Some(9)]);
    scoped.finish();
}

#[test]
fn entry_without_focusable_controls_is_unhandled() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
    create(
        &mut host,
        world,
        vec![sized(2, 4.0, 4.0)],
        Some(root_entity),
    );
    let mut rig = Rig::new(host, root, viewport(100, 100));
    for pressed in [Tab, BackTab, GuiPhysicalKey::Enter] {
        assert_eq!(
            rig.route(key(pressed)),
            Ok(GuiRoutingDisposition::Unhandled)
        );
    }
    rig.frame();
    assert!(terminals(&rig.ledger).is_empty());
    rig.finish();
}

#[test]
fn unscoped_traversal_follows_tree_order_and_wraps_both_ways() {
    let mut scoped = Scoped::new();
    // Outside every scope, traversal follows the whole tree order, including
    // scoped descendants, and wraps at both ends.
    scoped.tap(8);
    assert_eq!(scoped.walk(&[Tab, Tab, Tab]), [Some(9), Some(2), Some(4)]);
    scoped.tap(2);
    assert_eq!(scoped.walk(&[BackTab, BackTab]), [Some(9), Some(8)]);
    scoped.finish();
}

#[test]
fn traversal_stays_within_the_innermost_focus_scope() {
    let mut scoped = Scoped::new();
    // Scope 3 traverses 4, 6 (inside nested scope 5) and 7, wrapping.
    scoped.tap(7);
    assert_eq!(
        scoped.walk(&[Tab, BackTab, BackTab]),
        [Some(4), Some(7), Some(6)]
    );
    // Reaching nested scope 5 bounds traversal to its only control.
    assert_eq!(scoped.walk(&[Tab, BackTab]), [Some(6), Some(6)]);
    scoped.tap(4);
    assert_eq!(
        scoped.walk(&[BackTab, BackTab, Tab]),
        [Some(7), Some(6), Some(6)]
    );
    scoped.finish();
}

#[test]
fn raised_layers_keep_their_tree_place_in_the_tab_sequence() {
    let mut scoped = Scoped::new();
    let (world, nodes) = (scoped.world, scoped.nodes);
    // The first control and a later one paint above the rest of the canvas;
    // traversal still follows the tree, not painter order.
    let raised = [2, 8].map(|node| {
        Command::insert_value(
            EntityRef::Handle(nodes[node]),
            ComponentValue::CanvasStyle(CanvasStyle {
                layer: 1,
                ..Default::default()
            }),
        )
    });
    apply(&mut scoped.rig.host, world, raised.into());
    scoped.rig.frame();
    assert_eq!(scoped.walk(&[Tab]), [Some(2)]);
    scoped.tap(8);
    assert_eq!(scoped.walk(&[Tab, Tab, Tab]), [Some(9), Some(2), Some(4)]);
    scoped.tap(2);
    assert_eq!(scoped.walk(&[BackTab, BackTab]), [Some(9), Some(8)]);
    scoped.tap(7);
    assert_eq!(scoped.walk(&[Escape, BackTab]), [None, Some(9)]);
    scoped.finish();
}

#[test]
fn a_closed_overlay_leaves_the_tab_sequence_and_an_open_one_keeps_its_tree_place() {
    let mut scoped = Scoped::new();
    let (world, nodes) = (scoped.world, scoped.nodes);
    // A closed overlay below the first checkbox holds a checkbox of its own.
    let overlay = create(
        &mut scoped.rig.host,
        world,
        vec![
            ComponentValue::GuiOverlay(GuiOverlay::default()),
            ComponentValue::GuiBehavior(GuiBehavior {
                visible: false,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                layer: 1,
                ..Default::default()
            }),
            sized(2, 4.0, 1.0),
        ],
        Some(nodes[2]),
    );
    let inside = create(
        &mut scoped.rig.host,
        world,
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            sized(0, 4.0, 1.0),
        ],
        Some(overlay),
    );
    scoped.rig.frame();
    scoped.tap(2);
    assert_eq!(scoped.walk(&[Tab]), [Some(4)]);

    // Open, its checkbox follows its parent in tree order although it paints
    // above everything else.
    apply(
        &mut scoped.rig.host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(overlay),
            component: ComponentValue::GUI_BEHAVIOR,
            field: crate::FieldWrite {
                offset: std::mem::offset_of!(GuiBehavior, visible) as u32,
                value: crate::FieldValue::Bool(true),
            },
        }],
    );
    scoped.rig.frame();
    scoped.tap(2);
    scoped.rig.send(key(Tab));
    assert_eq!(
        scoped.rig.focused(&[(world, inside)]),
        Some((world, inside))
    );
    assert_eq!(scoped.walk(&[Tab]), [Some(4)]);
    scoped.finish();
}

#[test]
fn escape_leaves_the_scope_and_tab_reenters_the_panel() {
    let mut scoped = Scoped::new();
    scoped.tap(7);
    assert_eq!(scoped.walk(&[Escape, Tab]), [None, Some(2)]);
    assert_eq!(scoped.walk(&[Escape, BackTab]), [None, Some(9)]);
    scoped.finish();
}

#[test]
fn traversal_and_entry_skip_disabled_hidden_transparent_and_removed_controls() {
    let mut scoped = Scoped::new();
    let (world, nodes) = (scoped.world, scoped.nodes);
    let root = scoped.rig.root_entity();
    let hidden = create(
        &mut scoped.rig.host,
        world,
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiBehavior(GuiBehavior {
                visible: false,
                ..Default::default()
            }),
            sized(0, 4.0, 1.0),
        ],
        Some(root),
    );
    let mut edits = vec![
        Command::insert_value(
            EntityRef::Handle(nodes[2]),
            ComponentValue::GuiBehavior(GuiBehavior {
                enabled: false,
                ..Default::default()
            }),
        ),
        Command::insert_value(
            EntityRef::Handle(nodes[9]),
            ComponentValue::CanvasStyle(CanvasStyle {
                opacity: 0.0,
                ..Default::default()
            }),
        ),
    ];
    // Removing a subtree removes each descendant explicitly.
    for node in [7, 6, 5, 4, 3] {
        edits.push(Command::Delete {
            entity: EntityRef::Handle(nodes[node]),
        });
    }
    apply(&mut scoped.rig.host, world, edits);
    scoped.rig.frame();
    assert_eq!(scoped.walk(&[Tab]), [Some(8)]);
    assert_eq!(scoped.walk(&[Tab, BackTab]), [Some(8), Some(8)]);
    assert_eq!(scoped.walk(&[Escape, BackTab]), [None, Some(8)]);
    assert!(!scoped.rig.snapshot(world, hidden).focused);
    scoped.finish();
}

/// A child Canvas World presented inside the root Canvas as a `4 x 4` slot.
fn nested_panel(
    host: &mut HostRuntime,
    (root, root_entity): (OutputRef, EntityId),
    build: impl FnOnce(&mut HostRuntime, WorldRef, EntityId) -> Vec<EntityId>,
) -> (WorldRef, Vec<EntityId>) {
    let child = world(host);
    let (output, output_entity) = canvas_root(host, child, 4.0, 4.0);
    let controls = build(host, child, output_entity);
    let surface = Surface {
        width: 4.0,
        height: 4.0,
        ..Default::default()
    };
    create(
        host,
        root.world(),
        vec![
            ComponentValue::Surface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(output)),
            sized(0, 4.0, 4.0),
        ],
        Some(root_entity),
    );
    (child, controls)
}

fn checkbox_in(host: &mut HostRuntime, world: WorldRef, parent: EntityId) -> EntityId {
    create(
        host,
        world,
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            sized(0, 4.0, 1.0),
        ],
        Some(parent),
    )
}

#[test]
fn traversal_crosses_nested_panels_in_paint_order_and_scopes_stay_inside_one() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
    let (first_world, first) =
        nested_panel(&mut host, (root, root_entity), |host, child, parent| {
            vec![
                checkbox_in(host, child, parent),
                checkbox_in(host, child, parent),
            ]
        });
    let (second_world, second) =
        nested_panel(&mut host, (root, root_entity), |host, child, parent| {
            let plain = checkbox_in(host, child, parent);
            let scope = create(
                host,
                child,
                vec![
                    ComponentValue::GuiBehavior(GuiBehavior {
                        focus_scope: true,
                        ..Default::default()
                    }),
                    sized(2, 4.0, 2.0),
                ],
                Some(parent),
            );
            vec![
                plain,
                checkbox_in(host, child, scope),
                checkbox_in(host, child, scope),
            ]
        });
    let mut rig = Rig::new(host, root, viewport(100, 100));
    let first: Vec<_> = first.iter().map(|&entity| (first_world, entity)).collect();
    let second: Vec<_> = second
        .iter()
        .map(|&entity| (second_world, entity))
        .collect();
    let controls = [first.clone(), second.clone()].concat();
    let walk = |rig: &mut Rig, keys: &[GuiPhysicalKey]| {
        keys.iter()
            .map(|&pressed| {
                rig.send(key(pressed));
                rig.focused(&controls).unwrap()
            })
            .collect::<Vec<_>>()
    };
    // Entry takes the first panel; unscoped traversal continues across
    // panels and back.
    assert_eq!(
        walk(&mut rig, &[Tab, Tab, Tab, BackTab]),
        [first[0], first[1], second[0], first[1]]
    );
    // A scope never extends into another panel: it wraps inside its own.
    assert_eq!(
        walk(&mut rig, &[Tab, Tab, Tab, Tab, BackTab]),
        [second[0], second[1], second[2], second[1], second[2]]
    );
    // BackTab entry takes the last control of the first panel in order.
    rig.send(key(Escape));
    assert_eq!(rig.focused(&controls), None);
    assert_eq!(walk(&mut rig, &[BackTab]), [first[1]]);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}
