//! Opt-in stage timing and allocation counters for `instrumentation` builds.
//!
//! Each (System, composition) key registered at World construction owns
//! [`SYSTEM_PHASES`] stage slots, `system * SYSTEM_PHASES + phase`, with phases in
//! check, accept, prepare, evaluate, finish and observe order. Keys are never
//! freed, and their counters grow with registration instead of leaving later
//! Systems untimed. Every stage slot holds four counters (calls, nanoseconds,
//! allocation calls, requested bytes) at `slot * 4`; the fixed commit and
//! animation timers follow the System slots from [`fixed_stage_base`].
//!
//! Allocation categories below [`CATEGORIES`] are static: 0 is unattributed,
//! fixed timers use `1..=9`, and named allocation scopes use `193..`. Category
//! `CATEGORIES + slot` belongs to System stage slot `slot`. Categories are
//! exclusive, so their calls sum to the allocator total.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};

static ENABLED: AtomicBool = AtomicBool::new(false);

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct SystemProfileEntry {
    name: &'static str,
    composition: u64,
}

struct SystemProfileRegistry {
    entries: Vec<SystemProfileEntry>,
}

impl SystemProfileRegistry {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// The stable profile index of one key, appended on first registration.
    fn register(&mut self, name: &'static str, composition: u64) -> usize {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.name == name && entry.composition == composition)
        {
            return index;
        }

        self.entries.push(SystemProfileEntry {
            name,
            composition,
        });
        self.entries.len() - 1
    }
}

static SYSTEM_PROFILES: std::sync::Mutex<SystemProfileRegistry> =
    std::sync::Mutex::new(SystemProfileRegistry::new());

/// Counters in the first segment of a [`GrowingCounters`]; each later segment doubles.
const FIRST_SEGMENT: usize = 1024;

/// Segments of a [`GrowingCounters`], enough for about 2^34 counters.
const SEGMENTS: usize = 24;

/// Append-only atomic counters in doubling segments that never move or shrink.
///
/// Stage timers and the counting allocator index them without locks and
/// without allocating; only registration allocates a missing segment. A read
/// during that allocation sees the segment as absent and skips it.
struct GrowingCounters {
    segments: [OnceLock<Box<[AtomicU64]>>; SEGMENTS],
}

impl GrowingCounters {
    const fn new() -> Self {
        Self {
            segments: [const { OnceLock::new() }; SEGMENTS],
        }
    }

    fn locate(index: usize) -> (usize, usize) {
        let segment = (index / FIRST_SEGMENT + 1).ilog2() as usize;
        (segment, index - FIRST_SEGMENT * ((1 << segment) - 1))
    }

    fn get(&self, index: usize) -> Option<&AtomicU64> {
        let (segment, offset) = Self::locate(index);
        self.segments.get(segment)?.get()?.get(offset)
    }

    /// Make every index below `len` addressable.
    fn reserve(&self, len: usize) {
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

    fn clear(&self) {
        for counter in self.segments.iter().filter_map(OnceLock::get).flatten() {
            counter.store(0, Relaxed);
        }
    }
}

/// Four counters per System stage slot.
static SYSTEM_COUNTERS: GrowingCounters = GrowingCounters::new();

/// Call and byte counters per System stage allocation category.
static SYSTEM_CATEGORY_COUNTS: GrowingCounters = GrowingCounters::new();

/// Timed phases per scheduled System.
pub const SYSTEM_PHASES: usize = 6;

/// Static allocation categories; System stage categories follow them.
pub const CATEGORIES: usize = 256;

/// Register one semantic profile key at World construction, before frame dispatch.
///
/// Returns its stable profile index; the key's counters exist from then on.
pub fn register_system(name: &'static str, composition: u64) -> usize {
    let index = SYSTEM_PROFILES.lock().unwrap().register(name, composition);
    let slots = (index + 1) * SYSTEM_PHASES;
    SYSTEM_COUNTERS.reserve(slots * 4);
    SYSTEM_CATEGORY_COUNTS.reserve(slots * 2);
    index
}

/// Registered semantic profile keys, readable through [`system_name`].
pub fn system_count() -> usize {
    SYSTEM_PROFILES.lock().unwrap().entries.len()
}

/// First stage slot of the fixed timers, after every registered System slot.
pub fn fixed_stage_base() -> usize {
    system_count() * SYSTEM_PHASES
}

/// Entries readable through [`counter`]: System slots, then the fixed timers.
pub fn counter_count() -> usize {
    (fixed_stage_base() + FixedStage::ALL.len()) * 4
}

/// Entries readable through [`category_name`].
pub fn category_count() -> usize {
    CATEGORIES + fixed_stage_base()
}

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

// Fixed timer categories are `ordinal + 1`; named allocation scopes start at 193.
const _: () = assert!(FixedStage::ALL.len() < 193);

static ACTIVE_CATEGORY: AtomicUsize = AtomicUsize::new(0);
static CATEGORY_NAMES: [std::sync::Mutex<&'static str>; CATEGORIES] =
    [const { std::sync::Mutex::new("") }; CATEGORIES];
static CATEGORY_COUNTS: [AtomicU64; CATEGORIES * 2] = [const { AtomicU64::new(0) }; CATEGORIES * 2];

static FIXED_COUNTERS: [AtomicU64; FixedStage::ALL.len() * 4] =
    [const { AtomicU64::new(0) }; FixedStage::ALL.len() * 4];

/// One flat category counter: category * 2 for calls, then requested bytes.
fn category_slot(index: usize) -> Option<&'static AtomicU64> {
    match index.checked_sub(CATEGORIES * 2) {
        None => CATEGORY_COUNTS.get(index),
        Some(system) => SYSTEM_CATEGORY_COUNTS.get(system),
    }
}

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
        if let (Some(calls), Some(requested)) = (category_slot(index), category_slot(index + 1)) {
            calls.fetch_add(1, Relaxed);
            requested.fetch_add(bytes as u64, Relaxed);
        }
    }
}

/// Clear all measurement counters and enable/disable instrumented profiling.
pub fn reset(enabled: bool) {
    ENABLED.store(false, Relaxed);
    ALLOCS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    for counter in FIXED_COUNTERS.iter().chain(&CATEGORY_COUNTS) {
        counter.store(0, Relaxed);
    }
    SYSTEM_COUNTERS.clear();
    SYSTEM_CATEGORY_COUNTS.clear();
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
///
/// Indices below [`fixed_stage_base`]` * 4` read System stages; the fixed timers
/// follow. Indices from [`counter_count`] read zero.
pub fn counter(index: usize) -> u64 {
    stage_counter(index, fixed_stage_base() * 4).map_or(0, |counter| counter.load(Relaxed))
}

fn stage_counter(index: usize, fixed_base: usize) -> Option<&'static AtomicU64> {
    match index.checked_sub(fixed_base) {
        None => SYSTEM_COUNTERS.get(index),
        Some(fixed) => FIXED_COUNTERS.get(fixed),
    }
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

/// Counters one timed section adds to when it ends.
#[derive(Clone, Copy)]
enum StageCounters {
    System(usize),
    Fixed(usize),
}

impl StageCounters {
    fn get(self, offset: usize) -> Option<&'static AtomicU64> {
        match self {
            Self::System(slot) => SYSTEM_COUNTERS.get(slot * 4 + offset),
            Self::Fixed(ordinal) => FIXED_COUNTERS.get(ordinal * 4 + offset),
        }
    }
}

pub(crate) struct Stage {
    counters: Option<StageCounters>,
    start: u64,
    allocations: (u64, u64),
    _category: Option<AllocationScope>,
}

impl Stage {
    /// Time a pre-resolved semantic System slot without a per-frame lookup.
    pub(crate) fn system(profile: Option<usize>, phase: usize, name: &'static str) -> Self {
        if phase >= SYSTEM_PHASES || !ENABLED.load(Relaxed) {
            return Self::disabled();
        }
        let Some(profile) = profile else {
            return Self::disabled();
        };
        let slot = profile * SYSTEM_PHASES + phase;
        Self::start(StageCounters::System(slot), CATEGORIES + slot, name)
    }

    /// Time one fixed commit or animation section in its own slot.
    pub(crate) fn fixed(stage: FixedStage) -> Self {
        let ordinal = stage as usize;
        Self::start(StageCounters::Fixed(ordinal), ordinal + 1, stage.name())
    }

    fn start(counters: StageCounters, category: usize, name: &'static str) -> Self {
        let enabled = ENABLED.load(Relaxed);
        Self {
            _category: Some(AllocationScope::new(category, name)),
            counters: enabled.then_some(counters),
            start: if enabled {
                nanos()
            } else {
                0
            },
            allocations: allocations(),
        }
    }

    fn disabled() -> Self {
        Self {
            counters: None,
            start: 0,
            allocations: (0, 0),
            _category: None,
        }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if let Some(counters) = self.counters {
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
                if let Some(counter) = counters.get(offset) {
                    counter.fetch_add(value, Relaxed);
                }
            }
        }
    }
}

/// Stable schedule label for the System at one timed position.
pub fn system_name(index: usize) -> &'static str {
    SYSTEM_PROFILES
        .lock()
        .unwrap()
        .entries
        .get(index)
        .map_or("", |entry| entry.name)
}

/// Composition identity paired with [`system_name`] at one profile slot.
pub fn system_composition(index: usize) -> u64 {
    SYSTEM_PROFILES
        .lock()
        .unwrap()
        .entries
        .get(index)
        .map_or(0, |entry| entry.composition)
}

/// Label of one stage slot: `system#phase` names come from [`system_name`];
/// fixed slots return their fixed timer name, and unused slots are empty.
pub fn fixed_stage_name(slot: usize) -> &'static str {
    slot.checked_sub(fixed_stage_base())
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
            if let Some(label) = CATEGORY_NAMES.get(index) {
                *label.lock().unwrap() = name;
            }
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

/// Category label: a static category's name once its scope has been measured,
/// and the System name of a System stage category.
pub fn category_name(index: usize) -> &'static str {
    match index.checked_sub(CATEGORIES) {
        None => *CATEGORY_NAMES[index].lock().unwrap(),
        Some(slot) => system_name(slot / SYSTEM_PHASES),
    }
}

/// Flat counter index: category * 2 for calls, then requested bytes.
pub fn category_counter(index: usize) -> u64 {
    category_slot(index).map_or(0, |counter| counter.load(Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_timers_follow_every_registered_system_slot() {
        for base in [
            0,
            30 * SYSTEM_PHASES,
            31 * SYSTEM_PHASES,
            1000 * SYSTEM_PHASES,
        ] {
            for stage in FixedStage::ALL {
                let counter = stage_counter((base + stage as usize) * 4, base * 4).unwrap();
                assert!(std::ptr::eq(counter, &FIXED_COUNTERS[stage as usize * 4]));
            }
            assert!(stage_counter((base + FixedStage::ALL.len()) * 4, base * 4).is_none());
        }

        // Other tests register Systems concurrently and the registry only
        // grows, so read the names again until the base stays put around them.
        let names = loop {
            let base = fixed_stage_base();
            let names: Vec<_> = FixedStage::ALL
                .iter()
                .map(|stage| fixed_stage_name(base + *stage as usize))
                .collect();
            if fixed_stage_base() == base {
                break names;
            }
        };
        for (stage, name) in FixedStage::ALL.iter().zip(names) {
            assert_eq!(name, stage.name());
        }
    }

    #[test]
    fn system_slots_follow_identity_not_selected_schedule_position() {
        let mut profiles = SystemProfileRegistry::new();
        let one = profiles.register("profile.test.a", 11);
        let other = profiles.register("profile.test.b", 11);
        let different_composition = profiles.register("profile.test.a", 12);
        assert_eq!(profiles.register("profile.test.a", 11), one);
        assert_ne!(one, different_composition);
        assert_ne!(one, other);
        assert_eq!(profiles.entries[one].composition, 11);
        assert_eq!(profiles.entries[different_composition].composition, 12);
    }

    #[test]
    fn growing_counters_address_every_reserved_index_across_segments() {
        let counters = GrowingCounters::new();
        assert!(counters.get(0).is_none());

        let len = FIRST_SEGMENT * 7 + 1;
        counters.reserve(len);
        for index in [
            0,
            FIRST_SEGMENT - 1,
            FIRST_SEGMENT,
            FIRST_SEGMENT * 3,
            len - 1,
        ] {
            counters
                .get(index)
                .unwrap()
                .store(index as u64 + 1, Relaxed);
        }
        for index in [
            0,
            FIRST_SEGMENT - 1,
            FIRST_SEGMENT,
            FIRST_SEGMENT * 3,
            len - 1,
        ] {
            assert_eq!(counters.get(index).unwrap().load(Relaxed), index as u64 + 1);
        }
        assert_eq!(GrowingCounters::locate(FIRST_SEGMENT * 3), (2, 0));

        counters.clear();
        assert_eq!(counters.get(len - 1).unwrap().load(Relaxed), 0);
    }

    #[test]
    fn systems_registered_after_many_others_are_still_timed() {
        let composition = u64::MAX - 0x5eed;
        // More keys than the former fixed 30-slot table, all in one composition.
        let names: Vec<&'static str> = (0..40)
            .map(|index| &*Box::leak(format!("profile.test.growth.{index:02}").into_boxed_str()))
            .collect();
        let profiles: Vec<_> = names
            .iter()
            .map(|name| register_system(name, composition))
            .collect();
        assert!(system_count() >= names.len());

        let last = *profiles.last().unwrap();
        assert_eq!(system_name(last), names[names.len() - 1]);
        assert_eq!(system_composition(last), composition);

        ENABLED.store(true, Relaxed);
        for &profile in &profiles {
            drop(Stage::system(Some(profile), 3, "profile.test.growth"));
        }
        drop(Stage::system(Some(last), 5, names[names.len() - 1]));
        let observe = (last * SYSTEM_PHASES + 5) * 4;
        let category = (CATEGORIES + last * SYSTEM_PHASES + 5) * 2;
        let timed: Vec<_> = profiles
            .iter()
            .map(|profile| counter((profile * SYSTEM_PHASES + 3) * 4))
            .collect();
        ENABLED.store(false, Relaxed);

        assert!(timed.iter().all(|&calls| calls >= 1), "{timed:?}");
        assert!(counter(observe) >= 1);
        assert!(observe < counter_count());
        assert!(category < category_count() * 2);
        assert_eq!(category_name(category / 2), names[names.len() - 1]);
    }
}
