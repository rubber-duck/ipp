//! Optional renderer scope attribution; device pools own physical timer resources.
use super::RenderService;
use crate::{RenderDevice, RenderGpuAvailability, RenderGpuCapability, RenderGpuQueryToken};
use std::{cell::RefCell, rc::Rc};

/// Timestamp devices can capture enclosing frame and nested passes together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderGpuSampling {
    /// Selected-root draw and its cache/atlas prepasses; separate asset preparation is outside this interval.
    Frame,
    /// Individual nonoverlapping scopes on elapsed-only devices.
    Passes,
    /// Frame and nested passes; supported only by timestamp devices.
    All,
}

/// Command interval attributed to the issuing renderer operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderGpuScope {
    /// Selected root draw and its prepasses, explicitly shared across participating Worlds.
    Frame,
    /// Direct painting of one placed output Surface.
    Surface,
    /// Shared glyph atlas preparation/population, not assigned to a World.
    Atlas,
    /// Whole-Canvas cache repaint.
    CacheRepaint,
    /// Cached image composition.
    Composite,
}

/// Originating identity copied when commands are issued, never resolved at completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderGpuIdentity {
    /// Capture owner's stable id.
    pub capture: u64,
    /// Runtime Host identity.
    pub host: u64,
    /// Renderer-local context generation.
    pub context: u64,
    /// Renderer-local selected-root draw sequence.
    pub frame: u64,
    /// True for preparation issued outside any selected-root draw; frame is zero then.
    pub preparation: bool,
    /// Exact World lifetime; absent for shared work.
    pub world: Option<ipp_core::WorldRef>,
    /// Semantic context copied at issuance; `None` with a World means unavailable identity.
    /// Shared scopes have neither a World nor a semantic context.
    pub profile_context: Option<ipp_core::profiling::ProfileContext>,
    /// Exact generational Surface entity; absent for Canvas/shared work.
    pub surface: Option<ipp_core::EntityId>,
}

/// One asynchronously completed GPU interval, including explicit unavailable results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderGpuSample {
    /// Identity at command issuance.
    pub identity: RenderGpuIdentity,
    /// Operation's interval, inclusive of nested GPU scopes where supported.
    pub scope: RenderGpuScope,
    /// Completion or unavailability; zero duration remains a legitimate measurement.
    pub availability: RenderGpuAvailability,
}

/// Why a physical device observation ended; valid partial counts are retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderGlStopReason {
    /// Explicit owner stop at a quiescent control boundary.
    OwnerStopped,
    /// Observation ended before the original context was lost or replaced.
    ContextLost,
}

/// Actual physical-call observation window in the ipp-core monotonic nanosecond domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderGlCallWindow {
    /// Original context epoch.
    pub context: u64,
    /// Observation activation in nanoseconds.
    pub start: u64,
    /// Frozen observation end; absent while active.
    pub end: Option<u64>,
    /// Terminal reason, meaningful after end is present.
    pub stop_reason: RenderGlStopReason,
}

const RECORD_CAPACITY: usize = 256;
const POLL_BUDGET: usize = 16;

struct Record {
    sample: RenderGpuSample,
    token: Option<RenderGpuQueryToken>,
}

pub(super) struct RenderGpuCapture {
    active: Option<(u64, u64, RenderGpuSampling)>,
    context: u64,
    frame: u64,
    drawing: bool,
    records: Vec<Record>,
    dropped: u64,
    poll_cursor: usize,
    gl_recording: bool,
    gl_window: Option<RenderGlCallWindow>,
    gl_frozen: Option<crate::RenderGlCallCounts>,
}

impl Default for RenderGpuCapture {
    fn default() -> Self {
        Self {
            active: None,
            context: 1,
            frame: 0,
            drawing: false,
            records: Vec::new(),
            dropped: 0,
            poll_cursor: 0,
            gl_recording: false,
            gl_window: None,
            gl_frozen: None,
        }
    }
}

/// End an issued scope on every return/error path without borrowing the renderer.
pub(super) struct RenderGpuScopeGuard<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    token: RenderGpuQueryToken,
}

impl<D: RenderDevice> Drop for RenderGpuScopeGuard<D> {
    fn drop(&mut self) {
        self.device.borrow_mut().gpu_end(self.token);
    }
}

impl<D: RenderDevice> RenderService<D> {
    /// Begin independent physical GL-call counting; timer support is not required.
    pub fn start_gl_call_capture(&mut self) -> bool {
        self.gpu_capture.gl_frozen = None;
        self.gpu_capture.gl_recording = self.device.borrow_mut().gl_calls_start();
        self.gpu_capture.gl_window = self.gpu_capture.gl_recording.then(|| RenderGlCallWindow {
            context: self.gpu_capture.context,
            start: ipp_core::profiling::clock_nanos(),
            end: None,
            stop_reason: RenderGlStopReason::OwnerStopped,
        });
        self.gpu_capture.gl_recording
    }

    /// Issuance context and actual observation stamps, retained after loss.
    pub fn gl_call_window(&self) -> Option<RenderGlCallWindow> {
        self.gpu_capture.gl_window
    }

    /// Physical counts for this context, including separately classified profiler calls.
    pub fn gl_call_counts(&self) -> Option<crate::RenderGlCallCounts> {
        if self.gpu_capture.gl_recording {
            self.device.borrow().gl_calls_snapshot()
        } else {
            self.gpu_capture.gl_frozen
        }
    }

    /// Stop physical-call observation independently of GPU timer availability.
    pub fn stop_gl_call_capture(&mut self) {
        self.finish_gl_call_capture(RenderGlStopReason::OwnerStopped);
    }

    fn finish_gl_call_capture(&mut self, reason: RenderGlStopReason) {
        if self.gpu_capture.gl_recording {
            let loss_end = self.device.borrow().gl_calls_loss_end();
            self.gpu_capture.gl_frozen = self.device.borrow().gl_calls_snapshot();
            self.device.borrow_mut().gl_calls_stop();
            self.gpu_capture.gl_recording = false;
            if let Some(window) = self.gpu_capture.gl_window.as_mut() {
                window.end = Some(loss_end.unwrap_or_else(ipp_core::profiling::clock_nanos));
                window.stop_reason = if loss_end.is_some() {
                    RenderGlStopReason::ContextLost
                } else {
                    reason
                };
            }
        }
    }

    /// Copy this diagnostic context epoch at capture issuance, never resolve it later.
    pub fn gpu_context_generation(&self) -> u64 {
        self.gpu_capture.context
    }

    /// Begin optional GPU sampling. Timestamp-only `All` is rejected on elapsed devices.
    /// The capture owner supplies stable capture/Host identities; rendering never depends on success.
    pub fn start_gpu_capture(
        &mut self,
        capture: u64,
        host: u64,
        sampling: RenderGpuSampling,
    ) -> Result<RenderGpuCapability, RenderGpuAvailability> {
        self.stop_gpu_capture();
        self.gpu_capture.records.clear();
        self.gpu_capture.dropped = 0;
        self.gpu_capture.poll_cursor = 0;
        let capability = self.device.borrow().gpu_capability();
        if sampling == RenderGpuSampling::All && capability == RenderGpuCapability::Elapsed {
            return Err(RenderGpuAvailability::Unsupported);
        }
        self.gpu_capture.active = Some((capture, host, sampling));
        Ok(capability)
    }

    /// Stop recording and retire pending queries without waiting for GPU completion.
    pub fn stop_gpu_capture(&mut self) {
        self.gpu_capture.active = None;
        self.gpu_poll_samples();
        self.device
            .borrow_mut()
            .gpu_stop(RenderGpuAvailability::Stopped);
        self.gpu_finish_pending();
    }

    /// Invalidate an adapter context lifetime, including simulated loss of a valid context.
    /// Call only at a quiescent adapter boundary: cleanup deletes valid query resources
    /// or abandons names when the physical context reports loss;
    /// completed records survive, and restoration never silently resumes this capture.
    pub fn invalidate_gpu_context(&mut self) {
        self.gpu_poll_samples();
        self.device
            .borrow_mut()
            .gpu_stop(RenderGpuAvailability::Stopped);
        for record in &mut self.gpu_capture.records {
            if let Some(token) = record.token.take() {
                self.device.borrow_mut().gpu_poll(token);
                record.sample.availability = RenderGpuAvailability::ContextLost;
            }
        }
        self.finish_gl_call_capture(RenderGlStopReason::ContextLost);
        self.gpu_capture.active = None;
        self.gpu_capture.context = self
            .gpu_capture
            .context
            .checked_add(1)
            .expect("GPU context identity exhausted");
    }

    /// Drain terminal records; pending records retain their original identity until a later poll.
    pub fn drain_gpu_samples(&mut self) -> Vec<RenderGpuSample> {
        self.gpu_poll_samples();
        let mut completed = Vec::new();
        self.gpu_capture.records.retain(|record| {
            if record.sample.availability == RenderGpuAvailability::Pending {
                true
            } else {
                completed.push(record.sample);
                false
            }
        });
        completed
    }

    /// Diagnostic metadata records dropped by bounded capture storage; not a rendering limit.
    pub fn gpu_dropped_records(&self) -> u64 {
        self.gpu_capture.dropped
    }

    /// Snapshot pending and terminal metadata without consuming it.
    pub fn gpu_samples(&mut self) -> Vec<RenderGpuSample> {
        self.gpu_poll_samples();
        self.gpu_capture
            .records
            .iter()
            .map(|record| record.sample)
            .collect()
    }

    pub(super) fn gpu_begin_frame(&mut self) {
        if self.gpu_capture.active.is_none() {
            return;
        }
        self.gpu_poll_samples();
        self.gpu_capture.drawing = true;
        self.gpu_capture.frame = self
            .gpu_capture
            .frame
            .checked_add(1)
            .expect("GPU frame identity exhausted");
    }

    pub(super) fn gpu_end_frame(&mut self) {
        self.gpu_capture.drawing = false;
    }

    pub(super) fn gpu_scope(
        &mut self,
        scope: RenderGpuScope,
        world: Option<ipp_core::WorldRef>,
        surface: Option<ipp_core::EntityId>,
    ) -> Option<RenderGpuScopeGuard<D>> {
        let (capture, host, sampling) = self.gpu_capture.active?;
        if (sampling == RenderGpuSampling::Frame && scope != RenderGpuScope::Frame)
            || (sampling == RenderGpuSampling::Passes && scope == RenderGpuScope::Frame)
        {
            return None;
        }
        if self.gpu_capture.records.len() >= RECORD_CAPACITY {
            self.gpu_capture.dropped = self.gpu_capture.dropped.saturating_add(1);
            return None;
        }
        let started = self.device.borrow_mut().gpu_start();
        let (token, availability) = match started {
            Ok(token) => (Some(token), RenderGpuAvailability::Pending),
            Err(reason) => (None, reason),
        };
        self.gpu_capture.records.push(Record {
            sample: RenderGpuSample {
                identity: RenderGpuIdentity {
                    capture,
                    host,
                    context: self.gpu_capture.context,
                    frame: if self.gpu_capture.drawing {
                        self.gpu_capture.frame
                    } else {
                        0
                    },
                    preparation: !self.gpu_capture.drawing,
                    world,
                    profile_context: world.and_then(|reference| {
                        ipp_core::profiling::world_profile_context(host, reference)
                    }),
                    surface,
                },
                scope,
                availability,
            },
            token,
        });
        token.map(|token| RenderGpuScopeGuard {
            device: self.device.clone(),
            token,
        })
    }

    fn gpu_poll_samples(&mut self) {
        let length = self.gpu_capture.records.len();
        if length == 0 {
            return;
        }
        for _ in 0..POLL_BUDGET.min(length) {
            let index = self.gpu_capture.poll_cursor % length;
            self.gpu_capture.poll_cursor = (index + 1) % length;
            let record = &mut self.gpu_capture.records[index];
            if let Some(token) = record.token {
                record.sample.availability = self.device.borrow_mut().gpu_poll(token);
                if record.sample.availability != RenderGpuAvailability::Pending {
                    record.token = None;
                }
            }
        }
    }

    fn gpu_finish_pending(&mut self) {
        for record in &mut self.gpu_capture.records {
            if let Some(token) = record.token.take() {
                record.sample.availability = self.device.borrow_mut().gpu_poll(token);
            }
        }
    }

    pub(super) fn gpu_context_lost(&mut self) {
        self.device
            .borrow_mut()
            .gpu_stop(RenderGpuAvailability::ContextLost);
        self.gpu_finish_pending();
        self.finish_gl_call_capture(RenderGlStopReason::ContextLost);
    }

    pub(super) fn gpu_replace_context(&mut self) {
        self.gpu_context_lost();
        self.finish_gl_call_capture(RenderGlStopReason::ContextLost);
        self.gpu_capture.active = None;
        self.gpu_capture.context = self
            .gpu_capture
            .context
            .checked_add(1)
            .expect("GPU context identity exhausted");
    }
}
