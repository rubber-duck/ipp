//! Controls that do not take focus, and focus that reveals its control:
//! traversal skips a control whose `GuiBehavior.focusable` is false, a press
//! on it leaves focus where it is while it still takes feedback and
//! activates, and routed focus scrolls its control into view in the frame
//! that moves it.
//!
//! Logical focus a client command sets steers the keyboard of the input
//! context presenting its World: the next key reaches it, traversal continues
//! from it, a client blur ends it, and the command's focus outlives the
//! input session that adopted it.

use crate::components::{
    FlatSurface, GuiBehavior, GuiButton, GuiCheckbox, GuiScrollView, GuiTextInput,
};
use crate::services::gui_input::GuiDeliveryTerminal;
use crate::services::gui_input::router::*;
use crate::services::gui_input::routing_test_support::*;
use crate::services::gui_input::test_support::*;
use crate::systems::gui::local::{GuiLocalAction, GuiLocalEffectKind};
use crate::{
    Command, ComponentValue, EntityId, EntityRef, ErrorReason, HostRuntime, OutputRef,
    WorldAttachment, WorldRef,
};

use GuiPhysicalKey::{BackTab, Space, Tab};

/// A control of `kind` in a `4 x 1` column slot, taking focus or not.
fn control(kind: ComponentValue, focusable: bool) -> Vec<ComponentValue> {
    vec![
        kind,
        ComponentValue::GuiBehavior(GuiBehavior {
            focusable,
            ..Default::default()
        }),
        sized(0, 4.0, 1.0),
    ]
}

/// A control of `kind` placed last under `parent`.
fn place(
    host: &mut HostRuntime,
    world: WorldRef,
    parent: EntityId,
    kind: ComponentValue,
    focusable: bool,
) -> EntityId {
    create(host, world, control(kind, focusable), Some(parent))
}

fn checkbox() -> ComponentValue {
    ComponentValue::GuiCheckbox(GuiCheckbox::default())
}

fn button() -> ComponentValue {
    ComponentValue::GuiButton(GuiButton::default())
}

/// The focused control among `controls` after each key, in order.
fn walk(
    rig: &mut Rig,
    controls: &[(WorldRef, EntityId)],
    keys: &[GuiPhysicalKey],
) -> Vec<Option<EntityId>> {
    keys.iter()
        .map(|&pressed| {
            rig.send(key(pressed));
            rig.focused(controls).map(|(_, entity)| entity)
        })
        .collect()
}

/// Effects of `kind` applied to `entity`, in delivery order.
fn applied(rig: &Rig, entity: EntityId, kind: &GuiLocalEffectKind) -> usize {
    terminals(&rig.ledger)
        .iter()
        .filter(|terminal| {
            matches!(terminal, GuiDeliveryTerminal::Applied(effect)
                if effect.target.entity == entity && effect.kind == *kind)
        })
        .count()
}

#[test]
fn traversal_and_entry_skip_controls_that_do_not_take_focus() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 4.0, 4.0);
    let first = place(&mut host, world, root_entity, checkbox(), true);
    place(&mut host, world, root_entity, button(), false);
    place(&mut host, world, root_entity, checkbox(), false);
    let last = place(&mut host, world, root_entity, button(), true);
    let mut rig = Rig::new(host, root, viewport(100, 100));
    let controls: Vec<_> = rig
        .host
        .world_mut(world.id())
        .unwrap()
        .entity_children(Some(root_entity))
        .map(|entity| (world, entity))
        .collect();
    assert_eq!(
        walk(&mut rig, &controls, &[Tab, Tab, Tab, BackTab]),
        [Some(first), Some(last), Some(first), Some(last)]
    );
    rig.send(key(GuiPhysicalKey::Escape));
    assert_eq!(walk(&mut rig, &controls, &[BackTab]), [Some(last)]);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn pressing_a_control_that_does_not_take_focus_keeps_a_text_input_focused() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 4.0, 4.0);
    let field = create(
        &mut host,
        world,
        control(
            ComponentValue::GuiTextInput(GuiTextInput {
                text: "ab".into(),
                ..Default::default()
            }),
            true,
        ),
        Some(root_entity),
    );
    let option = place(&mut host, world, root_entity, button(), false);
    let toggle = place(&mut host, world, root_entity, checkbox(), false);
    let mut rig = Rig::new(host, root, viewport(100, 100));
    rig.send(key(Tab));
    let native = |rig: &mut Rig| {
        rig.router
            .with_native_text(&mut rig.host, rig.context.as_ref().unwrap(), |state| {
                state.map(|state| (state.fence.target.entity, state.fence.generation))
            })
    };
    let before = native(&mut rig);
    assert_eq!(before.map(|(entity, _)| entity), Some(field));
    assert!(rig.snapshot(world, field).focused);

    // The held press lights the button and moves no focus.
    let point = rig.point_in(option, [0.5, 0.5]);
    assert!(matches!(
        rig.send(press(1, point)),
        GuiRoutingDisposition::Routed { .. }
    ));
    let held = rig.snapshot(world, option);
    assert!(held.interaction.hovered && held.interaction.pressed);
    assert!(!held.focused);
    assert!(rig.snapshot(world, field).focused);

    // Release activates it; the input keeps focus and its native record.
    rig.send(release(1, point));
    assert_eq!(applied(&rig, option, &GuiLocalEffectKind::Pressed), 1);
    assert!(rig.snapshot(world, field).focused);
    assert_eq!(native(&mut rig), before);

    // A checkbox that does not take focus toggles the same way.
    let point = rig.point_in(toggle, [0.5, 0.5]);
    rig.send(press(2, point));
    rig.send(release(2, point));
    assert_eq!(
        rig.value(world, toggle),
        crate::systems::gui::test_support::GuiTestValue::Bool(true)
    );
    assert!(rig.snapshot(world, field).focused);
    assert_eq!(native(&mut rig), before);

    // The input is the only Tab stop, and the keyboard still targets it.
    rig.send(key(Tab));
    assert!(rig.snapshot(world, field).focused);
    rig.send(key(GuiPhysicalKey::Escape));
    assert!(!rig.snapshot(world, field).focused);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn a_focus_action_on_a_control_that_does_not_take_focus_is_refused() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (_, root_entity) = canvas_root(&mut host, world, 4.0, 4.0);
    let option = place(&mut host, world, root_entity, button(), false);
    let refused = target(&mut host, world, option);
    let outcome = batch(
        &mut host,
        world,
        vec![Command::GuiAction {
            target: crate::GuiActionTarget {
                entity: EntityRef::Handle(option),
                component: refused.component,
                incarnation: refused.incarnation,
            },
            action: GuiLocalAction::Focus(0),
        }],
    );
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ErrorReason::UnsupportedAction
    );

    // A focused control that stops taking focus loses it.
    let field = place(&mut host, world, root_entity, button(), true);
    let focused = target(&mut host, world, field);
    gui_action(&mut host, focused, GuiLocalAction::Focus(0));
    host.frame(0.0).unwrap();
    let read = |host: &mut HostRuntime| {
        crate::systems::gui::test_support::read_control(&host.world_mut(world.id()).unwrap(), field)
            .unwrap()
            .focused
    };
    assert!(read(&mut host));
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(field),
            ComponentValue::GuiBehavior(GuiBehavior {
                focusable: false,
                ..Default::default()
            }),
        )],
    );
    host.frame(0.0).unwrap();
    assert!(!read(&mut host));
}

/// A `4 x 2` vertical ScrollView over a column of five `4 x 1` checkboxes,
/// below a checkbox outside it.
fn scrolled(host: &mut HostRuntime, world: WorldRef, root: EntityId) -> (EntityId, Vec<EntityId>) {
    place(host, world, root, checkbox(), true);
    let view = create(
        host,
        world,
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            sized(0, 4.0, 2.5),
        ],
        Some(root),
    );
    let content = create(host, world, vec![sized(2, 4.0, 5.0)], Some(view));
    let rows = (0..5)
        .map(|_| place(host, world, content, checkbox(), true))
        .collect();
    (view, rows)
}

#[test]
fn routed_focus_reveals_its_control_in_the_frame_that_moves_it() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 4.0, 4.0);
    let (view, rows) = scrolled(&mut host, world, root_entity);
    let mut rig = Rig::new(host, root, viewport(100, 100));

    // Tab walks the outer checkbox, then rows 0 and 1, which the 2.5-tall
    // viewport shows, then row 2 (2..3), which it cuts: one frame applies
    // the focus and moves the view 0.5.
    for (presses, offset) in [(2, 0.0), (1, 0.0), (1, 0.5), (1, 1.5)] {
        for _ in 0..presses {
            rig.send(key(Tab));
        }
        assert_eq!(rig.scroll(world, view), [0.0, offset]);
    }
    assert!(rig.snapshot(world, rows[3]).focused);

    // BackTab to row 0 reveals it at the top.
    for _ in 0..3 {
        rig.send(key(BackTab));
    }
    assert!(rig.snapshot(world, rows[0]).focused);
    assert_eq!(rig.scroll(world, view), [0.0, 0.0]);

    // A pointer press focuses the row the viewport cuts and reveals it;
    // pressing a row that does not take focus moves nothing.
    let cut = rig.point_in(rows[2], [0.5, 0.25]);
    rig.send(press(1, cut));
    rig.send(release(1, cut));
    assert!(rig.snapshot(world, rows[2]).focused);
    assert_eq!(rig.scroll(world, view), [0.0, 0.5]);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn a_press_on_a_control_that_does_not_take_focus_reveals_nothing() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 4.0, 4.0);
    let (view, rows) = scrolled(&mut host, world, root_entity);
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(rows[2]),
            ComponentValue::GuiBehavior(GuiBehavior {
                focusable: false,
                ..Default::default()
            }),
        )],
    );
    let mut rig = Rig::new(host, root, viewport(100, 100));
    let cut = rig.point_in(rows[2], [0.5, 0.25]);
    rig.send(press(1, cut));
    rig.send(release(1, cut));
    assert_eq!(
        rig.value(world, rows[2]),
        crate::systems::gui::test_support::GuiTestValue::Bool(true)
    );
    assert_eq!(rig.scroll(world, view), [0.0, 0.0]);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

fn add_checkbox(host: &mut HostRuntime, world: WorldRef, parent: EntityId) -> EntityId {
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

/// One canvas World with a column of four checkboxes.
struct Column {
    rig: Rig,
    world: WorldRef,
    boxes: [EntityId; 4],
}

impl Column {
    fn new() -> Self {
        let (mut host, _) = host();
        let world = world(&mut host);
        let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
        let boxes = std::array::from_fn(|_| add_checkbox(&mut host, world, root_entity));
        Self {
            rig: Rig::new(host, root, viewport(100, 100)),
            world,
            boxes,
        }
    }

    fn focused(&mut self) -> Option<usize> {
        let controls: Vec<_> = self
            .boxes
            .iter()
            .map(|&entity| (self.world, entity))
            .collect();
        let (_, entity) = self.rig.focused(&controls)?;
        self.boxes.iter().position(|candidate| *candidate == entity)
    }

    fn checked(&mut self, index: usize) -> bool {
        self.rig.value(self.world, self.boxes[index]) == GuiTestValue::Bool(true)
    }

    /// Queue a client's `GuiAction` on one checkbox; the next frame applies it.
    fn client(&mut self, index: usize, action: GuiLocalAction) {
        let target = self.rig.snapshot(self.world, self.boxes[index]).target;
        gui_action(&mut self.rig.host, target, action);
    }

    /// A Host frame, the adapter's routing boundary after it and the frame
    /// applying what that boundary queued.
    fn boundary(&mut self) -> GuiRoutingCancellation {
        self.rig.frame();
        let cancelled = self.rig.synchronize();
        self.rig.frame();
        cancelled
    }

    /// Whether the World's focus is a command's, without an input session.
    fn commanded(&mut self) -> Option<bool> {
        self.rig
            .host
            .world_mut(self.world.id())
            .unwrap()
            .gui_logical_focus()
            .map(|(_, _, commanded, _)| commanded)
    }

    fn finish(self) {
        assert!(self.rig.rejected().is_empty(), "{:?}", self.rig.rejected());
        self.rig.finish();
    }
}

#[test]
fn client_focus_becomes_the_keyboard_target_and_traversal_continues_from_it() {
    let mut column = Column::new();
    column.rig.send(key(Tab));
    assert_eq!(column.focused(), Some(0));

    column.client(2, GuiLocalAction::Focus(0));
    column.boundary();
    assert_eq!(column.focused(), Some(2));
    assert_eq!(column.commanded(), Some(true));

    // The next key reaches the client's control, not the previous target,
    // and leaves its focus the command's.
    column.rig.send(key(Space));
    assert!(column.checked(2));
    assert!(!column.checked(0));
    assert_eq!(column.commanded(), Some(true));

    // Tab and BackTab continue from it.
    column.rig.send(key(Tab));
    assert_eq!(column.focused(), Some(3));
    column.rig.send(key(BackTab));
    column.rig.send(key(BackTab));
    assert_eq!(column.focused(), Some(1));
    assert_eq!(column.commanded(), Some(false));
    column.finish();
}

#[test]
fn client_blur_ends_the_keyboard_target_and_its_native_focus() {
    let mut column = Column::new();
    column.rig.send(key(Tab));
    column.rig.send(key(Tab));
    assert_eq!(column.focused(), Some(1));

    column.client(1, GuiLocalAction::Blur);
    assert!(column.boundary().focus);
    assert_eq!(column.focused(), None);
    assert_eq!(
        column.rig.route(key(Space)),
        Ok(GuiRoutingDisposition::Unhandled)
    );
    assert!(!column.checked(1));

    // Without a target, Tab enters the canvas again.
    column.rig.send(key(Tab));
    assert_eq!(column.focused(), Some(0));
    column.finish();
}

#[test]
fn command_focus_outlives_the_session_that_adopted_it_and_the_next_context_adopts_it() {
    let mut column = Column::new();
    column.client(3, GuiLocalAction::Focus(0));
    column.rig.frame();

    // The boundary queues the adoption, but the context is released before it
    // applies: its fenced commands are cancelled and the focus stays.
    column.rig.synchronize();
    column.rig.rebind();
    column.rig.frame();
    assert_eq!(column.focused(), Some(3));
    assert_eq!(column.commanded(), Some(true));

    // The adopting context's session also ends without ending the focus.
    column.boundary();
    column.rig.rebind();
    column.rig.frame();
    assert_eq!(column.focused(), Some(3));

    // The next context adopts it at its first boundary.
    column.boundary();
    column.rig.send(key(Space));
    assert!(column.checked(3));

    // Focus routed input moves belongs to its session and ends with it.
    column.rig.send(key(Tab));
    assert_eq!(column.focused(), Some(0));
    column.rig.rebind();
    column.rig.frame();
    assert_eq!(column.focused(), None);
    column.finish();
}

#[test]
fn a_context_never_mistakes_its_own_queued_focus_for_a_client_change() {
    let mut column = Column::new();
    column.client(1, GuiLocalAction::Focus(0));
    column.boundary();
    assert_eq!(column.focused(), Some(1));

    // Tab queues focus on the next control; a boundary before it applies sees
    // the World still focusing the command's control and leaves both alone.
    column
        .rig
        .route(key(Tab))
        .expect("Tab routes to the next control");
    assert!(!column.rig.synchronize().focus);
    column.rig.frame();
    assert_eq!(column.focused(), Some(2));
    column.rig.send(key(Space));
    assert!(column.checked(2));
    column.finish();
}

#[test]
fn an_adopted_target_that_becomes_ineligible_ends_like_any_other() {
    let mut column = Column::new();
    column.client(2, GuiLocalAction::Focus(0));
    column.boundary();
    let target = column.boxes[2];
    apply(
        &mut column.rig.host,
        column.world,
        vec![Command::insert_value(
            EntityRef::Handle(target),
            ComponentValue::GuiBehavior(GuiBehavior {
                enabled: false,
                ..Default::default()
            }),
        )],
    );
    assert!(column.boundary().focus);
    assert_eq!(column.focused(), None);
    assert_eq!(
        column.rig.route(key(Space)),
        Ok(GuiRoutingDisposition::Unhandled)
    );
    column.finish();
}

/// A child canvas World with `count` checkboxes, presented in the root canvas
/// as a `4 x 4` slot.
fn panel(
    host: &mut HostRuntime,
    (root, root_entity): (OutputRef, EntityId),
    count: usize,
) -> (WorldRef, Vec<EntityId>) {
    let child = world(host);
    let (output, output_entity) = canvas_root(host, child, 4.0, 4.0);
    let controls = (0..count)
        .map(|_| add_checkbox(host, child, output_entity))
        .collect();
    let surface = FlatSurface {
        width: 4.0,
        height: 4.0,
        ..Default::default()
    };
    create(
        host,
        root.world(),
        vec![
            ComponentValue::FlatSurface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(output)),
            sized(0, 4.0, 4.0),
        ],
        Some(root_entity),
    );
    (child, controls)
}

#[test]
fn client_focus_in_another_panel_moves_the_keyboard_target_and_blurs_the_previous_one() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
    let (first_world, first) = panel(&mut host, (root, root_entity), 2);
    let (second_world, second) = panel(&mut host, (root, root_entity), 2);
    let mut rig = Rig::new(host, root, viewport(100, 100));
    let first: Vec<_> = first.iter().map(|&entity| (first_world, entity)).collect();
    let second: Vec<_> = second
        .iter()
        .map(|&entity| (second_world, entity))
        .collect();
    let controls = [first.clone(), second.clone()].concat();
    rig.send(key(Tab));
    assert_eq!(rig.focused(&controls), Some(first[0]));

    let target = rig.snapshot(second_world, second[1].1).target;
    gui_action(&mut rig.host, target, GuiLocalAction::Focus(0));
    rig.frame();
    rig.synchronize();
    rig.frame();
    assert_eq!(rig.focused(&controls), Some(second[1]));

    // Traversal continues from the adopted target across the panels.
    rig.send(key(BackTab));
    assert_eq!(rig.focused(&controls), Some(second[0]));
    rig.send(key(BackTab));
    assert_eq!(rig.focused(&controls), Some(first[1]));

    // Commands focusing controls in both panels before one boundary are not
    // ordered against each other: the context keeps one target, the last
    // panel's, and blurs the other.
    for (world, entity) in [first[0], second[0]] {
        let target = rig.snapshot(world, entity).target;
        gui_action(&mut rig.host, target, GuiLocalAction::Focus(0));
    }
    rig.frame();
    rig.synchronize();
    rig.frame();
    assert_eq!(rig.focused(&controls), Some(second[0]));
    rig.send(key(Tab));
    assert_eq!(rig.focused(&controls), Some(second[1]));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}
