//! Changes that affect neither structure, GUI layout nor layers patch the
//! Canvas publication in place through real Host frames: an animated paint
//! property, a moved leaf, a dimmed subtree and a sampled skin transition each
//! walk only the entities they touch, whatever the size of the canvas, and
//! publish what the whole walk publishes.
//!
//! ipp-core's tests build with `checked-invariants`, under which every patched
//! evaluation is also compared with the whole walk of the same state. The
//! lockstep test here checks the same equivalence from outside: two identical
//! panels take the same changes, one patching and the other forced through the
//! whole walk by an unrelated structural change in the same frame.

mod support;

use ipp_core::components::{
    CanvasBox, CanvasPaint, CanvasStyle, GuiButton, GuiCheckbox, GuiLayout, GuiOverlay,
};
use ipp_core::systems::canvas::{CanvasPublication, CanvasWork};
use ipp_core::systems::gui::local::GuiLocalAction;
use ipp_core::*;
use std::mem::offset_of;
use support::gui_panel::*;

fn sized(width: f32, height: f32) -> GuiLayout {
    GuiLayout {
        width,
        height,
        ..Default::default()
    }
}

fn stack() -> GuiLayout {
    GuiLayout {
        kind: 3,
        ..Default::default()
    }
}

fn shape(width: f32, height: f32) -> Vec<ComponentValue> {
    vec![
        ComponentValue::CanvasBox(CanvasBox {
            width,
            height,
            ..Default::default()
        }),
        ComponentValue::CanvasStyle(CanvasStyle::default()),
        ComponentValue::GuiLayout(sized(width, height)),
    ]
}

/// A panel of `fillers` buttons beside the changing content.
struct Scene {
    panel: GuiPanel,
    /// A box filled by a custom paint with a `phase` property.
    painted: EntityId,
    /// A plain box.
    leaf: EntityId,
    /// A styled group of three boxes.
    group: EntityId,
    checkbox: EntityId,
    /// An empty top-level entity, re-placed to force the whole walk.
    marker: EntityId,
    /// Whether the marker is placed before the canvas root.
    marker_first: std::cell::Cell<bool>,
}

impl Scene {
    fn new(fillers: usize) -> Self {
        let mut panel = GuiPanel::new(stack());
        let root = panel.root_entity;
        for _ in 0..fillers {
            panel.button(root, sized(20.0, 4.0));
        }
        let checkbox = panel.create(
            Some(root),
            vec![
                ComponentValue::GuiCheckbox(GuiCheckbox::default()),
                ComponentValue::GuiLayout(sized(16.0, 16.0)),
            ],
        );
        let mut paint = CanvasPaint {
            source: "paint:///scope".into(),
            ..Default::default()
        };
        paint
            .properties
            .set("phase", DynamicValue::F32(0.0))
            .unwrap();
        let mut painted = shape(40.0, 20.0);
        painted.push(ComponentValue::CanvasPaint(paint));
        let painted = panel.create(Some(root), painted);
        let leaf = panel.create(Some(root), shape(10.0, 10.0));
        let group = panel.create(
            Some(root),
            vec![
                ComponentValue::CanvasStyle(CanvasStyle::default()),
                ComponentValue::GuiLayout(stack()),
            ],
        );
        for _ in 0..3 {
            panel.create(Some(group), shape(8.0, 8.0));
        }
        let marker = panel.create(None, vec![]);
        panel.frame();
        panel.frame();
        Self {
            panel,
            painted,
            leaf,
            group,
            checkbox,
            marker,
            marker_first: std::cell::Cell::new(false),
        }
    }

    fn work(&mut self) -> CanvasWork {
        self.panel
            .host
            .world_mut(self.panel.world)
            .unwrap()
            .canvas_work()
            .unwrap()
    }

    fn style(&self, entity: EntityId, offset: usize, value: f32) -> Command {
        Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CANVAS_STYLE,
            field: FieldWrite {
                offset: offset as u32,
                value: FieldValue::F32(value),
            },
        }
    }

    fn phase(&self, value: f32) -> Command {
        Command::SetDynamicProperty {
            entity: EntityRef::Handle(self.painted),
            component: ComponentValue::CANVAS_PAINT,
            name: "phase".into(),
            value: DynamicValue::F32(value),
        }
    }

    /// The structural change that forces the whole walk without changing
    /// the published content: the empty marker moves before or after the
    /// canvas root.
    fn restructure(&self) -> Command {
        let first = !self.marker_first.get();
        self.marker_first.set(first);
        Command::PlaceEntity {
            entity: EntityRef::Handle(self.marker),
            placement: EntityPlacementRef {
                parent: None,
                before: first.then_some(EntityRef::Handle(self.panel.root_entity)),
            },
        }
    }

    /// Apply `operations` in one frame and return its work.
    fn change(&mut self, operations: Vec<Command>) -> CanvasWork {
        let outcome = self.panel.apply(operations);
        assert!(outcome.result.is_ok(), "{outcome:?}");
        self.work()
    }
}

/// Work of each kind of patched change on a panel of `fillers` buttons.
fn patched_work(fillers: usize) -> Vec<CanvasWork> {
    let mut scene = Scene::new(fillers);
    let mut work = Vec::new();

    let phase = scene.phase(0.25);
    work.push(scene.change(vec![phase]));
    let moved = scene.style(scene.leaf, offset_of!(CanvasStyle, x), 5.0);
    work.push(scene.change(vec![moved]));
    let dimmed = scene.style(scene.group, offset_of!(CanvasStyle, opacity), 0.5);
    work.push(scene.change(vec![dimmed]));

    // Toggling the checkbox changes its control state, which walks the whole
    // canvas once; its fill then fades in through sampled transitions.
    scene.panel.act(scene.checkbox, GuiLocalAction::Toggle);
    scene.panel.frame_for(0.0);
    let toggled = scene.work();
    assert!(toggled.full, "{toggled:?}");
    assert!(toggled.entities > fillers, "{toggled:?}");
    scene.panel.frame_for(0.025);
    work.push(scene.work());

    scene.panel.frame_for(1.0);
    scene.panel.frame_for(1.0);
    work.push(scene.work());
    work
}

#[test]
fn patched_changes_walk_only_the_entities_they_touch_whatever_the_canvas_size() {
    let small = patched_work(8);
    let large = patched_work(256);
    assert_eq!(small, large, "work is independent of the canvas size");

    let [phase, moved, dimmed, sampled, settled] = small[..] else {
        panic!("{small:?}");
    };
    // A paint property write re-reads only its paint and walks nothing.
    assert_eq!(
        phase,
        CanvasWork {
            patched: true,
            paints: 1,
            ..Default::default()
        }
    );
    // A moved leaf walks itself and replaces its one entry.
    assert_eq!(
        moved,
        CanvasWork {
            patched: true,
            entities: 1,
            primitives: 1,
            replaced: 1,
            ..Default::default()
        }
    );
    // A dimmed group walks its subtree and replaces its children's entries.
    assert_eq!(
        dimmed,
        CanvasWork {
            patched: true,
            entities: 4,
            primitives: 3,
            replaced: 3,
            ..Default::default()
        }
    );
    // A sampled transition walks only its control.
    assert!(sampled.patched && sampled.entities == 1, "{sampled:?}");
    assert!(sampled.replaced >= 1, "{sampled:?}");
    // A settled canvas does no work.
    assert_eq!(settled, CanvasWork::default());
}

#[test]
fn a_patched_publication_names_the_entries_it_replaced() {
    let mut scene = Scene::new(4);
    let before = scene.panel.output();
    let moved = scene.style(scene.leaf, offset_of!(CanvasStyle, x), 5.0);
    scene.change(vec![moved]);
    let after = scene.panel.output();

    let changes = after.paint_changes.as_ref().expect("replaced in place");
    assert_eq!(changes.base, before.paint_revision);
    let [index] = changes.entries[..] else {
        panic!("one replaced entry: {changes:?}");
    };
    let index = index as usize;
    assert_eq!(primitive(&after.entries[index]).style().position[0], 5.0);
    for (at, (before, after)) in before.entries.iter().zip(after.entries.iter()).enumerate() {
        assert_eq!(
            std::sync::Arc::ptr_eq(before, after),
            at != index,
            "only the replaced entry is new"
        );
    }
    assert!(after.paint_revision > before.paint_revision);
    assert!(after.layout_revision > before.layout_revision);
    assert_eq!(after.input_revision, before.input_revision);

    // A paint property write leaves every entry and the paint revision alone.
    let phase = scene.phase(0.5);
    scene.change(vec![phase]);
    let painted = scene.panel.output();
    assert!(std::sync::Arc::ptr_eq(&painted.entries, &after.entries));
    assert_eq!(painted.paint_revision, after.paint_revision);
    assert_eq!(painted.paint_changes, after.paint_changes);
    assert!(painted.paints_revision > after.paints_revision);
}

/// One change of the lockstep sequence, as the operations of one frame.
type Step = fn(&Scene) -> Vec<Command>;

/// One revision of a publication.
type Revision = fn(&CanvasPublication) -> u64;

/// The content two equivalent publications share; revisions are compared by
/// what changed.
fn assert_same_content(patched: &CanvasPublication, whole: &CanvasPublication, step: &str) {
    assert_eq!(patched.entries, whole.entries, "{step}: entries");
    assert_eq!(patched.hits, whole.hits, "{step}: hits");
    assert_eq!(patched.layers, whole.layers, "{step}: layers");
    assert_eq!(patched.paints, whole.paints, "{step}: paints");
    assert_eq!(
        patched.interaction, whole.interaction,
        "{step}: interaction"
    );
    assert_eq!(
        patched
            .paint_changes
            .as_ref()
            .map(|changes| &changes.entries),
        whole.paint_changes.as_ref().map(|changes| &changes.entries),
        "{step}: replaced entries"
    );
}

#[test]
fn patched_and_whole_walks_publish_the_same_canvas_over_a_sequence_of_changes() {
    let mut patched = Scene::new(16);
    let mut whole = Scene::new(16);
    let steps: Vec<(&str, Step)> = vec![
        ("phase", |scene: &Scene| vec![scene.phase(0.3)]),
        ("leaf translation", |scene: &Scene| {
            vec![scene.style(scene.leaf, offset_of!(CanvasStyle, y), 3.0)]
        }),
        ("group tint and scale", |scene: &Scene| {
            vec![
                scene.style(scene.group, offset_of!(CanvasStyle, red), 0.25),
                scene.style(scene.group, offset_of!(CanvasStyle, scale_x), 2.0),
            ]
        }),
        ("group clip", |scene: &Scene| {
            vec![
                Command::SetField {
                    entity: EntityRef::Handle(scene.group),
                    component: ComponentValue::CANVAS_STYLE,
                    field: FieldWrite {
                        offset: offset_of!(CanvasStyle, clipped) as u32,
                        value: FieldValue::Bool(true),
                    },
                },
                scene.style(scene.group, offset_of!(CanvasStyle, clip_max_x), 10.0),
                scene.style(scene.group, offset_of!(CanvasStyle, clip_max_y), 10.0),
            ]
        }),
        ("root opacity and a paint together", |scene: &Scene| {
            vec![
                scene.style(
                    scene.panel.root_entity,
                    offset_of!(CanvasStyle, opacity),
                    0.5,
                ),
                scene.phase(0.9),
            ]
        }),
        ("two separate subtrees", |scene: &Scene| {
            vec![
                scene.style(scene.leaf, offset_of!(CanvasStyle, green), 0.5),
                scene.style(scene.group, offset_of!(CanvasStyle, x), -4.0),
            ]
        }),
    ];
    // The root has no style until the first step that writes one adds it.
    for scene in [&mut patched, &mut whole] {
        let root = scene.panel.root_entity;
        let outcome = scene.panel.apply(vec![Command::insert_value(
            EntityRef::Handle(root),
            ComponentValue::CanvasStyle(CanvasStyle::default()),
        )]);
        assert!(outcome.result.is_ok(), "{outcome:?}");
    }
    let mut previous = (patched.panel.output(), whole.panel.output());
    assert_same_content(&previous.0, &previous.1, "setup");

    for (step, operations) in steps {
        let work = patched.change(operations(&patched));
        assert!(work.patched, "{step}: {work:?}");
        let mut forced = operations(&whole);
        forced.push(whole.restructure());
        let work = whole.change(forced);
        assert!(work.full, "{step}: {work:?}");

        let next = (patched.panel.output(), whole.panel.output());
        assert_same_content(&next.0, &next.1, step);
        let revisions: [(&str, Revision); 3] = [
            ("paint", |canvas| canvas.paint_revision),
            ("input", |canvas| canvas.input_revision),
            ("paints", |canvas| canvas.paints_revision),
        ];
        for (name, revision) in revisions {
            assert_eq!(
                revision(&next.0) != revision(&previous.0),
                revision(&next.1) != revision(&previous.1),
                "{step}: the {name} revision changes alike"
            );
        }
        // The forcing structural change also moves the whole walk's layout
        // revision, so only a patched layout change must be matched.
        assert!(
            next.0.layout_revision == previous.0.layout_revision
                || next.1.layout_revision != previous.1.layout_revision,
            "{step}: a patched layout change is a whole-walk layout change"
        );
        previous = next;
    }
}

#[test]
fn a_change_the_patch_cannot_apply_walks_the_whole_canvas() {
    let mut scene = Scene::new(4);
    let root = scene.panel.root_entity;
    let overlay = scene.panel.create(
        Some(root),
        vec![
            ComponentValue::GuiOverlay(GuiOverlay::default()),
            ComponentValue::GuiButton(GuiButton::default()),
            ComponentValue::GuiLayout(sized(30.0, 10.0)),
            ComponentValue::CanvasStyle(CanvasStyle {
                layer: 1,
                ..Default::default()
            }),
        ],
    );
    scene.panel.frame();
    let raised = scene.panel.output();
    assert_eq!(
        raised
            .layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0.0, 1.0]
    );

    // A style write on the raised layer patches in place, though the
    // published order is by layer.
    let moved = scene.style(scene.leaf, offset_of!(CanvasStyle, x), 2.0);
    let work = scene.change(vec![moved]);
    assert!(work.patched && work.replaced == 1, "{work:?}");
    let tinted = scene.style(overlay, offset_of!(CanvasStyle, red), 0.5);
    let work = scene.change(vec![tinted]);
    assert!(work.patched && work.entities == 1, "{work:?}");

    // Hiding the open overlay by opacity removes its observation and its
    // blocker, which the patch cannot replace in place.
    let hidden = scene.style(overlay, offset_of!(CanvasStyle, opacity), 0.0);
    let work = scene.change(vec![hidden]);
    assert!(work.full, "{work:?}");

    // Moving an entity to another layer walks the whole canvas too.
    let layered = Command::SetField {
        entity: EntityRef::Handle(scene.leaf),
        component: ComponentValue::CANVAS_STYLE,
        field: FieldWrite {
            offset: offset_of!(CanvasStyle, layer) as u32,
            value: FieldValue::U32(2),
        },
    };
    let work = scene.change(vec![layered]);
    assert!(work.full, "{work:?}");
    assert_eq!(
        scene
            .panel
            .output()
            .layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0.0, 1.0, 2.0]
    );
}
