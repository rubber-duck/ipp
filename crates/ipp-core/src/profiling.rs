//! Opt-in stage timing and allocation counters for `instrumentation` builds.
//!
//! System and phase slots are prepared for one Host/World incarnation at
//! construction. Contextual fixed scopes and exclusive allocator categories
//! retain that identity, including nested System/phase context. Shared work has
//! explicit zero identities. Counters are appendable segmented atomics: hot
//! counter paths never allocate, format labels or lock the registry. Optional
//! span capture copies context through the registry at issuance into a bounded
//! preallocated recorder; disabled tracing does not touch recorder storage.
//!
//! Destroyed Worlds retain metadata until [`release_capture`]. Release pauses
//! measurement, clears the capture and makes only retired slots reusable; live
//! prepared slots never move. Storage retains its high-water capacity. This is
//! single-thread Host attribution. Allocations on background I/O threads are
//! excluded; totals describe the captured evaluation thread, not the process. Export
//! flat indices are read at a paused observation boundary without registration;
//! internal active category handles remain stable even during registry growth.
//! System stage counters are inclusive; allocation category counters are
//! exclusive and sum to the total requested allocation calls/bytes.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};

static ENABLED: AtomicBool = AtomicBool::new(false);

// Only the capture owner's evaluation thread participates. Background I/O
// allocations are excluded, never assigned to the current World. Const TLS
// initialization cannot allocate; try_with also remains safe during teardown.
std::thread_local! {
    static EVALUATION_CAPTURE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static SUPPRESSED_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn participating() -> bool {
    SUPPRESSED_DEPTH
        .try_with(|depth| depth.get() == 0)
        .unwrap_or(false)
        && EVALUATION_CAPTURE
            .try_with(|capture| capture.get() == CAPTURE.load(Relaxed))
            .unwrap_or(false)
}

fn enabled() -> bool {
    ENABLED.load(Relaxed) && participating()
}

pub(crate) fn measuring() -> bool {
    (ENABLED.load(Relaxed) || crate::profiling_trace::enabled()) && participating()
}

/// Enable the already prepared optional CPU span recorder on the owner thread.
pub fn enable_trace() {
    EVALUATION_CAPTURE.with(|capture| capture.set(capture_id()));
    crate::profiling_trace::enable();
}

static ALLOCATOR_PRESENT: AtomicBool = AtomicBool::new(false);

/// Whether this binary has actually selected and used the counting allocator.
/// Libraries never select an allocator for their embedding application.
pub fn allocator_available() -> bool {
    ALLOCATOR_PRESENT.load(Relaxed)
}

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct SystemProfileEntry {
    name: &'static str,
    composition: u64,
    world: usize,
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
    fn register(&mut self, name: &'static str, composition: u64, world: usize) -> usize {
        if let Some(index) = self.entries.iter().position(|entry| {
            entry.name == name && entry.composition == composition && entry.world == world
        }) {
            return index;
        }

        let entry = SystemProfileEntry {
            name,
            composition,
            world,
        };
        if let Some(index) = self.entries.iter().position(|entry| entry.name.is_empty()) {
            self.entries[index] = entry;
            index
        } else {
            self.entries.push(entry);
            self.entries.len() - 1
        }
    }
}

static SYSTEM_SLOTS: AtomicUsize = AtomicUsize::new(0);
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
pub fn register_system(name: &'static str, composition: u64, world: usize) -> usize {
    let index = SYSTEM_PROFILES
        .lock()
        .unwrap()
        .register(name, composition, world);
    let slots = (index + 1) * SYSTEM_PHASES;
    SYSTEM_COUNTERS.reserve(slots * 4);
    SYSTEM_CATEGORY_COUNTS.reserve(slots * 2);
    PROFILE_CONTEXTS.reserve(slots);
    for phase in 0..SYSTEM_PHASES {
        let context = register_context(ProfileContext {
            system: name,
            phase: Some(ProfilePhase::ALL[phase]),
            ..world_context(world)
        });
        CONTEXT_ROOTS
            .get(context)
            .unwrap()
            .store(world as u64, Relaxed);
        PROFILE_CONTEXTS
            .get(index * SYSTEM_PHASES + phase)
            .unwrap()
            .store(context as u64, Relaxed);
    }
    SYSTEM_SLOTS.fetch_max(system_count() * SYSTEM_PHASES, Relaxed);
    index
}

/// Registered semantic profile keys, readable through [`system_name`].
pub fn system_count() -> usize {
    SYSTEM_PROFILES.lock().unwrap().entries.len()
}

/// First stage slot of the fixed timers, after every registered System slot.
pub fn fixed_stage_base() -> usize {
    SYSTEM_SLOTS.load(Relaxed)
}

/// Entries readable through [`counter`]: System slots, then the fixed timers.
pub fn counter_count() -> usize {
    (fixed_stage_base() + context_count() * FixedStage::ALL.len()) * 4
}

/// Entries readable through [`category_name`].
pub fn category_count() -> usize {
    CATEGORIES + fixed_stage_base() + context_count() * CONTEXT_CATEGORIES
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

static ACTIVE_GUARDS: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_CATEGORY: AtomicUsize = AtomicUsize::new(0);
static CATEGORY_NAMES: [OnceLock<&'static str>; CATEGORIES] =
    [const { OnceLock::new() }; CATEGORIES];
static CATEGORY_COUNTS: [AtomicU64; CATEGORIES * 2] = [const { AtomicU64::new(0) }; CATEGORIES * 2];

static FIXED_COUNTERS: GrowingCounters = GrowingCounters::new();

/// One flat category counter: category * 2 for calls, then requested bytes.
fn category_slot(index: usize) -> Option<&'static AtomicU64> {
    match index.checked_sub(CATEGORIES * 2) {
        None => CATEGORY_COUNTS.get(index),
        Some(system) if system < fixed_stage_base() * 2 => SYSTEM_CATEGORY_COUNTS.get(system),
        Some(context) => CONTEXT_CATEGORY_COUNTS.get(context - fixed_stage_base() * 2),
    }
}

/// Opt-in allocator for profiling executables; libraries do not select a global allocator.
pub struct CountingAllocator;

// SAFETY: Every operation forwards the original layout/pointer to System. Counters
// do not allocate, retain pointers, or alter allocation lifetime or aliasing.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATOR_PRESENT.store(true, Relaxed);
        count(layout.size());
        // SAFETY: The caller supplies a valid allocation layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATOR_PRESENT.store(true, Relaxed);
        count(layout.size());
        // SAFETY: The caller supplies a valid allocation layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: The caller owns this live allocation from the forwarded allocator.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATOR_PRESENT.store(true, Relaxed);
        count(size);
        // SAFETY: Caller guarantees a live allocation, matching layout and valid size.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

fn count(bytes: usize) {
    if enabled() {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(bytes as u64, Relaxed);
        let category = ACTIVE_CATEGORY.load(Relaxed);
        if let (Some(calls), Some(requested)) = (
            active_category_counter(category, 0),
            active_category_counter(category, 1),
        ) {
            calls.fetch_add(1, Relaxed);
            requested.fetch_add(bytes as u64, Relaxed);
        }
    }
}

/// Clear all measurement counters and enable/disable instrumented profiling.
/// Capture controls and all measured Hosts belong to one evaluation thread;
/// other evaluation threads are excluded. [`reset_for_host`] also excludes
/// other Hosts evaluated on the captured thread.
/// Advances the capture generation and clears active attribution. Guards from
/// the previous generation neither record timings nor restore their context;
/// allocations after reset remain unassigned until a new scope is entered.
pub fn reset(enabled: bool) {
    reset_for_host(enabled, 0);
}

/// Start capture owned by one Host and its current evaluation thread.
pub fn reset_for_host(enabled: bool, host: u64) {
    CAPTURE_HOST_FILTER.store(host, Relaxed);
    SUPPRESSED_DEPTH.with(|depth| depth.set(0));
    ENABLED.store(false, Relaxed);
    crate::profiling_trace::pause();
    register_context(ProfileContext::default());
    ACTIVE_CATEGORY.store(0, Relaxed);
    ACTIVE_CONTEXT.store(0, Relaxed);
    ALLOCS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    for counter in &CATEGORY_COUNTS {
        counter.store(0, Relaxed);
    }
    FIXED_COUNTERS.clear();
    CONTEXT_CATEGORY_COUNTS.clear();
    CAPTURE.fetch_add(1, Relaxed);
    SYSTEM_COUNTERS.clear();
    SYSTEM_CATEGORY_COUNTS.clear();
    EVALUATION_CAPTURE.with(|capture| {
        capture.set(if enabled {
            CAPTURE.load(Relaxed)
        } else {
            0
        })
    });
    ENABLED.store(enabled, Relaxed);
}

/// Stop counting while retaining the captured counters for readback.
pub fn pause() {
    ENABLED.store(false, Relaxed);
    crate::profiling_trace::pause();
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

/// Monotonic profiler clock in nanoseconds, for Host capture correlation.
pub fn clock_nanos() -> u64 {
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
    _context: Option<ContextScope>,
    capture: u64,
    span: Option<crate::profiling_trace::ProfileSpan>,
    trace_guard: bool,
}

impl Stage {
    /// Time a pre-resolved semantic System slot without a per-frame lookup.
    pub(crate) fn system(profile: Option<usize>, phase: usize, name: &'static str) -> Self {
        if phase >= SYSTEM_PHASES || !measuring() {
            return Self::disabled();
        }
        let Some(profile) = profile else {
            return Self::disabled();
        };
        let slot = profile * SYSTEM_PHASES + phase;
        let context = PROFILE_CONTEXTS
            .get(slot)
            .map_or(0, |value| value.load(Relaxed) as usize);
        let scope = ContextScope::new(context);
        let mut stage = Self::start(StageCounters::System(slot), CATEGORIES + slot, name);
        stage._context = Some(scope);
        stage
    }

    /// Time one fixed commit or animation section in its own slot.
    pub(crate) fn fixed(stage: FixedStage) -> Self {
        let ordinal = ACTIVE_CONTEXT.load(Relaxed) * FixedStage::ALL.len() + stage as usize;
        Self::start(
            StageCounters::Fixed(ordinal),
            stage as usize + 1,
            stage.name(),
        )
    }

    fn start(counters: StageCounters, category: usize, name: &'static str) -> Self {
        let enabled = enabled();
        let timed = measuring();
        let start = if timed {
            clock_nanos()
        } else {
            0
        };
        let span = if timed && crate::profiling_trace::enabled() {
            Some(crate::profiling_trace::begin(
                match counters {
                    StageCounters::System(_) => "system",
                    StageCounters::Fixed(_) => "fixed",
                },
                name,
                world_context(ACTIVE_CONTEXT.load(Relaxed)),
                start,
            ))
        } else {
            None
        };
        let trace_guard = span.is_some() && !enabled;
        if trace_guard {
            ACTIVE_GUARDS.fetch_add(1, Relaxed);
        }
        Self {
            span,
            trace_guard,
            _category: Some(AllocationScope::new(category, name)),
            counters: enabled.then_some(counters),
            start,
            allocations: allocations(),
            _context: None,
            capture: CAPTURE.load(Relaxed),
        }
    }

    fn disabled() -> Self {
        Self {
            counters: None,
            span: None,
            trace_guard: false,
            start: 0,
            allocations: (0, 0),
            _category: None,
            _context: None,
            capture: CAPTURE.load(Relaxed),
        }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if self.trace_guard {
            ACTIVE_GUARDS.fetch_sub(1, Relaxed);
        }
        if self.capture != CAPTURE.load(Relaxed) || !measuring() {
            return;
        }
        let end = clock_nanos();
        if let Some(span) = self.span.take() {
            crate::profiling_trace::finish(span, end);
        }
        if let Some(counters) = self.counters {
            let current = allocations();
            for (offset, value) in [
                1,
                end - self.start,
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

/// Trace one World evaluation without adding an aggregate counter slot.
pub(crate) struct WorldTraceScope {
    span: Option<crate::profiling_trace::ProfileSpan>,
    capture: u64,
}

impl WorldTraceScope {
    pub(crate) fn new() -> Self {
        let span = if measuring() && crate::profiling_trace::enabled() {
            Some(crate::profiling_trace::begin(
                "world",
                "World evaluation",
                world_context(ACTIVE_CONTEXT.load(Relaxed)),
                clock_nanos(),
            ))
        } else {
            None
        };
        if span.is_some() {
            ACTIVE_GUARDS.fetch_add(1, Relaxed);
        }
        Self {
            span,
            capture: capture_id(),
        }
    }
}

impl Drop for WorldTraceScope {
    fn drop(&mut self) {
        if let Some(span) = self.span.take() {
            ACTIVE_GUARDS.fetch_sub(1, Relaxed);
            if self.capture == capture_id() && measuring() {
                crate::profiling_trace::finish(span, clock_nanos());
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
        .map_or(0, |entry| {
            if entry.name.is_empty() {
                0
            } else {
                entry.composition
            }
        })
}

/// Label of one stage slot: `system#phase` names come from [`system_name`];
/// fixed slots return their fixed timer name, and unused slots are empty.
pub fn fixed_stage_name(slot: usize) -> &'static str {
    slot.checked_sub(fixed_stage_base())
        .and_then(|ordinal| {
            if ordinal < context_count() * FixedStage::ALL.len()
                && context_occupied(ordinal / FixedStage::ALL.len())
            {
                FixedStage::ALL.get(ordinal % FixedStage::ALL.len())
            } else {
                None
            }
        })
        .map_or("", |stage| stage.name())
}

/// Exclusive allocation attribution for a single-threaded profiling Host.
/// Inner scopes own their allocations, so categories sum to the global total.
/// Fixed counters and static names never allocate while an allocator is running.
pub struct AllocationScope {
    previous: usize,
    enabled: bool,
    capture: u64,
}

impl AllocationScope {
    /// Enter a fixed, exclusive category until this scope is dropped.
    pub fn new(index: usize, name: &'static str) -> Self {
        let enabled = enabled();
        if enabled {
            ACTIVE_GUARDS.fetch_add(1, Relaxed);
        }
        let previous = if enabled {
            if let Some(label) = CATEGORY_NAMES.get(index) {
                let _ = label.set(name);
            }
            let category = contextual_category(index);
            ACTIVE_CATEGORY.swap(category, Relaxed)
        } else {
            0
        };
        Self {
            previous,
            enabled,
            capture: capture_id(),
        }
    }
}

impl Drop for AllocationScope {
    fn drop(&mut self) {
        if self.enabled {
            ACTIVE_GUARDS.fetch_sub(1, Relaxed);
        }
        if self.enabled && self.capture == capture_id() {
            ACTIVE_CATEGORY.store(self.previous, Relaxed);
        }
    }
}

/// Category label: a static category's name once its scope has been measured,
/// and the System name of a System stage category.
pub fn category_name(index: usize) -> &'static str {
    match index.checked_sub(CATEGORIES) {
        None if index == 0 => "unattributed",
        None => CATEGORY_NAMES
            .get(index)
            .and_then(OnceLock::get)
            .copied()
            .unwrap_or(""),
        Some(slot) if slot < fixed_stage_base() => system_name(slot / SYSTEM_PHASES),
        Some(slot) => {
            if !context_occupied((slot - fixed_stage_base()) / CONTEXT_CATEGORIES) {
                return "";
            }
            let local = (slot - fixed_stage_base()) % CONTEXT_CATEGORIES;
            if local == 0 {
                "unattributed"
            } else if local <= FixedStage::ALL.len() {
                FixedStage::ALL[local - 1].name()
            } else {
                CATEGORY_NAMES[193 + local - 10]
                    .get()
                    .copied()
                    .unwrap_or("")
            }
        }
    }
}

/// Flat counter index: category * 2 for calls, then requested bytes.
pub fn category_counter(index: usize) -> u64 {
    category_slot(index).map_or(0, |counter| counter.load(Relaxed))
}

/// Stable semantic phases in runtime dispatch order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProfilePhase {
    /// Frame preflight.
    Check,
    /// Ingress acceptance.
    Accept,
    /// Evaluation preparation.
    Prepare,
    /// Evaluation.
    Evaluate,
    /// Update completion.
    Finish,
    /// Final observations.
    Observe,
}

impl ProfilePhase {
    /// Stable phase order, independent of selected Systems.
    pub const ALL: [Self; SYSTEM_PHASES] = [
        Self::Check,
        Self::Accept,
        Self::Prepare,
        Self::Evaluate,
        Self::Finish,
        Self::Observe,
    ];

    /// Stable export label.
    pub fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Accept => "accept",
            Self::Prepare => "prepare",
            Self::Evaluate => "evaluate",
            Self::Finish => "finish",
            Self::Observe => "observe",
        }
    }
}

/// Semantic identity attached to a measurement. Zero identities and an empty
/// System explicitly identify shared/unassigned work, never an arbitrary World.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfileContext {
    /// Runtime Host identity.
    pub host: u64,
    /// Host-local World handle.
    pub world: u64,
    /// Monotonic World lifetime identity.
    pub incarnation: u64,
    /// Exact selected composition.
    pub composition: u64,
    /// Stable System identity, empty outside System dispatch.
    pub system: &'static str,
    /// Dispatch phase, absent outside System dispatch.
    pub phase: Option<ProfilePhase>,
}

#[derive(Clone, Copy)]
struct ContextEntry {
    context: ProfileContext,
    live: bool,
    occupied: bool,
}

static CONTEXTS: std::sync::Mutex<Vec<ContextEntry>> = std::sync::Mutex::new(Vec::new());
static CONTEXT_ROOTS: GrowingCounters = GrowingCounters::new();
static PROFILE_CONTEXTS: GrowingCounters = GrowingCounters::new();
static CONTEXT_CATEGORY_COUNTS: GrowingCounters = GrowingCounters::new();
static CONTEXT_HOSTS: GrowingCounters = GrowingCounters::new();
static CAPTURE_HOST_FILTER: AtomicU64 = AtomicU64::new(0);
static ACTIVE_CONTEXT: AtomicUsize = AtomicUsize::new(0);
static CAPTURE: AtomicU64 = AtomicU64::new(0);
// Unassigned + nine fixed scopes + named static categories 193..256.
const CONTEXT_CATEGORIES: usize = 73;

fn register_context(context: ProfileContext) -> usize {
    let mut entries = CONTEXTS.lock().unwrap();
    if let Some(index) = entries
        .iter()
        .position(|entry| entry.occupied && entry.context == context)
    {
        return index;
    }
    let entry = ContextEntry {
        context,
        live: true,
        occupied: true,
    };
    let index = if let Some(index) = entries.iter().position(|entry| !entry.occupied) {
        entries[index] = entry;
        index
    } else {
        entries.push(entry);
        entries.len() - 1
    };
    CONTEXT_HOSTS.reserve(index + 1);
    CONTEXT_HOSTS
        .get(index)
        .unwrap()
        .store(context.host, Relaxed);
    CONTEXT_ROOTS.reserve(index + 1);
    FIXED_COUNTERS.reserve((index + 1) * FixedStage::ALL.len() * 4);
    CONTEXT_CATEGORY_COUNTS.reserve((index + 1) * CONTEXT_CATEGORIES * 2);
    index
}

/// Resolve a live World lifetime at asynchronous measurement issuance.
/// Copy the returned context into the issued record; never resolve it at completion.
/// Host matching is required because World handles are local to each Host.
pub fn world_profile_context(host: u64, reference: crate::WorldRef) -> Option<ProfileContext> {
    CONTEXTS.lock().unwrap().iter().find_map(|entry| {
        (entry.live
            && entry.occupied
            && entry.context.host == host
            && entry.context.world == reference.id().0
            && entry.context.incarnation == reference.incarnation()
            && entry.context.system.is_empty()
            && entry.context.phase.is_none())
        .then_some(entry.context)
    })
}

/// Prepare attribution before dispatch; construction is the only allocating path.
pub(crate) fn register_world(host: u64, world: u64, incarnation: u64, composition: u64) -> usize {
    register_context(ProfileContext::default());
    let index = register_context(ProfileContext {
        host,
        world,
        incarnation,
        composition,
        ..ProfileContext::default()
    });
    CONTEXT_ROOTS
        .get(index)
        .unwrap()
        .store(index as u64, Relaxed);
    index
}

fn context_count() -> usize {
    CONTEXTS.lock().unwrap().len()
}

fn context_occupied(index: usize) -> bool {
    CONTEXTS
        .lock()
        .unwrap()
        .get(index)
        .is_some_and(|entry| entry.occupied)
}

fn world_context(index: usize) -> ProfileContext {
    CONTEXTS
        .lock()
        .unwrap()
        .get(index)
        .filter(|entry| entry.occupied)
        .map_or(ProfileContext::default(), |entry| entry.context)
}

/// Identity of one System profile.
pub fn system_context(index: usize) -> ProfileContext {
    let entry = SYSTEM_PROFILES.lock().unwrap().entries.get(index).copied();
    entry
        .filter(|entry| !entry.name.is_empty())
        .map_or(ProfileContext::default(), |entry| ProfileContext {
            system: entry.name,
            ..world_context(entry.world)
        })
}

/// Identity of a flat timed stage slot, including contextual fixed scopes.
pub fn stage_context(slot: usize) -> ProfileContext {
    let base = fixed_stage_base();
    if slot < base {
        if system_name(slot / SYSTEM_PHASES).is_empty() {
            return ProfileContext::default();
        }
        let context = PROFILE_CONTEXTS
            .get(slot)
            .map_or(0, |value| value.load(Relaxed) as usize);
        world_context(context)
    } else {
        world_context((slot - base) / FixedStage::ALL.len())
    }
}

/// Identity of an exclusive allocation category.
pub fn category_context(category: usize) -> ProfileContext {
    let base = fixed_stage_base();
    if category < CATEGORIES {
        ProfileContext::default()
    } else if category - CATEGORIES < base {
        stage_context(category - CATEGORIES)
    } else {
        world_context((category - CATEGORIES - base) / CONTEXT_CATEGORIES)
    }
}

const CONTEXT_CATEGORY_TAG: usize = 1 << (usize::BITS - 1);

fn active_category_counter(category: usize, offset: usize) -> Option<&'static AtomicU64> {
    if category & CONTEXT_CATEGORY_TAG != 0 {
        CONTEXT_CATEGORY_COUNTS.get((category & !CONTEXT_CATEGORY_TAG) * 2 + offset)
    } else if category >= CATEGORIES {
        SYSTEM_CATEGORY_COUNTS.get((category - CATEGORIES) * 2 + offset)
    } else {
        CATEGORY_COUNTS.get(category * 2 + offset)
    }
}

fn contextual_category(category: usize) -> usize {
    if category >= CATEGORIES {
        return category;
    }
    let local = match category {
        0..=9 => category,
        193..=255 => category - 193 + 10,
        _ => return category,
    };
    CONTEXT_CATEGORY_TAG | (ACTIVE_CONTEXT.load(Relaxed) * CONTEXT_CATEGORIES + local)
}

/// Capture generation, incremented by reset/release. Export with identity metadata.
pub fn capture_id() -> u64 {
    CAPTURE.load(Relaxed)
}

/// Retire metadata only after the World has ceased dispatching. Captured history
/// stays readable until explicit release; live prepared indices never move.
pub(crate) fn retire_world(index: usize) {
    let identity = world_context(index);
    for entry in CONTEXTS.lock().unwrap().iter_mut() {
        if entry.context.host == identity.host && entry.context.incarnation == identity.incarnation
        {
            entry.live = false;
        }
    }
}

/// Release a completed capture and reclaim retired World slots. The segmented
/// counter storage retains only its high-water capacity, reused by later Worlds.
/// Call at a Host boundary with no active measurement scopes.
///
/// Panics if a timer, allocation or context guard is still alive.
pub fn release_capture() {
    assert_eq!(
        ACTIVE_GUARDS.load(Relaxed),
        0,
        "capture release requires a Host boundary without active measurement scopes"
    );
    reset(false);
    crate::profiling_trace::release();
    // Both registry locks always follow System profiles then contexts, as in
    // visit_capture. Registration releases its System lock before context work.
    let mut systems = SYSTEM_PROFILES.lock().unwrap();
    let mut contexts = CONTEXTS.lock().unwrap();
    for entry in contexts.iter_mut().filter(|entry| !entry.live) {
        entry.occupied = false;
        entry.context = ProfileContext::default();
    }
    for entry in &mut systems.entries {
        if contexts
            .get(entry.world)
            .is_some_and(|world| !world.occupied)
        {
            entry.name = "";
        }
    }
}

/// Single-thread Host attribution guard. It restores the enclosing semantic
/// context and uses no allocation or registry lookup in phase dispatch.
pub struct ContextScope {
    previous: usize,
    capture: u64,
    enabled: bool,
    suppressed: bool,
}

impl ContextScope {
    pub(crate) fn world(world: usize) -> Self {
        let active = ACTIVE_CONTEXT.load(Relaxed);
        let root = CONTEXT_ROOTS
            .get(active)
            .map_or(0, |value| value.load(Relaxed) as usize);
        Self::new(if root == world {
            active
        } else {
            world
        })
    }

    pub(crate) fn new(context: usize) -> Self {
        let active = measuring();
        let owner = CAPTURE_HOST_FILTER.load(Relaxed);
        let host = CONTEXT_HOSTS
            .get(context)
            .map_or(0, |value| value.load(Relaxed));
        let suppressed = active && owner != 0 && host != 0 && host != owner;
        if suppressed {
            SUPPRESSED_DEPTH.with(|depth| depth.set(depth.get() + 1));
        }
        let enabled = active && !suppressed;
        if enabled || suppressed {
            ACTIVE_GUARDS.fetch_add(1, Relaxed);
        }
        Self {
            previous: if enabled {
                ACTIVE_CONTEXT.swap(context, Relaxed)
            } else {
                0
            },
            capture: capture_id(),
            enabled,
            suppressed,
        }
    }
}

impl Drop for ContextScope {
    fn drop(&mut self) {
        if self.enabled || self.suppressed {
            ACTIVE_GUARDS.fetch_sub(1, Relaxed);
        }
        if self.suppressed && self.capture == capture_id() {
            SUPPRESSED_DEPTH.with(|depth| depth.set(depth.get() - 1));
        }
        if self.enabled && self.capture == capture_id() {
            ACTIVE_CONTEXT.store(self.previous, Relaxed);
        }
    }
}

#[path = "profiling_tests.rs"]
#[cfg(test)]
mod tests;

/// Retained requested backing bytes for counter segments and registry vectors.
/// Excludes allocator overhead and static atomics; this is instrumentation
/// storage accounting, not process resident memory or live scene allocation.
pub fn storage_bytes() -> (usize, usize) {
    let counters = [
        &SYSTEM_COUNTERS,
        &SYSTEM_CATEGORY_COUNTS,
        &FIXED_COUNTERS,
        &CONTEXT_CATEGORY_COUNTS,
        &PROFILE_CONTEXTS,
        &CONTEXT_ROOTS,
        &CONTEXT_HOSTS,
    ]
    .into_iter()
    .map(|counters| {
        counters
            .segments
            .iter()
            .filter_map(OnceLock::get)
            .map(|segment| std::mem::size_of_val(segment.as_ref()))
            .sum::<usize>()
    })
    .sum();
    let systems = SYSTEM_PROFILES.lock().unwrap().entries.capacity()
        * std::mem::size_of::<SystemProfileEntry>();
    let contexts = CONTEXTS.lock().unwrap().capacity() * std::mem::size_of::<ContextEntry>();
    (counters, systems + contexts)
}

/// Semantic stage readback independent of exported flat offsets.
#[derive(Clone, Copy, Debug)]
pub struct ProfileStageRecord {
    /// True for a System dispatch phase; false for a contextual fixed timer.
    pub system_stage: bool,
    /// Stable timer label.
    pub name: &'static str,
    /// Issued semantic identity retained for this capture.
    pub context: ProfileContext,
    /// Calls, nanoseconds, allocation calls, requested bytes.
    pub values: [u64; 4],
}

/// Visit paused stage and category counters under stable metadata ownership.
/// Foreign Hosts can construct Worlds concurrently; no flattening offsets are
/// retained or recomputed during the visit. Return false to stop bounded output.
/// Visitors must not register Worlds or call other registry-reading APIs.
pub fn visit_capture(
    mut stage: impl FnMut(ProfileStageRecord) -> bool,
    mut category: impl FnMut(&'static str, ProfileContext, [u64; 2]) -> bool,
) {
    let systems = SYSTEM_PROFILES.lock().unwrap();
    let contexts = CONTEXTS.lock().unwrap();
    let context = |index: usize| {
        contexts
            .get(index)
            .filter(|entry| entry.occupied)
            .map_or(ProfileContext::default(), |entry| entry.context)
    };
    for (index, entry) in systems.entries.iter().enumerate() {
        if entry.name.is_empty() {
            continue;
        }
        for phase in 0..SYSTEM_PHASES {
            let slot = index * SYSTEM_PHASES + phase;
            let identity = ProfileContext {
                system: entry.name,
                phase: Some(ProfilePhase::ALL[phase]),
                ..context(entry.world)
            };
            let values = std::array::from_fn(|offset| {
                SYSTEM_COUNTERS
                    .get(slot * 4 + offset)
                    .map_or(0, |value| value.load(Relaxed))
            });
            if values != [0; 4]
                && !stage(ProfileStageRecord {
                    system_stage: true,
                    name: entry.name,
                    context: identity,
                    values,
                })
            {
                return;
            }
        }
    }
    for (index, entry) in contexts
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.occupied)
    {
        for fixed in FixedStage::ALL {
            let slot = index * FixedStage::ALL.len() + fixed as usize;
            let values = std::array::from_fn(|offset| {
                FIXED_COUNTERS
                    .get(slot * 4 + offset)
                    .map_or(0, |value| value.load(Relaxed))
            });
            if values != [0; 4]
                && !stage(ProfileStageRecord {
                    system_stage: false,
                    name: fixed.name(),
                    context: entry.context,
                    values,
                })
            {
                return;
            }
        }
    }
    for local in 0..CATEGORIES {
        let values =
            std::array::from_fn(|offset| CATEGORY_COUNTS[local * 2 + offset].load(Relaxed));
        let name = if local == 0 {
            "unattributed"
        } else {
            CATEGORY_NAMES[local].get().copied().unwrap_or("")
        };
        if values != [0; 2] && !category(name, ProfileContext::default(), values) {
            return;
        }
    }
    for (index, entry) in systems.entries.iter().enumerate() {
        if entry.name.is_empty() {
            continue;
        }
        for phase in 0..SYSTEM_PHASES {
            let slot = index * SYSTEM_PHASES + phase;
            let values = std::array::from_fn(|offset| {
                SYSTEM_CATEGORY_COUNTS
                    .get(slot * 2 + offset)
                    .map_or(0, |value| value.load(Relaxed))
            });
            let identity = ProfileContext {
                system: entry.name,
                phase: Some(ProfilePhase::ALL[phase]),
                ..context(entry.world)
            };
            if values != [0; 2] && !category(entry.name, identity, values) {
                return;
            }
        }
    }
    for (index, entry) in contexts
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.occupied)
    {
        for local in 0..CONTEXT_CATEGORIES {
            let values = std::array::from_fn(|offset| {
                CONTEXT_CATEGORY_COUNTS
                    .get((index * CONTEXT_CATEGORIES + local) * 2 + offset)
                    .map_or(0, |value| value.load(Relaxed))
            });
            let name = if local == 0 {
                "unattributed"
            } else if local <= FixedStage::ALL.len() {
                FixedStage::ALL[local - 1].name()
            } else {
                CATEGORY_NAMES[193 + local - 10]
                    .get()
                    .copied()
                    .unwrap_or("")
            };
            if values != [0; 2] && !category(name, entry.context, values) {
                return;
            }
        }
    }
}
