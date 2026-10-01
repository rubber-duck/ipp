//! Diagnostic-only exact-endpoint counter reads; no lifecycle or frame observation.

use crate::references::WorldReference;
use ipp_core::systems::lifecycle_publisher::{LifecycleTargetWork, LifecycleWatchTraffic};

/// Current fixed-session endpoint required by the read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifecycleDiagnosticQuery {
    /// Exact World lifetime selected by the outer session.
    pub world: WorldReference,
    /// Previously acknowledged endpoint; absence is an error, not a zero sample.
    pub output: u64,
}

/// Owned constant-size copy; the outer response tick is always zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifecycleDiagnosticSample {
    /// The exact lifetime validated by this read.
    pub endpoint: LifecycleDiagnosticQuery,
    /// World-wide sparse-index work, including candidates filtered by kind/liveness.
    pub work: LifecycleTargetWork,
    /// This endpoint's cumulative successfully retained event charges.
    pub traffic: LifecycleWatchTraffic,
}
