use super::*;

fn limits(bytes: usize, reply_reserve: usize) -> OutputLimits {
    OutputLimits {
        bytes,
        reply_reserve,
    }
}

fn charge(entries: usize, bytes: usize) -> OutputCharge {
    OutputCharge {
        entries,
        bytes,
    }
}

#[test]
fn reservations_and_mixed_resize_are_atomic_and_identity_fenced() {
    let account = ReliableOutputAccount::new(limits(100, 0));
    let other = ReliableOutputAccount::new(limits(100, 0));
    let mut lease = account.reserve(charge(1, 40)).unwrap();
    let metadata = account.reserve(charge(0, 20)).unwrap();
    assert_eq!(account.record_usage(), charge(1, 40));
    assert!(lease.belongs_to(&account.clone()));
    assert!(!lease.belongs_to(&other));
    assert_eq!(
        lease.resize(charge(2, 81)),
        Err(OutputReserveError::Capacity)
    );
    assert_eq!(lease.charge(), charge(1, 40));
    assert_eq!(account.usage(), charge(1, 60));

    lease.resize(charge(2, 80)).unwrap();
    assert!(account.reserve(charge(0, 1)).is_err());
    assert_eq!(account.usage(), charge(2, 100));
    assert_eq!(account.record_usage(), charge(2, 80));

    lease.resize(charge(0, 10)).unwrap();
    assert_eq!(account.record_usage(), OutputCharge::default());
    drop(metadata);
    drop(lease);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn records_are_bounded_only_by_their_byte_charges() {
    let account = ReliableOutputAccount::new(limits(1000, 0));
    let records: Vec<_> = (0..100)
        .map(|_| account.reserve(charge(1, 10)).unwrap())
        .collect();
    assert_eq!(account.usage(), charge(100, 1000));
    assert_eq!(
        account.reserve(charge(1, 10)).err(),
        Some(OutputReserveError::Capacity)
    );

    // Byte-free records cannot exceed a full budget's bytes, so an owner charging
    // per-record bookkeeping bounds the record count through bytes alone.
    let free = account.reserve(charge(1, 0)).unwrap();
    assert_eq!(account.usage(), charge(101, 1000));
    assert_eq!(account.record_usage(), charge(101, 1000));
    drop(free);
    drop(records);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn ordinary_output_leaves_the_reply_reserve_for_replies() {
    let account = ReliableOutputAccount::new(limits(100, 30));
    let mut events = account.reserve(charge(1, 70)).unwrap();
    assert_eq!(events.class(), OutputClass::Ordinary);
    assert_eq!(
        account.reserve(charge(1, 1)).err(),
        Some(OutputReserveError::Capacity)
    );
    assert_eq!(
        events.resize(charge(1, 71)),
        Err(OutputReserveError::Capacity)
    );

    let mut reply = account
        .reserve_as(OutputClass::Reply, charge(1, 10))
        .unwrap();
    assert_eq!(reply.class(), OutputClass::Reply);
    reply.resize(charge(1, 30)).unwrap();
    assert_eq!(account.usage(), charge(2, 100));
    assert_eq!(account.reply_usage(), charge(1, 30));
    assert_eq!(
        reply.resize(charge(1, 31)),
        Err(OutputReserveError::Capacity)
    );

    // Ordinary leases may shrink while replies hold usage above the ordinary share.
    events.resize(charge(1, 60)).unwrap();
    assert_eq!(
        events.resize(charge(1, 61)),
        Err(OutputReserveError::Capacity)
    );
    drop(reply);
    assert_eq!(account.reply_usage(), OutputCharge::default());
    events.resize(charge(1, 70)).unwrap();
    drop(events);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn overflow_and_failures_do_not_mutate_usage() {
    let account = ReliableOutputAccount::new(limits(usize::MAX, 0));
    let lease = account.reserve(charge(1, 1)).unwrap();
    assert!(matches!(
        account.reserve(charge(usize::MAX, 0)),
        Err(OutputReserveError::Capacity)
    ));
    assert!(matches!(
        account.reserve(charge(0, usize::MAX)),
        Err(OutputReserveError::Capacity)
    ));
    assert_eq!(account.status(), OutputStatus::Open);
    assert_eq!(account.usage(), lease.charge());
}

#[test]
fn closed_and_failed_accounts_keep_retained_charges_and_allow_only_shrink() {
    for failure in [false, true] {
        let account = ReliableOutputAccount::new(limits(100, 10));
        let mut lease = account
            .reserve_as(OutputClass::Reply, charge(2, 50))
            .unwrap();
        account.close();
        if failure {
            account.fail(OutputFailure::Capacity);
            account.fail(OutputFailure::InvalidPayload);
            account.close();
        }
        let expected = if failure {
            OutputReserveError::Failed(OutputFailure::Capacity)
        } else {
            OutputReserveError::Closed
        };
        assert!(
            matches!(account.reserve(OutputCharge::default()), Err(error) if error == expected)
        );
        assert!(matches!(
            account.reserve_as(OutputClass::Reply, OutputCharge::default()),
            Err(error) if error == expected
        ));
        assert_eq!(lease.resize(charge(1, 51)), Err(expected));
        assert_eq!(account.usage(), charge(2, 50));
        lease.resize(charge(1, 20)).unwrap();
        assert_eq!(account.usage(), lease.charge());
        let retained = account.clone();
        drop(account);
        drop(lease);
        assert_eq!(retained.usage(), OutputCharge::default());
    }
}

#[test]
fn transfer_retains_entry_and_peak_payload_credit_without_reacquisition() {
    let account = ReliableOutputAccount::new(limits(120, 0));
    let mut lease = account.reserve(charge(1, 40)).unwrap();
    lease.resize(charge(1, 120)).unwrap();
    let mut outbox_lease = lease;
    assert!(account.reserve(charge(1, 1)).is_err());
    outbox_lease.resize(charge(1, 80)).unwrap();
    account.fail(OutputFailure::InvalidPayload);
    assert_eq!(account.usage(), charge(1, 80));
    drop(outbox_lease);
    assert_eq!(account.usage(), OutputCharge::default());
}
