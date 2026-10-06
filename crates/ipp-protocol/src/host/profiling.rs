//! Optional Host profiling control and bounded immutable artifact transfer.
//! The wire contract is identical in production and instrumentation builds.

use crate::codec::{Reader, Writer};
use crate::{ProtocolError, contract::wire_manifest::*};

/// Host-owned observation controls; never advance simulation time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileRequest {
    /// Observe capability without changing a capture.
    Status,
    /// Begin an exclusive capture on this connection.
    Start {
        /// Enable intrusive CPU/allocation counters.
        counters: bool,
        /// Maximum retained UTF-8 artifact bytes; does not limit live Worlds.
        max_artifact_bytes: u64,
        /// Optional GPU sampling, unavailable on headless adapters.
        gpu: ProfileGpuSampling,
        /// Optional physical backend call accounting.
        gl_calls: bool,
        /// Maximum complete CPU spans; zero disables chronology.
        max_events: u64,
    },
    /// Pause and freeze the owned capture's immutable artifact.
    Stop(u64),
    /// Read the next bounded page of the stopped capture.
    Read {
        /// Exact capture generation.
        capture: u64,
        /// Sequential byte offset.
        offset: u64,
    },
    /// Cancel or release at a control boundary; stale/foreign generations are refused.
    Release(u64),
}

/// Explicit availability and ownership outcomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProfileStatus {
    /// Request succeeded; instrumentation is available.
    Available = 0,
    /// Ordinary build; no profiling hooks are compiled.
    Unavailable = 1,
    /// Another connection or Host owns the capture.
    Busy = 2,
    /// Stale/foreign generation or invalid read offset.
    InvalidCapture = 3,
    /// Read requires a successfully stopped capture.
    Incomplete = 4,
    /// Requested diagnostic retention is not representable or serialization exceeded it.
    Capacity = 5,
}

/// Bounded correlated control response. Artifact bytes are UTF-8 JSON with
/// exact unsigned decimal identifiers, self-described semantic records and units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileResponse {
    /// Capability/ownership result.
    pub status: ProfileStatus,
    /// Exact generation, or zero when no capture is owned.
    pub capture: u64,
    /// Immutable artifact length; zero before stop.
    pub total_bytes: u64,
    /// Offset of this artifact page.
    pub offset: u64,
    /// At most the protocol's existing `MAX_FIELD_BYTES` transfer page.
    pub bytes: Vec<u8>,
}

impl ProfileResponse {
    /// Empty control result without a payload allocation.
    pub fn status(status: ProfileStatus) -> Self {
        Self {
            status,
            capture: 0,
            total_bytes: 0,
            offset: 0,
            bytes: Vec::new(),
        }
    }
}

impl Reader<'_> {
    pub(crate) fn profile_request(&mut self) -> Result<ProfileRequest, ProtocolError> {
        Ok(match self.u8()? {
            PROFILE_REQUEST_STATUS => ProfileRequest::Status,
            PROFILE_REQUEST_START => ProfileRequest::Start {
                counters: self.boolean()?,
                max_events: self.u64()?,
                max_artifact_bytes: self.u64()?,
                gl_calls: self.boolean()?,
                gpu: match self.u8()? {
                    0 => ProfileGpuSampling::Off,
                    1 => ProfileGpuSampling::Frame,
                    2 => ProfileGpuSampling::Passes,
                    3 => ProfileGpuSampling::All,
                    value => return Err(ProtocolError::Unsupported(value)),
                },
            },
            PROFILE_REQUEST_STOP => ProfileRequest::Stop(self.u64()?),
            PROFILE_REQUEST_READ => ProfileRequest::Read {
                capture: self.u64()?,
                offset: self.u64()?,
            },
            PROFILE_REQUEST_RELEASE => ProfileRequest::Release(self.u64()?),
            value => return Err(ProtocolError::Unsupported(value)),
        })
    }

    pub(crate) fn profile_response(&mut self) -> Result<ProfileResponse, ProtocolError> {
        let status = match self.u8()? {
            PROFILE_STATUS_AVAILABLE => ProfileStatus::Available,
            PROFILE_STATUS_UNAVAILABLE => ProfileStatus::Unavailable,
            PROFILE_STATUS_BUSY => ProfileStatus::Busy,
            PROFILE_STATUS_INVALID_CAPTURE => ProfileStatus::InvalidCapture,
            PROFILE_STATUS_INCOMPLETE => ProfileStatus::Incomplete,
            PROFILE_STATUS_CAPACITY => ProfileStatus::Capacity,
            value => return Err(ProtocolError::Unsupported(value)),
        };
        Ok(ProfileResponse {
            status,
            capture: self.u64()?,
            total_bytes: self.u64()?,
            offset: self.u64()?,
            bytes: self.bytes()?,
        })
    }
}

impl Writer {
    pub(crate) fn profile_request(
        &mut self,
        request: &ProfileRequest,
    ) -> Result<(), ProtocolError> {
        match request {
            ProfileRequest::Status => self.u8(PROFILE_REQUEST_STATUS)?,
            ProfileRequest::Start {
                counters,
                max_events,
                max_artifact_bytes,
                gpu,
                gl_calls,
            } => {
                self.u8(PROFILE_REQUEST_START)?;
                self.u8(u8::from(*counters))?;
                self.u64(*max_events)?;
                self.u64(*max_artifact_bytes)?;
                self.u8(u8::from(*gl_calls))?;
                self.u8(*gpu as u8)?;
            }
            ProfileRequest::Stop(capture) => {
                self.u8(PROFILE_REQUEST_STOP)?;
                self.u64(*capture)?;
            }
            ProfileRequest::Read {
                capture,
                offset,
            } => {
                self.u8(PROFILE_REQUEST_READ)?;
                self.u64(*capture)?;
                self.u64(*offset)?;
            }
            ProfileRequest::Release(capture) => {
                self.u8(PROFILE_REQUEST_RELEASE)?;
                self.u64(*capture)?;
            }
        }
        Ok(())
    }

    pub(crate) fn profile_response(
        &mut self,
        response: &ProfileResponse,
    ) -> Result<(), ProtocolError> {
        self.u8(response.status as u8)?;
        self.u64(response.capture)?;
        self.u64(response.total_bytes)?;
        self.u64(response.offset)?;
        self.bytes(&response.bytes)
    }
}

/// Selected GPU measurement envelope, independent of render implementation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
/// GpuSampling diagnostic metadata.
#[repr(u8)]
pub enum ProfileGpuSampling {
    #[default]
    /// No GPU queries requested.
    Off = 0,
    /// Whole renderer draw scope.
    Frame = 1,
    /// Measure individual renderer passes.
    Passes = 2,
    /// Use timestamps for frame and pass scopes.
    All = 3,
}

/// GpuCapability diagnostic metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileGpuCapability {
    /// Device capability is unavailable.
    Unsupported,
    /// Elapsed queries are supported.
    Elapsed,
    /// Timestamp queries are supported.
    Timestamps,
}

/// GpuAvailability diagnostic metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileGpuAvailability {
    /// Result has not completed.
    Pending,
    /// Measurement completed successfully.
    Available {
        /// Measured interval in nanoseconds.
        duration_ns: u64,
    },
    /// Device capability is unavailable.
    Unsupported,
    /// Device clock was disjoint.
    Disjoint,
    /// Owning device context was lost.
    ContextLost,
    /// Bounded query capacity dropped this observation.
    CapacityDropped,
    /// Elapsed query would overlap an active query.
    OverlapSkipped,
    /// Capture stopped before completion.
    Stopped,
}

/// GpuScope diagnostic metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileGpuScope {
    /// Whole renderer draw scope.
    Frame,
    /// Surface pass.
    Surface,
    /// Shared atlas pass.
    Atlas,
    /// Cache repaint pass.
    CacheRepaint,
    /// Surface composition pass.
    Composite,
}

/// GpuWorld diagnostic metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileGpuWorld {
    /// Exact Host-local World identity.
    pub id: u64,
    /// World lifetime retained at issuance.
    pub incarnation: u64,
    /// Composition copied at issuance; absent when identity unavailable.
    pub composition: Option<u64>,
}

/// GpuEntity diagnostic metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileGpuEntity {
    /// Entity storage slot.
    pub slot: u32,
    /// Entity storage generation.
    pub generation: u32,
}

/// GpuRecord diagnostic metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileGpuRecord {
    /// Owning capture generation.
    pub capture: u64,
    /// Owning Host identity.
    pub host: u64,
    /// Device context identity.
    pub context: u64,
    /// Renderer draw ordinal, independent of CPU boundary.
    pub frame: u64,
    /// Work issued outside an active renderer draw.
    pub preparation: bool,
    /// World identity copied at issuance; absent for shared work.
    pub world: Option<ProfileGpuWorld>,
    /// Surface entity copied at issuance.
    pub surface: Option<ProfileGpuEntity>,
    /// Renderer scope measured.
    pub scope: ProfileGpuScope,
    /// Explicit measurement availability.
    pub availability: ProfileGpuAvailability,
}

/// GpuCapture diagnostic metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileGpuCapture {
    /// Requested GPU sampling policy.
    pub sampling: ProfileGpuSampling,
    /// Device query capability.
    pub capability: ProfileGpuCapability,
    /// Explicit measurement availability.
    pub availability: ProfileGpuAvailability,
    /// Records dropped by bounded renderer retention.
    pub dropped_records: u64,
    /// Frozen pending and terminal observations.
    pub records: Vec<ProfileGpuRecord>,
    /// Optional physical backend call totals.
    pub gl_calls: Option<ProfileGlCalls>,
}

/// RenderOptions diagnostic metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileRenderOptions {
    /// GPU measurement policy.
    pub gpu: ProfileGpuSampling,
    /// Optional physical backend call totals.
    pub gl_calls: bool,
}

/// GlAvailability diagnostic metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileGlAvailability {
    /// Measurement completed successfully.
    Available,
    /// Device capability is unavailable.
    Unsupported,
    /// Observation was not requested.
    NotRequested,
}

/// Physical device implementation issuing the counted entry points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileGlSource {
    /// Browser WebGL bridge.
    WebGl,
    /// Native GLES device.
    Gles,
}

/// Reason the physical observation window ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileGlStopReason {
    /// Capture owner stopped observation.
    OwnerStopped,
    /// Device context disappeared before the owner stopped.
    ContextLost,
}

/// Mutually exclusive physical device-call totals; profiler calls are separate.
/// GlCalls diagnostic metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileGlCalls {
    /// Terminal device lifecycle reason.
    pub stop_reason: ProfileGlStopReason,
    /// Actual physical counter enable boundary, when supported.
    pub window_start: Option<u64>,
    /// Actual freeze boundary, potentially after CPU pause cleanup.
    pub window_end: Option<u64>,
    /// Physical device implementation.
    pub source: ProfileGlSource,
    /// Captured device context identity.
    pub context: u64,
    /// Explicit measurement availability.
    pub availability: ProfileGlAvailability,
    /// Physical draw entry point calls.
    pub draws: u64,
    /// Physical state entry point calls.
    pub state: u64,
    /// Physical upload entry point calls.
    pub uploads: u64,
    /// Other physical device calls.
    pub other: u64,
    /// Calls made only for measurement, excluded from ordinary categories.
    pub profiler: u64,
    /// At least one saturating counter reached its limit.
    pub overflowed: bool,
}
