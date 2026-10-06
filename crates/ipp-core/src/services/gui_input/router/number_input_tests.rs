//! Numeric text inputs through routed input: edits that commit on Enter and
//! blur clamped to the range, rejected text that leaves the number, Escape
//! discarding an edit before it closes or blurs anything, Up and Down steps,
//! and step parts that step on press, repeat while held on the World's Host
//! clock, stop at their bound and take their own pointer feedback; and the
//! published report of an edit that ends without a commit.

use crate::components::{GuiButton, GuiTextInput};
use crate::services::gui_input::GuiDeliveryTerminal;
use crate::services::gui_input::router::*;
use crate::services::gui_input::routing_test_support::*;
use crate::services::gui_input::test_support::*;
use crate::systems::gui::local::controls::number::{
    GUI_NUMBER_REPEAT_DELAY, GUI_NUMBER_REPEAT_INTERVAL,
};
use crate::systems::gui::local::{
    GuiInteractionFlags, GuiLocalAction, GuiLocalEffectKind, GuiNativeTextState, GuiTextEdit,
};
use crate::systems::gui::observations::{
    GuiObservationClasses, GuiObservationCommand, GuiObservationEncoding, GuiObservationOutput,
    GuiObservationRecord,
};
use crate::{Command, ComponentValue, EntityId, EntityRef, WorldRef};

use GuiPhysicalKey::{Down, Enter, Escape, Tab, Up};

/// A number over -4..=4 at 1.25, stepping by a quarter and a fine twentieth,
/// shown with two decimals, with step parts.
fn exposure(value: f32) -> GuiTextInput {
    GuiTextInput {
        numeric: true,
        value,
        min: -4.0,
        max: 4.0,
        step: 0.25,
        fine_step: 0.05,
        precision: 2,
        step_parts: true,
        ..Default::default()
    }
}

/// A `10 x 2` numeric input, whose step parts are its `2 x 2` ends, and a
/// button after it in a `12 x 4` Canvas presented at ten pixels per unit.
struct Scene {
    rig: Rig,
    world: WorldRef,
    input: EntityId,
    button: EntityId,
}

impl Scene {
    fn new(input: GuiTextInput) -> Self {
        let (mut host, _) = host();
        let world = world(&mut host);
        let (root, root_entity) = canvas_root(&mut host, world, 12.0, 4.0);
        let entity = create(
            &mut host,
            world,
            vec![ComponentValue::GuiTextInput(input), sized(0, 10.0, 2.0)],
            Some(root_entity),
        );
        let button = create(
            &mut host,
            world,
            vec![
                ComponentValue::GuiButton(GuiButton::default()),
                sized(0, 1.0, 1.0),
            ],
            Some(root_entity),
        );
        Self {
            rig: Rig::new(host, root, viewport(120, 40)),
            world,
            input: entity,
            button,
        }
    }

    fn value(&mut self) -> f32 {
        match self.rig.value(self.world, self.input) {
            GuiTestValue::Number(value) => value,
            other => panic!("expected a number, got {other:?}"),
        }
    }

    /// The text being edited, while this context holds the native record.
    fn native(&mut self) -> Option<GuiNativeTextState> {
        self.rig.router.with_native_text(
            &mut self.rig.host,
            self.rig.context.as_ref().unwrap(),
            |state| state.cloned(),
        )
    }

    fn edited(&mut self) -> String {
        self.native().expect("an edit").text.to_string()
    }

    /// Replace the edit's text with `text` through native edits.
    fn type_text(&mut self, text: &str) {
        for edit in [GuiTextEdit::SelectAll, GuiTextEdit::Insert(text.into())] {
            let fence = self.native().expect("native text").fence;
            self.rig.send(GuiPhysicalInput::Text {
                fence,
                edit,
            });
        }
    }

    /// The viewport point at the centre of the decrement (0) or increment (1)
    /// part, or of the field between them.
    fn part(&mut self, index: usize) -> [f32; 2] {
        let x = [0.1, 0.9, 0.5][index];
        self.rig.point_in(self.input, [x, 0.5])
    }

    fn focused(&mut self) -> bool {
        self.rig.snapshot(self.world, self.input).focused
    }

    fn steps(&mut self) -> [GuiInteractionFlags; 2] {
        self.rig.snapshot(self.world, self.input).steps
    }

    /// The momentary effects routed to the input, in order.
    fn effects(&self) -> Vec<GuiLocalEffectKind> {
        terminals(&self.rig.ledger)
            .into_iter()
            .filter_map(|terminal| match terminal {
                GuiDeliveryTerminal::Applied(effect) if effect.target.entity == self.input => {
                    Some(effect.kind)
                }
                _ => None,
            })
            .filter(|kind| {
                matches!(
                    kind,
                    GuiLocalEffectKind::Submitted(_)
                        | GuiLocalEffectKind::Rejected(_)
                        | GuiLocalEffectKind::Discarded(_)
                )
            })
            .collect()
    }

    /// Subscribe to the World's application effects, as a client does.
    fn observe(&mut self) -> GuiObservationOutput {
        use crate::services::reliable_output::{OutputCharge, OutputLimits, ReliableOutputAccount};
        let output = GuiObservationOutput::new(
            ReliableOutputAccount::new(OutputLimits {
                bytes: 1 << 20,
                reply_reserve: 0,
            }),
            GuiObservationEncoding {
                control_bytes: 64,
                effect_bytes: 128,
                ancestry_entry_bytes: 16,
                text_byte_bytes: 1,
            },
        )
        .unwrap();
        let subscription = output
            .new_subscription(self.world, GuiObservationClasses::Application)
            .unwrap();
        let lease = output
            .account()
            .reserve(OutputCharge {
                entries: 1,
                bytes: 0,
            })
            .unwrap();
        let command =
            GuiObservationCommand::prepare_subscribe(&output, self.world, &subscription, 1, lease)
                .unwrap();
        self.rig
            .host
            .world_mut(self.world.id())
            .unwrap()
            .enqueue_system_command(crate::systems::gui::GuiSystem::ID, 44, command)
            .unwrap();
        self.rig.frame();
        assert!(subscription.is_active());
        std::mem::forget(subscription);
        published(&output);
        output
    }

    /// Complete a frame `dt` Host seconds after the last.
    fn advance(&mut self, dt: f64) {
        let report = self.rig.host.frame(dt).unwrap();
        assert!(report.worlds.values().all(Result::is_ok));
    }

    fn finish(self) {
        assert!(self.rig.rejected().is_empty(), "{:?}", self.rig.rejected());
        self.rig.finish();
    }
}

/// The numeric inputs' rejections and discards an output observed since it
/// was last read, in order.
fn published(output: &GuiObservationOutput) -> Vec<GuiLocalEffectKind> {
    let mut kinds = Vec::new();
    while let Some(delivery) = output.pop_front() {
        if let (
            GuiObservationRecord::Effect {
                effect,
                ..
            },
            _,
        ) = delivery.into_parts()
            && matches!(
                effect.kind,
                GuiLocalEffectKind::Rejected(_) | GuiLocalEffectKind::Discarded(_)
            )
        {
            kinds.push(effect.kind.clone());
        }
    }
    kinds
}

fn discarded(text: &str) -> GuiLocalEffectKind {
    GuiLocalEffectKind::Discarded(text.into())
}

fn submitted(text: &str) -> GuiLocalEffectKind {
    GuiLocalEffectKind::Submitted(text.into())
}

fn rejected(text: &str) -> GuiLocalEffectKind {
    GuiLocalEffectKind::Rejected(text.into())
}

#[test]
fn an_edit_commits_on_enter_clamped_to_the_range_and_shows_the_formatted_number() {
    let mut scene = Scene::new(exposure(1.25));
    scene.rig.send(key(Tab));
    assert_eq!(scene.edited(), "1.25");

    // The edit stays text until Enter commits it, clamped to the maximum,
    // not snapped to the step, and the field shows the formatted number.
    scene.type_text("9");
    assert_eq!((scene.edited().as_str(), scene.value()), ("9", 1.25));
    scene.rig.send(key(Enter));
    assert_eq!((scene.edited().as_str(), scene.value()), ("4.00", 4.0));

    assert_eq!(scene.effects(), [submitted("4.00")]);

    // The native buffer's own Enter commits too; its submission settles
    // with the native record rather than as a routed effect.
    scene.type_text(" -1.3 ");
    let fence = scene.native().unwrap().fence;
    scene.rig.send(GuiPhysicalInput::Text {
        fence,
        edit: GuiTextEdit::Submit,
    });
    assert_eq!((scene.edited().as_str(), scene.value()), ("-1.30", -1.3));
    scene.finish();
}

#[test]
fn text_that_does_not_parse_leaves_the_number_and_is_reported_on_enter_and_blur() {
    let mut scene = Scene::new(exposure(1.25));
    scene.rig.send(key(Tab));
    for text in ["abc", "1e3", "1,5", "--1", "."] {
        scene.type_text(text);
        scene.rig.send(key(Enter));
        assert_eq!((scene.edited().as_str(), scene.value()), (text, 1.25));
    }

    // Blur commits the edit, which is rejected again, and the edit ends.
    scene.rig.send(key(Tab));
    assert!(!scene.focused());
    assert_eq!(scene.value(), 1.25);
    let expected: Vec<_> = ["abc", "1e3", "1,5", "--1", ".", "."]
        .into_iter()
        .map(rejected)
        .collect();
    assert_eq!(scene.effects(), expected);

    // A valid edit commits on blur, without a submission.
    scene.rig.send(shifted(Tab));
    assert_eq!(scene.edited(), "1.25");
    scene.type_text("+2.5");
    scene.rig.send(key(Tab));
    assert_eq!(scene.value(), 2.5);
    assert_eq!(scene.effects().len(), expected.len());
    scene.finish();
}

#[test]
fn escape_discards_a_pending_edit_before_it_blurs() {
    let mut scene = Scene::new(exposure(1.25));
    scene.rig.send(key(Tab));
    scene.type_text("3");
    scene.rig.send(key(Escape));
    assert_eq!((scene.edited().as_str(), scene.value()), ("1.25", 1.25));
    assert!(scene.focused());

    // An edit and Escape routed before their World applies either still
    // discard the edit rather than blurring and committing it.
    let fence = scene.native().unwrap().fence;
    for input in [
        GuiPhysicalInput::Text {
            fence,
            edit: GuiTextEdit::Insert("7".into()),
        },
        key(Escape),
    ] {
        scene.rig.route(input).unwrap();
    }
    scene.rig.frame();
    assert_eq!((scene.edited().as_str(), scene.value()), ("1.25", 1.25));
    assert!(scene.focused());

    // Without an edit, Escape blurs. Each discard was reported.
    scene.rig.send(key(Escape));
    assert!(!scene.focused());
    assert_eq!(scene.value(), 1.25);
    assert_eq!(scene.effects(), [discarded("3"), discarded("1.257")]);
    scene.finish();
}

#[test]
fn up_and_down_step_after_committing_a_pending_edit_and_shift_takes_the_fine_step() {
    let mut scene = Scene::new(exposure(1.25));
    scene.rig.send(key(Tab));
    let mut values = Vec::new();
    for input in [key(Up), shifted(Down), key(Down)] {
        scene.rig.send(input);
        values.push(scene.value());
    }
    assert_eq!(values, [1.5, 1.45, 1.2]);
    assert_eq!(scene.edited(), "1.20");

    // A pending edit commits first and the step moves from it; a rejected
    // edit is reported and the step moves from the committed number.
    scene.type_text("3.9");
    scene.rig.send(key(Up));
    assert_eq!((scene.edited().as_str(), scene.value()), ("4.00", 4.0));
    scene.type_text("x");
    scene.rig.send(key(Down));
    assert_eq!((scene.edited().as_str(), scene.value()), ("3.75", 3.75));
    assert_eq!(scene.effects(), [rejected("x")]);
    scene.finish();
}

#[test]
fn a_press_on_a_step_part_steps_once_and_holding_it_repeats_on_the_host_clock() {
    let mut scene = Scene::new(exposure(1.25));
    let plus = scene.part(1);
    scene.rig.send(press(1, plus));
    assert_eq!(scene.value(), 1.5);
    assert!(scene.focused());
    assert!(scene.steps()[1].pressed);

    // Nothing more until the delay has run, then one step per interval;
    // a long frame makes every step it covered in one write.
    let mut values = Vec::new();
    for dt in [
        GUI_NUMBER_REPEAT_DELAY - 0.1,
        0.1,
        GUI_NUMBER_REPEAT_INTERVAL,
        GUI_NUMBER_REPEAT_INTERVAL * 0.5,
        GUI_NUMBER_REPEAT_INTERVAL * 2.4,
        0.0,
    ] {
        scene.advance(dt);
        values.push(scene.value());
    }
    assert_eq!(values, [1.5, 1.75, 2.0, 2.0, 2.5, 2.5]);
    assert_eq!(scene.edited(), "2.50");

    // Release stops it.
    scene.rig.send(release(1, plus));
    scene.advance(1.0);
    assert_eq!(scene.value(), 2.5);

    // So does a cancelled pointer.
    let minus = scene.part(0);
    scene.rig.send(press(2, minus));
    assert_eq!(scene.value(), 2.25);
    scene.rig.send(GuiPhysicalInput::PointerCancel {
        pointer: 2,
    });
    scene.advance(1.0);
    assert_eq!(scene.value(), 2.25);
    scene.finish();
}

#[test]
fn a_held_part_stops_at_its_bound_where_it_is_inert_while_the_other_part_steps() {
    let mut scene = Scene::new(exposure(3.5));
    let plus = scene.part(1);
    scene.rig.send(press(1, plus));
    for _ in 0..4 {
        scene.advance(GUI_NUMBER_REPEAT_DELAY);
    }
    assert_eq!(scene.value(), 4.0);
    scene.rig.send(release(1, plus));

    // At the bound the plus part does nothing, and the minus part steps.
    scene.rig.send(press(1, plus));
    scene.advance(GUI_NUMBER_REPEAT_DELAY);
    assert_eq!(scene.value(), 4.0);
    scene.rig.send(release(1, plus));
    let minus = scene.part(0);
    scene.rig.send(press(1, minus));
    scene.rig.send(release(1, minus));
    assert_eq!(scene.value(), 3.75);
    scene.finish();
}

#[test]
fn pointers_over_a_step_part_name_it_and_the_rest_of_the_field_names_none() {
    let mut scene = Scene::new(exposure(1.25));
    let none = GuiInteractionFlags::default();
    let hovered = GuiInteractionFlags {
        hovered: true,
        ..none
    };
    let mut seen = Vec::new();
    for index in [0, 2, 1] {
        let point = scene.part(index);
        scene.rig.send(movement(1, point));
        let read = scene.rig.snapshot(scene.world, scene.input);
        seen.push((read.steps, read.body));
    }
    assert_eq!(
        seen,
        [
            ([hovered, none], none),
            ([none, none], hovered),
            ([none, hovered], none),
        ]
    );

    scene.finish();
}

#[test]
fn a_client_write_of_the_number_replaces_the_edit_and_client_blur_commits_it() {
    let mut scene = Scene::new(exposure(1.25));
    scene.rig.send(key(Tab));
    scene.type_text("2");
    let target = scene.rig.snapshot(scene.world, scene.input).target;
    let set = |value: f32| Command::SetField {
        entity: EntityRef::Handle(target.entity),
        component: ComponentValue::GUI_TEXT_INPUT,
        field: crate::FieldWrite {
            offset: std::mem::offset_of!(GuiTextInput, value) as u32,
            value: crate::FieldValue::F32(value),
        },
    };
    apply(&mut scene.rig.host, scene.world, vec![set(-3.0)]);
    scene.rig.frame();
    assert_eq!((scene.edited().as_str(), scene.value()), ("-3.00", -3.0));

    // A write that leaves the formatted number as it is keeps the edit, and
    // a client's blur commits it.
    scene.type_text("0.5");
    apply(&mut scene.rig.host, scene.world, vec![set(-3.0)]);
    scene.rig.frame();
    assert_eq!(scene.edited(), "0.5");
    let action = |action| Command::GuiAction {
        target: crate::GuiActionTarget {
            entity: EntityRef::Handle(target.entity),
            component: target.component,
            incarnation: target.incarnation,
        },
        action,
    };
    apply(
        &mut scene.rig.host,
        scene.world,
        vec![action(GuiLocalAction::Blur)],
    );
    scene.rig.frame();
    assert_eq!(scene.value(), 0.5);
    assert!(!scene.focused());

    // Its semantic value is the number, bounded like a slider's.
    apply(
        &mut scene.rig.host,
        scene.world,
        vec![action(GuiLocalAction::SetScalar(-2.0))],
    );
    assert_eq!(scene.value(), -2.0);
    scene.finish();
}

#[test]
fn a_numeric_input_without_step_parts_is_a_number_field() {
    let mut scene = Scene::new(GuiTextInput {
        step_parts: false,
        ..exposure(1.25)
    });
    // Its ends are the field.
    let start = scene.part(0);
    scene.rig.send(movement(1, start));
    let read = scene.rig.snapshot(scene.world, scene.input);
    assert_eq!((read.steps, read.body.hovered), (Default::default(), true));
    scene.rig.send(key(Tab));
    scene.rig.send(key(Up));
    assert_eq!(scene.value(), 1.5);
    scene.rig.send(key(Tab));
    assert_eq!(
        scene
            .rig
            .focused(&[(scene.world, scene.input), (scene.world, scene.button)]),
        Some((scene.world, scene.button))
    );
    scene.finish();
}

#[test]
fn an_edit_that_ends_without_a_commit_is_reported_discarded() {
    let mut scene = Scene::new(exposure(1.25));
    let output = scene.observe();
    scene.rig.send(key(Tab));

    // Escape after a rejection discards the bad text and reports both.
    scene.type_text("abc");
    scene.rig.send(key(Enter));
    scene.rig.send(key(Escape));
    assert_eq!(scene.edited(), "1.25");
    assert_eq!(published(&output), [rejected("abc"), discarded("abc")]);
    assert_eq!(scene.effects(), [rejected("abc"), discarded("abc")]);

    // Escape without an edit, which blurs, and a blur that commits report
    // no discard.
    scene.rig.send(key(Escape));
    scene.rig.send(key(Tab));
    scene.type_text("2");
    scene.rig.send(key(Tab));
    assert_eq!(scene.value(), 2.0);
    assert_eq!(published(&output), []);

    // Disabling or hiding the control ends a pending edit: discarded.
    let target = scene.rig.snapshot(scene.world, scene.input).target;
    let behavior = |field: usize, value: bool| Command::SetField {
        entity: EntityRef::Handle(target.entity),
        component: ComponentValue::GUI_BEHAVIOR,
        field: crate::FieldWrite {
            offset: field as u32,
            value: crate::FieldValue::Bool(value),
        },
    };
    for field in [
        std::mem::offset_of!(crate::components::GuiBehavior, enabled),
        std::mem::offset_of!(crate::components::GuiBehavior, visible),
    ] {
        scene.rig.send(key(Tab));
        scene.type_text("3");
        apply(
            &mut scene.rig.host,
            scene.world,
            vec![behavior(field, false)],
        );
        scene.rig.frame();
        scene.rig.synchronize();
        assert!(!scene.focused());
        assert_eq!(scene.value(), 2.0);
        assert_eq!(published(&output), [discarded("3")]);
        apply(
            &mut scene.rig.host,
            scene.world,
            vec![behavior(field, true)],
        );
        scene.rig.frame();
    }

    // So does a client write that replaces the shown number.
    scene.rig.send(key(Tab));
    scene.type_text("4");
    let value = Command::SetField {
        entity: EntityRef::Handle(target.entity),
        component: ComponentValue::GUI_TEXT_INPUT,
        field: crate::FieldWrite {
            offset: std::mem::offset_of!(GuiTextInput, value) as u32,
            value: crate::FieldValue::F32(-1.0),
        },
    };
    apply(&mut scene.rig.host, scene.world, vec![value]);
    scene.rig.frame();
    assert_eq!(scene.edited(), "-1.00");
    assert_eq!(published(&output), [discarded("4")]);

    // And the end of the input session holding the edit.
    scene.type_text("5");
    let context = scene.rig.context.take().unwrap();
    scene.rig.router.release(&mut scene.rig.host, context);
    scene.rig.frame();
    assert_eq!(scene.value(), -1.0);
    assert_eq!(published(&output), [discarded("5")]);
    scene.rig.rebind();
    scene.finish();
}

#[test]
fn a_plain_text_input_reports_no_discard() {
    let mut scene = Scene::new(GuiTextInput {
        text: "ab".into(),
        ..Default::default()
    });
    let output = scene.observe();
    scene.rig.send(key(Tab));
    let fence = scene.native().unwrap().fence;
    scene.rig.send(GuiPhysicalInput::Text {
        fence,
        edit: GuiTextEdit::Insert("c".into()),
    });
    scene.rig.send(key(Escape));
    assert!(!scene.focused());
    let target = scene.rig.snapshot(scene.world, scene.input).target;
    assert_eq!(
        scene.rig.value(scene.world, target.entity),
        GuiTestValue::Text("abc".into())
    );
    assert_eq!(published(&output), []);
    scene.finish();
}
