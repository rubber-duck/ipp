pub(super) use super::input_test_support::GuiTestHost;
use super::input_test_support::{self, GuiTestOutcome};
use super::*;
use crate::services::gui_input::{GuiDeliveryTerminal, GuiInputError};
use crate::systems::gui::GuiSystem;
pub(super) use crate::systems::gui::test_support::{GuiControlRead, GuiTestValue};
use crate::{
    Batch, BatchOutcome, Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef,
    ErrorReason, FieldValue, FieldWrite, HostRuntime, WorldId,
};

/// The Systems a headless GUI World selects: Canvas owns the `CanvasBounds`
/// every control requires.
pub(super) const GUI_SYSTEMS: [crate::systems::SystemId; 2] =
    [crate::systems::canvas::CanvasSystem::ID, GuiSystem::ID];

/// Presented panels also lay out their controls, which gives them hit bounds.
pub(super) const PRESENTED_GUI_SYSTEMS: [crate::systems::SystemId; 3] = [
    crate::systems::canvas::CanvasSystem::ID,
    GuiSystem::ID,
    crate::systems::gui::GuiLayoutSystem::ID,
];

pub(super) fn fixture() -> (GuiTestHost, WorldId) {
    let mut host = GuiTestHost::default();
    let world = host.create_world(Default::default(), &GUI_SYSTEMS).unwrap();
    (host, world)
}

/// Queue one batch and return its outcome from the Host frame that applies it.
pub(super) fn submit(
    host: &mut HostRuntime,
    world: WorldId,
    operations: Vec<Command>,
) -> BatchOutcome {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();

    let mut outcomes = host
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes;
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.batch_id < input_test_support::ACTION_BATCHES),
        "a queued GuiAction batch applies in the next test frame, not in submit"
    );
    outcomes.remove(0)
}

pub(super) fn apply(
    host: &mut HostRuntime,
    world: WorldId,
    operations: Vec<Command>,
) -> Vec<(u32, EntityId)> {
    submit(host, world, operations).result.unwrap()
}

pub(super) fn create(host: &mut HostRuntime, world: WorldId, value: ComponentValue) -> EntityId {
    apply(
        host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(EntityRef::Alias(1), value),
        ],
    )[0]
    .1
}

/// Read a control's fields and GUI System query records.
pub(super) fn snapshot(host: &mut HostRuntime, world: WorldId, entity: EntityId) -> GuiControlRead {
    read(host, world, entity).unwrap()
}

/// Read a control's fields and GUI System query records, if it is a control.
pub(super) fn read(
    host: &mut HostRuntime,
    world: WorldId,
    entity: EntityId,
) -> Option<GuiControlRead> {
    crate::systems::gui::test_support::read_control(&host.world_mut(world).unwrap(), entity)
}

/// Write one field of a control component directly, as a client does.
pub(super) fn set_field(
    host: &mut HostRuntime,
    world: WorldId,
    entity: EntityId,
    component: u16,
    offset: usize,
    value: FieldValue,
) -> BatchOutcome {
    submit(
        host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component,
            field: FieldWrite {
                offset: offset as u32,
                value,
            },
        }],
    )
}

/// Submit one `GuiAction` batch; the next frame applies it.
pub(super) fn action(
    host: &mut GuiTestHost,
    world: WorldId,
    target: GuiEntityTarget,
    action: GuiLocalAction,
) {
    host.submit_action(world, target, action);
}

pub(super) fn frame(host: &mut GuiTestHost) {
    let report = host.frame_report(0.125);
    assert!(report.worlds.values().all(Result::is_ok));
    assert!(report.publication_errors.is_empty());
}

/// The reason a refused local operation maps to in a batch outcome.
pub(super) fn local_reason(error: GuiLocalActionError) -> ErrorReason {
    match error {
        GuiLocalActionError::StaleTarget => ErrorReason::StaleTarget,
        GuiLocalActionError::Unavailable => ErrorReason::Unavailable,
        GuiLocalActionError::UnsupportedAction => ErrorReason::UnsupportedAction,
        GuiLocalActionError::InvalidValue => ErrorReason::InvalidValue,
    }
}

/// Settled requests of `world` in request order: ticket terminals and applied
/// `GuiAction` batches, each with the momentary effect it published.
pub(super) fn outcomes(host: &mut GuiTestHost, world: WorldId) -> Vec<GuiTestOutcome> {
    let mut results = Vec::new();
    host.deliveries
        .borrow_mut()
        .retain(|(target, request, terminal)| {
            if *target != world {
                return true;
            }
            let result = match terminal {
                GuiDeliveryTerminal::Applied(effect) => Ok(Some(effect.clone())),
                GuiDeliveryTerminal::Written {
                    ..
                } => Ok(None),
                GuiDeliveryTerminal::Rejected(GuiInputError::Local(error)) => {
                    Err(local_reason(*error))
                }
                other => panic!("unexpected nonlocal terminal: {other:?}"),
            };
            results.push((
                *request,
                GuiTestOutcome {
                    result,
                },
            ));
            false
        });
    let actions = std::mem::take(&mut host.actions);
    for (action_world, request, target, action) in actions {
        let position = host.action_outcomes.iter().position(|(applied, outcome)| {
            *applied == action_world
                && outcome.batch_id == input_test_support::ACTION_BATCHES + request
        });
        let (Some(position), true) = (position, action_world == world) else {
            host.actions.push((action_world, request, target, action));
            continue;
        };
        let (_, outcome) = host.action_outcomes.remove(position);
        let result = match outcome.result {
            Err(error) => Err(error.reason),
            // The effect this action published: same target and tick, and
            // the kind the action publishes.
            Ok(_) => Ok(host
                .effects
                .iter()
                .position(|effect| {
                    effect.target == target
                        && effect.tick == outcome.tick
                        && match (&action, &effect.kind) {
                            (GuiLocalAction::Press, GuiLocalEffectKind::Pressed) => true,
                            (GuiLocalAction::Submit, GuiLocalEffectKind::Submitted(_)) => true,
                            (
                                GuiLocalAction::Focus | GuiLocalAction::Blur,
                                GuiLocalEffectKind::FocusChanged {
                                    focused,
                                    ..
                                },
                            ) => *focused == (action == GuiLocalAction::Focus),
                            _ => false,
                        }
                        && effect.source == GuiLocalEffectSource::Semantic
                })
                .map(|index| host.effects.remove(index))),
        };
        results.push((
            request,
            GuiTestOutcome {
                result,
            },
        ));
    }
    results.sort_by_key(|(request, _)| *request);
    results.into_iter().map(|(_, outcome)| outcome).collect()
}

fn bounded_ancestry_case(name: &str) -> bool {
    if std::env::var("IPP_GUI_ANCESTRY_CASE").as_deref() == Ok(name) {
        return true;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("IPP_GUI_ANCESTRY_CASE", name)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > std::time::Duration::from_secs(2) {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "GUI ancestry exceeded two seconds: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

fn cycle_recovery(focused: bool) {
    for cycle_kind in 0..3 {
        let (mut host, world) = fixture();
        let entity = create(
            &mut host,
            world,
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
        );
        let first = create(
            &mut host,
            world,
            ComponentValue::GuiBehavior(GuiBehavior::default()),
        );
        let second = create(
            &mut host,
            world,
            ComponentValue::GuiBehavior(GuiBehavior::default()),
        );
        let place = |entity, parent: Option<EntityId>| Command::PlaceEntity {
            entity: EntityRef::Handle(entity),
            placement: EntityPlacementRef {
                parent: parent.map(EntityRef::Handle),
                before: None,
            },
        };
        let (broken, target) = match cycle_kind {
            0 => (entity, entity),
            1 => {
                apply(&mut host, world, vec![place(first, Some(entity))]);
                (entity, first)
            }
            _ => {
                apply(
                    &mut host,
                    world,
                    vec![place(entity, Some(first)), place(first, Some(second))],
                );
                (second, first)
            }
        };
        let target_control = snapshot(&mut host, world, entity).target;
        action(&mut host, world, target_control, GuiLocalAction::Toggle);
        frame(&mut host);
        outcomes(&mut host, world);
        if focused {
            action(&mut host, world, target_control, GuiLocalAction::Focus);
            frame(&mut host);
            outcomes(&mut host, world);
            assert!(snapshot(&mut host, world, entity).focused);
        }
        let committed = snapshot(&mut host, world, entity).value;
        let failed = submit(&mut host, world, vec![place(broken, Some(target))]);
        assert_eq!(
            failed.result.unwrap_err().reason,
            ErrorReason::UnsupportedDependency
        );
        assert_eq!(host.world_mut(world).unwrap().fault(), None);
        let invalid = snapshot(&mut host, world, entity);
        assert!(!invalid.available);

        // The applying frame evaluated the cycle, which clears logical focus.
        assert!(!invalid.focused);
        assert_eq!(committed, invalid.value);
        action(&mut host, world, target_control, GuiLocalAction::Toggle);
        frame(&mut host);
        assert!(!snapshot(&mut host, world, entity).focused);
        assert_eq!(
            outcomes(&mut host, world)[0].result,
            Err(ErrorReason::Unavailable)
        );
        apply(&mut host, world, vec![place(broken, None)]);
        let recovered = snapshot(&mut host, world, entity);
        assert!(recovered.available);
        assert_eq!(recovered.target, target_control);
        assert_eq!(committed, recovered.value);
        action(&mut host, world, target_control, GuiLocalAction::Toggle);
        frame(&mut host);
        assert_eq!(
            snapshot(&mut host, world, entity).value,
            GuiTestValue::Bool(false)
        );
    }
}

#[test]
fn cyclic_control_ancestry_is_unavailable_until_corrected() {
    if bounded_ancestry_case(
        "world::systems::gui::local::local_tests::cyclic_control_ancestry_is_unavailable_until_corrected",
    ) {
        cycle_recovery(false);
    }
}

#[test]
fn cyclic_focused_control_commit_clears_focus_without_losing_committed_state() {
    if bounded_ancestry_case(
        "world::systems::gui::local::local_tests::cyclic_focused_control_commit_clears_focus_without_losing_committed_state",
    ) {
        cycle_recovery(true);
    }
}

#[test]
fn transient_eligibility_changes_between_frames_preserve_logical_focus() {
    for change in 0..3 {
        let (mut host, world) = fixture();
        let entity = create(
            &mut host,
            world,
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
        );
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiBehavior(GuiBehavior::default()),
            )],
        );
        let target = snapshot(&mut host, world, entity).target;
        action(&mut host, world, target, GuiLocalAction::Focus);
        frame(&mut host);
        let committed = snapshot(&mut host, world, entity).value;
        let mutation = |invalid: bool| {
            if change == 0 {
                Command::PlaceEntity {
                    entity: EntityRef::Handle(entity),
                    placement: EntityPlacementRef {
                        parent: invalid.then_some(EntityRef::Handle(entity)),
                        before: None,
                    },
                }
            } else {
                Command::SetField {
                    entity: EntityRef::Handle(entity),
                    component: ComponentValue::GUI_BEHAVIOR,
                    field: FieldWrite {
                        offset: if change == 1 {
                            std::mem::offset_of!(GuiBehavior, enabled) as u32
                        } else {
                            std::mem::offset_of!(GuiBehavior, visible) as u32
                        },
                        value: FieldValue::Bool(!invalid),
                    },
                }
            }
        };
        // Both batches reach the same mutation boundary before evaluation.
        for (id, invalid) in [(91, true), (92, false)] {
            host.world_mut(world)
                .unwrap()
                .enqueue(Batch {
                    id,
                    operations: vec![mutation(invalid)],
                })
                .unwrap();
        }
        let mut report = host
            .frame(0.125)
            .unwrap()
            .worlds
            .remove(&world)
            .unwrap()
            .unwrap();
        assert_eq!(report.outcomes.remove(0).result.is_err(), change == 0);
        report.outcomes.remove(0).result.unwrap();
        let resumed = snapshot(&mut host, world, entity);
        assert!(resumed.focused && resumed.available && resumed.enabled && resumed.visible);
        assert_eq!(committed, resumed.value);

        apply(
            &mut host,
            world,
            vec![Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_CHECKBOX,
            }],
        );
        assert!(read(&mut host, world, entity).is_none());
        assert!(
            host.world_mut(world)
                .unwrap()
                .system::<GuiSystem>(GuiSystem::ID)
                .unwrap()
                .local
                .focus
                .is_none()
        );
    }
}

#[test]
fn controls_insert_their_required_behavior_and_bounds_and_keep_them_after_removal() {
    use crate::systems::canvas::CanvasBounds;

    let (mut host, world) = fixture();
    for control in [
        ComponentValue::GuiButton(GuiButton::default()),
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
        ComponentValue::GuiSlider(GuiSlider::default()),
        ComponentValue::GuiTextInput(GuiTextInput::default()),
        ComponentValue::GuiScrollView(GuiScrollView::default()),
        ComponentValue::GuiVirtualList(GuiVirtualList::default()),
    ] {
        let component = control.type_id();
        let entity = create(&mut host, world, control);
        let components = host
            .world_mut(world)
            .unwrap()
            .inspect(entity)
            .unwrap()
            .components;
        assert!(
            components.contains(&ComponentValue::GuiBehavior(GuiBehavior::default())),
            "{component} requires GuiBehavior: {components:?}"
        );
        assert!(
            components.contains(&ComponentValue::CanvasBounds(CanvasBounds::default())),
            "{component} requires CanvasBounds: {components:?}"
        );
        // CanvasStyle is an optional override; controls do not require it.
        assert!(
            !components
                .iter()
                .any(|value| value.type_id() == ComponentValue::CANVAS_STYLE),
            "{component} does not require CanvasStyle: {components:?}"
        );

        // The required components are ordinary: they stay when the control
        // that required them is removed.
        apply(
            &mut host,
            world,
            vec![Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component,
            }],
        );
        let types: Vec<_> = host
            .world_mut(world)
            .unwrap()
            .inspect(entity)
            .unwrap()
            .components
            .iter()
            .map(ComponentValue::type_id)
            .collect();
        assert_eq!(
            types,
            [ComponentValue::GUI_BEHAVIOR, ComponentValue::CANVAS_BOUNDS]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn semantic_actions_write_the_value_field_without_output_or_input_context() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    let original = snapshot(&mut host, world, entity);
    assert_eq!(original.value, GuiTestValue::Bool(false));
    assert!(original.available);
    assert_eq!(original.target.component, ComponentValue::GUI_CHECKBOX);
    assert_eq!(
        GuiEntityTarget::from_canvas(original.target.world, original.target.canvas_target()),
        original.target
    );
    action(&mut host, world, original.target, GuiLocalAction::Toggle);
    frame(&mut host);
    assert!(host.root_output(world).is_none());
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Bool(true)
    );
    // The batch applied; a value action publishes no momentary effect.
    let events = outcomes(&mut host, world);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].result, Ok(None));
    assert!(host.effects.is_empty());
    assert!(outcomes(&mut host, world).is_empty());

    // A client write to the field is the value; the next relative action
    // starts from it without any staleness check.
    let written = set_field(
        &mut host,
        world,
        entity,
        ComponentValue::GUI_CHECKBOX,
        std::mem::offset_of!(GuiCheckbox, checked),
        FieldValue::Bool(false),
    );
    assert!(written.result.is_ok());
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Bool(false)
    );
    action(&mut host, world, original.target, GuiLocalAction::Toggle);
    frame(&mut host);
    assert_eq!(outcomes(&mut host, world)[0].result, Ok(None));
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Bool(true)
    );
}

#[test]
fn semantic_button_press_does_not_move_logical_focus_or_commit_a_value() {
    let (mut host, world) = fixture();
    let text = create(
        &mut host,
        world,
        ComponentValue::GuiTextInput(GuiTextInput::default()),
    );
    let button = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    let text_target = snapshot(&mut host, world, text).target;
    let button_target = snapshot(&mut host, world, button).target;
    action(&mut host, world, text_target, GuiLocalAction::Focus);
    frame(&mut host);
    assert!(snapshot(&mut host, world, text).focused);
    outcomes(&mut host, world);
    action(&mut host, world, button_target, GuiLocalAction::Press);
    frame(&mut host);
    let pressed = outcomes(&mut host, world).remove(0).into_effect();
    assert_eq!(pressed.kind, GuiLocalEffectKind::Pressed);
    assert_eq!(pressed.source, GuiLocalEffectSource::Semantic);
    assert_eq!(pressed.ancestry.as_ref(), &[button]);
    assert!(snapshot(&mut host, world, text).focused);
    let unchanged = snapshot(&mut host, world, button);
    assert!(!unchanged.focused);
    assert_eq!(unchanged.value, GuiTestValue::None);
}

#[test]
fn local_eligibility_is_inherited_but_client_writes_are_not_user_input() {
    let (mut host, world) = fixture();
    let parent = create(
        &mut host,
        world,
        ComponentValue::GuiBehavior(GuiBehavior {
            enabled: false,
            ..Default::default()
        }),
    );
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    apply(
        &mut host,
        world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(entity),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        }],
    );
    let observed = snapshot(&mut host, world, entity);
    assert!(!observed.enabled);
    action(&mut host, world, observed.target, GuiLocalAction::Toggle);
    frame(&mut host);
    assert_eq!(
        outcomes(&mut host, world)[0].result,
        Err(ErrorReason::Unavailable)
    );

    // A client compare-and-set is not user input: eligibility does not gate it.
    let checked = |expected: bool, value: bool| {
        Command::set_field_if(
            EntityRef::Handle(entity),
            ComponentValue::GUI_CHECKBOX,
            FieldWrite {
                offset: std::mem::offset_of!(GuiCheckbox, checked) as u32,
                value: FieldValue::Bool(value),
            },
            FieldValue::Bool(expected),
        )
    };
    assert!(
        submit(&mut host, world, vec![checked(false, true)])
            .result
            .is_ok()
    );
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Bool(true)
    );
    assert_eq!(
        submit(&mut host, world, vec![checked(false, true)])
            .result
            .unwrap_err()
            .reason,
        ErrorReason::ValueMismatch
    );
    assert!(outcomes(&mut host, world).is_empty());
}

#[test]
fn component_replacement_and_control_kind_changes_fence_old_actions() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    let old = snapshot(&mut host, world, entity).target;
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_CHECKBOX,
            },
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ),
        ],
    );
    let replacement = snapshot(&mut host, world, entity).target;
    assert_ne!(old, replacement);
    action(&mut host, world, old, GuiLocalAction::Toggle);
    frame(&mut host);
    assert_eq!(
        outcomes(&mut host, world)[0].result,
        Err(ErrorReason::StaleTarget)
    );
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_CHECKBOX,
            },
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiButton(GuiButton::default()),
            ),
        ],
    );
    action(&mut host, world, replacement, GuiLocalAction::Toggle);
    frame(&mut host);
    assert_eq!(
        outcomes(&mut host, world)[0].result,
        Err(ErrorReason::StaleTarget)
    );
    let button = snapshot(&mut host, world, entity);
    assert_eq!(button.kind, GuiControlKind::Button);
    assert_eq!(button.value, GuiTestValue::None);
}

#[test]
fn invalid_configuration_write_has_no_effect_on_the_control() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiSlider(GuiSlider::default()),
    );
    let target = snapshot(&mut host, world, entity).target;
    action(&mut host, world, target, GuiLocalAction::SetScalar(0.75));
    frame(&mut host);
    outcomes(&mut host, world);
    let write_max = |value| Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::GUI_SLIDER,
        field: FieldWrite {
            offset: std::mem::offset_of!(GuiSlider, max) as u32,
            value: FieldValue::F32(value),
        },
    };
    let failed = submit(&mut host, world, vec![write_max(-1.0)]);
    assert_eq!(failed.result.unwrap_err().reason, ErrorReason::InvalidValue);
    let unchanged = snapshot(&mut host, world, entity);
    assert_eq!(unchanged.target, target);
    assert!(unchanged.available);
    assert_eq!(unchanged.value, GuiTestValue::Scalar(0.75));
    assert!(
        host.world_mut(world)
            .unwrap()
            .inspect(entity)
            .unwrap()
            .components
            .contains(&ComponentValue::GuiSlider(GuiSlider {
                value: 0.75,
                ..Default::default()
            }))
    );
    action(&mut host, world, target, GuiLocalAction::SetScalar(0.5));
    frame(&mut host);
    assert!(outcomes(&mut host, world)[0].result.is_ok());
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Scalar(0.5)
    );
}

#[test]
fn slider_range_edit_excluding_the_value_is_accepted_and_keeps_the_value() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiSlider(GuiSlider {
            max: 1.0,
            ..Default::default()
        }),
    );
    let target = snapshot(&mut host, world, entity).target;
    action(&mut host, world, target, GuiLocalAction::SetScalar(0.75));
    frame(&mut host);
    outcomes(&mut host, world);
    let write = |field: usize, value| Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::GUI_SLIDER,
        field: FieldWrite {
            offset: field as u32,
            value: FieldValue::F32(value),
        },
    };
    let max = std::mem::offset_of!(GuiSlider, max);
    let min = std::mem::offset_of!(GuiSlider, min);

    // Plain fields: the range edit is written and nothing rechecks the value.
    apply(&mut host, world, vec![write(max, 0.5)]);
    let narrowed = snapshot(&mut host, world, entity);
    assert!(narrowed.available);
    assert_eq!(narrowed.value, GuiTestValue::Scalar(0.75));

    // A semantic value change still has to lie inside the current range.
    action(&mut host, world, target, GuiLocalAction::SetScalar(0.75));
    frame(&mut host);
    assert_eq!(
        outcomes(&mut host, world)[0].result,
        Err(ErrorReason::InvalidValue)
    );
    action(&mut host, world, target, GuiLocalAction::SetScalar(0.25));
    frame(&mut host);
    assert!(outcomes(&mut host, world)[0].result.is_ok());
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Scalar(0.25)
    );

    apply(&mut host, world, vec![write(min, 0.2), write(max, 2.0)]);
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Scalar(0.25)
    );
}

#[test]
fn raw_actions_are_not_admitted_system_commands() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command_with_reply(GuiSystem::ID, 7, 1, GuiLocalAction::Toggle)
        .unwrap();
    let report = host.frame(0.125).unwrap();
    let results = &report.worlds[&world]
        .as_ref()
        .unwrap()
        .system_command_outcomes;
    assert_eq!(results.len(), 1);
    assert!(results.iter().all(|result| result.result.is_err()));
    assert!(outcomes(&mut host, world).is_empty());
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Bool(false)
    );
}

#[test]
fn logical_focus_publishes_compact_ring_and_direct_priority_without_native_focus() {
    use crate::systems::canvas::{CanvasPaintEntry, CanvasPart, CanvasPublication, CanvasSystem};
    use crate::systems::gui::presentation::GuiCanvasPublication;
    let mut host = GuiTestHost::default();
    let world = host
        .create_world(Default::default(), &PRESENTED_GUI_SYSTEMS)
        .unwrap();
    let root = create(
        &mut host,
        world,
        ComponentValue::CanvasStyle(Default::default()),
    );
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    apply(
        &mut host,
        world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(entity),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(root)),
                before: None,
            },
        }],
    );
    let selection = crate::OutputRef::canvas(host.world_ref(world).unwrap());
    let local = snapshot(&mut host, world, entity);
    action(&mut host, world, local.target, GuiLocalAction::Focus);
    frame(&mut host);
    assert!(host.root_output(world).is_none());
    let publication = host
        .publication(host.latest_publication(world).unwrap())
        .unwrap();
    let canvas = publication
        .output(selection)
        .unwrap()
        .data::<CanvasPublication>()
        .unwrap();
    assert!(canvas.interaction.focused);
    assert!(
        !canvas.interaction.hovered && !canvas.interaction.pressed && !canvas.interaction.captured
    );
    assert!(canvas.entries.iter().any(|entry| matches!(entry.as_ref(), CanvasPaintEntry::Primitive { primitive, .. } if primitive.style().identity.part == CanvasPart::FocusRing)));
    let semantics = publication
        .chunk(CanvasSystem::ID)
        .unwrap()
        .data::<GuiCanvasPublication>()
        .unwrap();
    assert_eq!(
        semantics.views[&selection].controls[0].record.target,
        local.target
    );
    assert!(snapshot(&mut host, world, entity).focused);
    assert!(matches!(
        outcomes(&mut host, world)[0].effect().kind,
        GuiLocalEffectKind::FocusChanged {
            focused: true,
            changed: true
        }
    ));
    action(&mut host, world, local.target, GuiLocalAction::Press);
    frame(&mut host);
    assert!(snapshot(&mut host, world, entity).focused);
    host.world_mut(world).unwrap().release_system_session(7);
    assert!(snapshot(&mut host, world, entity).focused);
    action(&mut host, world, local.target, GuiLocalAction::Blur);
    frame(&mut host);
    let publication = host
        .publication(host.latest_publication(world).unwrap())
        .unwrap();
    let canvas = publication
        .output(selection)
        .unwrap()
        .data::<CanvasPublication>()
        .unwrap();
    assert!(!canvas.interaction.focused);
    assert!(canvas.entries.iter().all(|entry| !matches!(entry.as_ref(), CanvasPaintEntry::Primitive { primitive, .. } if primitive.style().identity.part == CanvasPart::FocusRing)));
}

#[test]
fn ordinary_focus_ring_uses_theme_and_override_color_with_border_precedence() {
    use crate::components::rows::Rows;
    use crate::systems::canvas::{
        CanvasPaintEntry, CanvasPart, CanvasPrimitive, CanvasPublication, CanvasShapeFill,
    };
    use crate::systems::gui::GuiPartId;
    use crate::systems::gui::GuiPrimitivePart;
    use crate::systems::gui::presentation::{GuiPaintPart, GuiSkin, GuiTheme};

    let mut host = GuiTestHost::default();
    let world = host
        .create_world(
            Default::default(),
            &[
                crate::systems::animation::AnimationSystem::ID,
                crate::systems::asset_dependencies::AssetDependencySystem::ID,
                crate::systems::canvas::CanvasSystem::ID,
                crate::systems::gui::GuiSystem::ID,
                crate::systems::gui::GuiLayoutSystem::ID,
            ],
        )
        .unwrap();
    let root = create(
        &mut host,
        world,
        ComponentValue::CanvasStyle(Default::default()),
    );
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    let theme = create(
        &mut host,
        world,
        ComponentValue::GuiTheme(GuiTheme::default()),
    );
    apply(
        &mut host,
        world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(entity),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(root)),
                before: None,
            },
        }],
    );
    let selection = crate::OutputRef::canvas(host.world_ref(world).unwrap());
    let target = snapshot(&mut host, world, entity).target;
    action(&mut host, world, target, GuiLocalAction::Focus);
    frame(&mut host);
    let theme_color = [0.8, 0.3, 0.1, 1.0];
    let override_color = [0.1, 0.2, 0.9, 1.0];
    let theme_border = [0.1, 0.9, 0.2, 1.0];
    let override_border = [0.8, 0.8, 0.1, 1.0];
    let cases = [
        (None, None, None, None, [1.0; 4]),
        (Some(theme_color), None, None, None, theme_color),
        (
            Some(theme_color),
            None,
            Some(override_color),
            None,
            override_color,
        ),
        (
            Some(theme_color),
            Some(theme_border),
            Some(override_color),
            None,
            theme_border,
        ),
        (
            Some(theme_color),
            Some(theme_border),
            Some(override_color),
            Some(override_border),
            override_border,
        ),
    ];
    let parts = |color, border_color| {
        let mut parts = Rows::new();
        parts
            .push(GuiPaintPart {
                color,
                border_color,
                ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::FocusRing)).unwrap()
            })
            .unwrap();
        parts
    };
    for (theme_color, theme_border, override_color, override_border, expected) in cases {
        apply(
            &mut host,
            world,
            vec![
                Command::insert_value(
                    EntityRef::Handle(theme),
                    ComponentValue::GuiTheme(GuiTheme {
                        parts: parts(theme_color, theme_border),
                    }),
                ),
                Command::insert_value(
                    EntityRef::Handle(entity),
                    ComponentValue::GuiSkin(GuiSkin {
                        theme,
                        parts: parts(override_color, override_border),
                        ..Default::default()
                    }),
                ),
            ],
        );
        frame(&mut host);
        let publication = host
            .publication(host.latest_publication(world).unwrap())
            .unwrap();
        let canvas = publication
            .output(selection)
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap();
        let ring = canvas
            .entries
            .iter()
            .find_map(|entry| match entry.as_ref() {
                CanvasPaintEntry::Primitive {
                    primitive,
                    ..
                } if primitive.style().identity.part == CanvasPart::FocusRing => Some(primitive),
                _ => None,
            })
            .expect("focused ordinary control must paint a compact focus ring");
        let CanvasPrimitive::Box {
            style,
            border_width,
            border_color,
            fill,
            ..
        } = ring
        else {
            unreachable!()
        };
        assert_eq!(*border_color, expected);
        assert_eq!(*fill, CanvasShapeFill::Solid([0.0; 4]));
        assert_eq!(*border_width, crate::systems::gui::FOCUS_BORDER_WIDTH);
        assert_eq!(style.identity.target, target.canvas_target());
        assert!(canvas.interaction.focused);
    }
    let local = snapshot(&mut host, world, entity);
    assert_eq!(local.target, target);
    assert_eq!(local.value, GuiTestValue::None);
    assert!(local.focused);
}

#[test]
fn hidden_disabled_and_unavailable_controls_refuse_every_semantic_action() {
    let (mut host, world) = fixture();
    let behavior = |enabled, visible| {
        ComponentValue::GuiBehavior(GuiBehavior {
            enabled,
            visible,
            ..Default::default()
        })
    };
    for (state, ineligible) in [("disabled", false), ("hidden", true)] {
        let entity = create(
            &mut host,
            world,
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
        );
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                behavior(ineligible, !ineligible),
            )],
        );
        let observed = snapshot(&mut host, world, entity);
        assert!(!(observed.enabled && observed.visible), "{state}");
        for request in [GuiLocalAction::Toggle, GuiLocalAction::Focus] {
            action(&mut host, world, observed.target, request.clone());
            frame(&mut host);
            assert_eq!(
                outcomes(&mut host, world)[0].result,
                Err(ErrorReason::Unavailable),
                "{state} {request:?}"
            );
        }
        assert_eq!(snapshot(&mut host, world, entity), observed, "{state}");
    }

    // An ambiguous control, with a second control component, is unavailable.
    let slider = create(
        &mut host,
        world,
        ComponentValue::GuiSlider(GuiSlider::default()),
    );
    let failed = submit(
        &mut host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(slider),
            component: ComponentValue::GUI_SLIDER,
            field: FieldWrite {
                offset: std::mem::offset_of!(GuiSlider, step) as u32,
                value: FieldValue::F32(-1.0),
            },
        }],
    );
    assert!(failed.result.is_err());
    assert!(
        snapshot(&mut host, world, slider).available,
        "an invalid write has no effect"
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(slider),
            ComponentValue::GuiButton(GuiButton::default()),
        )],
    );
    let unavailable = snapshot(&mut host, world, slider);
    let target = unavailable.target;
    assert!(!unavailable.available);
    for request in [GuiLocalAction::SetScalar(0.5), GuiLocalAction::Focus] {
        action(&mut host, world, target, request.clone());
        frame(&mut host);
        assert_eq!(
            outcomes(&mut host, world)[0].result,
            Err(ErrorReason::Unavailable),
            "{request:?}"
        );
    }
    assert!(!snapshot(&mut host, world, slider).focused);
}

fn gui_action(entity: EntityRef, target: GuiEntityTarget, action: GuiLocalAction) -> Command {
    Command::GuiAction {
        target: crate::GuiActionTarget {
            entity,
            component: target.component,
            incarnation: target.incarnation,
        },
        action,
    }
}

#[test]
fn gui_action_refusals_stop_the_batch_with_their_reason_and_no_effect() {
    let (mut host, world) = fixture();
    let checkbox = create(
        &mut host,
        world,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    let slider = create(
        &mut host,
        world,
        ComponentValue::GuiSlider(GuiSlider::default()),
    );
    let checkbox_target = snapshot(&mut host, world, checkbox).target;
    let slider_target = snapshot(&mut host, world, slider).target;
    let stale = GuiEntityTarget {
        incarnation: checkbox_target.incarnation + 1,
        ..checkbox_target
    };

    // Each refusal names its reason at the action's operation; the toggle
    // after it in the same batch never applies.
    let later_toggle = gui_action(
        EntityRef::Handle(checkbox),
        checkbox_target,
        GuiLocalAction::Toggle,
    );
    for (refused, reason) in [
        (
            gui_action(EntityRef::Handle(checkbox), stale, GuiLocalAction::Toggle),
            ErrorReason::StaleTarget,
        ),
        (
            gui_action(
                EntityRef::Handle(checkbox),
                checkbox_target,
                GuiLocalAction::SetScalar(0.5),
            ),
            ErrorReason::UnsupportedAction,
        ),
        (
            gui_action(
                EntityRef::Handle(slider),
                slider_target,
                GuiLocalAction::SetScalar(2.0),
            ),
            ErrorReason::InvalidValue,
        ),
        (
            gui_action(
                EntityRef::Handle(slider),
                slider_target,
                GuiLocalAction::SetScalar(f32::NAN),
            ),
            ErrorReason::InvalidValue,
        ),
    ] {
        let outcome = submit(&mut host, world, vec![refused, later_toggle.clone()]);
        let error = outcome.result.unwrap_err();
        assert_eq!((error.operation, error.reason), (Some(0), reason));
        assert_eq!(
            snapshot(&mut host, world, checkbox).value,
            GuiTestValue::Bool(false)
        );
        assert_eq!(
            snapshot(&mut host, world, slider).value,
            GuiTestValue::Scalar(GuiSlider::default().value)
        );
    }

    // A disabled control refuses with Unavailable once its stored
    // eligibility reflects the policy.
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(checkbox),
            ComponentValue::GuiBehavior(GuiBehavior {
                enabled: false,
                ..Default::default()
            }),
        )],
    );
    let outcome = submit(&mut host, world, vec![later_toggle.clone()]);
    assert_eq!(outcome.result.unwrap_err().reason, ErrorReason::Unavailable);
    assert_eq!(
        snapshot(&mut host, world, checkbox).value,
        GuiTestValue::Bool(false)
    );

    // A deleted entity is a stale target, too.
    apply(
        &mut host,
        world,
        vec![Command::Delete {
            entity: EntityRef::Handle(slider),
        }],
    );
    let outcome = submit(
        &mut host,
        world,
        vec![gui_action(
            EntityRef::Handle(slider),
            slider_target,
            GuiLocalAction::SetScalar(0.5),
        )],
    );
    assert_eq!(outcome.result.unwrap_err().reason, ErrorReason::StaleTarget);
}

#[test]
fn gui_actions_name_controls_by_alias_or_symbol_and_apply_in_batch_order() {
    let (mut host, world) = fixture();
    let checkbox = create(
        &mut host,
        world,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    let target = snapshot(&mut host, world, checkbox).target;
    apply(
        &mut host,
        world,
        vec![Command::SetMetadata {
            entity: EntityRef::Handle(checkbox),
            metadata: crate::EntityMetadata {
                symbolic_id: Some("agree".into()),
                classes: Vec::new(),
            },
        }],
    );

    // Two toggles in one batch apply in order; the symbol resolves at its
    // operation and the outcome reports it.
    let outcome = submit(
        &mut host,
        world,
        vec![
            gui_action(
                EntityRef::Symbol("agree".into()),
                target,
                GuiLocalAction::Toggle,
            ),
            gui_action(EntityRef::Handle(checkbox), target, GuiLocalAction::Toggle),
            gui_action(EntityRef::Handle(checkbox), target, GuiLocalAction::Toggle),
        ],
    );
    assert!(outcome.result.is_ok());
    assert_eq!(outcome.symbols, [("agree".into(), checkbox)]);
    assert_eq!(
        snapshot(&mut host, world, checkbox).value,
        GuiTestValue::Bool(true)
    );

    // An alias bound by adoption names the same control.
    let outcome = submit(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 2,
                metadata: crate::EntityMetadata {
                    symbolic_id: Some("agree".into()),
                    classes: Vec::new(),
                },
                adopt: true,
            },
            gui_action(EntityRef::Alias(2), target, GuiLocalAction::Toggle),
        ],
    );
    assert_eq!(outcome.result, Ok(vec![(2, checkbox)]));
    assert_eq!(
        snapshot(&mut host, world, checkbox).value,
        GuiTestValue::Bool(false)
    );
}

#[test]
fn gui_actions_are_refused_without_the_gui_system() {
    let mut host = GuiTestHost::default();
    let world = host
        .create_world(
            Default::default(),
            &[crate::systems::canvas::CanvasSystem::ID],
        )
        .unwrap();
    let outcome = submit(
        &mut host,
        world,
        vec![Command::GuiAction {
            target: crate::GuiActionTarget {
                entity: EntityRef::Handle(EntityId::from_bits(1)),
                component: ComponentValue::GUI_CHECKBOX,
                incarnation: 1,
            },
            action: GuiLocalAction::Toggle,
        }],
    );
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ErrorReason::UnsupportedDependency
    );
}
