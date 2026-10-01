use super::local_tests::{GuiTestValue, action, apply, create, fixture, frame, outcomes, snapshot};
use super::*;
use crate::services::reliable_output::*;
use crate::systems::gui::GuiSystem;
use crate::systems::gui::observations::*;
use crate::{Command, ComponentValue, EntityPlacementRef, EntityRef, HostRuntime, WorldId};
use std::sync::Arc;

const OUTPUT_BYTES: usize = 2 * 1024 * 1024;

fn output() -> GuiObservationOutput {
    GuiObservationOutput::new(
        ReliableOutputAccount::new(OutputLimits {
            bytes: OUTPUT_BYTES,
            reply_reserve: 0,
        }),
        GuiObservationEncoding {
            control_bytes: 64,
            effect_bytes: 128,
            ancestry_entry_bytes: 16,
            text_byte_bytes: 1,
        },
    )
    .unwrap()
}

/// Hold every remaining byte so the output's next record exhausts its account.
fn exhaust(output: &GuiObservationOutput) -> ReliableOutputLease {
    output
        .account()
        .reserve(OutputCharge {
            entries: 0,
            bytes: OUTPUT_BYTES - output.account().usage().bytes,
        })
        .unwrap()
}

fn cut(
    host: &mut HostRuntime,
    world: WorldId,
    output: &GuiObservationOutput,
    subscription: &GuiObservationSubscription,
    request: u64,
    subscribe: bool,
) {
    let command = prepare_cut(
        host.world_ref(world).unwrap(),
        output,
        subscription,
        request,
        subscribe,
    );
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command(GuiSystem::ID, 44, command)
        .unwrap();
}

fn prepare_cut(
    world: crate::WorldRef,
    output: &GuiObservationOutput,
    subscription: &GuiObservationSubscription,
    request: u64,
    subscribe: bool,
) -> GuiObservationCommand {
    let lease = output
        .account()
        .reserve(OutputCharge {
            entries: 1,
            bytes: 0,
        })
        .unwrap();
    let prepare = if subscribe {
        GuiObservationCommand::prepare_subscribe
    } else {
        GuiObservationCommand::prepare_unsubscribe
    };
    prepare(output, world, subscription, request, lease).unwrap()
}

fn records(output: &GuiObservationOutput) -> Vec<GuiObservationRecord> {
    let mut records = Vec::new();
    while let Some(delivery) = output.pop_front() {
        let (record, lease) = delivery.into_parts();
        records.push(record);
        drop(lease);
    }
    records
}

fn subscribed(
    host: &mut super::input_test_support::GuiTestHost,
    world: WorldId,
    output: &GuiObservationOutput,
    classes: GuiObservationClasses,
) -> GuiObservationSubscription {
    let subscription = output
        .new_subscription(host.world_ref(world).unwrap(), classes)
        .unwrap();
    cut(host, world, output, &subscription, 1, true);
    frame(host);
    assert!(subscription.is_active());
    assert!(matches!(
        records(output).as_slice(),
        [GuiObservationRecord::Control {
            result: GuiObservationControlResult::Subscribed,
            ..
        }]
    ));
    subscription
}

#[test]
fn whole_world_fifo_cuts_surround_same_tick_effects_without_extra_replies() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    frame(&mut host);
    let target = snapshot(&mut host, world, entity).target;
    let output = output();
    let subscription = output
        .new_subscription(target.world, GuiObservationClasses::Application)
        .unwrap();
    cut(&mut host, world, &output, &subscription, 10, true);
    assert!(!subscription.is_active());
    assert!(output.pop_front().is_none());
    action(&mut host, world, target, GuiLocalAction::Press);
    action(&mut host, world, target, GuiLocalAction::Press);
    cut(&mut host, world, &output, &subscription, 11, false);
    action(&mut host, world, target, GuiLocalAction::Press);
    frame(&mut host);
    assert!(!subscription.is_active());
    let received = records(&output);
    assert_eq!(received.len(), 4);
    assert!(matches!(
        received[0],
        GuiObservationRecord::Control {
            request: 10,
            result: GuiObservationControlResult::Subscribed,
            ..
        }
    ));
    assert!(matches!(
        received[3],
        GuiObservationRecord::Control {
            request: 11,
            result: GuiObservationControlResult::Unsubscribed,
            ..
        }
    ));
    let terminals = outcomes(&mut host, world);
    assert_eq!(terminals.len(), 3);
    for (index, terminal) in terminals.iter().enumerate() {
        let effect = terminal.effect();
        assert_eq!(
            effect.id,
            Some(GuiEffectId {
                world: target.world,
                ordinal: index as u64 + 1
            })
        );
        if index < 2 {
            let GuiObservationRecord::Effect {
                effect: observed,
                ..
            } = &received[index + 1]
            else {
                panic!("missing effect")
            };
            assert_eq!(observed.as_ref(), effect);
            assert_eq!(effect.tick, terminals[0].effect().tick);
        }
    }
    assert_eq!(snapshot(&mut host, world, entity).value, GuiTestValue::None);
}

#[test]
fn application_and_feedback_classes_share_ordinals_but_noops_publish_nothing() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    frame(&mut host);
    let target = snapshot(&mut host, world, entity).target;
    let application = output();
    let feedback = output();
    let _application_subscription = subscribed(
        &mut host,
        world,
        &application,
        GuiObservationClasses::Application,
    );
    let _feedback_subscription =
        subscribed(&mut host, world, &feedback, GuiObservationClasses::Feedback);
    for operation in [
        GuiLocalAction::Focus,
        GuiLocalAction::Focus,
        GuiLocalAction::Press,
        GuiLocalAction::Blur,
        GuiLocalAction::Blur,
    ] {
        action(&mut host, world, target, operation);
    }
    frame(&mut host);
    let terminals = outcomes(&mut host, world);
    let ids: Vec<_> = terminals
        .iter()
        .map(|terminal| {
            terminal
                .result
                .as_ref()
                .unwrap()
                .as_ref()
                .and_then(|effect| effect.id.map(|id| id.ordinal))
        })
        .collect();
    assert_eq!(ids, [Some(1), None, Some(2), Some(3), None]);
    assert!(
        matches!(records(&application).as_slice(), [GuiObservationRecord::Effect { effect, .. }] if effect.id.unwrap().ordinal == 2)
    );
    assert_eq!(records(&feedback).len(), 2);
}

#[test]
fn foreign_output_exhaustion_preserves_other_observers_and_applied_actions() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    frame(&mut host);
    let target = snapshot(&mut host, world, entity).target;
    let slow = output();
    let healthy = output();
    let slow_subscription = subscribed(&mut host, world, &slow, GuiObservationClasses::Application);
    let _healthy_subscription = subscribed(
        &mut host,
        world,
        &healthy,
        GuiObservationClasses::Application,
    );
    for _ in 0..2 {
        action(&mut host, world, target, GuiLocalAction::Press);
    }
    frame(&mut host);
    assert!(
        outcomes(&mut host, world)
            .iter()
            .all(|result| result.result.is_ok())
    );
    assert_eq!(slow.account().status(), OutputStatus::Open);
    let filler = exhaust(&slow);
    action(&mut host, world, target, GuiLocalAction::Press);
    frame(&mut host);
    assert_eq!(
        slow.account().status(),
        OutputStatus::Failed(OutputFailure::Capacity)
    );
    assert_eq!(slow.account().usage().entries, 2);
    assert!(!slow_subscription.is_active());
    assert!(slow.pop_front().is_none());
    assert_eq!(records(&healthy).len(), 3);
    assert!(
        outcomes(&mut host, world)
            .iter()
            .all(|result| result.result.is_ok())
    );
    slow.close();
    drop(filler);
    assert_eq!(slow.account().usage().entries, 0);
    action(&mut host, world, target, GuiLocalAction::Press);
    frame(&mut host);
    assert!(
        matches!(records(&healthy).as_slice(), [GuiObservationRecord::Effect { effect, .. }] if effect.id.unwrap().ordinal == 4)
    );
}

#[test]
fn unsubscribe_cancels_a_delayed_generation_without_retargeting_its_replacement() {
    let (mut host, world) = fixture();
    let output = output();
    let reference = host.world_ref(world).unwrap();
    let old = output
        .new_subscription(reference, GuiObservationClasses::All)
        .unwrap();
    let lease = output
        .account()
        .reserve(OutputCharge {
            entries: 1,
            bytes: 0,
        })
        .unwrap();
    let delayed =
        GuiObservationCommand::prepare_subscribe(&output, reference, &old, 3, lease).unwrap();
    cut(&mut host, world, &output, &old, 1, false);
    frame(&mut host);
    records(&output);
    let replacement = subscribed(&mut host, world, &output, GuiObservationClasses::All);
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command(GuiSystem::ID, 44, delayed)
        .unwrap();
    cut(&mut host, world, &output, &old, 4, false);
    frame(&mut host);
    assert!(replacement.is_active());
    assert!(!old.is_active());
    assert!(matches!(
        records(&output).as_slice(),
        [
            GuiObservationRecord::Control {
                result: GuiObservationControlResult::Rejected(
                    GuiObservationRejection::StaleSubscription
                ),
                ..
            },
            GuiObservationRecord::Control {
                result: GuiObservationControlResult::Unsubscribed,
                ..
            }
        ]
    ));
}

#[test]
fn discarded_commands_publish_one_cancel_and_never_activate() {
    let (host, world) = fixture();
    let output = output();
    let reference = host.world_ref(world).unwrap();
    let subscription = output
        .new_subscription(reference, GuiObservationClasses::Application)
        .unwrap();
    let lease = output
        .account()
        .reserve(OutputCharge {
            entries: 1,
            bytes: 0,
        })
        .unwrap();
    let command =
        GuiObservationCommand::prepare_subscribe(&output, reference, &subscription, 1, lease)
            .unwrap();
    assert!(output.pop_front().is_none());
    drop(command);
    assert!(!subscription.is_active());
    assert!(matches!(
        records(&output).as_slice(),
        [GuiObservationRecord::Control {
            result: GuiObservationControlResult::Cancelled,
            ..
        }]
    ));
}

#[test]
fn queued_subscribe_and_unsubscribe_cancel_once_on_session_release_without_a_frame() {
    let (mut host, world) = fixture();
    let output = output();
    let active = subscribed(&mut host, world, &output, GuiObservationClasses::All);
    let reference = host.world_ref(world).unwrap();
    let pending = output
        .new_subscription(reference, GuiObservationClasses::All)
        .unwrap();
    cut(&mut host, world, &output, &pending, 10, true);
    cut(&mut host, world, &output, &active, 11, false);
    let tick = host.world_mut(world).unwrap().tick();
    assert_eq!(output.account().usage().entries, 2);
    assert!(output.pop_front().is_none());
    host.world_mut(world).unwrap().release_system_session(44);
    host.world_mut(world).unwrap().release_system_session(44);
    assert_eq!(host.world_mut(world).unwrap().tick(), tick);
    assert!(active.is_active());
    assert!(!pending.is_active());
    assert_eq!(
        records(&output),
        vec![
            GuiObservationRecord::Control {
                world: reference,
                subscription: pending.id(),
                request: 10,
                result: GuiObservationControlResult::Cancelled,
            },
            GuiObservationRecord::Control {
                world: reference,
                subscription: active.id(),
                request: 11,
                result: GuiObservationControlResult::Cancelled,
            },
        ]
    );
    assert_eq!(output.account().usage().entries, 0);
    let report = host.frame(0.125).unwrap();
    assert!(
        report.worlds[&world]
            .as_ref()
            .unwrap()
            .system_command_outcomes
            .is_empty()
    );
    assert!(output.pop_front().is_none());
    assert!(active.is_active());
    assert!(!pending.is_active());
}

#[test]
fn misdispatched_observer_group_cancels_unexecuted_tail_in_the_same_frame() {
    use crate::systems::lifecycle_publisher::LifecyclePublisherSystem;

    let (mut host, _) = fixture();
    let world = host
        .create_world(
            Default::default(),
            &[
                LifecyclePublisherSystem::ID,
                crate::systems::canvas::CanvasSystem::ID,
                GuiSystem::ID,
            ],
        )
        .unwrap();
    let output = output();
    let active = subscribed(&mut host, world, &output, GuiObservationClasses::All);
    let reference = host.world_ref(world).unwrap();
    let pending = output
        .new_subscription(reference, GuiObservationClasses::All)
        .unwrap();
    let commands = vec![
        prepare_cut(reference, &output, &pending, 10, true),
        prepare_cut(reference, &output, &active, 11, false),
    ];
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command_batch_with_reply(LifecyclePublisherSystem::ID, 44, 0, commands)
        .unwrap();
    assert!(output.pop_front().is_none());
    let report = host.frame(0.125).unwrap();
    assert!(
        report.worlds[&world]
            .as_ref()
            .unwrap()
            .system_command_outcomes
            .is_empty()
    );
    assert_eq!(
        records(&output),
        vec![
            GuiObservationRecord::Control {
                world: reference,
                subscription: pending.id(),
                request: 10,
                result: GuiObservationControlResult::Cancelled,
            },
            GuiObservationRecord::Control {
                world: reference,
                subscription: active.id(),
                request: 11,
                result: GuiObservationControlResult::Cancelled,
            },
        ]
    );
    assert!(active.is_active());
    assert!(!pending.is_active());
    assert_eq!(output.account().usage().entries, 0);
    frame(&mut host);
    assert!(output.pop_front().is_none());
}

#[test]
fn world_destruction_keeps_committed_observations_and_cancels_pending_cuts_without_a_frame() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    frame(&mut host);
    let target = snapshot(&mut host, world, entity).target;
    let output = output();
    let active = subscribed(&mut host, world, &output, GuiObservationClasses::All);
    action(&mut host, world, target, GuiLocalAction::Press);
    frame(&mut host);
    let effect = outcomes(&mut host, world).remove(0).into_effect();
    assert_eq!(effect.kind, GuiLocalEffectKind::Pressed);
    let pending = output
        .new_subscription(target.world, GuiObservationClasses::All)
        .unwrap();
    cut(&mut host, world, &output, &pending, 10, true);
    cut(&mut host, world, &output, &active, 11, false);
    assert_eq!(output.account().usage().entries, 3);
    assert!(host.destroy_world(world));
    assert_eq!(host.world_ref(world), None);
    assert!(!active.is_active());
    assert!(!pending.is_active());
    assert_eq!(output.account().status(), OutputStatus::Open);
    assert_eq!(output.account().usage().entries, 3);
    let received = records(&output);
    assert_eq!(
        received,
        vec![
            GuiObservationRecord::Effect {
                subscription: active.id(),
                effect: Arc::new(effect.clone()),
            },
            GuiObservationRecord::Control {
                world: target.world,
                subscription: pending.id(),
                request: 10,
                result: GuiObservationControlResult::Cancelled,
            },
            GuiObservationRecord::Control {
                world: target.world,
                subscription: active.id(),
                request: 11,
                result: GuiObservationControlResult::Cancelled,
            },
        ]
    );
    assert_eq!(
        effect.id.unwrap(),
        GuiEffectId {
            world: target.world,
            ordinal: 1
        }
    );
    assert_eq!(effect.target, target);
    assert_eq!(effect.ancestry.as_ref(), &[entity]);
    let GuiObservationRecord::Effect {
        effect: observed,
        ..
    } = &received[0]
    else {
        panic!("missing committed observation")
    };
    assert_eq!(observed.kind, GuiLocalEffectKind::Pressed);
    assert_eq!(output.account().usage().entries, 0);
    let replacement = host
        .create_world(Default::default(), &super::local_tests::GUI_SYSTEMS)
        .unwrap();
    assert_ne!(host.world_ref(replacement), Some(target.world));
    let replacement_subscription =
        subscribed(&mut host, replacement, &output, GuiObservationClasses::All);
    assert!(replacement_subscription.is_active());
    assert!(!active.is_active());
    assert_eq!(observed.id.unwrap().world, target.world);
    assert!(output.pop_front().is_none());
}

#[test]
fn retained_effect_has_original_ancestry_after_reparenting_and_component_replacement() {
    let (mut host, world) = fixture();
    let parent = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    let child = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    apply(
        &mut host,
        world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(child),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        }],
    );
    frame(&mut host);
    let target = snapshot(&mut host, world, child).target;
    let output = output();
    let _subscription = subscribed(
        &mut host,
        world,
        &output,
        GuiObservationClasses::Application,
    );
    action(&mut host, world, target, GuiLocalAction::Press);
    frame(&mut host);
    apply(
        &mut host,
        world,
        vec![
            Command::PlaceEntity {
                entity: EntityRef::Handle(child),
                placement: EntityPlacementRef {
                    parent: None,
                    before: None,
                },
            },
            Command::insert_value(
                EntityRef::Handle(child),
                ComponentValue::GuiButton(GuiButton::default()),
            ),
        ],
    );
    let GuiObservationRecord::Effect {
        effect,
        ..
    } = records(&output).remove(0)
    else {
        panic!("effect")
    };
    assert_eq!(effect.target, target);
    assert_eq!(effect.ancestry.as_ref(), &[parent, child]);
    assert_ne!(
        snapshot(&mut host, world, child).target.incarnation,
        target.incarnation
    );
}

#[test]
fn refused_actions_consume_no_ordinal_and_lifetime_replacement_never_resets_it() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    frame(&mut host);
    let target = snapshot(&mut host, world, entity).target;
    let output = output();
    let _subscription = subscribed(&mut host, world, &output, GuiObservationClasses::All);
    // Refused actions change nothing and consume no ordinal.
    let stale = GuiEntityTarget {
        incarnation: target.incarnation + 1,
        ..target
    };
    action(&mut host, world, stale, GuiLocalAction::Focus);
    action(&mut host, world, stale, GuiLocalAction::Press);
    frame(&mut host);
    assert!(!snapshot(&mut host, world, entity).focused);
    assert!(records(&output).is_empty());
    assert!(
        outcomes(&mut host, world)
            .iter()
            .all(|outcome| outcome.result == Err(crate::ErrorReason::StaleTarget))
    );
    action(&mut host, world, target, GuiLocalAction::Press);
    frame(&mut host);
    assert!(
        matches!(records(&output).as_slice(), [GuiObservationRecord::Effect { effect, .. }] if effect.id.unwrap().ordinal == 1)
    );
    apply(
        &mut host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_BUTTON,
        }],
    );
    frame(&mut host);
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::GuiButton(GuiButton::default()),
        )],
    );
    frame(&mut host);
    let replacement = snapshot(&mut host, world, entity).target;
    assert_ne!(replacement.incarnation, target.incarnation);
    action(&mut host, world, replacement, GuiLocalAction::Press);
    frame(&mut host);
    assert!(
        matches!(records(&output).as_slice(), [GuiObservationRecord::Effect { effect, .. }] if effect.id.unwrap().ordinal == 2 && effect.target == replacement)
    );
}

#[test]
fn subscription_receiver_checks_actual_world_and_world_ordinals_are_independent() {
    let (mut host, first) = fixture();
    let second = host
        .create_world(Default::default(), &super::local_tests::GUI_SYSTEMS)
        .unwrap();
    let output = output();
    let first_ref = host.world_ref(first).unwrap();
    let subscription = output
        .new_subscription(first_ref, GuiObservationClasses::Application)
        .unwrap();
    let lease = output
        .account()
        .reserve(OutputCharge {
            entries: 1,
            bytes: 0,
        })
        .unwrap();
    let command =
        GuiObservationCommand::prepare_subscribe(&output, first_ref, &subscription, 1, lease)
            .unwrap();
    host.world_mut(second)
        .unwrap()
        .enqueue_system_command(GuiSystem::ID, 7, command)
        .unwrap();
    frame(&mut host);
    assert!(matches!(
        records(&output).as_slice(),
        [GuiObservationRecord::Control {
            result: GuiObservationControlResult::Rejected(GuiObservationRejection::StaleWorld),
            ..
        }]
    ));
    for world in [first, second] {
        let entity = create(
            &mut host,
            world,
            ComponentValue::GuiButton(GuiButton::default()),
        );
        frame(&mut host);
        let target = snapshot(&mut host, world, entity).target;
        let _subscription = subscribed(
            &mut host,
            world,
            &output,
            GuiObservationClasses::Application,
        );
        action(&mut host, world, target, GuiLocalAction::Press);
        frame(&mut host);
        assert!(
            matches!(records(&output).as_slice(), [GuiObservationRecord::Effect { effect, .. }] if effect.id.unwrap() == GuiEffectId { world: target.world, ordinal: 1 })
        );
    }
}

#[test]
fn routed_pointer_feedback_observes_only_changed_records_without_application_callbacks() {
    let (mut host, _, target, context) =
        super::receiver_tests::presented(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let world = target.world.id();
    let feedback = output();
    let application = output();
    let _feedback_subscription =
        subscribed(&mut host, world, &feedback, GuiObservationClasses::Feedback);
    let _application_subscription = subscribed(
        &mut host,
        world,
        &application,
        GuiObservationClasses::Application,
    );
    let prior = snapshot(&mut host, world, target.entity).value;
    let mut lease: Option<crate::services::gui_input::GuiPointerLease> = None;

    for update in [
        GuiInteractionUpdate::Hover(true),
        GuiInteractionUpdate::Hover(true),
        GuiInteractionUpdate::Press,
        GuiInteractionUpdate::Capture,
        GuiInteractionUpdate::Release,
    ] {
        let request = host.next_request();
        let input = host
            .service
            .reserve_routed(
                &host.host,
                &context,
                target,
                request,
                &[],
                host.permit(world, request),
            )
            .unwrap();
        let pointer = lease
            .get_or_insert_with(|| host.service.pointer_lease(&input, 7).unwrap())
            .clone();
        let command = GuiLocalCommand::interaction(input, pointer, update).unwrap();
        host.world_mut(world)
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 7, command)
            .unwrap();
    }

    frame(&mut host);
    assert!(records(&application).is_empty());
    let observed = records(&feedback);
    assert_eq!(observed.len(), 4);
    for (index, record) in observed.iter().enumerate() {
        let GuiObservationRecord::Effect {
            effect,
            ..
        } = record
        else {
            panic!("feedback record")
        };
        assert_eq!(effect.id.unwrap().ordinal, index as u64 + 1);
        assert!(matches!(effect.source, GuiLocalEffectSource::Routed { .. }));
        assert!(
            matches!(&effect.kind, GuiLocalEffectKind::InteractionChanged(feedback) if feedback.changed)
        );
    }
    let after = snapshot(&mut host, world, target.entity);
    assert_eq!(prior, after.value);
    assert!(!after.focused);
    let terminals = outcomes(&mut host, world);
    assert_eq!(terminals.len(), 5);
    assert!(terminals[1].effect().id.is_none());
}
