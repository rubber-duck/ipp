//! Adapter-owned mapping from retained renderer samples to the Host capture envelope.
use ipp_protocol::profiling::*;
use ipp_render_gl::{
    RenderDevice, RenderGpuAvailability as Availability, RenderGpuCapability as Capability,
    RenderGpuSampling as Sampling, RenderGpuScope as Scope, RenderService,
};

pub(super) struct RenderProfileCapture {
    capture: u64,
    context: u64,
    options: ProfileRenderOptions,
    capability: ProfileGpuCapability,
    availability: ProfileGpuAvailability,
}

impl RenderProfileCapture {
    pub(super) fn start<D: RenderDevice>(
        renderer: &mut RenderService<D>,
        capture: u64,
        host: u64,
        options: ProfileRenderOptions,
    ) -> Self {
        if options.gl_calls {
            renderer.start_gl_call_capture();
        }
        let sampling = match options.gpu {
            ProfileGpuSampling::Off => None,
            ProfileGpuSampling::Frame => Some(Sampling::Frame),
            ProfileGpuSampling::Passes => Some(Sampling::Passes),
            ProfileGpuSampling::All => Some(Sampling::All),
        };
        let (capability, availability) = match sampling {
            None => (
                ProfileGpuCapability::Unsupported,
                ProfileGpuAvailability::Unsupported,
            ),
            Some(sampling) => match renderer.start_gpu_capture(capture, host, sampling) {
                Ok(value) => {
                    let capability = map_capability(value);
                    let availability = if capability == ProfileGpuCapability::Unsupported {
                        ProfileGpuAvailability::Unsupported
                    } else {
                        ProfileGpuAvailability::Pending
                    };
                    (capability, availability)
                }
                Err(reason) => (ProfileGpuCapability::Unsupported, map_availability(reason)),
            },
        };
        Self {
            context: renderer.gpu_context_generation(),
            capture,
            options,
            capability,
            availability,
        }
    }

    pub(super) fn capability(&self) -> ProfileGpuCapability {
        self.capability
    }

    pub(super) fn matches(&self, capture: u64) -> bool {
        self.capture == capture
    }

    pub(super) fn stop<D: RenderDevice>(
        self,
        renderer: &mut RenderService<D>,
    ) -> ProfileGpuCapture {
        renderer.stop_gpu_capture();
        renderer.stop_gl_call_capture();
        let counts = renderer.gl_call_counts();
        let window = renderer.gl_call_window();
        let records = renderer
            .drain_gpu_samples()
            .into_iter()
            .map(|sample| {
                let identity = sample.identity;
                ProfileGpuRecord {
                    capture: identity.capture,
                    host: identity.host,
                    context: identity.context,
                    frame: identity.frame,
                    preparation: identity.preparation,
                    world: identity.world.map(|reference| ProfileGpuWorld {
                        id: reference.id().0,
                        incarnation: reference.incarnation(),
                        composition: identity.profile_context.map(|context| context.composition),
                    }),
                    surface: identity.surface.map(|entity| ProfileGpuEntity {
                        slot: entity.index(),
                        generation: entity.generation(),
                    }),
                    scope: match sample.scope {
                        Scope::Frame => ProfileGpuScope::Frame,
                        Scope::Surface => ProfileGpuScope::Surface,
                        Scope::Atlas => ProfileGpuScope::Atlas,
                        Scope::CacheRepaint => ProfileGpuScope::CacheRepaint,
                        Scope::Composite => ProfileGpuScope::Composite,
                    },
                    availability: map_availability(sample.availability),
                }
            })
            .collect();
        ProfileGpuCapture {
            sampling: self.options.gpu,
            capability: self.capability,
            availability: if self.availability == ProfileGpuAvailability::Pending {
                ProfileGpuAvailability::Stopped
            } else {
                self.availability
            },
            dropped_records: renderer.gpu_dropped_records(),
            records,
            gl_calls: self.options.gl_calls.then_some(ProfileGlCalls {
                availability: if counts.is_some() {
                    ProfileGlAvailability::Available
                } else {
                    ProfileGlAvailability::Unsupported
                },
                source: ProfileGlSource::WebGl,
                context: self.context,
                stop_reason: match window.map(|value| value.stop_reason) {
                    Some(ipp_render_gl::RenderGlStopReason::ContextLost) => {
                        ProfileGlStopReason::ContextLost
                    }
                    _ => ProfileGlStopReason::OwnerStopped,
                },
                window_start: window.map(|value| value.start),
                window_end: window.and_then(|value| value.end),
                draws: counts.map_or(0, |value| value.draws),
                state: counts.map_or(0, |value| value.state),
                uploads: counts.map_or(0, |value| value.uploads),
                other: counts.map_or(0, |value| value.other),
                profiler: counts.map_or(0, |value| value.profiler),
                overflowed: counts.is_some_and(|value| value.overflowed),
            }),
        }
    }

    pub(super) fn cancel<D: RenderDevice>(self, renderer: &mut RenderService<D>) {
        renderer.stop_gpu_capture();
        renderer.stop_gl_call_capture();
        renderer.drain_gpu_samples();
    }
}

fn map_capability(value: Capability) -> ProfileGpuCapability {
    match value {
        Capability::Unsupported => ProfileGpuCapability::Unsupported,
        Capability::Elapsed => ProfileGpuCapability::Elapsed,
        Capability::Timestamps => ProfileGpuCapability::Timestamps,
    }
}

fn map_availability(value: Availability) -> ProfileGpuAvailability {
    match value {
        Availability::Pending => ProfileGpuAvailability::Pending,
        Availability::Available {
            duration_ns,
        } => ProfileGpuAvailability::Available {
            duration_ns,
        },
        Availability::Unsupported => ProfileGpuAvailability::Unsupported,
        Availability::Disjoint => ProfileGpuAvailability::Disjoint,
        Availability::ContextLost => ProfileGpuAvailability::ContextLost,
        Availability::CapacityDropped => ProfileGpuAvailability::CapacityDropped,
        Availability::OverlapSkipped => ProfileGpuAvailability::OverlapSkipped,
        Availability::Stopped => ProfileGpuAvailability::Stopped,
    }
}
