//! Diagnostic-only exact-endpoint counter reads; no lifecycle or frame observation.

use crate::codec::{ProtocolError, Reader, Writer};
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

impl Reader<'_> {
    /// Decode the exact endpoint; every identity is nonzero.
    pub(super) fn lifecycle_diagnostic_query(
        &mut self,
    ) -> Result<LifecycleDiagnosticQuery, ProtocolError> {
        let world = self.world_reference()?;
        let output = self.u64()?;
        if world.id == 0 || world.incarnation == 0 || output == 0 {
            return Err(ProtocolError::Malformed("lifecycle diagnostic endpoint"));
        }
        Ok(LifecycleDiagnosticQuery {
            world,
            output,
        })
    }
}

impl Writer {
    /// Encode the sample after its validated envelope and response tag.
    pub(super) fn lifecycle_diagnostic_sample(
        &mut self,
        sample: &LifecycleDiagnosticSample,
    ) -> Result<(), ProtocolError> {
        self.u64(sample.endpoint.world.id)?;
        self.u64(sample.endpoint.world.incarnation)?;
        self.u64(sample.endpoint.output)?;
        self.u64(sample.work.lookups)?;
        self.u64(sample.work.recipient_visits)?;
        self.u8(u8::from(sample.work.saturated))?;
        self.u64(sample.traffic.queued_events)?;
        self.u64(sample.traffic.queued_bytes)?;
        self.u8(u8::from(sample.traffic.saturated))
    }
}
