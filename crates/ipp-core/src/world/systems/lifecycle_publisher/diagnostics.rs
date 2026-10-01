//! Allocation-free diagnostic counters, not lifecycle history or frame barriers.

/// Cumulative sparse-index work for this selected publisher's lifetime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LifecycleTargetWork {
    /// One lookup per entity/component transition, independent of target count.
    pub lookups: u64,
    /// Exact-target candidates visited before kind and liveness filtering.
    pub recipient_visits: u64,
    /// Sticky overflow flag; saturated samples cannot establish exact deltas.
    pub saturated: bool,
}

impl LifecycleTargetWork {
    pub(super) fn record(&mut self, lookups: usize, candidates: usize) {
        increment(&mut self.lookups, lookups, &mut self.saturated);
        increment(&mut self.recipient_visits, candidates, &mut self.saturated);
    }
}

/// Cumulative successfully retained events for one exact output lifetime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LifecycleWatchTraffic {
    /// Events successfully enqueued; never decremented on drain or removal.
    pub queued_events: u64,
    /// Their complete reserved byte charges, including retained and encoding storage.
    pub queued_bytes: u64,
    /// Sticky overflow flag; saturated samples cannot establish exact deltas.
    pub saturated: bool,
}

impl LifecycleWatchTraffic {
    pub(super) fn record(&mut self, bytes: usize) {
        increment(&mut self.queued_events, 1, &mut self.saturated);
        increment(&mut self.queued_bytes, bytes, &mut self.saturated);
    }
}

fn increment(counter: &mut u64, amount: usize, saturated: &mut bool) {
    let Ok(amount) = u64::try_from(amount) else {
        *counter = u64::MAX;
        *saturated = true;
        return;
    };
    *saturated |= counter.checked_add(amount).is_none();
    *counter = counter.saturating_add(amount);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_saturate_and_never_clear_the_overflow_flag() {
        let mut work = LifecycleTargetWork {
            lookups: u64::MAX,
            recipient_visits: u64::MAX - 1,
            saturated: false,
        };
        work.record(1, 2);
        assert_eq!(work.lookups, u64::MAX);
        assert_eq!(work.recipient_visits, u64::MAX);
        assert!(work.saturated);
        work.record(0, 0);
        assert!(work.saturated);

        let mut traffic = LifecycleWatchTraffic {
            queued_events: u64::MAX,
            queued_bytes: u64::MAX - 1,
            saturated: false,
        };
        traffic.record(2);
        assert_eq!(traffic.queued_events, u64::MAX);
        assert_eq!(traffic.queued_bytes, u64::MAX);
        assert!(traffic.saturated);
        traffic.record(0);
        assert!(traffic.saturated);
    }
}
