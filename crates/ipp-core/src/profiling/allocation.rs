//! Opt-in counting allocator and exclusive allocation categories.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};

use super::capture::{ACTIVE_GUARDS, capture_id, enabled};
use super::contexts::{ACTIVE_CONTEXT, context_count, context_occupied, system_name};
use super::counters::GrowingCounters;
use super::stages::{FixedStage, SYSTEM_PHASES, fixed_stage_base};

static ALLOCATOR_PRESENT: AtomicBool = AtomicBool::new(false);

/// Whether this binary has actually selected and used the counting allocator.
/// Libraries never select an allocator for their embedding application.
pub fn allocator_available() -> bool {
    ALLOCATOR_PRESENT.load(Relaxed)
}

pub(super) static ALLOCS: AtomicU64 = AtomicU64::new(0);
pub(super) static BYTES: AtomicU64 = AtomicU64::new(0);

/// Call and byte counters per System stage allocation category.
pub(super) static SYSTEM_CATEGORY_COUNTS: GrowingCounters = GrowingCounters::new();

/// Static allocation categories; System stage categories follow them.
pub const CATEGORIES: usize = 256;

/// Entries readable through [`category_name`].
pub fn category_count() -> usize {
    CATEGORIES + fixed_stage_base() + context_count() * CONTEXT_CATEGORIES
}

pub(super) static ACTIVE_CATEGORY: AtomicUsize = AtomicUsize::new(0);
pub(super) static CATEGORY_NAMES: [OnceLock<&'static str>; CATEGORIES] =
    [const { OnceLock::new() }; CATEGORIES];
pub(super) static CATEGORY_COUNTS: [AtomicU64; CATEGORIES * 2] =
    [const { AtomicU64::new(0) }; CATEGORIES * 2];

pub(super) static CONTEXT_CATEGORY_COUNTS: GrowingCounters = GrowingCounters::new();

// Unassigned + nine fixed scopes + named static categories 193..256.
pub(super) const CONTEXT_CATEGORIES: usize = 73;

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

pub(super) fn count(bytes: usize) {
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

/// Total allocation/reallocation calls and requested bytes, not retained memory.
pub fn allocations() -> (u64, u64) {
    (ALLOCS.load(Relaxed), BYTES.load(Relaxed))
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

const CONTEXT_CATEGORY_TAG: usize = 1 << (usize::BITS - 1);

pub(super) fn active_category_counter(
    category: usize,
    offset: usize,
) -> Option<&'static AtomicU64> {
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
