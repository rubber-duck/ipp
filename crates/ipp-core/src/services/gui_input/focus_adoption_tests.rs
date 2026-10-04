//! Logical focus a client command sets steers the keyboard of the input
//! context presenting its World: the next key reaches it, traversal continues
//! from it, a client blur ends it, and the command's focus outlives the
//! input session that adopted it.

use super::router_test_support::*;
use super::*;
use crate::components::{GuiBehavior, GuiCheckbox};
use crate::services::gui_input::router::*;
use crate::systems::gui::local::GuiLocalAction;

use GuiPhysicalKey::{BackTab, Space, Tab};

fn checkbox(host: &mut HostRuntime, world: WorldRef, parent: EntityId) -> EntityId {
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
        let boxes = std::array::from_fn(|_| checkbox(&mut host, world, root_entity));
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
        .map(|_| checkbox(host, child, output_entity))
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
