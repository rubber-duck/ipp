//! Bounded Metal completion durations in the common instrumentation protocol.

use ipp_protocol::host::profiling::{
    ProfileGpuAvailability, ProfileGpuCapability, ProfileGpuCapture, ProfileGpuRecord,
    ProfileGpuSampling, ProfileGpuScope,
};

const MAX_RECORDS: usize = 256;

pub(super) struct MetalProfileCapture {
    capture: u64,
    host: u64,
    sampling: ProfileGpuSampling,
    enabled: bool,
    records: Vec<ProfileGpuRecord>,
    dropped: u64,
}

impl MetalProfileCapture {
    pub(super) fn new(
        capture: u64,
        host: u64,
        sampling: ProfileGpuSampling,
        enabled: bool,
    ) -> Self {
        Self {
            capture,
            host,
            sampling,
            enabled,
            records: Vec::new(),
            dropped: 0,
        }
    }

    pub(super) fn matches(&self, capture: u64) -> bool {
        self.capture == capture
    }

    pub(super) fn capability(&self) -> ProfileGpuCapability {
        if self.enabled {
            ProfileGpuCapability::Elapsed
        } else {
            ProfileGpuCapability::Unsupported
        }
    }

    pub(super) fn record(&mut self, frame: u64, elapsed_ns: Option<u64>) {
        if !self.enabled {
            return;
        }
        if self.records.len() == MAX_RECORDS {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.records.push(ProfileGpuRecord {
            capture: self.capture,
            host: self.host,
            context: 1,
            frame,
            preparation: false,
            world: None,
            surface: None,
            scope: ProfileGpuScope::Frame,
            availability: elapsed_ns.map_or(ProfileGpuAvailability::Unsupported, |duration_ns| {
                ProfileGpuAvailability::Available {
                    duration_ns,
                }
            }),
        });
    }

    pub(super) fn finish(self, capture: u64) -> ProfileGpuCapture {
        assert!(self.matches(capture), "Metal capture generation mismatch");
        ProfileGpuCapture {
            sampling: self.sampling,
            capability: self.capability(),
            availability: if self.enabled {
                ProfileGpuAvailability::Stopped
            } else {
                ProfileGpuAvailability::Unsupported
            },
            dropped_records: self.dropped,
            records: self.records,
            gl_calls: None,
        }
    }
}
