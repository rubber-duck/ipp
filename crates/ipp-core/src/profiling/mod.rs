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

mod allocation;
mod capture;
mod contexts;
mod counters;
mod stages;
pub mod trace;

pub use allocation::{
    AllocationScope, CATEGORIES, CountingAllocator, allocations, allocator_available,
    category_count, category_counter, category_name,
};
pub use capture::{
    ProfileStageRecord, capture_id, enable_trace, pause, release_capture, reset, reset_for_host,
    storage_bytes, visit_capture,
};
pub use contexts::{
    ContextScope, ProfileContext, ProfilePhase, category_context, register_system, stage_context,
    system_composition, system_context, system_count, system_name, world_profile_context,
};
pub(crate) use contexts::{register_world, retire_world};
pub(crate) use stages::{FixedStage, Stage, WorldTraceScope};
pub use stages::{
    SYSTEM_PHASES, clock_nanos, counter, counter_count, fixed_stage_base, fixed_stage_name,
};

#[cfg(test)]
#[path = "profiling_tests.rs"]
mod tests;
