//! Timed System phases and fixed commit/animation stages.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

use super::allocation::{AllocationScope, CATEGORIES, allocations};
use super::capture::{ACTIVE_GUARDS, CAPTURE, capture_id, enabled, measuring};
use super::contexts::{
    ACTIVE_CONTEXT, ContextScope, PROFILE_CONTEXTS, context_count, context_occupied, world_context,
};
use super::counters::GrowingCounters;
use super::trace;

pub(super) static SYSTEM_SLOTS: AtomicUsize = AtomicUsize::new(0);

/// Four counters per System stage slot.
pub(super) static SYSTEM_COUNTERS: GrowingCounters = GrowingCounters::new();

pub(super) static FIXED_COUNTERS: GrowingCounters = GrowingCounters::new();

/// Timed phases per scheduled System.
pub const SYSTEM_PHASES: usize = 6;

/// First stage slot of the fixed timers, after every registered System slot.
pub fn fixed_stage_base() -> usize {
    SYSTEM_SLOTS.load(Relaxed)
}

/// Entries readable through [`counter`]: System slots, then the fixed timers.
pub fn counter_count() -> usize {
    (fixed_stage_base() + context_count() * FixedStage::ALL.len()) * 4
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
    pub(super) const ALL: [Self; 9] = [
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

    pub(super) fn name(self) -> &'static str {
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
    span: Option<trace::ProfileSpan>,
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
        let span = if timed && trace::enabled() {
            Some(trace::begin(
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
            trace::finish(span, end);
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
    span: Option<trace::ProfileSpan>,
    capture: u64,
}

impl WorldTraceScope {
    pub(crate) fn new() -> Self {
        let span = if measuring() && trace::enabled() {
            Some(trace::begin(
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
                trace::finish(span, clock_nanos());
            }
        }
    }
}

/// Label of one stage slot: `system#phase` names come from
/// [`system_name`](super::system_name); fixed slots return their fixed timer
/// name, and unused slots are empty.
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
