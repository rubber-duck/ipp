//! The platform services a Host adapter implements, and provider completion shared by adapters.
//!
//! Recoverable presentation failures are separate from World and Host lifetime.

use crate::services;
use crate::services::presentation::{PresentationCompletion, PresentationDrawSummary};

/// HostServices work executed at Host-owned service and frame boundaries.
pub trait HostServices {
    /// Short diagnostic identity such as `server` or `wasm`.
    const NAME: &'static str;

    /// Supported semantic GPU encodings for this compiled Host/device.
    fn asset_gpu_formats(
        &self,
        _kind: ipp_core::services::asset_management::AssetTypeId,
    ) -> Vec<ipp_core::services::asset_management::export::AssetExportFormat> {
        Vec::new()
    }

    /// Capture an owned renderer operation without graphics calls at this factory boundary.
    /// The Host scheduler establishes current context before polling and cancellation cleanup.
    fn asset_gpu_export(
        &mut self,
        _provider: &ipp_core::services::asset_management::AssetProvider,
        _format: ipp_core::services::asset_management::export::AssetExportFormat,
        _observer: std::rc::Rc<
            dyn ipp_core::services::asset_management::export::AssetOutputObserver,
        >,
    ) -> Result<ipp_core::services::asset_management::export::AssetExportFuture, String> {
        Err("GPU asset export is unsupported by this Host".into())
    }

    /// Start optional renderer measurements at an accepted capture boundary.
    #[cfg(feature = "instrumentation")]
    fn render_profile_start(
        &mut self,
        _capture: u64,
        _host: u64,
        _options: ipp_protocol::host::profiling::ProfileRenderOptions,
    ) -> ipp_protocol::host::profiling::ProfileGpuCapability {
        ipp_protocol::host::profiling::ProfileGpuCapability::Unsupported
    }

    /// Freeze terminal and pending GPU records without waiting for the device.
    #[cfg(feature = "instrumentation")]
    fn render_profile_stop(
        &mut self,
        _capture: u64,
    ) -> ipp_protocol::host::profiling::ProfileGpuCapture {
        use ipp_protocol::host::profiling::*;
        ProfileGpuCapture {
            sampling: ProfileGpuSampling::Off,
            capability: ProfileGpuCapability::Unsupported,
            availability: ProfileGpuAvailability::Unsupported,
            dropped_records: 0,
            records: Vec::new(),
            gl_calls: None,
        }
    }

    /// Cancel pending owned GPU queries at a quiescent control boundary.
    #[cfg(feature = "instrumentation")]
    fn render_profile_cancel(&mut self, _capture: u64) {}

    /// Initialize shared facilities before publishing any world.
    fn initialize(
        host: &mut ipp_core::HostRuntime,
        _schedulers: &crate::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String>
    where
        Self: Sized;

    /// Prepare platform context for ready task polls and cancellation destructors.
    /// This boundary must not mutate or evaluate Worlds.
    fn prepare_task_poll(&mut self, _host: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }

    /// Current rendered surface dimensions; headless hosts accept query dimensions.
    fn render_viewport(&self) -> Option<(u32, u32)> {
        None
    }

    /// Observe the completed Host evaluation boundary without advancing or mutating Worlds.
    fn record_frame(
        &mut self,
        _host: &mut ipp_core::HostRuntime,
        _frame: &ipp_core::HostFrameReport,
    ) {
    }

    /// Admit composed input, continuing healthy roots and returning scoped failures.
    fn route_input(
        &mut self,
        host: &mut ipp_core::HostRuntime,
        _frame: &ipp_core::HostFrameReport,
    ) -> Vec<HostInputFailure> {
        let surface = self.presentation_surface().ok();
        if let Some(input) = self.gui_input() {
            input.route(host, surface);
        }

        Vec::new()
    }

    /// Optional physical GUI adapter. Headless semantic commands do not use this owner.
    fn gui_input(&mut self) -> Option<&mut services::gui_input::GuiHostInputService> {
        None
    }

    /// Physical selection lifetime hook, independent of authoring sessions.
    fn presentation_selection(
        &mut self,
        host: &mut ipp_core::HostRuntime,
        selection: Option<ipp_protocol::host::presentation::PresentationView>,
    ) {
        if let Some(input) = self.gui_input() {
            input.selection(host, selection);
        }
    }

    /// Prepare renderer demand without borrowing live World component values.
    fn prepare_presentation(
        &mut self,
        _host: &mut ipp_core::HostRuntime,
        _selected: Option<(ipp_core::OutputRef, ipp_core::WorldPublicationId)>,
    ) -> Result<(), HostPresentationFailure> {
        Ok(())
    }

    /// Present an explicitly selected completed root output, independently of sessions.
    fn present(
        &mut self,
        _host: &ipp_core::HostRuntime,
        _output: ipp_core::OutputRef,
        _publication: ipp_core::WorldPublicationId,
        _viewport: ipp_core::WorldViewport,
        _presentation_time: f64,
        _completion: PresentationCompletion<'_>,
    ) -> Result<PresentationDrawSummary, HostPresentationFailure> {
        Err(HostPresentationFailure {
            scope: ipp_protocol::world::RuntimeFailureScope::Context,
            message: "presentation is unsupported".into(),
        })
    }

    /// Actual surface/context identity and device bounds, independent of World sessions.
    fn presentation_surface(
        &self,
    ) -> Result<
        ipp_protocol::host::presentation::PresentationSurface,
        ipp_protocol::host::presentation::PresentationError,
    > {
        Err(ipp_protocol::host::presentation::PresentationError::Unsupported)
    }

    /// Configure the exact acknowledged extent; adapters must not silently clamp it.
    fn configure_presentation(
        &mut self,
        _viewport: ipp_core::WorldViewport,
    ) -> Result<(), ipp_protocol::host::presentation::PresentationError> {
        Err(ipp_protocol::host::presentation::PresentationError::Unsupported)
    }

    /// Service provider cancellations and requests at the host boundary.
    fn service_resources(&mut self, host: &mut ipp_core::HostRuntime) -> Result<(), String>;

    /// Progress shared loaders exactly once before world evaluation.
    fn progress_assets(&mut self, host: &mut ipp_core::HostRuntime) {
        self.progress_resources(host);
    }

    /// Progress shared loaders between frames without resetting frame-local state.
    fn progress_resources(&mut self, host: &mut ipp_core::HostRuntime) {
        host.progress_assets();
    }

    /// Dequeue one services-owned provider request, when the host exposes one.
    fn take_resource_request(&mut self) -> Option<Vec<u8>> {
        None
    }
}

/// A presentation adapter reports the scope without faulting the Host.
#[derive(Clone, Debug)]
pub struct HostPresentationFailure {
    /// Draw, resource or context boundary that failed.
    pub scope: ipp_protocol::world::RuntimeFailureScope,
    /// Diagnostic retained separately from semantic mutation outcomes.
    pub message: String,
}

/// A platform input context failed without invalidating evaluation outcomes.
#[derive(Clone, Debug)]
pub struct HostInputFailure {
    /// Exact root whose platform input context failed.
    pub output: ipp_core::OutputRef,
    /// Diagnostic retained separately from semantic mutation outcomes.
    pub message: String,
}

/// Deliver owned provider input without turning resource pressure into session failure.
///
/// The core can reject a complete buffer before accepting ownership into its input
/// queue. This adapter has already consumed the provider result, so a rejection
/// ends that resource's reader with an ordinary failure. Its declaration and any
/// committed batch outcomes remain intact; the next resource poll reports Failed.
pub fn deliver_resource(
    world: &mut ipp_core::HostRuntime,
    id: u64,
    result: Result<Vec<u8>, String>,
) {
    if let Err(error) = world.complete_resource(id, result) {
        world.asset_input_end(id, Err(format!("Asset provider input rejected: {error}")));
    }
}

/// Generate a compiled built-in source through the ordinary provider contract.
pub fn builtin_resource(request: &ipp_core::AssetAcquisitionRequest) -> Result<Vec<u8>, String> {
    let result = match request.kind {
        ipp_core::AssetResourceKind::Mesh => {
            ipp_core::services::asset_management::builtin::mesh(&request.source)
        }
        ipp_core::AssetResourceKind::Texture => {
            ipp_core::services::asset_management::builtin::texture(&request.source)
        }
        kind if kind == ipp_core::SKELETON_TYPE || kind == ipp_core::POSE_TYPE => {
            ipp_core::services::asset_management::builtin::rig(kind, &request.source)
        }
        ipp_core::SKIN_TYPE => {
            ipp_core::services::asset_management::builtin::rig(request.kind, &request.source)
        }
        _ => return Err("Built-in resource type unavailable".into()),
    };
    result.map_err(|error| error.to_string())
}
