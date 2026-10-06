//! Append-only segmented atomic counters shared by stage and allocation tables.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// Counters in the first segment of a [`GrowingCounters`]; each later segment doubles.
pub(super) const FIRST_SEGMENT: usize = 1024;

/// Segments of a [`GrowingCounters`], enough for about 2^34 counters.
const SEGMENTS: usize = 24;

/// Append-only atomic counters in doubling segments that never move or shrink.
///
/// Stage timers and the counting allocator index them without locks and
/// without allocating; only registration allocates a missing segment. A read
/// during that allocation sees the segment as absent and skips it.
pub(super) struct GrowingCounters {
    pub(super) segments: [OnceLock<Box<[AtomicU64]>>; SEGMENTS],
}

impl GrowingCounters {
    pub(super) const fn new() -> Self {
        Self {
            segments: [const { OnceLock::new() }; SEGMENTS],
        }
    }

    pub(super) fn locate(index: usize) -> (usize, usize) {
        let segment = (index / FIRST_SEGMENT + 1).ilog2() as usize;
        (segment, index - FIRST_SEGMENT * ((1 << segment) - 1))
    }

    pub(super) fn get(&self, index: usize) -> Option<&AtomicU64> {
        let (segment, offset) = Self::locate(index);
        self.segments.get(segment)?.get()?.get(offset)
    }

    /// Make every index below `len` addressable.
    pub(super) fn reserve(&self, len: usize) {
        let Some(last) = len.checked_sub(1) else {
            return;
        };

        for segment in 0..=Self::locate(last).0 {
            self.segments[segment].get_or_init(|| {
                (0..FIRST_SEGMENT << segment)
                    .map(|_| AtomicU64::new(0))
                    .collect()
            });
        }
    }

    pub(super) fn clear(&self) {
        for counter in self.segments.iter().filter_map(OnceLock::get).flatten() {
            counter.store(0, Relaxed);
        }
    }
}
