//! Groups through routed physical input: one Tab stop entered at the
//! selected or first item, arrows, Home and End among eligible items that
//! stop at the ends, selection by activation, by arrow movement and by client
//! writes, and the active item of a group whose items do not take focus,
//! moved by hover and, while the group is in the topmost open overlay, by the
//! keys of a focused control outside it.

use super::router_test_support::*;
use super::*;
use crate::components::{
    CanvasStyle, GuiBehavior, GuiCheckbox, GuiGroup, GuiOverlay, GuiSlider, GuiTextInput,
};
use crate::services::gui_input::router::*;
use crate::systems::gui::local::{
    GUI_GROUP_HORIZONTAL, GUI_GROUP_SELECT_FOLLOW, GUI_GROUP_SELECT_NONE, GUI_GROUP_SELECT_SINGLE,
    GUI_GROUP_VERTICAL, GuiLocalAction, GuiTextEdit,
};

use GuiPhysicalKey::{BackTab, Down, End, Enter, Home, Left, Right, Space, Tab, Up};

const ROW: u32 = 1;
const COLUMN: u32 = 2;

fn button(selected: bool) -> ComponentValue {
    ComponentValue::GuiButton(GuiButton {
        selected,
        ..Default::default()
    })
}

fn behavior(enabled: bool, visible: bool, focusable: bool) -> ComponentValue {
    ComponentValue::GuiBehavior(GuiBehavior {
        enabled,
        visible,
        focusable,
        ..Default::default()
    })
}

/// A `1 x 1` control under `parent`, with its behavior.
fn item(
    host: &mut HostRuntime,
    world: WorldRef,
    parent: EntityId,
    control: ComponentValue,
    behavior: ComponentValue,
) -> EntityId {
    create(
        host,
        world,
        vec![control, behavior, sized(0, 1.0, 1.0)],
        Some(parent),
    )
}

/// A focusable `1 x 1` button under `parent`.
fn focusable(
    host: &mut HostRuntime,
    world: WorldRef,
    parent: EntityId,
    selected: bool,
) -> EntityId {
    item(
        host,
        world,
        parent,
        button(selected),
        behavior(true, true, true),
    )
}

/// A `1 x 1` button under `parent` that does not take focus.
fn option(host: &mut HostRuntime, world: WorldRef, parent: EntityId, selected: bool) -> EntityId {
    item(
        host,
        world,
        parent,
        button(selected),
        behavior(true, true, false),
    )
}

/// A `4 x 1` group container under `parent`, laid out as `kind`.
fn group(
    host: &mut HostRuntime,
    world: WorldRef,
    parent: EntityId,
    axis: u32,
    selection: u32,
    kind: u32,
) -> EntityId {
    let height = if kind == COLUMN {
        3.0
    } else {
        1.0
    };
    create(
        host,
        world,
        vec![
            ComponentValue::GuiGroup(GuiGroup {
                axis,
                selection,
            }),
            sized(kind, 4.0, height),
        ],
        Some(parent),
    )
}

/// The focused entity among `entities` of `world`.
fn focused(rig: &mut Rig, world: WorldRef, entities: &[EntityId]) -> Option<EntityId> {
    let controls: Vec<_> = entities.iter().map(|&entity| (world, entity)).collect();
    rig.focused(&controls).map(|(_, entity)| entity)
}

/// The selected entities among `entities`.
fn selected(rig: &mut Rig, world: WorldRef, entities: &[EntityId]) -> Vec<EntityId> {
    entities
        .iter()
        .copied()
        .filter(|&entity| rig.snapshot(world, entity).selected)
        .collect()
}

/// The `GuiActiveItems` records of `world`: group and item entities.
fn active(rig: &mut Rig, world: WorldRef) -> Vec<(EntityId, EntityId)> {
    rig.host
        .world_mut(world.id())
        .unwrap()
        .gui_active_item_page(0, 0, 16)
        .into_iter()
        .map(|record| (record.group, record.target.entity))
        .collect()
}

/// Momentary effects of `kind` applied to `entity`.
fn applied(rig: &Rig, entity: EntityId, kind: &GuiLocalEffectKind) -> usize {
    terminals(&rig.ledger)
        .iter()
        .filter(|terminal| {
            matches!(terminal, GuiDeliveryTerminal::Applied(effect)
                if effect.target.entity == entity && effect.kind == *kind)
        })
        .count()
}

fn set_selected(entity: EntityId, value: bool) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::GUI_BUTTON,
        field: crate::FieldWrite {
            offset: std::mem::offset_of!(GuiButton, selected) as u32,
            value: crate::FieldValue::Bool(value),
        },
    }
}

/// A canvas column: a button, a horizontal group row of three buttons, the
/// second selected, and another button.
struct Strip {
    world: WorldRef,
    before: EntityId,
    group: EntityId,
    items: Vec<EntityId>,
    after: EntityId,
}

fn strip(selection: u32, selected: Option<usize>) -> (Rig, Strip) {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 4.0, 4.0);
    let before = item(
        &mut host,
        world,
        root_entity,
        button(false),
        behavior(true, true, true),
    );
    let group = group(
        &mut host,
        world,
        root_entity,
        GUI_GROUP_HORIZONTAL,
        selection,
        ROW,
    );
    let items = (0..3)
        .map(|index| focusable(&mut host, world, group, selected == Some(index)))
        .collect();
    let after = item(
        &mut host,
        world,
        root_entity,
        button(false),
        behavior(true, true, true),
    );
    (
        Rig::new(host, root, viewport(100, 100)),
        Strip {
            world,
            before,
            group,
            items,
            after,
        },
    )
}

impl Strip {
    fn all(&self) -> Vec<EntityId> {
        let mut all = vec![self.before];
        all.extend(&self.items);
        all.push(self.after);
        all
    }
}

#[test]
fn a_group_is_one_tab_stop_entered_at_its_selected_item_or_its_first() {
    let (mut rig, strip) = strip(GUI_GROUP_SELECT_NONE, Some(1));
    let all = strip.all();
    let walk = |rig: &mut Rig, key| {
        rig.send(GuiPhysicalInput::Key {
            key,
            shift: false,
        });
        focused(rig, strip.world, &all)
    };

    // Tab enters at the selected item and leaves the group; BackTab enters
    // it at the selected item again.
    assert_eq!(walk(&mut rig, Tab), Some(strip.before));
    assert_eq!(walk(&mut rig, Tab), Some(strip.items[1]));
    assert_eq!(walk(&mut rig, Tab), Some(strip.after));
    assert_eq!(walk(&mut rig, BackTab), Some(strip.items[1]));
    assert_eq!(walk(&mut rig, BackTab), Some(strip.before));

    // Arrows move within the stop; Tab leaves from the item focus moved to.
    walk(&mut rig, Tab);
    assert_eq!(walk(&mut rig, Right), Some(strip.items[2]));
    assert_eq!(walk(&mut rig, Tab), Some(strip.after));

    // Without a selected item the group is entered at its first, either way.
    apply(
        &mut rig.host,
        strip.world,
        vec![set_selected(strip.items[1], false)],
    );
    rig.frame();
    assert_eq!(walk(&mut rig, BackTab), Some(strip.items[0]));
    assert_eq!(walk(&mut rig, BackTab), Some(strip.before));
    assert_eq!(walk(&mut rig, Tab), Some(strip.items[0]));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn arrows_home_and_end_skip_ineligible_items_and_stop_at_the_ends() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 6.0, 4.0);
    let group = group(&mut host, world, root_entity, GUI_GROUP_HORIZONTAL, 0, ROW);
    let a = focusable(&mut host, world, group, false);
    let disabled = item(
        &mut host,
        world,
        group,
        button(false),
        behavior(false, true, true),
    );
    let c = focusable(&mut host, world, group, false);
    let hidden = item(
        &mut host,
        world,
        group,
        button(false),
        behavior(true, false, true),
    );
    let e = focusable(&mut host, world, group, false);
    let all = [a, disabled, c, hidden, e];
    let mut rig = Rig::new(host, root, viewport(100, 100));
    rig.send(key(Tab));
    let walk = |rig: &mut Rig, pressed| {
        let disposition = rig.send(key(pressed));
        (focused(rig, world, &all), disposition)
    };
    let routed = |entity: EntityId| move |disposition: GuiRoutingDisposition| matches!(disposition, GuiRoutingDisposition::Routed { target } if target.entity == entity);
    for (pressed, expected) in [
        (Right, c),
        (Right, e),
        (Right, e),
        (Home, a),
        (Left, a),
        (End, e),
        (Left, c),
    ] {
        let (focus, disposition) = walk(&mut rig, pressed);
        assert_eq!(focus, Some(expected), "after {pressed:?}");
        assert!(
            routed(expected)(disposition),
            "{pressed:?}: {disposition:?}"
        );
    }

    // Keys off the group's axis are left to the control, which has no use for
    // them: they stay unhandled and reach clients as such.
    for pressed in [Up, Down] {
        let (focus, disposition) = walk(&mut rig, pressed);
        assert_eq!(focus, Some(c));
        assert_eq!(disposition, GuiRoutingDisposition::Unhandled);
    }
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn arrow_movement_selects_in_a_group_whose_selection_follows() {
    let (mut rig, strip) = strip(GUI_GROUP_SELECT_FOLLOW, Some(0));
    rig.send(key(Tab));
    rig.send(key(Tab));
    assert!(rig.snapshot(strip.world, strip.items[0]).focused);

    // Focus and selection move in the frame that applies the key.
    rig.send(key(Right));
    assert!(rig.snapshot(strip.world, strip.items[1]).focused);
    assert_eq!(
        selected(&mut rig, strip.world, &strip.items),
        [strip.items[1]]
    );
    rig.send(key(End));
    assert_eq!(
        selected(&mut rig, strip.world, &strip.items),
        [strip.items[2]]
    );

    // At the end nothing moves and nothing is written.
    let commits = rig.commits().len();
    rig.send(key(Right));
    assert_eq!(rig.commits().len(), commits);
    assert_eq!(
        selected(&mut rig, strip.world, &strip.items),
        [strip.items[2]]
    );
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn activation_selects_in_a_single_selection_group_and_arrows_only_move_focus() {
    let (mut rig, strip) = strip(GUI_GROUP_SELECT_SINGLE, Some(0));
    rig.send(key(Tab));
    rig.send(key(Tab));
    rig.send(key(Right));
    assert!(rig.snapshot(strip.world, strip.items[1]).focused);
    assert_eq!(
        selected(&mut rig, strip.world, &strip.items),
        [strip.items[0]]
    );

    // Enter presses the focused item and selects it in the same frame.
    rig.send(key(Enter));
    assert_eq!(
        applied(&rig, strip.items[1], &GuiLocalEffectKind::Pressed),
        1
    );
    assert_eq!(
        selected(&mut rig, strip.world, &strip.items),
        [strip.items[1]]
    );

    // Space does too, and so does a pointer press, which also focuses.
    rig.send(key(Right));
    rig.send(key(Space));
    assert_eq!(
        selected(&mut rig, strip.world, &strip.items),
        [strip.items[2]]
    );
    let point = rig.point_in(strip.items[0], [0.5, 0.5]);
    rig.send(press(1, point));
    rig.send(release(1, point));
    assert!(rig.snapshot(strip.world, strip.items[0]).focused);
    assert_eq!(
        selected(&mut rig, strip.world, &strip.items),
        [strip.items[0]]
    );

    // A semantic press selects as well, and moves no focus.
    let target = rig.snapshot(strip.world, strip.items[2]).target;
    gui_action(&mut rig.host, target, GuiLocalAction::Press);
    rig.frame();
    assert_eq!(
        selected(&mut rig, strip.world, &strip.items),
        [strip.items[2]]
    );
    assert!(rig.snapshot(strip.world, strip.items[0]).focused);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn client_writes_leave_the_last_written_item_selected() {
    let (mut rig, strip) = strip(GUI_GROUP_SELECT_SINGLE, Some(0));
    let [a, b, c] = [strip.items[0], strip.items[1], strip.items[2]];

    // Within one batch, the last write of `true` wins.
    apply(
        &mut rig.host,
        strip.world,
        vec![set_selected(b, true), set_selected(c, true)],
    );
    assert_eq!(selected(&mut rig, strip.world, &strip.items), [c]);
    apply(&mut rig.host, strip.world, vec![set_selected(a, true)]);
    assert_eq!(selected(&mut rig, strip.world, &strip.items), [a]);

    // Writing `false` leaves nothing selected.
    apply(&mut rig.host, strip.world, vec![set_selected(a, false)]);
    assert!(selected(&mut rig, strip.world, &strip.items).is_empty());

    // An item inserted selected clears the others.
    apply(&mut rig.host, strip.world, vec![set_selected(b, true)]);
    let inserted = focusable(&mut rig.host, strip.world, strip.group, true);
    let mut items = strip.items.clone();
    items.push(inserted);
    assert_eq!(selected(&mut rig, strip.world, &items), [inserted]);

    // A button outside the group is unaffected.
    apply(
        &mut rig.host,
        strip.world,
        vec![set_selected(strip.after, true), set_selected(a, true)],
    );
    assert_eq!(selected(&mut rig, strip.world, &items), [a]);
    assert!(rig.snapshot(strip.world, strip.after).selected);
    rig.finish();
}

#[test]
fn a_group_without_selection_writes_no_selected_field() {
    let (mut rig, strip) = strip(GUI_GROUP_SELECT_NONE, Some(0));
    apply(
        &mut rig.host,
        strip.world,
        vec![set_selected(strip.items[2], true)],
    );
    assert_eq!(
        selected(&mut rig, strip.world, &strip.items),
        [strip.items[0], strip.items[2]]
    );
    rig.send(key(Tab));
    rig.send(key(Tab));
    rig.send(key(Right));
    rig.send(key(Enter));
    assert_eq!(
        selected(&mut rig, strip.world, &strip.items),
        [strip.items[0], strip.items[2]]
    );
    rig.finish();
}

#[test]
fn checkbox_items_toggle_and_are_not_exclusive() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 4.0, 4.0);
    let group = group(
        &mut host,
        world,
        root_entity,
        GUI_GROUP_VERTICAL,
        GUI_GROUP_SELECT_SINGLE,
        COLUMN,
    );
    let boxes: Vec<_> = (0..3)
        .map(|_| {
            item(
                &mut host,
                world,
                group,
                ComponentValue::GuiCheckbox(GuiCheckbox::default()),
                behavior(true, true, true),
            )
        })
        .collect();
    let mut rig = Rig::new(host, root, viewport(100, 100));
    rig.send(key(Tab));
    rig.send(key(Space));
    rig.send(key(Down));
    rig.send(key(Space));
    assert!(rig.snapshot(world, boxes[1]).focused);
    for (index, checked) in [true, true, false].into_iter().enumerate() {
        assert_eq!(rig.value(world, boxes[index]), GuiTestValue::Bool(checked));
    }
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn a_nested_group_owns_its_items_and_its_own_tab_stop() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 4.0, 4.0);
    let outer = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiGroup(GuiGroup::default()),
            sized(COLUMN, 4.0, 3.0),
        ],
        Some(root_entity),
    );
    let a = focusable(&mut host, world, outer, false);
    let inner = group(&mut host, world, outer, GUI_GROUP_HORIZONTAL, 0, ROW);
    let x = focusable(&mut host, world, inner, false);
    let y = focusable(&mut host, world, inner, false);
    let b = focusable(&mut host, world, outer, false);
    let all = [a, x, y, b];
    let mut rig = Rig::new(host, root, viewport(100, 100));
    let walk = |rig: &mut Rig, pressed| {
        rig.send(key(pressed));
        focused(rig, world, &all)
    };
    assert_eq!(walk(&mut rig, Tab), Some(a));

    // The outer group's arrows skip the nested group's items.
    assert_eq!(walk(&mut rig, Down), Some(b));
    assert_eq!(walk(&mut rig, Up), Some(a));

    // The nested group is its own stop, with its own axis.
    assert_eq!(walk(&mut rig, Tab), Some(x));
    assert_eq!(walk(&mut rig, Right), Some(y));
    assert_eq!(walk(&mut rig, Down), Some(y));
    assert_eq!(walk(&mut rig, Tab), Some(a));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn a_slider_and_a_text_input_in_a_group_keep_their_own_keys() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 4.0, 6.0);
    let group = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiGroup(GuiGroup::default()),
            sized(COLUMN, 4.0, 4.0),
        ],
        Some(root_entity),
    );
    let a = focusable(&mut host, world, group, false);
    let slider = item(
        &mut host,
        world,
        group,
        ComponentValue::GuiSlider(GuiSlider {
            min: 0.0,
            max: 10.0,
            step: 1.0,
            value: 5.0,
            ..Default::default()
        }),
        behavior(true, true, true),
    );
    let text = item(
        &mut host,
        world,
        group,
        ComponentValue::GuiTextInput(GuiTextInput {
            text: "ab".into(),
            ..Default::default()
        }),
        behavior(true, true, true),
    );
    let b = focusable(&mut host, world, group, false);
    let outside = focusable(&mut host, world, root_entity, false);
    let all = [a, slider, text, b, outside];
    let mut rig = Rig::new(host, root, viewport(100, 100));
    rig.send(key(Tab));
    rig.send(key(Down));
    assert_eq!(focused(&mut rig, world, &all), Some(slider));

    // The slider keeps its arrows, Home and End.
    for (pressed, value) in [(Down, 4.0), (Up, 5.0), (Home, 0.0), (End, 10.0)] {
        rig.send(key(pressed));
        assert_eq!(focused(&mut rig, world, &all), Some(slider));
        assert_eq!(rig.value(world, slider), GuiTestValue::Scalar(value));
    }

    // Tab leaves the group from the slider; BackTab returns to its first item.
    rig.send(key(Tab));
    assert_eq!(focused(&mut rig, world, &all), Some(outside));
    rig.send(key(BackTab));
    assert_eq!(focused(&mut rig, world, &all), Some(a));

    // A focused text input keeps Left, Right, Home and End, unhandled as
    // keys, and gives Up and Down to the group.
    rig.send(key(End));
    rig.send(key(Up));
    assert_eq!(focused(&mut rig, world, &all), Some(text));
    for pressed in [Left, Right, Home, End] {
        assert_eq!(rig.send(key(pressed)), GuiRoutingDisposition::Unhandled);
        assert_eq!(focused(&mut rig, world, &all), Some(text));
    }
    rig.send(key(Down));
    assert_eq!(focused(&mut rig, world, &all), Some(b));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

/// A light overlay placed on the right side of its parent's box.
fn overlay() -> ComponentValue {
    overlay_in(GuiOverlay::MODE_LIGHT)
}

/// An overlay of `mode` placed on the right side of its parent's box.
fn overlay_in(mode: u32) -> ComponentValue {
    ComponentValue::GuiOverlay(GuiOverlay {
        side: 2,
        mode,
        ..Default::default()
    })
}

/// A style raising its entity `layer` layers.
fn raised(layer: u32) -> ComponentValue {
    ComponentValue::CanvasStyle(CanvasStyle {
        layer,
        ..Default::default()
    })
}

/// A text input and a button below it, and an open light overlay beside one
/// of them holding a column group of three items that do not take focus.
struct OptionList {
    world: WorldRef,
    field: EntityId,
    trigger: EntityId,
    group: EntityId,
    options: Vec<EntityId>,
}

/// The option list of the text input.
fn option_list(selection: u32, selected: Option<usize>) -> (Rig, OptionList) {
    option_list_of(selection, selected, false)
}

/// The option list of the text input, or of the button as its trigger.
fn option_list_of(selection: u32, selected: Option<usize>, of_trigger: bool) -> (Rig, OptionList) {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 6.0, 5.0);
    let field = item(
        &mut host,
        world,
        root_entity,
        ComponentValue::GuiTextInput(GuiTextInput {
            text: "ab".into(),
            ..Default::default()
        }),
        behavior(true, true, true),
    );
    let trigger = focusable(&mut host, world, root_entity, false);
    let group = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiGroup(GuiGroup {
                axis: GUI_GROUP_VERTICAL,
                selection,
            }),
            overlay(),
            raised(1),
            sized(COLUMN, 4.0, 3.0),
        ],
        Some(if of_trigger {
            trigger
        } else {
            field
        }),
    );
    let options = (0..3)
        .map(|index| option(&mut host, world, group, selected == Some(index)))
        .collect();
    (
        Rig::new(host, root, viewport(100, 100)),
        OptionList {
            world,
            field,
            trigger,
            group,
            options,
        },
    )
}

impl OptionList {
    fn active(&self, rig: &mut Rig) -> Option<EntityId> {
        let records = active(rig, self.world);
        assert!(records.len() <= 1, "{records:?}");
        records.first().map(|&(group, item)| {
            assert_eq!(group, self.group);
            item
        })
    }

    fn submit(&self, rig: &mut Rig) -> GuiRoutingDisposition {
        let fence =
            rig.router
                .with_native_text(&mut rig.host, rig.context.as_ref().unwrap(), |state| {
                    state.unwrap().fence
                });
        rig.send(GuiPhysicalInput::Text {
            fence,
            edit: GuiTextEdit::Submit,
        })
    }
}

#[test]
fn keys_from_a_focused_text_input_move_and_activate_the_active_item() {
    let (mut rig, list) = option_list(GUI_GROUP_SELECT_SINGLE, None);
    let [first, second, third] = [list.options[0], list.options[1], list.options[2]];
    rig.send(key(Tab));
    assert!(rig.snapshot(list.world, list.field).focused);
    assert_eq!(list.active(&mut rig), None);

    // Down and Up move the active item, stop at the ends and leave focus on
    // the field; the active item paints as hovered.
    for (pressed, expected) in [
        (Down, first),
        (Down, second),
        (Down, third),
        (Down, third),
        (Up, second),
    ] {
        let disposition = rig.send(key(pressed));
        assert!(
            matches!(disposition, GuiRoutingDisposition::Routed { target } if target.entity == expected)
        );
        assert_eq!(list.active(&mut rig), Some(expected), "after {pressed:?}");
        assert!(rig.snapshot(list.world, list.field).focused);
    }
    assert!(rig.snapshot(list.world, second).painted.hovered);
    assert!(!rig.snapshot(list.world, third).painted.hovered);

    // Home and End stay with the field's editing.
    for pressed in [Home, End, Left, Right] {
        assert_eq!(rig.send(key(pressed)), GuiRoutingDisposition::Unhandled);
        assert_eq!(list.active(&mut rig), Some(second));
    }

    // Typing still reaches the field.
    let fence =
        rig.router
            .with_native_text(&mut rig.host, rig.context.as_ref().unwrap(), |state| {
                state.unwrap().fence
            });
    rig.send(GuiPhysicalInput::Text {
        fence,
        edit: GuiTextEdit::Insert("c".into()),
    });
    assert_eq!(
        rig.value(list.world, list.field),
        GuiTestValue::Text("abc".into())
    );

    // Enter, as the field's native submit, activates and selects the active
    // item instead of submitting.
    let disposition = list.submit(&mut rig);
    assert!(
        matches!(disposition, GuiRoutingDisposition::Routed { target } if target.entity == second)
    );
    assert_eq!(applied(&rig, second, &GuiLocalEffectKind::Pressed), 1);
    assert_eq!(
        terminals(&rig.ledger)
            .iter()
            .filter(
                |terminal| matches!(terminal, GuiDeliveryTerminal::Applied(effect)
                if matches!(effect.kind, GuiLocalEffectKind::Submitted(_)))
            )
            .count(),
        0
    );
    assert_eq!(selected(&mut rig, list.world, &list.options), [second]);

    // Enter as a key does the same; Space is the field's, not the group's.
    rig.send(key(Up));
    rig.send(key(Enter));
    assert_eq!(applied(&rig, first, &GuiLocalEffectKind::Pressed), 1);
    assert_eq!(selected(&mut rig, list.world, &list.options), [first]);
    assert_eq!(rig.send(key(Space)), GuiRoutingDisposition::Unhandled);
    assert_eq!(applied(&rig, first, &GuiLocalEffectKind::Pressed), 1);
    assert!(rig.snapshot(list.world, list.field).focused);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn space_activates_the_active_item_before_a_focused_button() {
    let (mut rig, list) = option_list_of(GUI_GROUP_SELECT_NONE, None, true);

    // Focus entering the field, outside the trigger's list, closes it; the
    // trigger opens it again once it holds focus, as a dropdown does.
    rig.send(key(Tab));
    assert!(!rig.read(list.world, list.options[0]).unwrap().visible);
    rig.send(key(Tab));
    assert!(rig.snapshot(list.world, list.trigger).focused);
    apply(
        &mut rig.host,
        list.world,
        vec![Command::insert_value(
            EntityRef::Handle(list.group),
            behavior(true, true, true),
        )],
    );
    rig.frame();

    // Without an active item the trigger takes Enter.
    rig.send(key(Enter));
    assert_eq!(applied(&rig, list.trigger, &GuiLocalEffectKind::Pressed), 1);

    // With one, Home and End reach the group and Space activates its item.
    rig.send(key(End));
    assert_eq!(list.active(&mut rig), Some(list.options[2]));
    rig.send(key(Home));
    assert_eq!(list.active(&mut rig), Some(list.options[0]));
    rig.send(key(Space));
    assert_eq!(
        applied(&rig, list.options[0], &GuiLocalEffectKind::Pressed),
        1
    );
    assert_eq!(applied(&rig, list.trigger, &GuiLocalEffectKind::Pressed), 1);
    assert!(selected(&mut rig, list.world, &list.options).is_empty());
    assert!(rig.snapshot(list.world, list.trigger).focused);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn the_active_item_starts_at_the_selected_item() {
    let (mut rig, list) = option_list(GUI_GROUP_SELECT_SINGLE, Some(1));
    rig.send(key(Tab));
    rig.send(key(Up));
    assert_eq!(list.active(&mut rig), Some(list.options[1]));
    rig.send(key(Down));
    assert_eq!(list.active(&mut rig), Some(list.options[2]));
    rig.finish();
}

#[test]
fn keys_routed_before_a_frame_continue_from_the_last_active_item() {
    let (mut rig, list) = option_list(GUI_GROUP_SELECT_SINGLE, None);
    rig.send(key(Tab));

    // Three keys against one completed publication, then the frame.
    for pressed in [Down, Down, Enter] {
        rig.route(key(pressed)).unwrap();
    }
    rig.frame();
    rig.synchronize();
    assert_eq!(list.active(&mut rig), Some(list.options[1]));
    assert_eq!(
        applied(&rig, list.options[1], &GuiLocalEffectKind::Pressed),
        1
    );
    assert_eq!(
        selected(&mut rig, list.world, &list.options),
        [list.options[1]]
    );
    rig.finish();
}

#[test]
fn hover_moves_the_active_item_and_the_group_shows_one_highlight() {
    let (mut rig, list) = option_list(GUI_GROUP_SELECT_NONE, None);
    rig.send(key(Tab));

    // Hover makes an item active without scrolling or focusing it.
    let point = rig.point_in(list.options[2], [0.5, 0.5]);
    rig.send(movement(1, point));
    assert_eq!(list.active(&mut rig), Some(list.options[2]));
    assert!(rig.snapshot(list.world, list.options[2]).painted.hovered);
    assert!(rig.snapshot(list.world, list.field).focused);

    // A key moves the active item from there; the pointer still rests on the
    // previous item, which no longer lights.
    rig.send(key(Up));
    assert_eq!(list.active(&mut rig), Some(list.options[1]));
    let rested = rig.snapshot(list.world, list.options[2]);
    assert!(rested.interaction.hovered && !rested.painted.hovered);
    assert!(rig.snapshot(list.world, list.options[1]).painted.hovered);

    // Moving onto another item makes it active again.
    let point = rig.point_in(list.options[0], [0.5, 0.5]);
    rig.send(movement(1, point));
    assert_eq!(list.active(&mut rig), Some(list.options[0]));
    let lit: Vec<_> = list
        .options
        .iter()
        .filter(|&&option| rig.snapshot(list.world, option).painted.hovered)
        .copied()
        .collect();
    assert_eq!(lit, [list.options[0]]);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn only_a_group_in_the_topmost_open_overlay_takes_keys() {
    let (mut rig, list) = option_list(GUI_GROUP_SELECT_NONE, None);

    // A light overlay without a group opens above the option list.
    let above = create(
        &mut rig.host,
        list.world,
        vec![overlay(), raised(2), sized(0, 1.0, 1.0)],
        Some(list.field),
    );
    rig.frame();
    rig.send(key(Tab));
    assert_eq!(rig.send(key(Down)), GuiRoutingDisposition::Unhandled);
    assert_eq!(list.active(&mut rig), None);

    // Closing it gives the keys back to the option list.
    apply(
        &mut rig.host,
        list.world,
        vec![Command::insert_value(
            EntityRef::Handle(above),
            behavior(true, false, true),
        )],
    );
    rig.frame();
    rig.send(key(Down));
    assert_eq!(list.active(&mut rig), Some(list.options[0]));

    // A manual overlay, such as a toast stack, and a hint above it leave the
    // keys with the option list.
    for mode in [GuiOverlay::MODE_MANUAL, GuiOverlay::MODE_HINT] {
        let other = create(
            &mut rig.host,
            list.world,
            vec![overlay_in(mode), raised(3), sized(0, 1.0, 1.0)],
            Some(list.field),
        );
        rig.frame();
        rig.send(key(Down));
        rig.send(key(Up));
        assert_eq!(list.active(&mut rig), Some(list.options[0]), "{mode}");
        apply(
            &mut rig.host,
            list.world,
            vec![Command::Delete {
                entity: EntityRef::Handle(other),
            }],
        );
    }

    // A raised group that is not in an overlay takes none.
    apply(
        &mut rig.host,
        list.world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(list.group),
            component: ComponentValue::GUI_OVERLAY,
        }],
    );
    rig.frame();
    assert_eq!(rig.send(key(Down)), GuiRoutingDisposition::Unhandled);
    rig.finish();
}

#[test]
fn removing_hiding_and_reordering_items_follow_the_current_tree() {
    let (mut rig, list) = option_list(GUI_GROUP_SELECT_NONE, None);
    rig.send(key(Tab));
    rig.send(key(Down));
    rig.send(key(Down));
    assert_eq!(list.active(&mut rig), Some(list.options[1]));

    // Removing the active item ends it; the next key starts again.
    apply(
        &mut rig.host,
        list.world,
        vec![Command::Delete {
            entity: EntityRef::Handle(list.options[1]),
        }],
    );
    rig.frame();
    rig.synchronize();
    assert_eq!(list.active(&mut rig), None);
    rig.send(key(Down));
    assert_eq!(list.active(&mut rig), Some(list.options[0]));

    // Reordering moves along the new order: the third option now comes first.
    apply(
        &mut rig.host,
        list.world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(list.options[2]),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(list.group)),
                before: Some(EntityRef::Handle(list.options[0])),
            },
        }],
    );
    rig.frame();
    rig.send(key(Up));
    assert_eq!(list.active(&mut rig), Some(list.options[2]));

    // Hiding the group ends its active item and frees the keys.
    apply(
        &mut rig.host,
        list.world,
        vec![Command::insert_value(
            EntityRef::Handle(list.group),
            behavior(true, false, true),
        )],
    );
    rig.frame();
    assert_eq!(list.active(&mut rig), None);
    assert_eq!(rig.send(key(Down)), GuiRoutingDisposition::Unhandled);
    assert!(rig.snapshot(list.world, list.field).focused);
    rig.finish();
}

#[test]
fn a_canvas_without_groups_publishes_no_group_and_leaves_arrows_unhandled() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 4.0, 4.0);
    let a = focusable(&mut host, world, root_entity, true);
    let b = focusable(&mut host, world, root_entity, true);
    let mut rig = Rig::new(host, root, viewport(100, 100));
    let publication = rig.host.latest_publication(world.id()).unwrap();
    let gui = rig
        .host
        .publication(publication)
        .unwrap()
        .chunk(crate::systems::canvas::CanvasSystem::ID)
        .and_then(|chunk| chunk.data::<crate::systems::gui::presentation::GuiCanvasPublication>())
        .unwrap();
    assert!(
        gui.views
            .values()
            .all(|view| view.controls.iter().all(|control| control.group.is_none()))
    );
    rig.send(key(Tab));
    for pressed in [Up, Down, Left, Right, Home, End] {
        assert_eq!(rig.send(key(pressed)), GuiRoutingDisposition::Unhandled);
    }
    assert!(rig.snapshot(world, a).focused);
    assert_eq!(selected(&mut rig, world, &[a, b]), [a, b]);
    rig.send(key(Tab));
    assert!(rig.snapshot(world, b).focused);
    rig.finish();
}

#[test]
fn a_control_inside_an_item_is_part_of_it_and_a_mixed_group_has_no_active_item() {
    let (mut rig, strip) = strip(GUI_GROUP_SELECT_SINGLE, Some(0));

    // A close mark on the second tab, a Button below it that does not take
    // focus, and a direct item beside the tabs that does not either.
    let close = option(&mut rig.host, strip.world, strip.items[1], false);
    let extra = option(&mut rig.host, strip.world, strip.group, false);
    rig.frame();
    for control in [close, extra] {
        let point = rig.point_in(control, [0.5, 0.5]);
        rig.send(movement(1, point));
        assert!(rig.snapshot(strip.world, control).painted.hovered);
        assert!(active(&mut rig, strip.world).is_empty());
    }

    // Pressing the close mark leaves focus and selection alone; its hover
    // ends with the pointer.
    rig.send(key(Tab));
    rig.send(key(Tab));
    let point = rig.point_in(close, [0.5, 0.5]);
    rig.send(press(1, point));
    rig.send(release(1, point));
    assert_eq!(applied(&rig, close, &GuiLocalEffectKind::Pressed), 1);
    assert!(rig.snapshot(strip.world, strip.items[0]).focused);
    assert_eq!(
        selected(&mut rig, strip.world, &strip.items),
        [strip.items[0]]
    );
    let away = rig.point_in(strip.after, [0.5, 0.5]);
    rig.send(movement(1, away));
    assert!(!rig.snapshot(strip.world, close).painted.hovered);

    // Arrows reach neither: the tabs are the group's focusable items.
    rig.send(key(End));
    assert!(rig.snapshot(strip.world, strip.items[2]).focused);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

/// Parts of `entity` whose skin transition is under way.
fn transitions(rig: &mut Rig, world: WorldRef, entity: EntityId) -> usize {
    rig.host
        .world_mut(world.id())
        .unwrap()
        .inspect(entity)
        .unwrap()
        .components
        .iter()
        .find_map(|component| match component {
            ComponentValue::GuiBehavior(behavior) => Some(behavior.motion.transitions()),
            _ => None,
        })
        .unwrap()
}

/// Transitions started in the latest frame.
fn started(rig: &mut Rig, world: WorldRef) -> usize {
    rig.host
        .world_mut(world.id())
        .unwrap()
        .gui_motion_work()
        .unwrap()
        .1
        .bindings
}

#[test]
fn a_key_moving_the_active_item_fades_it_like_a_hover_and_a_held_key_at_an_end_starts_nothing() {
    let (mut rig, list) = option_list(GUI_GROUP_SELECT_NONE, None);
    let [first, second, third] = [list.options[0], list.options[1], list.options[2]];
    rig.send(key(Tab));
    rig.send(key(Down));
    assert_eq!(list.active(&mut rig), Some(first));
    assert!(transitions(&mut rig, list.world, first) > 0);

    // Each move, 40 ms into the previous one's 80 ms hover fade, fades the
    // new active item in and the previous one back out from where it is.
    for (expected, previous) in [(second, first), (third, second)] {
        rig.host.frame(0.04).unwrap();
        rig.send(key(Down));
        assert_eq!(list.active(&mut rig), Some(expected));
        assert!(started(&mut rig, list.world) > 0);
        assert!(transitions(&mut rig, list.world, expected) > 0);
        assert!(transitions(&mut rig, list.world, previous) > 0);
    }

    // A repeated key at the end leaves the active item where it is, and its
    // transition under way is not restarted.
    for _ in 0..3 {
        rig.send(key(Down));
        assert_eq!(list.active(&mut rig), Some(third));
        assert_eq!(started(&mut rig, list.world), 0);
    }
    rig.finish();
}
