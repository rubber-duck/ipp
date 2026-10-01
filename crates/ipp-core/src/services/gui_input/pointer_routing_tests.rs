//! Ported legacy pointer routing: slider mapping through published rails,
//! hover, current published geometry, tap-on-release, keys without focus,
//! and focus lifecycle when a focused or captured control changes.

use super::router_test_support::*;
use super::*;
use crate::components::{GuiBehavior, GuiCheckbox, GuiSlider};
use crate::services::gui_input::router::*;
use crate::systems::gui::local::slider::slider_rail;

/// A single-World Canvas root of `size` logical units presented at ten
/// pixels per unit, holding `children` as a column in order.
fn canvas_scene(size: [f32; 2], children: Vec<Vec<ComponentValue>>) -> (Rig, Vec<EntityId>) {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, size[0], size[1]);
    let entities = children
        .into_iter()
        .map(|values| create(&mut host, world, values, Some(root_entity)))
        .collect();
    let rig = Rig::new(
        host,
        root,
        viewport((size[0] * 10.0) as u32, (size[1] * 10.0) as u32),
    );
    (rig, entities)
}

fn slider(min: f32, max: f32, step: f32, value: f32) -> ComponentValue {
    ComponentValue::GuiSlider(GuiSlider {
        value,
        min,
        max,
        step,
    })
}

/// Painted thumb centre of a slider laid out at `rect`, at a value fraction.
fn thumb_centre(rect: [f32; 4], fraction: f32) -> [f32; 2] {
    let thumb = slider_rail(rect).unwrap().thumb_rect(fraction).unwrap();
    [thumb[0] + thumb[2] * 0.5, thumb[1] + thumb[3] * 0.5]
}

fn scalar(value: GuiTestValue) -> f32 {
    match value {
        GuiTestValue::Scalar(value) => value,
        other => panic!("expected a slider value, got {other:?}"),
    }
}

#[test]
fn slider_drag_commits_quantized_values_in_order() {
    let (mut rig, entities) = canvas_scene(
        [10.0, 2.0],
        vec![vec![slider(0.0, 1.0, 0.25, 0.0), sized(0, 10.0, 1.0)]],
    );
    let (world, slider) = (rig.root.world(), entities[0]);
    let rect = rig.bounds(world, slider);
    let grab = rig.logical(thumb_centre(rect, 0.0));
    rig.route(press(1, grab)).unwrap();
    // Several moves route against one completed frame; each off-step point
    // quantizes, and the writes land in routing order: each changes the value
    // and the last one stays.
    for fraction in [0.3, 0.55, 0.8] {
        let point = rig.logical(thumb_centre(rect, fraction));
        rig.route(movement(1, point)).unwrap();
    }
    rig.frame();
    rig.send(release(1, rig.logical(thumb_centre(rect, 0.8))));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    assert_eq!(rig.commits(), [(slider, true); 3]);
    assert_eq!(scalar(rig.value(world, slider)), 0.75);
    rig.finish();
}

#[test]
fn slider_press_at_the_painted_centre_keeps_an_off_step_value() {
    let (mut rig, entities) = canvas_scene(
        [10.0, 2.0],
        vec![vec![slider(0.0, 10.0, 2.0, 5.0), sized(0, 10.0, 1.0)]],
    );
    let (world, slider) = (rig.root.world(), entities[0]);
    let before = rig.snapshot(world, slider).value;
    assert_eq!(before, GuiTestValue::Scalar(5.0));
    let rect = rig.bounds(world, slider);
    let centre = rig.logical(thumb_centre(rect, 0.5));
    // Grabbing the painted thumb and releasing in place keeps 5, which no
    // step reaches; even a zero-length move maps back to the same value.
    rig.send(press(1, centre));
    rig.send(movement(1, centre));
    rig.send(release(1, centre));
    assert_eq!(scalar(rig.value(world, slider)), 5.0);
    assert!(rig.commits().iter().all(|(_, changed)| !changed));

    // A later drag from the same grab moves by whole steps.
    rig.send(press(2, centre));
    rig.send(movement(2, rig.logical(thumb_centre(rect, 0.8))));
    rig.send(release(2, rig.logical(thumb_centre(rect, 0.8))));
    assert_eq!(scalar(rig.value(world, slider)), 8.0);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn captured_slider_moves_clamp_beyond_the_rail_and_quantize_inside_it() {
    // A 10-unit slider in a 20-unit Canvas: captured motion keeps mapping
    // through the rail after the pointer leaves the control or the viewport.
    let (mut rig, entities) = canvas_scene(
        [20.0, 2.0],
        vec![vec![slider(0.0, 10.0, 2.0, 0.0), sized(0, 10.0, 1.0)]],
    );
    let (world, slider) = (rig.root.world(), entities[0]);
    let rect = rig.bounds(world, slider);
    let track = rig.logical(thumb_centre(rect, 0.36));
    // A track press jumps to the nearest step: 3.6 quantizes to 4.
    rig.send(press(1, track));
    assert_eq!(scalar(rig.value(world, slider)), 4.0);
    rig.send(movement(1, rig.logical([19.0, rect[1] + 0.5])));
    assert_eq!(scalar(rig.value(world, slider)), 10.0);
    rig.send(movement(1, [-0.5, rig.logical([0.0, rect[1] + 0.5])[1]]));
    assert_eq!(scalar(rig.value(world, slider)), 0.0);
    rig.send(movement(1, rig.logical(thumb_centre(rect, 0.52))));
    assert_eq!(scalar(rig.value(world, slider)), 6.0);
    // A non-finite captured point commits nothing.
    assert!(rig.route(movement(1, [f32::NAN, 0.25])).is_err());
    rig.frame();
    assert_eq!(scalar(rig.value(world, slider)), 6.0);
    rig.finish();
}

#[test]
fn slider_keys_step_and_clamp_at_both_ends() {
    let (mut rig, entities) = canvas_scene(
        [10.0, 2.0],
        vec![vec![slider(0.0, 10.0, 2.0, 0.0), sized(0, 10.0, 1.0)]],
    );
    let (world, slider) = (rig.root.world(), entities[0]);
    rig.send(key(GuiPhysicalKey::Tab));
    let mut values = Vec::new();
    for pressed in [
        GuiPhysicalKey::Left,
        GuiPhysicalKey::Right,
        GuiPhysicalKey::Up,
        GuiPhysicalKey::End,
        GuiPhysicalKey::Right,
        GuiPhysicalKey::Down,
        GuiPhysicalKey::Home,
    ] {
        rig.send(key(pressed));
        values.push(scalar(rig.value(world, slider)));
    }
    assert_eq!(values, [0.0, 2.0, 4.0, 10.0, 10.0, 8.0, 0.0]);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn slider_painted_centres_round_trip_on_narrow_and_off_step_rails() {
    // Narrow rails shrink the thumb below the control height; values off the
    // step grid still map back to themselves at their painted centre.
    for width in [0.5, 1.0] {
        for value in [-3.0, 1.0, 8.0] {
            let (mut rig, entities) = canvas_scene(
                [2.0, 2.0],
                vec![vec![slider(-3.0, 8.0, 2.5, value), sized(0, width, 1.0)]],
            );
            let (world, slider) = (rig.root.world(), entities[0]);
            let rect = rig.bounds(world, slider);
            assert_eq!(rect[2], width);
            let centre = rig.logical(thumb_centre(rect, (value + 3.0) / 11.0));
            rig.send(press(1, centre));
            rig.send(movement(1, centre));
            rig.send(release(1, centre));
            assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
            assert_eq!(
                rig.value(world, slider),
                GuiTestValue::Scalar(value),
                "width {width}"
            );
            assert!(
                rig.commits().iter().all(|(entity, _)| *entity == slider),
                "width {width}: {:?}",
                rig.commits()
            );
            rig.finish();
        }
    }
}

#[test]
fn hover_follows_uncaptured_moves_and_clears_on_a_miss_without_mutation() {
    let (mut rig, entities) = canvas_scene(
        [10.0, 10.0],
        vec![vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            sized(0, 4.0, 4.0),
        ]],
    );
    let (world, checkbox) = (rig.root.world(), entities[0]);
    let value = rig.snapshot(world, checkbox).value;
    let over = rig.point_in(checkbox, [0.5, 0.5]);
    rig.send(movement(4, over));
    let hovered = rig.snapshot(world, checkbox);
    assert!(hovered.interaction.hovered);
    assert!(!hovered.interaction.pressed && !hovered.interaction.captured);
    // Moving over empty Canvas content clears the hover without capturing.
    assert_eq!(
        rig.send(movement(4, rig.logical([9.5, 9.5]))),
        GuiRoutingDisposition::Unhandled
    );
    let cleared = rig.snapshot(world, checkbox);
    assert!(!cleared.interaction.hovered && !cleared.interaction.pressed);
    assert_eq!(cleared.value, value);
    assert!(rig.commits().is_empty());
    rig.finish();
}

fn two_checkboxes() -> (Rig, Vec<EntityId>) {
    let checkbox = || {
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            sized(0, 4.0, 2.0),
        ]
    };
    canvas_scene([10.0, 10.0], vec![checkbox(), checkbox()])
}

#[test]
fn a_reordered_checkbox_routes_against_the_published_order() {
    let (mut rig, entities) = two_checkboxes();
    let (world, first, second) = (rig.root.world(), entities[0], entities[1]);
    let root = rig.root_entity();
    let old_second = rig.point_in(second, [0.5, 0.5]);
    // Move the second checkbox before the first, then tap where the second
    // used to be: the published order puts the first there now.
    apply(
        &mut rig.host,
        world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(second),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(root)),
                before: Some(EntityRef::Handle(first)),
            },
        }],
    );
    rig.frame();
    assert_ne!(
        rig.point_in(first, [0.5, 0.5]),
        rig.point_in(first, [0.5, 0.0])
    );
    rig.send(press(1, old_second));
    rig.send(release(1, old_second));
    assert_eq!(rig.commits(), [(first, true)]);
    assert_eq!(rig.value(world, second), GuiTestValue::Bool(false));
    rig.finish();
}

#[test]
fn a_resized_checkbox_routes_against_the_published_bounds() {
    let (mut rig, entities) = two_checkboxes();
    let (world, first) = (rig.root.world(), entities[0]);
    let beyond = rig.point_in(first, [1.1, 0.5]);
    // Just past the old right edge the tap misses.
    assert_eq!(rig.send(press(1, beyond)), GuiRoutingDisposition::Miss);
    apply(
        &mut rig.host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(first),
            sized(0, 6.0, 2.0),
        )],
    );
    rig.frame();
    rig.send(press(2, beyond));
    rig.send(release(2, beyond));
    assert_eq!(rig.commits(), [(first, true)]);
    rig.finish();
}

#[test]
fn checkbox_tap_commits_on_an_in_bounds_release_only() {
    let (mut rig, entities) = two_checkboxes();
    let (world, checkbox) = (rig.root.world(), entities[0]);
    let on = rig.point_in(checkbox, [0.5, 0.5]);
    let value = rig.snapshot(world, checkbox).value;
    assert_eq!(value, GuiTestValue::Bool(false));
    // A held press changes nothing across frames while it stays observable.
    rig.send(press(1, on));
    rig.frame();
    rig.frame();
    let held = rig.snapshot(world, checkbox);
    assert!(held.interaction.pressed && held.interaction.captured);
    assert_eq!(held.value, value);
    // Releasing outside the control completes nothing.
    rig.send(release(1, rig.logical([9.5, 9.5])));
    assert!(!rig.snapshot(world, checkbox).interaction.pressed);
    // A cancelled press discards the activation; its later release finds no
    // capture.
    rig.route(press(2, on)).unwrap();
    assert_eq!(
        rig.route(GuiPhysicalInput::PointerCancel {
            pointer: 2
        }),
        Ok(GuiRoutingDisposition::Unhandled)
    );
    assert_eq!(
        rig.route(release(2, on)),
        Ok(GuiRoutingDisposition::Unhandled)
    );
    rig.frame();
    assert!(rig.commits().is_empty());
    assert_eq!(rig.snapshot(world, checkbox).value, value);
    // An in-bounds tap writes the value exactly once.
    rig.send(press(3, on));
    rig.send(release(3, on));
    assert_eq!(rig.commits(), [(checkbox, true)]);
    assert_eq!(
        rig.snapshot(world, checkbox).value,
        GuiTestValue::Bool(true)
    );
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn keys_without_focus_are_unhandled() {
    let (mut rig, _) = two_checkboxes();
    for pressed in [
        GuiPhysicalKey::Enter,
        GuiPhysicalKey::Space,
        GuiPhysicalKey::Right,
        GuiPhysicalKey::Escape,
    ] {
        assert_eq!(
            rig.route(key(pressed)),
            Ok(GuiRoutingDisposition::Unhandled)
        );
    }
    rig.frame();
    assert!(terminals(&rig.ledger).is_empty());
    rig.finish();
}

/// A Canvas root holding a text input above a button.
fn text_scene() -> (Rig, EntityId, EntityId) {
    let (rig, entities) = canvas_scene(
        [10.0, 10.0],
        vec![
            vec![
                ComponentValue::GuiTextInput(crate::components::GuiTextInput {
                    text: "ae".into(),
                    ..Default::default()
                }),
                sized(0, 10.0, 3.0),
            ],
            vec![
                ComponentValue::GuiButton(GuiButton::default()),
                sized(0, 4.0, 2.0),
            ],
        ],
    );
    (rig, entities[0], entities[1])
}

/// Focus the text input by keyboard and open a native composition.
fn focus_composing(rig: &mut Rig, text: EntityId) {
    use crate::systems::gui::local::{GuiTextComposition, GuiTextEdit};
    rig.send(key(GuiPhysicalKey::Tab));
    let fence =
        rig.router
            .with_native_text(&mut rig.host, rig.context.as_ref().unwrap(), |state| {
                state.unwrap().fence
            });
    assert_eq!(fence.target.entity, text);
    rig.send(GuiPhysicalInput::Text {
        fence,
        edit: GuiTextEdit::Compose(GuiTextComposition {
            text: "a".into(),
            selection: [1, 1],
        }),
    });
    let composing =
        rig.router
            .with_native_text(&mut rig.host, rig.context.as_ref().unwrap(), |state| {
                state.unwrap().composition.is_some()
            });
    assert!(composing);
}

fn native_focus(rig: &mut Rig) -> bool {
    rig.router
        .with_native_text(&mut rig.host, rig.context.as_ref().unwrap(), |state| {
            state.is_some()
        })
}

#[test]
fn disabling_or_hiding_focused_text_clears_focus_composition_and_native_focus() {
    for visible in [true, false] {
        let (mut rig, text, _) = text_scene();
        let world = rig.root.world();
        focus_composing(&mut rig, text);
        apply(
            &mut rig.host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(text),
                ComponentValue::GuiBehavior(GuiBehavior {
                    enabled: !visible,
                    visible,
                    ..Default::default()
                }),
            )],
        );
        rig.frame();
        let cancelled = rig.synchronize();
        assert!(cancelled.focus, "native focus must end, visible {visible}");
        assert!(!native_focus(&mut rig));
        assert!(!rig.snapshot(world, text).focused);
        // No composition survives into a later focus of the same control.
        apply(
            &mut rig.host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(text),
                ComponentValue::GuiBehavior(GuiBehavior::default()),
            )],
        );
        rig.frame();
        rig.send(key(GuiPhysicalKey::Tab));
        let composition =
            rig.router
                .with_native_text(&mut rig.host, rig.context.as_ref().unwrap(), |state| {
                    state.map(|state| state.composition.is_some())
                });
        assert_eq!(composition, Some(false));
        rig.finish();
    }
}

#[test]
fn a_captured_control_removed_mid_drag_reports_unavailable_and_commits_nothing() {
    let (mut rig, entities) = canvas_scene(
        [10.0, 2.0],
        vec![vec![slider(0.0, 10.0, 0.0, 0.0), sized(0, 10.0, 1.0)]],
    );
    let (world, slider) = (rig.root.world(), entities[0]);
    let (start, dragged) = (
        rig.point_in(slider, [0.3, 0.5]),
        rig.point_in(slider, [0.5, 0.5]),
    );
    rig.send(press(1, start));
    rig.send(movement(1, dragged));
    let commits = rig.commits().len();
    assert!(commits > 0);
    apply(
        &mut rig.host,
        world,
        vec![Command::Delete {
            entity: EntityRef::Handle(slider),
        }],
    );
    rig.frame();
    // Input for the removed capture target is reported unavailable, both for
    // continued motion and for the release, until the routing boundary
    // cancels the capture; a later release then finds none.
    assert_eq!(
        rig.route(movement(1, [0.7, 0.5])),
        Err(GuiInputError::Unavailable)
    );
    assert_eq!(
        rig.route(release(1, [0.7, 0.5])),
        Err(GuiInputError::Unavailable)
    );
    assert_eq!(rig.synchronize().pointers(), [1]);
    assert_eq!(
        rig.route(release(1, [0.7, 0.5])),
        Ok(GuiRoutingDisposition::Unhandled)
    );
    rig.frame();
    assert_eq!(rig.commits().len(), commits);
    rig.finish();
}

/// Minimal font: a fallback glyph plus `A`, one em per logical unit.
fn font_bytes() -> Vec<u8> {
    let mut bytes = b"IPPF".to_vec();
    for value in [1_u32, 1000] {
        bytes.extend(value.to_le_bytes());
    }
    for value in [800.0_f32, -200.0, 200.0] {
        bytes.extend(value.to_le_bytes());
    }
    for value in [2_u32, 1, 0] {
        bytes.extend(value.to_le_bytes());
    }
    for advance in [500.0_f32, 600.0] {
        bytes.extend(advance.to_le_bytes());
        for _ in 0..5 {
            bytes.extend(0.0_f32.to_le_bytes());
        }
        bytes.extend(0_u32.to_le_bytes());
    }
    for value in [u32::from('A'), 1] {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}

/// Whether the root Canvas paints any glyph run.
fn glyphs(rig: &Rig) -> bool {
    use crate::systems::canvas::{CanvasPaintEntry, CanvasPrimitive};
    rig.canvas(rig.root).entries.iter().any(|entry| {
        matches!(
            entry.as_ref(),
            CanvasPaintEntry::Primitive {
                primitive: CanvasPrimitive::Glyphs { .. },
                ..
            }
        )
    })
}

#[test]
fn unloading_the_font_of_a_focused_text_input_keeps_its_focus() {
    use crate::services::asset_management::AssetSource;
    use crate::services::asset_management::font::FONT_TYPE;
    const SOURCE: &str = "gui-font:///body.ippf";
    let (mut host, _) = host();
    host.register_stream_resource_provider("gui-font").unwrap();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(root_entity),
            ComponentValue::GuiFont(crate::systems::gui::presentation::GuiFont {
                source: SOURCE.into(),
                variant: 0,
                font_size: 1.0,
            }),
        )],
    );
    let text = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiTextInput(crate::components::GuiTextInput {
                text: "AA".into(),
                ..Default::default()
            }),
            sized(0, 10.0, 3.0),
        ],
        Some(root_entity),
    );
    let button = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiButton(GuiButton::default()),
            sized(0, 4.0, 2.0),
        ],
        Some(root_entity),
    );
    let mut rig = Rig::new(host, root, viewport(100, 100));
    let mut requests = Vec::new();
    for _ in 0..8 {
        requests.extend(rig.host.take_resource_requests());
        if !requests.is_empty() {
            break;
        }
        rig.frame();
    }
    assert_eq!(requests.len(), 1);
    rig.host
        .complete_resource(requests[0].id, Ok(font_bytes()))
        .unwrap();
    rig.frame();
    rig.frame();
    assert!(glyphs(&rig));
    rig.send(key(GuiPhysicalKey::Tab));
    assert!(native_focus(&mut rig));
    assert!(rig.snapshot(world, text).focused);

    // Losing the font drops glyph work only: logical focus, native focus and
    // the control's availability stay, and traversal continues normally.
    let font = rig
        .host
        .asset_resources()
        .find(&AssetSource {
            kind: FONT_TYPE,
            uri: SOURCE.into(),
            variant: 0,
        })
        .expect("loaded font");
    rig.host.asset_resources_mut().unload(font);
    rig.frame();
    rig.frame();
    assert!(!glyphs(&rig));
    let cancelled = rig.synchronize();
    assert!(cancelled.is_empty());
    assert!(native_focus(&mut rig));
    let snapshot = rig.snapshot(world, text);
    assert!(snapshot.focused && snapshot.available);
    rig.send(key(GuiPhysicalKey::Tab));
    assert_eq!(
        rig.focused(&[(world, text), (world, button)]),
        Some((world, button))
    );
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn activation_keys_press_a_focused_button_and_space_toggles_a_focused_checkbox() {
    let (mut rig, entities) = canvas_scene(
        [10.0, 10.0],
        vec![
            vec![
                ComponentValue::GuiCheckbox(GuiCheckbox::default()),
                sized(0, 4.0, 2.0),
            ],
            vec![
                ComponentValue::GuiButton(GuiButton::default()),
                sized(0, 4.0, 2.0),
            ],
        ],
    );
    let (world, checkbox, button) = (rig.root.world(), entities[0], entities[1]);
    let pressed = |rig: &Rig| {
        terminals(&rig.ledger)
            .iter()
            .filter(|terminal| {
                matches!(
                    terminal,
                    GuiDeliveryTerminal::Applied(GuiLocalEffect {
                        kind: GuiLocalEffectKind::Pressed,
                        ..
                    })
                )
            })
            .count()
    };
    rig.send(key(GuiPhysicalKey::Tab));
    // Enter does not toggle a checkbox; Space does.
    assert_eq!(
        rig.send(key(GuiPhysicalKey::Enter)),
        GuiRoutingDisposition::Unhandled
    );
    assert_eq!(rig.value(world, checkbox), GuiTestValue::Bool(false));
    rig.send(key(GuiPhysicalKey::Space));
    assert_eq!(rig.value(world, checkbox), GuiTestValue::Bool(true));
    // Enter and Space each press a focused button without a value commit.
    rig.send(key(GuiPhysicalKey::Tab));
    rig.send(key(GuiPhysicalKey::Enter));
    rig.send(key(GuiPhysicalKey::Space));
    assert_eq!(pressed(&rig), 2);
    assert_eq!(rig.commits(), [(checkbox, true)]);
    assert!(rig.snapshot(world, button).focused);
    rig.finish();
}

#[test]
fn a_second_pointer_cannot_take_a_held_control() {
    let (mut rig, entities) = two_checkboxes();
    let (world, checkbox) = (rig.root.world(), entities[0]);
    let on = rig.point_in(checkbox, [0.5, 0.5]);
    rig.send(press(1, on));
    assert_eq!(rig.send(press(2, on)), GuiRoutingDisposition::Blocked);
    assert_eq!(
        rig.route(release(2, on)),
        Ok(GuiRoutingDisposition::Unhandled)
    );
    rig.frame();
    // The first pointer still owns the press and completes the tap once.
    assert!(rig.snapshot(world, checkbox).interaction.captured);
    rig.send(release(1, on));
    assert_eq!(rig.commits(), [(checkbox, true)]);
    rig.finish();
}

/// Commit `value` to a slider through a `GuiAction` command from another
/// client, as the gallery's `galleryGuiAction` does.
fn external_scalar(rig: &mut Rig, world: WorldRef, slider: EntityId, value: f32) {
    let snapshot = rig.snapshot(world, slider);
    super::gui_action(
        &mut rig.host,
        snapshot.target,
        crate::systems::gui::local::GuiLocalAction::SetScalar(value),
    );
    rig.frame();
    assert_eq!(scalar(rig.value(world, slider)), value);
}

/// Press, move through `fractions` of the rail and release, one frame each.
fn slider_drag(rig: &mut Rig, slider: EntityId, fractions: &[f32]) {
    let points: Vec<_> = fractions
        .iter()
        .map(|&fraction| rig.point_in(slider, [fraction, 0.5]))
        .collect();
    rig.send(press(1, points[0]));
    for &point in &points[1..] {
        rig.send(movement(1, point));
    }
    rig.send(release(1, *points.last().unwrap()));
}

#[test]
fn a_new_slider_drag_starts_from_the_published_revision_after_an_external_commit() {
    // NEW-5: the slider stays focused after a drag. The next drag must start
    // from the committed revision in the current publication, not chain on
    // the finished drag's receipt that an external commit has superseded.
    let (mut rig, entities) = canvas_scene(
        [10.0, 2.0],
        vec![vec![slider(0.0, 1.0, 0.0, 0.5), sized(0, 10.0, 1.0)]],
    );
    let (world, slider) = (rig.root.world(), entities[0]);
    slider_drag(&mut rig, slider, &[0.25, 0.5, 0.75]);
    assert!(rig.snapshot(world, slider).focused);
    let dragged = scalar(rig.value(world, slider));
    assert!(dragged > 0.6, "{dragged}");
    external_scalar(&mut rig, world, slider, 0.1);
    slider_drag(&mut rig, slider, &[0.25, 0.5, 0.75]);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    assert!(
        !terminals(&rig.ledger)
            .iter()
            .any(|terminal| matches!(terminal, GuiDeliveryTerminal::Cancelled))
    );
    assert_eq!(scalar(rig.value(world, slider)), dragged);
    rig.finish();
}

#[test]
fn a_sustained_slider_drag_keeps_chaining_across_frames_after_an_earlier_drag() {
    // Within one gesture, pipelined moves still chain on their predecessor
    // receipts, including after a previous gesture on the same slider.
    let (mut rig, entities) = canvas_scene(
        [10.0, 2.0],
        vec![vec![slider(0.0, 100.0, 1.0, 0.0), sized(0, 10.0, 1.0)]],
    );
    let (world, slider) = (rig.root.world(), entities[0]);
    slider_drag(&mut rig, slider, &[0.1, 0.3]);
    let start = rig.point_in(slider, [0.1, 0.5]);
    rig.send(press(1, start));
    let mut last = start;
    for frame in 0..6 {
        for step in 0..3 {
            last = rig.point_in(slider, [0.1 + 0.04 * (frame * 3 + step) as f32, 0.5]);
            rig.route(movement(1, last)).unwrap();
        }
        rig.frame();
    }
    let dragged = scalar(rig.value(world, slider));
    rig.send(release(1, last));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    assert!(dragged > 60.0, "{dragged}");
    assert_eq!(scalar(rig.value(world, slider)), dragged);
    rig.finish();
}

#[test]
fn a_key_step_after_an_external_commit_starts_from_the_published_revision() {
    // The slider keeps focus and its settled key receipt; an external commit
    // supersedes that receipt, so the next key starts from the publication.
    let (mut rig, entities) = canvas_scene(
        [10.0, 2.0],
        vec![vec![slider(0.0, 10.0, 1.0, 0.0), sized(0, 10.0, 1.0)]],
    );
    let (world, slider) = (rig.root.world(), entities[0]);
    rig.send(key(GuiPhysicalKey::Tab));
    rig.send(key(GuiPhysicalKey::Right));
    assert_eq!(scalar(rig.value(world, slider)), 1.0);
    external_scalar(&mut rig, world, slider, 5.0);
    rig.send(key(GuiPhysicalKey::Right));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    assert_eq!(scalar(rig.value(world, slider)), 6.0);
    rig.finish();
}

#[test]
fn repeated_keys_within_one_run_still_chain_on_pending_receipts() {
    let (mut rig, entities) = canvas_scene(
        [10.0, 2.0],
        vec![vec![slider(0.0, 10.0, 1.0, 0.0), sized(0, 10.0, 1.0)]],
    );
    let (world, slider) = (rig.root.world(), entities[0]);
    rig.send(key(GuiPhysicalKey::Tab));
    rig.send(key(GuiPhysicalKey::Right));
    external_scalar(&mut rig, world, slider, 5.0);
    // A held key: several steps route against one completed publication and
    // each chains on its still-pending predecessor.
    for _ in 0..3 {
        rig.route(key(GuiPhysicalKey::Right)).unwrap();
    }
    rig.frame();
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    assert_eq!(scalar(rig.value(world, slider)), 8.0);
    rig.finish();
}

#[test]
fn a_hover_burst_between_frames_ends_on_the_last_control() {
    let (mut rig, entities) = two_checkboxes();
    let (world, first, second) = (rig.root.world(), entities[0], entities[1]);
    let points = [
        rig.point_in(first, [0.5, 0.5]),
        rig.point_in(second, [0.5, 0.5]),
    ];
    // More moves than the World queue holds Hover and Cancel pairs for, all
    // before the next mutation boundary: each superseded hover gives way
    // without queueing a Cancel, so routing never runs out of queue.
    for index in 0..41 {
        assert!(matches!(
            rig.route(movement(91, points[index % 2])),
            Ok(GuiRoutingDisposition::Routed { .. })
        ));
    }
    rig.frame();
    assert!(!rig.snapshot(world, second).interaction.hovered);
    assert!(rig.snapshot(world, first).interaction.hovered);
    // The superseded hovers settle cancelled without lighting their control.
    assert_eq!(rig.rejected(), [GuiInputError::Cancelled; 40]);
    // An applied hover still ends with an ordinary Cancel.
    rig.send(movement(91, points[1]));
    assert!(!rig.snapshot(world, first).interaction.hovered);
    assert!(rig.snapshot(world, second).interaction.hovered);
    rig.finish();
}
