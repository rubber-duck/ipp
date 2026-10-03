//! Optional counts of physical GL entry points, after retained-state suppression.
#[cfg(not(target_arch = "wasm32"))]
use std::{cell::RefCell, rc::Rc};

/// Mutually exclusive counts over one context-owned capture window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderGlCallCounts {
    /// Drawing entry points, including instanced draws.
    pub draws: u64,
    /// Context state and uniform operations.
    pub state: u64,
    /// Buffer and texture data upload operations.
    pub uploads: u64,
    /// Other workload entry points, such as resource creation and diagnostics.
    pub other: u64,
    /// Timer discovery, issuance, polling and cleanup entry points.
    pub profiler: u64,
    /// At least one category saturated rather than wrapping.
    pub overflowed: bool,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
pub(super) enum GlCallCategory {
    Draw,
    State,
    Upload,
    Other,
    Profiler,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Capture {
    active: bool,
    counts: RenderGlCallCounts,
}

/// One device's counters; copied optional entry points retain this same owner.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Default)]
pub(super) struct GlCallCounters(Rc<RefCell<Capture>>);

#[cfg(not(target_arch = "wasm32"))]
impl GlCallCounters {
    pub fn start(&self) {
        *self.0.borrow_mut() = Capture {
            active: true,
            counts: RenderGlCallCounts::default(),
        };
    }

    pub fn stop(&self) {
        self.0.borrow_mut().active = false;
    }

    pub fn snapshot(&self) -> RenderGlCallCounts {
        self.0.borrow().counts
    }

    pub fn record(&self, category: GlCallCategory) {
        let mut capture = self.0.borrow_mut();
        if !capture.active {
            return;
        }
        let counter = match category {
            GlCallCategory::Draw => &mut capture.counts.draws,
            GlCallCategory::State => &mut capture.counts.state,
            GlCallCategory::Upload => &mut capture.counts.uploads,
            GlCallCategory::Other => &mut capture.counts.other,
            GlCallCategory::Profiler => &mut capture.counts.profiler,
        };
        if let Some(next) = counter.checked_add(1) {
            *counter = next;
        } else {
            capture.counts.overflowed = true;
        }
    }
}
