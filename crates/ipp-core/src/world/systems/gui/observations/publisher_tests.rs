use super::*;
use crate::services::gui_input::GuiInputError;
use crate::services::reliable_output::*;
use crate::systems::gui::local::{GuiEntityTarget, GuiLocalEffectSource};
use crate::{EntityId, HostRuntime};

fn effect() -> GuiLocalEffect {
    let mut host = HostRuntime::new();
    let world = host
        .create_world(
            Default::default(),
            &[
                crate::systems::canvas::CanvasSystem::ID,
                crate::systems::gui::GuiSystem::ID,
            ],
        )
        .unwrap();
    let world = host.world_ref(world).unwrap();
    GuiLocalEffect {
        id: Some(GuiEffectId {
            world,
            ordinal: 1,
        }),
        target: GuiEntityTarget {
            world,
            entity: EntityId::from_bits(1),
            component: crate::ComponentValue::GUI_BUTTON,
            incarnation: 1,
        },
        source: GuiLocalEffectSource::Semantic,
        tick: 1,
        ancestry: vec![EntityId::from_bits(1)].into(),
        kind: GuiLocalEffectKind::Pressed,
    }
}

#[test]
fn ordinal_is_preflighted_without_consumption_and_noop_survives_exhaustion() {
    let mut publisher = GuiEffectPublisher::default();
    let effect = effect();
    for _ in 0..2 {
        assert_eq!(
            publisher
                .candidate_id(effect.target.world, &effect.kind)
                .unwrap(),
            effect.id
        );
    }
    publisher.publish(&effect);
    assert_eq!(
        publisher
            .candidate_id(effect.target.world, &effect.kind)
            .unwrap()
            .unwrap()
            .ordinal,
        2
    );
    publisher.ordinal = u64::MAX;
    assert_eq!(
        publisher.candidate_id(effect.target.world, &effect.kind),
        Err(GuiInputError::Capacity)
    );

    // Unchanged focus is never published, so it needs no ordinal even when
    // none is left.
    let unchanged = GuiLocalEffectKind::FocusChanged {
        focused: false,
        changed: false,
    };
    assert_eq!(
        publisher
            .candidate_id(effect.target.world, &unchanged)
            .unwrap(),
        None
    );
}

#[test]
fn retained_submitted_text_and_peak_encoding_stay_charged_through_transfer_and_close() {
    let account = ReliableOutputAccount::new(OutputLimits {
        bytes: 64 * 1024,
        reply_reserve: 0,
    });
    let output = GuiObservationOutput::new(
        account.clone(),
        GuiObservationEncoding {
            effect_bytes: 512,
            ancestry_entry_bytes: 16,
            text_byte_bytes: 2,
            control_bytes: 64,
        },
    )
    .unwrap();
    let mut effect = effect();
    let text: Arc<str> = "a".repeat(8192).into();
    let length = text.len();
    effect.kind = GuiLocalEffectKind::Submitted(text);
    let subscription = output
        .new_subscription(effect.target.world, GuiObservationClasses::Application)
        .unwrap();
    let baseline = account.usage();
    output.0.observe(subscription.id(), &effect);
    let delivery = output.pop_front().unwrap();
    assert_eq!(delivery.charge().entries, 1);
    assert!(
        delivery.charge().bytes
            >= length + 512 + 16 + 2 * length + std::mem::size_of::<GuiLocalEffect>()
    );
    assert_eq!(
        account.usage().bytes,
        baseline.bytes + delivery.charge().bytes
    );
    output.close();
    account.close();
    assert_eq!(account.usage().entries, 1);
    let (record, mut lease) = delivery.into_parts();
    drop(record);
    lease
        .resize(OutputCharge {
            entries: 1,
            bytes: 538,
        })
        .unwrap();
    assert_eq!(account.usage().bytes, baseline.bytes + 538);
    drop(output);
    assert!(account.usage().bytes > 538);
    drop(subscription);
    assert_eq!(account.usage().bytes, 538);
    drop(lease);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn endpoint_allocation_credit_survives_every_retained_subscription_weak_reference() {
    let probe = ReliableOutputAccount::new(OutputLimits {
        bytes: 8192,
        reply_reserve: 0,
    });
    let world = effect().target.world;
    let output = GuiObservationOutput::new(probe.clone(), Default::default()).unwrap();
    let subscription = output
        .new_subscription(world, GuiObservationClasses::All)
        .unwrap();
    let pair_charge = probe.usage();
    drop(subscription);
    drop(output);
    assert_eq!(probe.usage(), OutputCharge::default());

    let account = ReliableOutputAccount::new(OutputLimits {
        bytes: pair_charge.bytes * 2,
        reply_reserve: 0,
    });
    let mut retained = Vec::new();
    for count in 1..=2 {
        let output = GuiObservationOutput::new(account.clone(), Default::default()).unwrap();
        let subscription = output
            .new_subscription(world, GuiObservationClasses::All)
            .unwrap();
        output.close();
        drop(output);
        assert!(!subscription.is_active());
        assert!(subscription.0.output.upgrade().is_none());
        retained.push(subscription);
        assert_eq!(account.usage().bytes, pair_charge.bytes * count);
    }
    assert!(matches!(
        GuiObservationOutput::new(account.clone(), Default::default()),
        Err(OutputReserveError::Capacity)
    ));
    drop(retained.pop());
    let replacement = GuiObservationOutput::new(account.clone(), Default::default()).unwrap();
    assert!(!retained[0].is_active());
    assert!(retained[0].0.output.upgrade().is_none());
    drop(replacement);
    drop(retained);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn endpoint_credit_is_shared_until_the_last_distinct_weak_subscription_drops() {
    let account = ReliableOutputAccount::new(OutputLimits {
        bytes: 8192,
        reply_reserve: 0,
    });
    let world = effect().target.world;
    let output = GuiObservationOutput::new(account.clone(), Default::default()).unwrap();
    let endpoint = account.usage().bytes;
    let first = output
        .new_subscription(world, GuiObservationClasses::All)
        .unwrap();
    let subscription = account.usage().bytes - endpoint;
    let second = output
        .new_subscription(world, GuiObservationClasses::All)
        .unwrap();
    assert_eq!(account.usage().bytes, endpoint + 2 * subscription);
    drop(output);
    assert_eq!(account.usage().bytes, endpoint + 2 * subscription);
    assert!(first.0.output.upgrade().is_none());
    drop(first);
    assert_eq!(account.usage().bytes, endpoint + subscription);
    assert!(second.0.output.upgrade().is_none());
    drop(second);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn arithmetic_overflow_fails_only_destination_without_rejecting_the_record() {
    let account = ReliableOutputAccount::new(OutputLimits {
        bytes: 8192,
        reply_reserve: 0,
    });
    let output = GuiObservationOutput::new(
        account.clone(),
        GuiObservationEncoding {
            effect_bytes: usize::MAX,
            ..Default::default()
        },
    )
    .unwrap();
    let effect = effect();
    let subscription = output
        .new_subscription(effect.target.world, GuiObservationClasses::Application)
        .unwrap();
    output.0.observe(subscription.id(), &effect);
    assert_eq!(
        account.status(),
        OutputStatus::Failed(OutputFailure::InvalidPayload)
    );
    assert_eq!(account.usage().entries, 0);
}

#[test]
fn preparation_rejects_foreign_lease_and_output_without_losing_reserved_credit() {
    let account = ReliableOutputAccount::new(OutputLimits {
        bytes: 8192,
        reply_reserve: 0,
    });
    let foreign = ReliableOutputAccount::new(OutputLimits {
        bytes: 8192,
        reply_reserve: 0,
    });
    let output = GuiObservationOutput::new(account.clone(), Default::default()).unwrap();
    let other_output = GuiObservationOutput::new(account.clone(), Default::default()).unwrap();
    let world = effect().target.world;
    let subscription = output
        .new_subscription(world, GuiObservationClasses::Application)
        .unwrap();
    let lease = foreign
        .reserve(OutputCharge {
            entries: 1,
            bytes: 64,
        })
        .unwrap();
    let error =
        match GuiObservationCommand::prepare_subscribe(&output, world, &subscription, 1, lease) {
            Ok(_) => panic!("foreign account admitted"),
            Err(error) => error,
        };
    assert_eq!(error.reason, GuiObservationPrepareFailure::InvalidLease);
    assert_eq!(foreign.usage(), error.lease.charge());
    let lease = account
        .reserve(OutputCharge {
            entries: 1,
            bytes: 64,
        })
        .unwrap();
    let error = match GuiObservationCommand::prepare_subscribe(
        &other_output,
        world,
        &subscription,
        1,
        lease,
    ) {
        Ok(_) => panic!("foreign output admitted"),
        Err(error) => error,
    };
    assert_eq!(error.reason, GuiObservationPrepareFailure::InvalidIdentity);
    assert!(!subscription.is_active());
}

#[test]
fn closed_account_keeps_prepared_command_credit_until_drop_without_fake_failure() {
    let account = ReliableOutputAccount::new(OutputLimits {
        bytes: 8192,
        reply_reserve: 0,
    });
    let output = GuiObservationOutput::new(account.clone(), Default::default()).unwrap();
    let world = effect().target.world;
    let subscription = output
        .new_subscription(world, GuiObservationClasses::All)
        .unwrap();
    let lease = account
        .reserve(OutputCharge {
            entries: 1,
            bytes: 64,
        })
        .unwrap();
    let command =
        GuiObservationCommand::prepare_subscribe(&output, world, &subscription, 1, lease).unwrap();
    let retained = account.usage();
    account.close();
    assert_eq!(account.usage(), retained);
    let mut publisher = GuiEffectPublisher::default();
    publisher.command(world, &command);
    assert_eq!(account.status(), OutputStatus::Closed);
    assert_eq!(account.usage().entries, 0);
    assert!(!subscription.is_active());
}

#[test]
fn command_metadata_remains_charged_after_its_control_result_is_drained() {
    let account = ReliableOutputAccount::new(OutputLimits {
        bytes: 8192,
        reply_reserve: 0,
    });
    let output = GuiObservationOutput::new(account.clone(), Default::default()).unwrap();
    let world = effect().target.world;
    let subscription = output
        .new_subscription(world, GuiObservationClasses::All)
        .unwrap();
    let lease = account
        .reserve(OutputCharge {
            entries: 1,
            bytes: 64,
        })
        .unwrap();
    let command =
        GuiObservationCommand::prepare_subscribe(&output, world, &subscription, 1, lease).unwrap();
    let mut publisher = GuiEffectPublisher::default();
    publisher.command(world, &command);
    drop(output.pop_front().unwrap());
    let before = account.usage();
    assert_eq!(before.entries, 0);
    drop(command);
    assert_eq!(
        before.bytes - account.usage().bytes,
        std::mem::size_of::<GuiObservationCommand>()
    );
    drop(output);
    publisher.prune();
    drop(subscription);
    assert_eq!(account.usage(), OutputCharge::default());
}
