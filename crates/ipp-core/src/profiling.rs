//! Opt-in stage timing and allocation counters for profiling builds.
//!
//! Stage slots hold four counters each (calls, nanoseconds, allocation calls,
//! requested bytes) at `slot * 4`. Slots `0..FIXED_STAGE_BASE` time the
//! scheduled Systems: `position * SYSTEM_PHASES + phase`, with phases in
//! check, accept, restore, prepare, evaluate and finish order. Slots from
//! [`FIXED_STAGE_BASE`] time fixed commit and animation sections, so a System
//! at any timed position never shares a slot with a fixed timer. Each stage
//! also owns allocation category `slot + 1`; named allocation scopes use
//! categories above [`STAGE_SLOTS`].
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};

static ENABLED: AtomicBool = AtomicBool::new(false);

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static NAMES: [std::sync::Mutex<&'static str>; 32] = [const { std::sync::Mutex::new("") }; 32];

/// Timed phases per scheduled System.
pub const SYSTEM_PHASES: usize = 6;

/// Scheduled System positions with stage slots; later positions are not timed.
pub const SYSTEM_STAGE_LIMIT: usize = 30;

/// First stage slot of the fixed timers, after every System slot.
pub const FIXED_STAGE_BASE: usize = SYSTEM_STAGE_LIMIT * SYSTEM_PHASES;

/// Total stage slots; counters hold four values per slot.
pub const STAGE_SLOTS: usize = 192;

/// Fixed timers inside commit and animation work, independent of schedule positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FixedStage {
    AnimationSampleAndStage,
    AnimationRestoreAndStage,
    AnimationApplyComponent,
    CommitValidate,
    CommitBefore,
    CommitStorage,
    CommitAfter,
    AnimationValidate,
    AnimationInvalidate,
}

impl FixedStage {
    const ALL: [Self; 9] = [
        Self::AnimationSampleAndStage,
        Self::AnimationRestoreAndStage,
        Self::AnimationApplyComponent,
        Self::CommitValidate,
        Self::CommitBefore,
        Self::CommitStorage,
        Self::CommitAfter,
        Self::AnimationValidate,
        Self::AnimationInvalidate,
    ];

    fn slot(self) -> usize {
        FIXED_STAGE_BASE + self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Self::AnimationSampleAndStage => "profile.animation.sample_and_stage",
            Self::AnimationRestoreAndStage => "profile.animation.restore_and_stage",
            Self::AnimationApplyComponent => "profile.animation.apply_component",
            Self::CommitValidate => "profile.commit.validate",
            Self::CommitBefore => "profile.commit.before",
            Self::CommitStorage => "profile.commit.storage",
            Self::CommitAfter => "profile.commit.after",
            Self::AnimationValidate => "profile.animation.validate",
            Self::AnimationInvalidate => "profile.animation.invalidate",
        }
    }
}

// Stage categories are `slot + 1`; named allocation scopes start at 193.
const _: () = assert!(FIXED_STAGE_BASE + FixedStage::ALL.len() <= STAGE_SLOTS);
const _: () = assert!(STAGE_SLOTS < 193);

static ACTIVE_CATEGORY: AtomicUsize = AtomicUsize::new(0);
static CATEGORY_NAMES: [std::sync::Mutex<&'static str>; 256] =
    [const { std::sync::Mutex::new("") }; 256];
static CATEGORY_COUNTS: [AtomicU64; 512] = [const { AtomicU64::new(0) }; 512];

static COUNTERS: [AtomicU64; STAGE_SLOTS * 4] = [const { AtomicU64::new(0) }; STAGE_SLOTS * 4];

/// Opt-in allocator for profiling executables; libraries do not select a global allocator.
pub struct CountingAllocator;

// SAFETY: Every operation forwards the original layout/pointer to System. Counters
// do not allocate, retain pointers, or alter allocation lifetime or aliasing.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: The caller supplies a valid allocation layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: The caller supplies a valid allocation layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: The caller owns this live allocation from the forwarded allocator.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(size);
        // SAFETY: Caller guarantees a live allocation, matching layout and valid size.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

fn count(bytes: usize) {
    if ENABLED.load(Relaxed) {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(bytes as u64, Relaxed);
        let index = ACTIVE_CATEGORY.load(Relaxed) * 2;
        CATEGORY_COUNTS[index].fetch_add(1, Relaxed);
        CATEGORY_COUNTS[index + 1].fetch_add(bytes as u64, Relaxed);
    }
}

/// Clear all measurement counters and enable/disable instrumented profiling.
pub fn reset(enabled: bool) {
    ENABLED.store(false, Relaxed);
    ALLOCS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    for counter in &COUNTERS {
        counter.store(0, Relaxed);
    }
    for counter in &CATEGORY_COUNTS {
        counter.store(0, Relaxed);
    }
    ENABLED.store(enabled, Relaxed);
}

/// Stop counting while retaining the captured counters for readback.
pub fn pause() {
    ENABLED.store(false, Relaxed);
}

/// Total allocation/reallocation calls and requested bytes, not retained memory.
pub fn allocations() -> (u64, u64) {
    (ALLOCS.load(Relaxed), BYTES.load(Relaxed))
}

/// Counter tuple per stage: calls, nanoseconds, allocation calls, requested bytes.
pub fn counter(index: usize) -> u64 {
    COUNTERS[index].load(Relaxed)
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "ipp_profiling")]
unsafe extern "C" {
    fn now() -> f64;
}

fn nanos() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: The profiling host supplies a pure, non-reentrant monotonic clock.
        (unsafe { now() } * 1_000_000.0) as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_nanos() as u64
    }
}

pub(crate) struct Stage {
    index: usize,
    start: u64,
    allocations: (u64, u64),
    enabled: bool,
    _category: Option<AllocationScope>,
}

impl Stage {
    /// Time one phase of the System at schedule `position`. Positions from
    /// [`SYSTEM_STAGE_LIMIT`] are not timed rather than sharing another slot.
    pub(crate) fn system(position: usize, phase: usize, name: &'static str) -> Self {
        if position >= SYSTEM_STAGE_LIMIT || phase >= SYSTEM_PHASES {
            return Self::disabled();
        }

        if ENABLED.load(Relaxed) {
            *NAMES[position].lock().unwrap() = name;
        }
        Self::start(position * SYSTEM_PHASES + phase, name)
    }

    /// Time one fixed commit or animation section in its own slot.
    pub(crate) fn fixed(stage: FixedStage) -> Self {
        Self::start(stage.slot(), stage.name())
    }

    fn start(index: usize, name: &'static str) -> Self {
        let enabled = ENABLED.load(Relaxed);
        Self {
            _category: Some(AllocationScope::new(index + 1, name)),
            index,
            start: if enabled {
                nanos()
            } else {
                0
            },
            allocations: allocations(),
            enabled,
        }
    }

    fn disabled() -> Self {
        Self {
            index: 0,
            start: 0,
            allocations: (0, 0),
            enabled: false,
            _category: None,
        }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if self.enabled {
            let current = allocations();
            for (offset, value) in [
                1,
                nanos() - self.start,
                current.0 - self.allocations.0,
                current.1 - self.allocations.1,
            ]
            .into_iter()
            .enumerate()
            {
                COUNTERS[self.index * 4 + offset].fetch_add(value, Relaxed);
            }
        }
    }
}

/// Stable schedule label for the System at one timed position.
pub fn system_name(index: usize) -> &'static str {
    *NAMES[index].lock().unwrap()
}

/// Label of one stage slot: `system#phase` names come from [`system_name`];
/// fixed slots return their fixed timer name, and unused slots are empty.
pub fn fixed_stage_name(slot: usize) -> &'static str {
    slot.checked_sub(FIXED_STAGE_BASE)
        .and_then(|ordinal| FixedStage::ALL.get(ordinal))
        .map_or("", |stage| stage.name())
}

/// Exclusive allocation attribution for a single-threaded profiling Host.
/// Inner scopes own their allocations, so categories sum to the global total.
/// Fixed counters and static names never allocate while an allocator is running.
pub struct AllocationScope {
    previous: usize,
    enabled: bool,
}

impl AllocationScope {
    /// Enter a fixed, exclusive category until this scope is dropped.
    pub fn new(index: usize, name: &'static str) -> Self {
        let enabled = ENABLED.load(Relaxed);
        let previous = if enabled {
            *CATEGORY_NAMES[index].lock().unwrap() = name;
            ACTIVE_CATEGORY.swap(index, Relaxed)
        } else {
            0
        };
        Self {
            previous,
            enabled,
        }
    }
}

impl Drop for AllocationScope {
    fn drop(&mut self) {
        if self.enabled {
            ACTIVE_CATEGORY.store(self.previous, Relaxed);
        }
    }
}

/// Static category label, empty until that scope has been measured.
pub fn category_name(index: usize) -> &'static str {
    *CATEGORY_NAMES[index].lock().unwrap()
}

/// Flat counter index: category * 2 for calls, then requested bytes.
pub fn category_counter(index: usize) -> u64 {
    CATEGORY_COUNTS[index].load(Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_timers_never_share_a_system_slot() {
        let last_system_slot = SYSTEM_STAGE_LIMIT * SYSTEM_PHASES - 1;

        for stage in FixedStage::ALL {
            assert!(stage.slot() > last_system_slot);
            assert!(stage.slot() < STAGE_SLOTS);
            assert_eq!(fixed_stage_name(stage.slot()), stage.name());
        }

        assert_eq!(fixed_stage_name(last_system_slot), "");
    }
}
