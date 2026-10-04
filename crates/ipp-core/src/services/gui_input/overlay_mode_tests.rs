//! Overlay modes through routed physical input: light overlays closing on a
//! swallowed outside press and when focus leaves, nested from the top down;
//! modal overlays blocking their canvas below them and keeping Tab inside;
//! focus entering and returning in its modality; the Escape order; the wheel
//! chain ending at an overlay; and hints on the Host clock.

use super::router_test_support::*;
use super::*;
use crate::components::{
    CanvasStyle, GuiBehavior, GuiGroup, GuiOverlay, GuiScrollView, GuiTextInput,
};
use crate::services::gui_input::router::*;
use crate::systems::canvas::CanvasHitKind;
use crate::systems::gui::local::{
    GUI_GROUP_SELECT_NONE, GUI_GROUP_VERTICAL, GuiLocalAction, GuiLocalEffectKind,
};
use crate::{FieldValue, FieldWrite};
use std::mem::offset_of;

use GuiPhysicalKey::{BackTab, Down, Enter, Escape, Tab};

const ROW: u32 = 1;
const COLUMN: u32 = 2;

/// `GuiOverlay.side`: below the parent's box, beside it on the right and
/// centred over the canvas.
const BELOW: u32 = 0;
const RIGHT: u32 = 2;
const CENTRE: u32 = 4;

/// `GuiOverlay.align`: centred along its side.
const CENTRED: u32 = 1;

fn behavior(visible: bool, focusable: bool) -> ComponentValue {
    ComponentValue::GuiBehavior(GuiBehavior {
        visible,
        focusable,
        ..Default::default()
    })
}

fn raised(layer: u32) -> ComponentValue {
    ComponentValue::CanvasStyle(CanvasStyle {
        layer,
        ..Default::default()
    })
}

/// A layout box of `kind` with `padding` above its content.
fn padded(kind: u32, size: [f32; 2], padding: f32) -> ComponentValue {
    ComponentValue::GuiLayout(GuiLayout {
        kind,
        width: size[0],
        height: size[1],
        padding_top: padding,
        ..Default::default()
    })
}

/// An overlay's own `GuiBehavior.visible` field: whether it is open.
fn open_field(host: &mut HostRuntime, world: WorldRef, overlay: EntityId) -> bool {
    host.world_mut(world.id())
        .unwrap()
        .inspect(overlay)
        .unwrap()
        .components
        .into_iter()
        .find_map(|component| match component {
            ComponentValue::GuiBehavior(behavior) => Some(behavior.visible),
            _ => None,
        })
        .unwrap()
}

/// Whether any pointer feedback lights a control.
fn lit(flags: crate::systems::gui::local::GuiInteractionFlags) -> bool {
    flags.hovered || flags.pressed || flags.captured
}

/// One `10 x 10` canvas column presented at ten pixels per unit.
struct Scene {
    rig: Rig,
    world: WorldRef,
    root: EntityId,
}

impl Scene {
    fn new() -> Self {
        let (mut host, _) = host();
        let world = world(&mut host);
        let (output, root) = canvas_root(&mut host, world, 10.0, 10.0);
        Self {
            rig: Rig::new(host, output, viewport(100, 100)),
            world,
            root,
        }
    }

    fn add(&mut self, parent: EntityId, values: Vec<ComponentValue>) -> EntityId {
        create(&mut self.rig.host, self.world, values, Some(parent))
    }

    /// A row of `width x 1` under `parent`.
    fn row(&mut self, parent: EntityId, width: f32) -> EntityId {
        self.add(parent, vec![sized(ROW, width, 1.0)])
    }

    /// A `4 x 1` button under `parent`.
    fn button(&mut self, parent: EntityId, focusable: bool) -> EntityId {
        self.add(
            parent,
            vec![
                ComponentValue::GuiButton(GuiButton::default()),
                behavior(true, focusable),
                sized(0, 4.0, 1.0),
            ],
        )
    }

    /// A closed overlay of `mode` under `parent`, raised `layer` layers and
    /// laid out by `layout`; `None` places it against the canvas.
    fn overlay(
        &mut self,
        parent: Option<EntityId>,
        mode: u32,
        side: u32,
        layer: u32,
        layout: ComponentValue,
    ) -> EntityId {
        create(
            &mut self.rig.host,
            self.world,
            vec![
                ComponentValue::GuiOverlay(GuiOverlay {
                    side,
                    mode,
                    ..Default::default()
                }),
                behavior(false, true),
                raised(layer),
                layout,
            ],
            parent,
        )
    }

    /// A client's write of an overlay's `visible` field, the frame applying
    /// it and the routing boundary after it.
    fn open(&mut self, overlay: EntityId, open: bool) {
        apply(
            &mut self.rig.host,
            self.world,
            vec![Command::SetField {
                entity: EntityRef::Handle(overlay),
                component: ComponentValue::GUI_BEHAVIOR,
                field: crate::FieldWrite {
                    offset: std::mem::offset_of!(GuiBehavior, visible) as u32,
                    value: crate::FieldValue::Bool(open),
                },
            }],
        );
        self.rig.synchronize();
    }

    /// An overlay's own `visible` field: whether it is open.
    fn is_open(&mut self, overlay: EntityId) -> bool {
        open_field(&mut self.rig.host, self.world, overlay)
    }

    /// The World's focus and whether it shows the ring.
    fn focus(&mut self) -> Option<(EntityId, bool)> {
        self.rig
            .host
            .world_mut(self.world.id())
            .unwrap()
            .gui_focus_page(0, 0, 1)
            .first()
            .map(|record| (record.target.entity, record.visible))
    }

    /// Momentary effects of `kind` routed to `entity`.
    fn effects(&self, entity: EntityId, kind: &GuiLocalEffectKind) -> usize {
        terminals(&self.rig.ledger)
            .iter()
            .filter(|terminal| {
                matches!(terminal, GuiDeliveryTerminal::Applied(effect)
                    if effect.target.entity == entity && effect.kind == *kind)
            })
            .count()
    }

    fn pressed(&self, entity: EntityId) -> usize {
        self.effects(entity, &GuiLocalEffectKind::Pressed)
    }

    /// A viewport point at a fraction of a control's box.
    fn at(&mut self, entity: EntityId, fraction: [f32; 2]) -> [f32; 2] {
        self.rig.point_in(entity, fraction)
    }

    /// A primary press and release at `point`, each followed by its frame
    /// and the routing boundary after it, and the press's disposition.
    fn click(&mut self, point: [f32; 2]) -> GuiRoutingDisposition {
        let disposition = self.rig.send(press(1, point));
        self.rig.synchronize();
        self.rig.send(release(1, point));
        self.rig.synchronize();
        disposition
    }

    /// A key and the routing boundary after it.
    fn key(&mut self, pressed: GuiPhysicalKey) -> GuiRoutingDisposition {
        let disposition = self.rig.send(key(pressed));
        self.rig.synchronize();
        disposition
    }

    /// A pointer move to `point` and the routing boundary after it.
    fn hover(&mut self, point: [f32; 2]) {
        self.rig.send(movement(1, point));
        self.rig.synchronize();
    }

    /// `count` Host frames of `seconds` each.
    fn advance(&mut self, count: usize, seconds: f64) {
        for _ in 0..count {
            let report = self.rig.host.frame(seconds).unwrap();
            assert!(report.worlds.values().all(Result::is_ok));
        }
        self.rig.synchronize();
    }

    /// A client's focus or blur of a control.
    fn client(&mut self, entity: EntityId, action: GuiLocalAction) {
        let target = self.rig.snapshot(self.world, entity).target;
        gui_action(&mut self.rig.host, target, action);
        self.rig.frame();
        self.rig.synchronize();
    }

    fn finish(self) {
        assert!(self.rig.rejected().is_empty(), "{:?}", self.rig.rejected());
        self.rig.finish();
    }
}

/// A dropdown in the scene's column:
///
/// ```text
/// root Column 10 x 10
/// ├── Row 10 x 1
/// │   ├── trigger 4 x 1     list: light overlay below it, Column 4 x 4
/// │   │                     with a one-unit top padding and three
/// │   │                     options that do not take focus
/// │   └── away 4 x 1
/// ├── beneath 4 x 1         under the list's padding
/// └── lower 4 x 1           under the list's first option
/// ```
struct Dropdown {
    scene: Scene,
    trigger: EntityId,
    away: EntityId,
    beneath: EntityId,
    lower: EntityId,
    list: EntityId,
    options: Vec<EntityId>,
}

fn dropdown() -> Dropdown {
    let mut scene = Scene::new();
    let row = scene.row(scene.root, 10.0);
    let trigger = scene.button(row, true);
    let away = scene.button(row, true);
    let beneath = scene.button(scene.root, true);
    let lower = scene.button(scene.root, true);
    let list = scene.overlay(
        Some(trigger),
        GuiOverlay::MODE_LIGHT,
        BELOW,
        1,
        padded(COLUMN, [4.0, 4.0], 1.0),
    );
    apply(
        &mut scene.rig.host,
        scene.world,
        vec![Command::insert_value(
            EntityRef::Handle(list),
            ComponentValue::GuiGroup(GuiGroup {
                axis: GUI_GROUP_VERTICAL,
                selection: GUI_GROUP_SELECT_NONE,
            }),
        )],
    );
    let options = (0..3).map(|_| scene.button(list, false)).collect();
    Dropdown {
        scene,
        trigger,
        away,
        beneath,
        lower,
        list,
        options,
    }
}

#[test]
fn a_press_outside_a_light_overlay_closes_it_and_is_swallowed() {
    let mut menu = dropdown();
    let scene = &mut menu.scene;
    scene.key(Tab);
    assert_eq!(scene.focus(), Some((menu.trigger, true)));
    scene.open(menu.list, true);

    // A press on another control closes the list and reaches nothing: no
    // press, no focus, no hover.
    let away = scene.at(menu.away, [0.5, 0.5]);
    assert_eq!(scene.click(away), GuiRoutingDisposition::Blocked);
    assert!(!scene.is_open(menu.list));
    assert_eq!(scene.pressed(menu.away), 0);
    assert_eq!(scene.focus(), Some((menu.trigger, true)));
    assert!(!lit(scene.rig.snapshot(scene.world, menu.away).interaction));

    // A press on empty canvas, which reaches the scene while nothing is
    // open, closes it and is swallowed too, and so is a secondary press,
    // which requests no context.
    let empty = scene.rig.logical([8.0, 8.0]);
    assert_eq!(scene.click(empty), GuiRoutingDisposition::Miss);
    scene.open(menu.list, true);
    assert_eq!(scene.click(empty), GuiRoutingDisposition::Blocked);
    assert!(!scene.is_open(menu.list));
    scene.open(menu.list, true);
    let secondary = GuiPhysicalInput::PointerDown {
        pointer: 2,
        point: away,
        button: GuiPhysicalButton::Secondary,
    };
    assert_eq!(scene.rig.send(secondary), GuiRoutingDisposition::Blocked);
    assert!(!scene.is_open(menu.list));
    assert!(
        !terminals(&scene.rig.ledger).iter().any(|terminal| matches!(
            terminal,
            GuiDeliveryTerminal::Applied(GuiLocalEffect {
                kind: GuiLocalEffectKind::ContextRequested { .. },
                ..
            })
        ))
    );

    // Inside the list's box the press stays with the list: its padding takes
    // it from the control beneath, which neither lights nor activates, and
    // an option takes it as usual. The runtime leaves the list open.
    scene.open(menu.list, true);
    let padding = scene.at(menu.beneath, [0.5, 0.5]);
    scene.hover(padding);
    assert!(
        !scene
            .rig
            .snapshot(scene.world, menu.beneath)
            .interaction
            .hovered
    );
    assert_eq!(scene.click(padding), GuiRoutingDisposition::Blocked);
    assert!(scene.is_open(menu.list));
    assert_eq!(scene.pressed(menu.beneath), 0);
    let option = scene.at(menu.options[0], [0.5, 0.5]);
    assert!(matches!(
        scene.click(option),
        GuiRoutingDisposition::Routed { target } if target.entity == menu.options[0]
    ));
    assert_eq!(scene.pressed(menu.options[0]), 1);
    assert_eq!(scene.pressed(menu.lower), 0);
    assert!(scene.is_open(menu.list));

    // A press on its parent, the trigger, reaches the trigger, whose client
    // closes the list itself.
    let trigger = scene.at(menu.trigger, [0.5, 0.5]);
    assert!(matches!(
        scene.click(trigger),
        GuiRoutingDisposition::Routed { target } if target.entity == menu.trigger
    ));
    assert_eq!(scene.pressed(menu.trigger), 1);
    assert!(scene.is_open(menu.list));
    menu.scene.finish();
}

#[test]
fn focus_leaving_a_light_overlay_and_its_parent_closes_it() {
    let mut menu = dropdown();
    let scene = &mut menu.scene;
    scene.key(Tab);
    scene.open(menu.list, true);

    // The trigger's keys drive the list's active item; focus stays put.
    scene.key(Down);
    assert!(scene.rig.snapshot(scene.world, menu.options[0]).active);
    assert!(scene.is_open(menu.list));

    // Tab moves focus outside both and closes it.
    scene.key(Tab);
    assert_eq!(scene.focus().map(|(entity, _)| entity), Some(menu.away));
    assert!(!scene.is_open(menu.list));

    // Opening it while focus is elsewhere leaves it open until focus moves;
    // focus moving to its parent keeps it.
    scene.open(menu.list, true);
    assert!(scene.is_open(menu.list));
    scene.key(BackTab);
    assert_eq!(scene.focus().map(|(entity, _)| entity), Some(menu.trigger));
    assert!(scene.is_open(menu.list));

    // Focus ending, as when a client blurs or focus moves to another panel,
    // closes it too.
    scene.client(menu.trigger, GuiLocalAction::Blur);
    assert_eq!(scene.focus(), None);
    assert!(!scene.is_open(menu.list));
    menu.scene.finish();
}

/// A popover under its opener, holding an inner button that opens a menu:
///
/// ```text
/// root Column
/// ├── Row: opener 4 x 1 | outside 4 x 1
/// │   opener → popover: light, below, Column 4 x 4, one-unit top padding
/// │                ├── ok 4 x 1
/// │                └── inner 4 x 1 → menu: light, right, Column 4 x 2,
/// │                                   two items that take no focus
/// └── ...
/// ```
struct Popover {
    scene: Scene,
    opener: EntityId,
    outside: EntityId,
    popover: EntityId,
    ok: EntityId,
    inner: EntityId,
    menu: EntityId,
    items: Vec<EntityId>,
}

fn popover() -> Popover {
    let mut scene = Scene::new();
    let row = scene.row(scene.root, 10.0);
    let opener = scene.button(row, true);
    let outside = scene.button(row, true);
    let popover = scene.overlay(
        Some(opener),
        GuiOverlay::MODE_LIGHT,
        BELOW,
        1,
        padded(COLUMN, [4.0, 4.0], 1.0),
    );
    let ok = scene.button(popover, true);
    let inner = scene.button(popover, true);
    let menu = scene.overlay(
        Some(inner),
        GuiOverlay::MODE_LIGHT,
        RIGHT,
        1,
        sized(COLUMN, 4.0, 2.0),
    );
    let items = (0..2).map(|_| scene.button(menu, false)).collect();
    Popover {
        scene,
        opener,
        outside,
        popover,
        ok,
        inner,
        menu,
        items,
    }
}

#[test]
fn nested_light_overlays_close_from_the_top_down_to_the_one_a_press_lands_in() {
    let mut nested = popover();
    let scene = &mut nested.scene;
    scene.key(Tab);
    scene.open(nested.popover, true);
    scene.client(nested.inner, GuiLocalAction::Focus(0));
    scene.open(nested.menu, true);
    assert_eq!(scene.focus().map(|(entity, _)| entity), Some(nested.inner));

    // A press inside the popover but outside the menu closes only the menu,
    // and is swallowed: the button under it is not pressed.
    let ok = scene.at(nested.ok, [0.5, 0.5]);
    assert_eq!(scene.click(ok), GuiRoutingDisposition::Blocked);
    assert!(!scene.is_open(nested.menu));
    assert!(scene.is_open(nested.popover));
    assert_eq!(scene.pressed(nested.ok), 0);

    // A press on the menu's parent keeps both open; one on an item reaches it.
    scene.open(nested.menu, true);
    let inner = scene.at(nested.inner, [0.5, 0.5]);
    scene.click(inner);
    assert_eq!(scene.pressed(nested.inner), 1);
    let item = scene.at(nested.items[1], [0.5, 0.5]);
    scene.click(item);
    assert_eq!(scene.pressed(nested.items[1]), 1);
    assert!(scene.is_open(nested.menu) && scene.is_open(nested.popover));

    // A press outside both closes both.
    let outside = scene.at(nested.outside, [0.5, 0.5]);
    assert_eq!(scene.click(outside), GuiRoutingDisposition::Blocked);
    assert!(!scene.is_open(nested.menu));
    assert!(!scene.is_open(nested.popover));
    assert_eq!(scene.pressed(nested.outside), 0);
    nested.scene.finish();
}

#[test]
fn focus_enters_an_overlay_in_its_modality_and_returns_to_its_invoker() {
    let mut nested = popover();
    let scene = &mut nested.scene;

    // A pointer press focuses the opener without the ring; opening the
    // popover then moves focus to its first focusable control, still without
    // the ring, and the input context takes it as its keyboard target.
    let opener = scene.at(nested.opener, [0.5, 0.5]);
    scene.click(opener);
    assert_eq!(scene.focus(), Some((nested.opener, false)));
    scene.open(nested.popover, true);
    assert_eq!(scene.focus(), Some((nested.ok, false)));
    scene.key(Enter);
    assert_eq!(scene.pressed(nested.ok), 1);
    assert_eq!(scene.focus(), Some((nested.ok, true)));

    // Closing it while focus is inside returns focus to the opener.
    scene.open(nested.popover, false);
    assert_eq!(scene.focus(), Some((nested.opener, true)));

    // From the keyboard the ring comes along; Escape closes the popover and
    // returns focus to the opener with the ring.
    scene.open(nested.popover, true);
    assert_eq!(scene.focus(), Some((nested.ok, true)));
    assert!(matches!(
        scene.key(Escape),
        GuiRoutingDisposition::Routed { target }
            if target.entity == nested.popover && target.component == ComponentValue::GUI_OVERLAY
    ));
    assert!(!scene.is_open(nested.popover));
    assert_eq!(scene.focus(), Some((nested.opener, true)));

    // An outside press that closes it returns focus too, without moving it
    // to what was pressed.
    scene.open(nested.popover, true);
    let outside = scene.at(nested.outside, [0.5, 0.5]);
    scene.click(outside);
    assert!(!scene.is_open(nested.popover));
    assert_eq!(scene.focus().map(|(entity, _)| entity), Some(nested.opener));

    // Focus leaving it closes it without returning: focus stays where it
    // went.
    scene.open(nested.popover, true);
    scene.key(Tab);
    scene.key(Tab);
    assert_eq!(
        scene.focus().map(|(entity, _)| entity),
        Some(nested.outside)
    );
    assert!(!scene.is_open(nested.popover));

    // An invoker that can no longer take focus gets none back: focus inside
    // the closed popover ends.
    scene.open(nested.popover, true);
    assert_eq!(scene.focus().map(|(entity, _)| entity), Some(nested.ok));
    apply(
        &mut scene.rig.host,
        scene.world,
        vec![Command::insert_value(
            EntityRef::Handle(nested.outside),
            ComponentValue::GuiBehavior(GuiBehavior {
                enabled: false,
                ..Default::default()
            }),
        )],
    );
    scene.open(nested.popover, false);
    assert_eq!(scene.focus(), None);

    // Nor does a removed one.
    scene.click(opener);
    scene.open(nested.popover, true);
    apply(
        &mut scene.rig.host,
        scene.world,
        vec![Command::insert_value(
            EntityRef::Handle(nested.outside),
            ComponentValue::GuiBehavior(GuiBehavior::default()),
        )],
    );
    scene.client(nested.outside, GuiLocalAction::Focus(0));
    assert!(!scene.is_open(nested.popover));
    scene.open(nested.popover, true);
    assert_eq!(scene.focus().map(|(entity, _)| entity), Some(nested.ok));
    apply(
        &mut scene.rig.host,
        scene.world,
        vec![Command::Delete {
            entity: EntityRef::Handle(nested.outside),
        }],
    );
    scene.open(nested.popover, false);
    assert_eq!(scene.focus(), None);
    nested.scene.finish();
}

#[test]
fn a_light_overlay_without_focusable_controls_leaves_focus_on_its_invoker() {
    let mut menu = dropdown();
    let scene = &mut menu.scene;
    let trigger = scene.at(menu.trigger, [0.5, 0.5]);
    scene.click(trigger);
    scene.open(menu.list, true);
    assert_eq!(scene.focus(), Some((menu.trigger, false)));
    scene.key(Down);
    scene.key(Down);
    assert!(scene.rig.snapshot(scene.world, menu.options[1]).active);
    scene.key(Enter);
    assert_eq!(scene.pressed(menu.options[1]), 1);
    assert_eq!(scene.pressed(menu.trigger), 1);
    assert_eq!(scene.focus().map(|(entity, _)| entity), Some(menu.trigger));
    menu.scene.finish();
}

/// A text field whose option list opens beside it, and a toast stack above
/// everything, all in one canvas.
#[test]
fn escape_closes_the_topmost_overlay_that_is_not_manual_and_then_blurs() {
    let mut scene = Scene::new();
    let field = scene.add(
        scene.root,
        vec![
            ComponentValue::GuiTextInput(GuiTextInput {
                text: "ab".into(),
                ..Default::default()
            }),
            sized(0, 4.0, 1.0),
        ],
    );
    let list = scene.overlay(
        Some(field),
        GuiOverlay::MODE_LIGHT,
        BELOW,
        1,
        sized(COLUMN, 4.0, 2.0),
    );
    scene.button(list, false);
    let toasts = scene.overlay(
        None,
        GuiOverlay::MODE_MANUAL,
        BELOW,
        3,
        sized(COLUMN, 4.0, 1.0),
    );
    scene.button(toasts, false);
    scene.key(Tab);
    scene.open(list, true);
    scene.open(toasts, true);

    // Escape closes the option list, leaving the text and focus alone.
    assert!(matches!(
        scene.key(Escape),
        GuiRoutingDisposition::Routed { target } if target.entity == list
    ));
    assert!(!scene.is_open(list));
    assert!(scene.is_open(toasts));
    assert_eq!(scene.focus(), Some((field, true)));
    assert_eq!(
        scene.rig.value(scene.world, field),
        GuiTestValue::Text("ab".into())
    );

    // With only a manual overlay open it blurs, as without overlays.
    assert_eq!(scene.key(Escape), GuiRoutingDisposition::Unhandled);
    assert_eq!(scene.focus(), None);
    assert!(scene.is_open(toasts));
    scene.finish();
}

/// A dialog centred on its canvas, panel A, beside panel B, both child
/// canvases of one root canvas:
///
/// ```text
/// panel A Column 5 x 5: beneath 4 x 1, page ScrollView 4 x 2 (capacity 2)
///                        dialog: modal, centred, Column 4 x 3 at (0.5, 1),
///                        cancel and confirm 4 x 1
/// panel B Column 5 x 5: other 4 x 1
/// ```
struct Dialog {
    rig: Rig,
    a: WorldRef,
    b: WorldRef,
    beneath: EntityId,
    page: EntityId,
    dialog: EntityId,
    cancel: EntityId,
    confirm: EntityId,
    other: EntityId,
}

fn dialog() -> Dialog {
    let (mut host, _) = host();
    let root_world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, root_world, 10.0, 5.0);
    let row = create(
        &mut host,
        root_world,
        vec![sized(ROW, 10.0, 5.0)],
        Some(root_entity),
    );
    let panel = |host: &mut HostRuntime| {
        let child = world(host);
        let (output, column) = canvas_root(host, child, 5.0, 5.0);
        create(
            host,
            root_world,
            vec![
                ComponentValue::FlatSurface(FlatSurface {
                    width: 5.0,
                    height: 5.0,
                    ..Default::default()
                }),
                ComponentValue::WorldAttachment(WorldAttachment::surface(output)),
                sized(0, 5.0, 5.0),
            ],
            Some(row),
        );
        (child, column)
    };
    let (a, a_column) = panel(&mut host);
    let (b, b_column) = panel(&mut host);
    let button = |host: &mut HostRuntime, world: WorldRef, parent: EntityId| {
        create(
            host,
            world,
            vec![
                ComponentValue::GuiButton(GuiButton::default()),
                sized(0, 4.0, 1.0),
            ],
            Some(parent),
        )
    };
    let beneath = button(&mut host, a, a_column);
    let page = create(
        &mut host,
        a,
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            sized(0, 4.0, 2.0),
        ],
        Some(a_column),
    );
    create(&mut host, a, vec![sized(COLUMN, 4.0, 4.0)], Some(page));
    let dialog = create(
        &mut host,
        a,
        vec![
            ComponentValue::GuiOverlay(GuiOverlay {
                side: CENTRE,
                align: CENTRED,
                mode: GuiOverlay::MODE_MODAL,
                band: GuiOverlay::BAND_DIALOG,
            }),
            behavior(false, true),
            raised(2),
            sized(COLUMN, 4.0, 3.0),
        ],
        None,
    );
    let cancel = button(&mut host, a, dialog);
    let confirm = button(&mut host, a, dialog);
    let other = button(&mut host, b, b_column);
    Dialog {
        rig: Rig::new(host, root, viewport(100, 50)),
        a,
        b,
        beneath,
        page,
        dialog,
        cancel,
        confirm,
        other,
    }
}

impl Dialog {
    /// Panel A's World's focus and whether it shows the ring.
    fn focus(&mut self) -> Option<(WorldRef, EntityId)> {
        let controls: Vec<_> = [self.beneath, self.cancel, self.confirm]
            .into_iter()
            .map(|entity| (self.a, entity))
            .chain([(self.b, self.other)])
            .collect();
        self.rig.focused(&controls)
    }

    /// The root viewport point of panel A's logical point.
    fn in_a(&self, point: [f32; 2]) -> [f32; 2] {
        self.rig.logical(point)
    }

    fn open(&mut self, open: bool) {
        apply(
            &mut self.rig.host,
            self.a,
            vec![Command::SetField {
                entity: EntityRef::Handle(self.dialog),
                component: ComponentValue::GUI_BEHAVIOR,
                field: crate::FieldWrite {
                    offset: std::mem::offset_of!(GuiBehavior, visible) as u32,
                    value: crate::FieldValue::Bool(open),
                },
            }],
        );
        self.rig.synchronize();
    }

    fn is_open(&mut self) -> bool {
        open_field(&mut self.rig.host, self.a, self.dialog)
    }

    fn pressed(&self, world: WorldRef, entity: EntityId) -> usize {
        terminals(&self.rig.ledger)
            .iter()
            .filter(|terminal| {
                matches!(terminal, GuiDeliveryTerminal::Applied(effect)
                    if effect.target.world == world
                        && effect.target.entity == entity
                        && effect.kind == GuiLocalEffectKind::Pressed)
            })
            .count()
    }

    fn key(&mut self, pressed: GuiPhysicalKey) -> GuiRoutingDisposition {
        let disposition = self.rig.send(key(pressed));
        self.rig.synchronize();
        disposition
    }
}

#[test]
fn a_modal_overlay_blocks_its_canvas_below_it_and_keeps_tab_inside() {
    let mut scene = dialog();
    scene.key(Tab);
    assert_eq!(scene.focus(), Some((scene.a, scene.beneath)));

    // Opening it from the keyboard moves focus to its first control with
    // the ring. The points pressed below lie outside its box.
    scene.open(true);
    assert_eq!(scene.focus(), Some((scene.a, scene.cancel)));
    assert!(scene.rig.snapshot(scene.a, scene.cancel).focused);
    assert_eq!(
        scene.rig.snapshot(scene.a, scene.cancel).bounds,
        [0.5, 1.0, 4.0, 1.0]
    );

    // A press beneath it reaches nothing and leaves it open; hover beneath
    // lights nothing and the wheel scrolls nothing beneath.
    let beneath = scene.in_a([2.0, 0.5]);
    let disposition = scene.rig.send(press(1, beneath));
    scene.rig.send(release(1, beneath));
    assert_eq!(disposition, GuiRoutingDisposition::Blocked);
    assert_eq!(scene.pressed(scene.a, scene.beneath), 0);
    assert!(scene.is_open());
    scene.rig.send(movement(1, beneath));
    assert!(!lit(scene.rig.snapshot(scene.a, scene.beneath).interaction));
    let page = scene.in_a([0.25, 2.0]);
    assert_eq!(
        scene.rig.send(wheel(page, [0.0, 1.0])),
        GuiRoutingDisposition::Unhandled
    );
    assert_eq!(scene.rig.scroll(scene.a, scene.page), [0.0, 0.0]);

    // Tab and BackTab cycle inside it.
    for (pressed, expected) in [
        (Tab, scene.confirm),
        (Tab, scene.cancel),
        (BackTab, scene.confirm),
    ] {
        scene.key(pressed);
        assert_eq!(scene.focus(), Some((scene.a, expected)));
    }

    // Its buttons take presses.
    let confirm = scene.rig.snapshot(scene.a, scene.confirm).bounds;
    let confirm = scene.in_a([confirm[0] + 1.0, confirm[1] + 0.5]);
    scene.rig.send(press(1, confirm));
    scene.rig.send(release(1, confirm));
    assert_eq!(scene.pressed(scene.a, scene.confirm), 1);

    // A control beneath focused by a client takes no keys, and Tab moves
    // into the dialog.
    let target = scene.rig.snapshot(scene.a, scene.beneath).target;
    gui_action(&mut scene.rig.host, target, GuiLocalAction::Focus(0));
    scene.rig.frame();
    scene.rig.synchronize();
    assert_eq!(scene.focus(), Some((scene.a, scene.beneath)));
    assert_eq!(scene.key(Enter), GuiRoutingDisposition::Blocked);
    assert_eq!(scene.pressed(scene.a, scene.beneath), 0);
    scene.key(Tab);
    assert_eq!(scene.focus(), Some((scene.a, scene.cancel)));

    // The other panel stays usable, and focus may move there.
    let other = scene.rig.logical([7.0, 0.5]);
    scene.rig.send(press(1, other));
    scene.rig.send(release(1, other));
    scene.rig.synchronize();
    assert_eq!(scene.pressed(scene.b, scene.other), 1);
    assert_eq!(scene.focus(), Some((scene.b, scene.other)));
    assert!(scene.is_open());

    // Escape in its canvas closes it, and focus inside it returns to the
    // control focused when it opened.
    let target = scene.rig.snapshot(scene.a, scene.confirm).target;
    gui_action(&mut scene.rig.host, target, GuiLocalAction::Focus(0));
    scene.rig.frame();
    scene.rig.synchronize();
    assert!(matches!(
        scene.key(Escape),
        GuiRoutingDisposition::Routed { target } if target.entity == scene.dialog
    ));
    assert!(!scene.is_open());
    assert_eq!(scene.focus(), Some((scene.a, scene.beneath)));

    // Closed, it blocks nothing.
    let disposition = scene.rig.send(press(1, beneath));
    scene.rig.send(release(1, beneath));
    assert!(matches!(disposition, GuiRoutingDisposition::Routed { .. }));
    assert_eq!(scene.pressed(scene.a, scene.beneath), 1);
    scene.rig.send(wheel(page, [0.0, 1.0]));
    assert_eq!(scene.rig.scroll(scene.a, scene.page), [0.0, 1.0]);
    assert!(
        scene.rig.rejected().is_empty(),
        "{:?}",
        scene.rig.rejected()
    );
    scene.rig.finish();
}

#[test]
fn a_press_in_another_panel_closes_a_light_overlay_and_reaches_nothing() {
    let mut scene = dialog();

    // A light list opens below the button in panel A.
    let list = create(
        &mut scene.rig.host,
        scene.a,
        vec![
            ComponentValue::GuiOverlay(GuiOverlay {
                side: BELOW,
                mode: GuiOverlay::MODE_LIGHT,
                ..Default::default()
            }),
            behavior(true, true),
            raised(1),
            sized(COLUMN, 4.0, 1.0),
        ],
        Some(scene.beneath),
    );
    scene.rig.frame();
    scene.rig.synchronize();
    assert!(open_field(&mut scene.rig.host, scene.a, list));

    // A press on panel B's button closes it and is swallowed there too.
    let other = scene.rig.logical([7.0, 0.5]);
    assert_eq!(
        scene.rig.send(press(1, other)),
        GuiRoutingDisposition::Blocked
    );
    scene.rig.send(release(1, other));
    assert!(!open_field(&mut scene.rig.host, scene.a, list));
    assert_eq!(scene.pressed(scene.b, scene.other), 0);
    assert_eq!(scene.focus(), None);

    // Closed, panel B takes the next press.
    scene.rig.send(press(1, other));
    scene.rig.send(release(1, other));
    assert_eq!(scene.pressed(scene.b, scene.other), 1);
    assert!(
        scene.rig.rejected().is_empty(),
        "{:?}",
        scene.rig.rejected()
    );
    scene.rig.finish();
}

#[test]
fn the_wheel_scroll_chain_ends_at_an_overlay() {
    let mut scene = Scene::new();

    // A page that scrolls, holding a trigger whose light list holds a short
    // list that scrolls one unit.
    let page = scene.add(
        scene.root,
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            sized(0, 10.0, 6.0),
        ],
    );
    let content = scene.add(page, vec![sized(COLUMN, 10.0, 12.0)]);
    let trigger = scene.button(content, true);
    let list = scene.overlay(
        Some(trigger),
        GuiOverlay::MODE_LIGHT,
        BELOW,
        1,
        padded(COLUMN, [4.0, 3.0], 1.0),
    );
    let inner = scene.add(
        list,
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            sized(0, 4.0, 2.0),
        ],
    );
    scene.add(inner, vec![sized(COLUMN, 4.0, 3.0)]);
    scene.open(list, true);

    // The list scrolls to its end; the page beneath does not take the rest.
    let point = scene.at(inner, [0.5, 0.5]);
    scene.rig.send(wheel(point, [0.0, 5.0]));
    assert_eq!(scene.rig.scroll(scene.world, inner), [0.0, 1.0]);
    assert_eq!(scene.rig.scroll(scene.world, page), [0.0, 0.0]);
    scene.rig.send(wheel(point, [0.0, 5.0]));
    assert_eq!(scene.rig.scroll(scene.world, page), [0.0, 0.0]);

    // Over the list's own padding nothing scrolls either.
    let padding = scene.at(trigger, [0.5, 1.5]);
    assert_eq!(
        scene.rig.send(wheel(padding, [0.0, 5.0])),
        GuiRoutingDisposition::Unhandled
    );
    assert_eq!(scene.rig.scroll(scene.world, page), [0.0, 0.0]);

    // Closed, the page scrolls there.
    scene.open(list, false);
    scene.rig.send(wheel(padding, [0.0, 5.0]));
    assert_eq!(scene.rig.scroll(scene.world, page), [0.0, 5.0]);
    scene.finish();
}

/// Two help buttons in a row, each with a hint to its right, and a button
/// the first hint covers:
///
/// ```text
/// Row: help 2 x 1 | covered 4 x 1 | second 2 x 1
///      help → tip: hint, right, 4 x 1, holding a button
///      second → second_tip: hint, right, 2 x 1
/// ```
struct Hints {
    scene: Scene,
    help: EntityId,
    covered: EntityId,
    second: EntityId,
    tip: EntityId,
    tip_button: EntityId,
    second_tip: EntityId,
}

fn hints() -> Hints {
    let mut scene = Scene::new();
    let row = scene.row(scene.root, 10.0);
    let narrow = |scene: &mut Scene, parent| {
        scene.add(
            parent,
            vec![
                ComponentValue::GuiButton(GuiButton::default()),
                sized(0, 2.0, 1.0),
            ],
        )
    };
    let help = narrow(&mut scene, row);
    let covered = scene.button(row, true);
    let second = narrow(&mut scene, row);
    let tip = scene.overlay(
        Some(help),
        GuiOverlay::MODE_HINT,
        RIGHT,
        1,
        sized(ROW, 4.0, 1.0),
    );
    let tip_button = scene.button(tip, true);
    let second_tip = scene.overlay(
        Some(second),
        GuiOverlay::MODE_HINT,
        RIGHT,
        1,
        sized(0, 2.0, 1.0),
    );
    Hints {
        scene,
        help,
        covered,
        second,
        tip,
        tip_button,
        second_tip,
    }
}

/// Whether the scene's GUI System has overlay or hint work for its next frame.
fn pending(scene: &mut Scene) -> bool {
    scene
        .rig
        .host
        .world_mut(scene.world.id())
        .unwrap()
        .system::<crate::systems::gui::GuiSystem>(crate::systems::gui::GuiSystem::ID)
        .unwrap()
        .overlay_work_pending()
}

#[test]
fn a_hint_opens_after_its_hover_delay_and_closes_after_its_grace_on_the_host_clock() {
    let mut hints = hints();
    let scene = &mut hints.scene;
    let help = scene.at(hints.help, [0.5, 0.5]);
    scene.hover(help);

    // Hovering for 0.375 s leaves it closed; 0.4 s opens it. Once open,
    // nothing waits and frames do no hint work.
    scene.advance(3, 0.125);
    assert!(!scene.is_open(hints.tip));
    assert!(pending(scene));
    scene.advance(1, 0.025);
    assert!(scene.is_open(hints.tip));
    assert!(
        scene
            .rig
            .read(scene.world, hints.tip_button)
            .unwrap()
            .visible
    );
    scene.advance(1, 0.1);
    assert!(!pending(scene));

    // Leaving keeps it open for the grace, then closes it.
    let away = scene.rig.logical([5.0, 6.0]);
    scene.hover(away);
    scene.advance(1, 0.05);
    assert!(scene.is_open(hints.tip));
    scene.advance(1, 0.05);
    assert!(!scene.is_open(hints.tip));

    // Coming back within the delay and leaving again never opens it; a
    // paused Host, whose frames carry no time, holds the delay.
    scene.hover(help);
    scene.advance(1, 0.2);
    scene.advance(10, 0.0);
    scene.hover(away);
    scene.advance(1, 0.3);
    assert!(!scene.is_open(hints.tip));
    hints.scene.finish();
}

#[test]
fn a_hint_opens_after_its_delay_while_its_parent_holds_visible_focus() {
    let mut hints = hints();
    let scene = &mut hints.scene;

    // Keyboard focus shows the ring: 0.3 s opens the hint.
    scene.key(Tab);
    assert_eq!(scene.focus(), Some((hints.help, true)));
    scene.advance(2, 0.125);
    assert!(!scene.is_open(hints.tip));
    scene.advance(1, 0.05);
    assert!(scene.is_open(hints.tip));

    // The hint never takes focus: Tab passes its button by, and focus
    // leaving closes it after the grace.
    scene.key(Tab);
    assert_eq!(scene.focus().map(|(entity, _)| entity), Some(hints.covered));
    scene.advance(1, 0.1);
    assert!(!scene.is_open(hints.tip));

    // Pointer focus shows no ring and opens nothing once the pointer leaves.
    let help = scene.at(hints.help, [0.5, 0.5]);
    scene.click(help);
    let away = scene.rig.logical([5.0, 6.0]);
    scene.hover(away);
    assert_eq!(scene.focus(), Some((hints.help, false)));
    scene.advance(4, 0.25);
    assert!(!scene.is_open(hints.tip));
    hints.scene.finish();
}

#[test]
fn a_press_or_escape_closes_a_hint_until_its_parent_lets_it_go() {
    let mut hints = hints();
    let scene = &mut hints.scene;
    let help = scene.at(hints.help, [0.5, 0.5]);
    scene.hover(help);
    scene.advance(1, 0.4);
    assert!(scene.is_open(hints.tip));

    // A press on its parent closes it at once and reaches the parent; with
    // the pointer resting there it stays closed.
    let disposition = scene.rig.send(press(1, help));
    assert!(!scene.is_open(hints.tip));
    assert!(matches!(disposition, GuiRoutingDisposition::Routed { .. }));
    scene.rig.send(release(1, help));
    assert_eq!(scene.pressed(hints.help), 1);
    scene.advance(4, 0.25);
    assert!(!scene.is_open(hints.tip));

    // Leaving and coming back opens it again after the delay.
    let away = scene.rig.logical([5.0, 6.0]);
    scene.hover(away);
    scene.hover(help);
    scene.advance(1, 0.4);
    assert!(scene.is_open(hints.tip));

    // Escape without focus closes the hovered canvas's hint, which stays
    // closed while hovered.
    assert_eq!(scene.focus(), Some((hints.help, false)));
    scene.client(hints.help, GuiLocalAction::Blur);
    assert_eq!(scene.focus(), None);
    assert!(matches!(
        scene.key(Escape),
        GuiRoutingDisposition::Routed { target } if target.entity == hints.tip
    ));
    assert!(!scene.is_open(hints.tip));
    scene.advance(4, 0.25);
    assert!(!scene.is_open(hints.tip));
    hints.scene.finish();
}

#[test]
fn one_hint_is_open_at_a_time_and_a_hint_is_inert_to_the_pointer() {
    let mut hints = hints();
    let scene = &mut hints.scene;
    let help = scene.at(hints.help, [0.5, 0.5]);
    let covered = scene.at(hints.covered, [0.5, 0.5]);
    scene.hover(help);
    scene.advance(1, 0.4);
    assert!(scene.is_open(hints.tip));

    // The open hint covers a button, which still takes the pointer: hover
    // and a press pass through the hint, and its own button takes neither.
    let hits = &scene.rig.canvas(scene.rig.root).hits;
    assert!(
        hits.iter()
            .all(|hit| !(hit.target.entity == hints.tip_button && hit.eligible))
    );
    assert!(hits.iter().all(|hit| hit.kind != CanvasHitKind::Overlay));
    scene.hover(covered);
    assert!(
        scene
            .rig
            .snapshot(scene.world, hints.covered)
            .interaction
            .hovered
    );
    scene.click(covered);
    assert_eq!(scene.pressed(hints.covered), 1);
    assert_eq!(scene.pressed(hints.tip_button), 0);
    scene.advance(1, 0.1);
    assert!(!scene.is_open(hints.tip));

    // Moving from one hint's parent to another's closes the first after its
    // grace and opens the second after its delay, never both.
    scene.hover(help);
    scene.advance(1, 0.4);
    let second = scene.at(hints.second, [0.5, 0.5]);
    scene.hover(second);
    for _ in 0..8 {
        scene.advance(1, 0.05);
        assert!(!(scene.is_open(hints.tip) && scene.is_open(hints.second_tip)));
    }
    assert!(!scene.is_open(hints.tip));
    assert!(scene.is_open(hints.second_tip));
    hints.scene.finish();
}

#[test]
fn a_canvas_without_open_light_or_modal_overlays_publishes_no_overlay_hits() {
    let mut menu = dropdown();
    let scene = &mut menu.scene;
    let overlay_hits = |scene: &Scene| {
        scene
            .rig
            .canvas(scene.rig.root)
            .hits
            .iter()
            .filter(|hit| hit.kind == CanvasHitKind::Overlay)
            .count()
    };
    assert_eq!(overlay_hits(scene), 0);
    let empty = scene.rig.logical([8.0, 8.0]);
    assert_eq!(scene.click(empty), GuiRoutingDisposition::Miss);
    let trigger = scene.at(menu.trigger, [0.5, 0.5]);
    scene.hover(trigger);
    scene.key(Tab);
    assert!(!pending(scene));
    scene.open(menu.list, true);
    assert_eq!(overlay_hits(scene), 1);
    scene.open(menu.list, false);
    assert_eq!(overlay_hits(scene), 0);
    menu.scene.finish();
}

#[test]
fn a_toast_stack_above_a_modal_dialog_takes_presses_and_one_below_it_does_not() {
    let mut scene = Scene::new();
    let field = scene.button(scene.root, true);

    // A toast stack, a top-level manual overlay at the canvas's top edge
    // whose toast body is a button that takes no focus, shown over the field.
    let toasts = scene.overlay(None, GuiOverlay::MODE_MANUAL, 1, 3, sized(COLUMN, 4.0, 1.0));
    apply(
        &mut scene.rig.host,
        scene.world,
        vec![Command::SetField {
            entity: EntityRef::Handle(toasts),
            component: ComponentValue::GUI_OVERLAY,
            field: FieldWrite {
                offset: offset_of!(GuiOverlay, band) as u32,
                value: FieldValue::U32(GuiOverlay::BAND_NOTIFICATION),
            },
        }],
    );
    let toast = scene.button(toasts, false);
    scene.open(toasts, true);
    let dialog = create(
        &mut scene.rig.host,
        scene.world,
        vec![
            ComponentValue::GuiOverlay(GuiOverlay {
                side: CENTRE,
                align: CENTRED,
                mode: GuiOverlay::MODE_MODAL,
                band: GuiOverlay::BAND_DIALOG,
            }),
            behavior(false, true),
            raised(2),
            sized(COLUMN, 4.0, 2.0),
        ],
        None,
    );
    let confirm = scene.button(dialog, true);
    scene.key(Tab);
    assert_eq!(scene.focus(), Some((field, true)));

    // A dialog opened while the toast shows takes focus; the toast keeps
    // taking presses above it without taking focus, Tab stays in the dialog
    // and Escape closes the dialog, never the toast.
    scene.open(dialog, true);
    assert_eq!(scene.focus(), Some((confirm, true)));
    let toast_at = scene.at(toast, [0.5, 0.5]);
    assert!(matches!(
        scene.click(toast_at),
        GuiRoutingDisposition::Routed { target } if target.entity == toast
    ));
    assert_eq!(scene.pressed(toast), 1);
    assert_eq!(scene.pressed(field), 0);
    assert_eq!(scene.focus(), Some((confirm, true)));
    scene.key(Tab);
    assert_eq!(scene.focus(), Some((confirm, true)));
    scene.key(Escape);
    assert!(!scene.is_open(dialog));
    assert!(scene.is_open(toasts));
    assert_eq!(scene.focus(), Some((field, true)));

    // Moving the stack to the popup band puts it below the dialog, even
    // though its authored component layer remains higher.
    apply(
        &mut scene.rig.host,
        scene.world,
        vec![Command::SetField {
            entity: EntityRef::Handle(toasts),
            component: ComponentValue::GUI_OVERLAY,
            field: FieldWrite {
                offset: offset_of!(GuiOverlay, band) as u32,
                value: FieldValue::U32(GuiOverlay::BAND_POPUP),
            },
        }],
    );
    scene.open(dialog, true);
    assert_eq!(scene.click(toast_at), GuiRoutingDisposition::Blocked);
    assert_eq!(scene.pressed(toast), 1);
    assert!(scene.is_open(dialog) && scene.is_open(toasts));
    scene.finish();
}
