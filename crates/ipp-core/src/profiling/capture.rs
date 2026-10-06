//! Capture generations: participation, enable/reset/pause/release and paused
//! readback of every counter table.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};

use super::allocation::{
    ACTIVE_CATEGORY, ALLOCS, BYTES, CATEGORIES, CATEGORY_COUNTS, CATEGORY_NAMES,
    CONTEXT_CATEGORIES, CONTEXT_CATEGORY_COUNTS, SYSTEM_CATEGORY_COUNTS,
};
use super::contexts::{
    ACTIVE_CONTEXT, CONTEXT_HOSTS, CONTEXT_ROOTS, CONTEXTS, ContextEntry, PROFILE_CONTEXTS,
    ProfileContext, ProfilePhase, SYSTEM_PROFILES, SystemProfileEntry, register_context,
};
use super::stages::{FIXED_COUNTERS, FixedStage, SYSTEM_COUNTERS, SYSTEM_PHASES};
use super::trace;

pub(super) static ENABLED: AtomicBool = AtomicBool::new(false);

// Only the capture owner's evaluation thread participates. Background I/O
// allocations are excluded, never assigned to the current World. Const TLS
// initialization cannot allocate; try_with also remains safe during teardown.
std::thread_local! {
    static EVALUATION_CAPTURE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    pub(super) static SUPPRESSED_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn participating() -> bool {
    SUPPRESSED_DEPTH
        .try_with(|depth| depth.get() == 0)
        .unwrap_or(false)
        && EVALUATION_CAPTURE
            .try_with(|capture| capture.get() == CAPTURE.load(Relaxed))
            .unwrap_or(false)
}

pub(super) fn enabled() -> bool {
    ENABLED.load(Relaxed) && participating()
}

pub(crate) fn measuring() -> bool {
    (ENABLED.load(Relaxed) || trace::enabled()) && participating()
}

/// Enable the already prepared optional CPU span recorder on the owner thread.
pub fn enable_trace() {
    EVALUATION_CAPTURE.with(|capture| capture.set(capture_id()));
    trace::enable();
}

pub(super) static ACTIVE_GUARDS: AtomicUsize = AtomicUsize::new(0);
pub(super) static CAPTURE_HOST_FILTER: AtomicU64 = AtomicU64::new(0);
pub(super) static CAPTURE: AtomicU64 = AtomicU64::new(0);

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
    trace::pause();
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
    trace::pause();
}

/// Capture generation, incremented by reset/release. Export with identity metadata.
pub fn capture_id() -> u64 {
    CAPTURE.load(Relaxed)
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
    trace::release();
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
