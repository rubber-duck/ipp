use super::*;
use ipp_core::services::reliable_output::OutputLimits;

fn response(budget: &SharedReplyBudget, length: usize) -> ReliableResponse {
    let reservation = ReplyReservation::new(budget.clone(), OutputClass::Reply, length).unwrap();
    ReliableResponse {
        bytes: vec![7; length],
        reservation: Rc::new(RefCell::new(reservation)),
    }
}

#[test]
fn copy_peak_is_reserved_before_allocation_and_survives_source_invalidation() {
    let length = 512 * 1024;
    let budget = SharedReplyBudget::default();
    let source = response(&budget, length);
    let original = budget.0.usage();
    let mut copy = source.prepare_copy().ok().unwrap();
    let peak = OutputCharge {
        entries: 1,
        bytes: length * 2 + RESPONSE_METADATA_BYTES + COPY_METADATA_BYTES,
    };
    assert_eq!(budget.0.usage(), peak);
    assert_eq!(copy.bytes().len(), length);

    let delivered = copy.bytes().to_vec();
    copy.release_source();
    assert!(copy.bytes().is_empty());
    assert_eq!(budget.0.usage().entries, original.entries);
    assert_eq!(budget.0.usage().bytes, peak.bytes - length);
    budget.0.close();
    assert_eq!(budget.0.usage().bytes, peak.bytes - length);
    drop(delivered);
    drop(copy);
    assert_eq!(budget.0.usage(), OutputCharge::default());
}

#[test]
fn copy_capacity_failure_retains_the_original_response_and_charge() {
    let length = 512 * 1024;
    let budget = SharedReplyBudget(ReliableOutputAccount::new(OutputLimits {
        bytes: length * 2 + RESPONSE_METADATA_BYTES + COPY_METADATA_BYTES - 1,
        reply_reserve: 0,
    }));
    let original = response(&budget, length);
    let before = budget.0.usage();
    let (returned, error) = original.prepare_copy().err().unwrap();
    assert_eq!(error, ErrorReason::Capacity);
    assert_eq!(returned.len(), length);
    assert_eq!(budget.0.usage(), before);
    assert_eq!(
        budget.0.status(),
        ipp_core::services::reliable_output::OutputStatus::Open
    );
    drop(returned);
    assert_eq!(budget.0.usage(), OutputCharge::default());
}

/// Keep `RESPONSE_METADATA_BYTES` honest: it must cover what one retained record actually keeps.
#[test]
fn record_overhead_covers_retained_bookkeeping() {
    use std::mem::size_of;

    let pointer = size_of::<usize>();
    // Allocator header and size-class rounding for each heap allocation a record owns.
    let allocation = 2 * pointer;
    let outbox_node = 2 * pointer + size_of::<ReliableResponse>() + allocation;
    let reservation = 2 * pointer + size_of::<RefCell<ReplyReservation>>() + allocation;
    let payload = allocation;
    // B-tree leaves may be half full, doubling the per-entry share of a node.
    let correlation =
        2 * (size_of::<(u64, (u64, Option<u64>))>() + size_of::<(u64, SharedReplyReservation)>());
    // A WASM delivery node holding the prepared copy, or a native in-flight lease and channel slot.
    let transport = (2 * pointer + 2 * size_of::<u64>() + size_of::<PreparedOutputCopy>())
        .max(size_of::<ResponseLease>() + size_of::<Vec<u8>>() + 2 * pointer)
        + allocation;
    let measured = outbox_node + reservation + payload + correlation + transport;
    println!("record-bookkeeping measured={measured} charged={RESPONSE_METADATA_BYTES}");
    assert!(2 * measured <= RESPONSE_METADATA_BYTES, "{measured}");
}
