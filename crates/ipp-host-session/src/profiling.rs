//! Single evaluation-thread capture ownership and immutable diagnostic readback.
//! Measurement work is feature-gated; Host wire status remains in every build.

use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};

use ipp_core::profiling::{self as profile, ProfileContext};
use ipp_protocol::host::profiling::{ProfileRequest, ProfileResponse, ProfileStatus};

// The core counters are global: two Host adapters must not reset each other's capture.
static CAPTURE_HOST: AtomicU64 = AtomicU64::new(0);

struct Capture {
    owner: u64,
    thread: std::thread::ThreadId,
    id: u64,
    counters: bool,
    max_events: usize,
    max_artifact_bytes: usize,
    exceeded: bool,
    gpu_sampling: ipp_protocol::host::profiling::ProfileGpuSampling,
    gpu: Option<ipp_protocol::host::profiling::ProfileGpuCapture>,
    first_boundary: u64,
    start: u64,
    end: u64,
    artifact: Option<Vec<u8>>,
    read: usize,
}

#[derive(Default)]
pub(crate) struct HostProfiling {
    host: u64,
    boundary: u64,
    capture: Option<Capture>,
    abandoned: bool,
}

impl HostProfiling {
    pub(crate) fn completed_boundary(&mut self) {
        self.boundary += 1;
    }

    pub(crate) fn disconnect(&mut self, connection: u64) {
        if self
            .capture
            .as_ref()
            .is_some_and(|capture| capture.owner == connection)
        {
            // A transport may fail while frame guards are live. Reclaim only at
            // the next control boundary; no released index can be reused early.
            profile::pause();
            self.abandoned = true;
        }
    }

    pub(crate) fn cleanup<P: crate::HostServices>(&mut self, services: &mut P) {
        if self.abandoned {
            if let Some(capture) = &self.capture {
                services.render_profile_cancel(capture.id);
            }
            profile::release_capture();
            self.capture = None;
            self.abandoned = false;
            CAPTURE_HOST.store(0, Ordering::Relaxed);
        }
    }

    pub(crate) fn request<P: crate::HostServices>(
        &mut self,
        host: u64,
        adapter: &str,
        connection: u64,
        request: ProfileRequest,
        services: &mut P,
    ) -> ProfileResponse {
        self.cleanup(services);
        let mut response = ProfileResponse::status(ProfileStatus::Available);
        if matches!(request, ProfileRequest::Status) {
            response.capture = self
                .capture
                .as_ref()
                .filter(|capture| capture.owner == connection)
                .map_or(0, |capture| capture.id);
            return response;
        }
        if let ProfileRequest::Start {
            counters,
            max_events,
            max_artifact_bytes,
            gpu,
            gl_calls,
        } = request
        {
            let Ok(max_artifact_bytes) = usize::try_from(max_artifact_bytes) else {
                return ProfileResponse::status(ProfileStatus::Capacity);
            };
            if max_artifact_bytes == 0 || max_artifact_bytes > isize::MAX as usize {
                return ProfileResponse::status(ProfileStatus::Capacity);
            }
            if self.capture.is_some()
                || CAPTURE_HOST
                    .compare_exchange(0, host, Ordering::Relaxed, Ordering::Relaxed)
                    .is_err()
            {
                return ProfileResponse::status(ProfileStatus::Busy);
            }
            let Ok(max_events) = usize::try_from(max_events) else {
                CAPTURE_HOST.store(0, Ordering::Relaxed);
                return ProfileResponse::status(ProfileStatus::Capacity);
            };
            self.host = host;
            // Also reclaim history left by a disconnected/dropped capture owner.
            profile::release_capture();
            if ipp_core::profiling::trace::prepare(max_events).is_err() {
                CAPTURE_HOST.store(0, Ordering::Relaxed);
                return ProfileResponse::status(ProfileStatus::Capacity);
            }
            let start = profile::clock_nanos();
            profile::reset_for_host(counters, host);
            if max_events != 0 {
                profile::enable_trace();
            }
            let id = profile::capture_id();
            let capability = services.render_profile_start(
                id,
                host,
                ipp_protocol::host::profiling::ProfileRenderOptions {
                    gpu,
                    gl_calls,
                },
            );
            self.capture = Some(Capture {
                owner: connection,
                thread: std::thread::current().id(),
                id,
                counters,
                max_events,
                max_artifact_bytes,
                exceeded: false,
                gpu_sampling: gpu,
                gpu: Some(ipp_protocol::host::profiling::ProfileGpuCapture {
                    sampling: gpu,
                    capability,
                    availability:
                        ipp_protocol::host::profiling::ProfileGpuAvailability::Unsupported,
                    dropped_records: 0,
                    records: Vec::new(),
                    gl_calls: None,
                }),
                first_boundary: self.boundary,
                start,
                end: 0,
                artifact: None,
                read: 0,
            });
            response.capture = id;
            return response;
        }
        let requested = match request {
            ProfileRequest::Stop(id) | ProfileRequest::Release(id) => id,
            ProfileRequest::Read {
                capture,
                ..
            } => capture,
            _ => unreachable!(),
        };
        let Some(capture) = self.capture.as_mut() else {
            return ProfileResponse::status(ProfileStatus::InvalidCapture);
        };
        if capture.owner != connection
            || capture.id != requested
            || capture.thread != std::thread::current().id()
        {
            return ProfileResponse::status(ProfileStatus::InvalidCapture);
        }
        response.capture = capture.id;
        match request {
            ProfileRequest::Stop(_) => {
                if capture.exceeded {
                    return ProfileResponse::status(ProfileStatus::Capacity);
                }
                if capture.artifact.is_none() {
                    profile::pause();
                    capture.end = profile::clock_nanos();
                    let mut gpu = services.render_profile_stop(capture.id);
                    gpu.sampling = capture.gpu_sampling;
                    capture.gpu = Some(gpu);
                    capture.artifact = artifact(host, adapter, capture, self.boundary);
                    if capture.artifact.is_none() {
                        capture.exceeded = true;
                        return ProfileResponse::status(ProfileStatus::Capacity);
                    }
                }
                response.total_bytes = capture.artifact.as_ref().unwrap().len() as u64;
            }
            ProfileRequest::Read {
                offset,
                ..
            } => {
                let Some(artifact) = &capture.artifact else {
                    return ProfileResponse::status(ProfileStatus::Incomplete);
                };
                if offset != capture.read as u64 {
                    return ProfileResponse::status(ProfileStatus::InvalidCapture);
                }
                let end = capture
                    .read
                    .saturating_add(ipp_protocol::MAX_FIELD_BYTES)
                    .min(artifact.len());
                response.offset = offset;
                response.total_bytes = artifact.len() as u64;
                response.bytes = artifact[capture.read..end].to_vec();
                capture.read = end;
            }
            ProfileRequest::Release(_) => {
                // Explicit owner cancellation also releases running, failed or partial captures.
                services.render_profile_cancel(capture.id);
                profile::release_capture();
                self.capture = None;
                CAPTURE_HOST.store(0, Ordering::Relaxed);
            }
            _ => unreachable!(),
        }
        response
    }
}

impl Drop for HostProfiling {
    fn drop(&mut self) {
        if self.capture.is_some() {
            profile::pause();
            // Dropping the owning Host cancels delivery. A later capture start
            // reclaims retained history at its quiescent control boundary.
            let _ =
                CAPTURE_HOST.compare_exchange(self.host, 0, Ordering::Relaxed, Ordering::Relaxed);
        }
    }
}

fn quote(out: &mut BoundedArtifact, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            character if character < ' ' => {
                write!(out, "\\u{:04x}", character as u32).unwrap();
            }
            character => out.push(character),
        }
    }
    out.push('"');
}

fn identity(out: &mut BoundedArtifact, context: ProfileContext) {
    let scope = if context.host == 0 {
        "unassigned"
    } else if context.incarnation == 0 {
        "shared-host"
    } else {
        "world"
    };
    write!(out, "{{\"scope\":\"{scope}\",\"hostId\":\"{}\",\"worldId\":\"{}\",\"incarnation\":\"{}\",\"compositionId\":\"{}\",\"system\":", context.host, context.world, context.incarnation, context.composition).unwrap();
    quote(out, context.system);
    out.push_str(",\"phase\":");
    if let Some(phase) = context.phase {
        quote(out, phase.name());
    } else {
        out.push_str("null");
    }
    out.push('}');
}

fn artifact(host: u64, adapter: &str, capture: &Capture, boundary: u64) -> Option<Vec<u8>> {
    let mut out = BoundedArtifact {
        text: String::new(),
        limit: capture.max_artifact_bytes,
        exceeded: false,
    };
    let target = if cfg!(target_arch = "wasm32") {
        "wasm"
    } else {
        "native"
    };
    let availability = if capture.counters {
        "available"
    } else {
        "not-requested"
    };
    let allocation_availability = if !capture.counters {
        "not-requested"
    } else if profile::allocator_available() {
        "available"
    } else {
        "unavailable"
    };
    let gpu_availability =
        if capture.gpu_sampling == ipp_protocol::host::profiling::ProfileGpuSampling::Off {
            "not-requested"
        } else if capture.gpu.as_ref().is_some_and(|gpu| {
            gpu.capability != ipp_protocol::host::profiling::ProfileGpuCapability::Unsupported
        }) {
            "available"
        } else {
            "unavailable"
        };
    write!(out, "{{\"format\":\"ipp-profile/1\",\"captureId\":\"{}\",\"hostId\":\"{host}\",\"source\":{{\"target\":\"{target}\",\"schemaHash\":\"{}\",\"instrumentation\":true,\"scope\":\"evaluation-thread\",\"backgroundAllocations\":\"excluded\",\"adapter\":", capture.id, ipp_protocol::contract::schema_hash()).unwrap();
    quote(&mut out, adapter);
    let trace_availability = if capture.max_events == 0 {
        "not-requested"
    } else {
        "available"
    };
    write!(out, "}},\"window\":{{\"firstBoundary\":\"{}\",\"lastBoundary\":\"{boundary}\",\"start\":\"{}\",\"end\":\"{}\",\"clockDomain\":\"ipp-core.monotonic\",\"unit\":\"nanoseconds\"}},\"availability\":{{\"cpu\":\"{availability}\",\"allocations\":\"{allocation_availability}\",\"gpu\":\"{gpu_availability}\",\"trace\":\"{trace_availability}\"}},\"units\":{{\"calls\":\"calls\",\"duration\":\"nanoseconds\",\"allocationCalls\":\"allocation-or-reallocation calls\",\"requestedBytes\":\"requested bytes, not retained memory\"}},\"stages\":[", capture.first_boundary, capture.start, capture.end).unwrap();
    // Metadata is bounded while it is written, without first allocating an oversized artifact.
    let category_started = std::cell::Cell::new(false);
    {
        let output = std::cell::RefCell::new(&mut out);
        let mut stage_separator = "";
        let mut category_separator = "";
        profile::visit_capture(
            |record| {
                let mut output = output.borrow_mut();
                let out = &mut **output;
                if out.exceeded {
                    return false;
                }
                out.push_str(stage_separator);
                stage_separator = ",";
                let kind = if record.system_stage {
                    "system"
                } else {
                    "fixed"
                };
                write!(out, "{{\"kind\":\"{kind}\",\"name\":").unwrap();
                quote(out, record.name);
                out.push_str(",\"identity\":");
                identity(out, record.context);
                let values = record.values;
                write!(out, ",\"calls\":\"{}\",\"duration\":\"{}\",\"allocationCalls\":\"{}\",\"requestedBytes\":\"{}\"}}", values[0], values[1], values[2], values[3]).unwrap();
                !out.exceeded
            },
            |name, context, [calls, bytes]| {
                let mut output = output.borrow_mut();
                let out = &mut **output;
                if out.exceeded {
                    return false;
                }
                if !category_started.replace(true) {
                    out.push_str("],\"categories\":[");
                }
                out.push_str(category_separator);
                category_separator = ",";
                out.push_str("{\"name\":");
                quote(out, name);
                out.push_str(",\"identity\":");
                identity(out, context);
                write!(
                    out,
                    ",\"allocationCalls\":\"{calls}\",\"requestedBytes\":\"{bytes}\"}}"
                )
                .unwrap();
                !out.exceeded
            },
        );
    }
    if !category_started.get() {
        out.push_str("],\"categories\":[");
    }
    let (calls, bytes) = profile::allocations();
    let (counter_bytes, metadata_bytes) = profile::storage_bytes();
    write!(out, "],\"allocations\":{{\"calls\":\"{calls}\",\"requestedBytes\":\"{bytes}\"}},\"retention\":{{\"artifactLimitBytes\":\"{}\"}},\"instrumentationStorage\":{{\"counterBytes\":\"{counter_bytes}\",\"metadataBytes\":\"{metadata_bytes}\"}}}}", capture.max_artifact_bytes).unwrap();
    if out.exceeded {
        return None;
    }
    if let Some(gpu) = &capture.gpu {
        out.text.pop();
        gpu_artifact(&mut out, gpu);
        out.push('}');
    }
    if capture.max_events != 0 && !out.exceeded {
        out.text.pop();
        let (capacity, dropped, backing) = ipp_core::profiling::trace::retention();
        write!(out, ",\"trace\":{{\"clockDomain\":\"ipp-core.monotonic\",\"unit\":\"nanoseconds\",\"policy\":\"drop-new\",\"capacity\":{capacity},\"droppedEvents\":\"{dropped}\",\"retainedBytes\":\"{backing}\",\"events\":[").unwrap();
        let mut separator = "";
        ipp_core::profiling::trace::visit(|span| {
            write!(
                out,
                "{separator}{{\"sequence\":\"{}\",\"kind\":\"{}\",\"name\":",
                span.sequence, span.kind
            )
            .unwrap();
            quote(&mut out, span.name);
            out.push_str(",\"identity\":");
            identity(&mut out, span.context);
            write!(
                out,
                ",\"start\":\"{}\",\"end\":\"{}\",\"threadId\":\"{}\",\"hostFrameId\":",
                span.start, span.end, span.thread
            )
            .unwrap();
            if let Some(frame) = span.host_frame {
                write!(out, "\"{frame}\"").unwrap();
            } else {
                out.push_str("null");
            }
            out.push('}');
            separator = ",";
            !out.exceeded
        });
        out.push_str("]}}");
    }
    (!out.exceeded).then(|| out.text.into_bytes())
}

struct BoundedArtifact {
    text: String,
    limit: usize,
    exceeded: bool,
}

impl BoundedArtifact {
    fn push_str(&mut self, value: &str) {
        let Some(required) = self
            .text
            .len()
            .checked_add(value.len())
            .filter(|size| *size <= self.limit)
        else {
            self.exceeded = true;
            return;
        };
        if required > self.text.capacity() {
            let target = required
                .checked_next_power_of_two()
                .unwrap_or(self.limit)
                .min(self.limit);
            if self
                .text
                .try_reserve_exact(target - self.text.len())
                .is_err()
            {
                self.exceeded = true;
                return;
            }
        }
        self.text.push_str(value);
    }

    fn push(&mut self, value: char) {
        self.push_str(value.encode_utf8(&mut [0; 4]));
    }
}

impl Write for BoundedArtifact {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.push_str(value);
        Ok(())
    }
}

fn gpu_artifact(out: &mut BoundedArtifact, gpu: &ipp_protocol::host::profiling::ProfileGpuCapture) {
    use ipp_protocol::host::profiling::*;
    let sampling = match gpu.sampling {
        ProfileGpuSampling::Off => "off",
        ProfileGpuSampling::Frame => "frame",
        ProfileGpuSampling::Passes => "passes",
        ProfileGpuSampling::All => "all",
    };
    let capability = match gpu.capability {
        ProfileGpuCapability::Unsupported => "unsupported",
        ProfileGpuCapability::Elapsed => "elapsed",
        ProfileGpuCapability::Timestamps => "timestamps",
    };
    write!(out, ",\"gpu\":{{\"sampling\":\"{sampling}\",\"capability\":\"{capability}\",\"droppedRecords\":\"{}\",\"records\":[", gpu.dropped_records).unwrap();
    let mut separator = "";
    for record in &gpu.records {
        if out.exceeded {
            return;
        }
        out.push_str(separator);
        separator = ",";
        write!(out, "{{\"captureId\":\"{}\",\"hostId\":\"{}\",\"contextId\":\"{}\",\"frameId\":\"{}\",\"preparation\":{},\"world\":", record.capture, record.host, record.context, record.frame, record.preparation).unwrap();
        if let Some(world) = record.world {
            write!(
                out,
                "{{\"id\":\"{}\",\"incarnation\":\"{}\",\"compositionId\":",
                world.id, world.incarnation
            )
            .unwrap();
            if let Some(composition) = world.composition {
                write!(out, "\"{composition}\"").unwrap();
            } else {
                out.push_str("null");
            }
            out.push('}');
        } else {
            out.push_str("null");
        }
        out.push_str(",\"surface\":");
        if let Some(surface) = record.surface {
            write!(
                out,
                "{{\"slot\":{},\"generation\":{}}}",
                surface.slot, surface.generation
            )
            .unwrap();
        } else {
            out.push_str("null");
        }
        let scope = match record.scope {
            ProfileGpuScope::Frame => "frame",
            ProfileGpuScope::Surface => "surface",
            ProfileGpuScope::Atlas => "atlas",
            ProfileGpuScope::CacheRepaint => "cache-repaint",
            ProfileGpuScope::Composite => "composite",
        };
        write!(out, ",\"scope\":\"{scope}\",\"availability\":").unwrap();
        gpu_availability(out, record.availability);
        out.push('}');
    }
    out.push_str("],\"glCalls\":");
    if let Some(calls) = gpu.gl_calls {
        let availability = match calls.availability {
            ProfileGlAvailability::Available => "available",
            ProfileGlAvailability::Unsupported => "unsupported",
            ProfileGlAvailability::NotRequested => "not-requested",
        };
        let reason = match calls.stop_reason {
            ProfileGlStopReason::OwnerStopped => "owner-stopped",
            ProfileGlStopReason::ContextLost => "context-lost",
        };
        let source = match calls.source {
            ProfileGlSource::WebGl => "webgl",
            ProfileGlSource::Gles => "gles",
        };
        write!(out, "{{\"stopReason\":\"{reason}\",\"window\":{{\"start\":").unwrap();
        if let Some(start) = calls.window_start {
            write!(out, "\"{start}\"").unwrap();
        } else {
            out.push_str("null");
        }
        out.push_str(",\"end\":");
        if let Some(end) = calls.window_end {
            write!(out, "\"{end}\"").unwrap();
        } else {
            out.push_str("null");
        }
        out.push_str(",\"clockDomain\":\"ipp-core.monotonic\",\"unit\":\"nanoseconds\"},");
        write!(out, "\"source\":\"{source}\",\"contextId\":\"{}\",\"scope\":\"device-context\",\"availability\":\"{availability}\",\"draws\":\"{}\",\"state\":\"{}\",\"uploads\":\"{}\",\"other\":\"{}\",\"profiler\":\"{}\",\"overflowed\":{},\"unit\":\"physical entry point calls\"}}", calls.context, calls.draws, calls.state, calls.uploads, calls.other, calls.profiler, calls.overflowed).unwrap();
    } else {
        out.push_str("null");
    }
    out.push_str(",\"availability\":");
    gpu_availability(out, gpu.availability);
    out.push('}');
}

fn gpu_availability(
    out: &mut BoundedArtifact,
    availability: ipp_protocol::host::profiling::ProfileGpuAvailability,
) {
    use ipp_protocol::host::profiling::ProfileGpuAvailability::*;
    let name = match availability {
        Pending => "pending",
        Available {
            duration_ns,
        } => {
            write!(out, "{{\"status\":\"available\",\"duration\":\"{duration_ns}\",\"unit\":\"nanoseconds\"}}").unwrap();
            return;
        }
        Unsupported => "unsupported",
        Disjoint => "disjoint",
        ContextLost => "context-lost",
        CapacityDropped => "capacity-dropped",
        OverlapSkipped => "overlap-skipped",
        Stopped => "stopped",
    };
    write!(out, "{{\"status\":\"{name}\"}}").unwrap();
}

#[cfg(test)]
#[path = "profiling_tests.rs"]
mod tests;
