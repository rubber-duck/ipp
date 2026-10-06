//! The browser worker's `HostServices` implementation and the Host composed
//! from it.

#[cfg(all(feature = "render", target_arch = "wasm32"))]
use super::render;

use std::collections::VecDeque;

use ipp_core::HostRuntime;
use ipp_host_session::HostServices;

/// Host composition for this platform.
pub(crate) type WasmHost = ipp_host_session::Host<WasmHostServices>;

pub(crate) struct WasmHostServices {
    input: ipp_host_session::services::gui_input::GuiHostInputService,
    pending: VecDeque<ipp_core::AssetAcquisitionRequest>,
    outbox: VecDeque<Vec<u8>>,
    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    pub(crate) presentation: render::RenderSurfaceService,
}

impl WasmHostServices {
    fn queue_resource(&mut self, kind: u8, id: u64, source: &str) -> Result<(), String> {
        let source_length = u32::try_from(source.len())
            .map_err(|_| "resource identifier exceeds u32".to_owned())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(13 + source.len())
            .map_err(|error| error.to_string())?;
        bytes.push(kind);
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&source_length.to_le_bytes());
        bytes.extend_from_slice(source.as_bytes());
        self.outbox.push_back(bytes);
        Ok(())
    }
}

impl HostServices for WasmHostServices {
    const NAME: &'static str = "wasm";

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn asset_gpu_formats(
        &self,
        kind: ipp_core::services::asset_management::AssetTypeId,
    ) -> Vec<ipp_core::services::asset_management::export::AssetExportFormat> {
        self.presentation.asset_gpu_formats(kind)
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn asset_gpu_export(
        &mut self,
        provider: &ipp_core::services::asset_management::AssetProvider,
        format: ipp_core::services::asset_management::export::AssetExportFormat,
        observer: std::rc::Rc<
            dyn ipp_core::services::asset_management::export::AssetOutputObserver,
        >,
    ) -> Result<ipp_core::services::asset_management::export::AssetExportFuture, String> {
        self.presentation
            .asset_gpu_export(provider, format, observer)
    }

    #[cfg(all(
        feature = "instrumentation",
        feature = "render",
        target_arch = "wasm32"
    ))]
    fn render_profile_start(
        &mut self,
        capture: u64,
        host: u64,
        options: ipp_protocol::host::profiling::ProfileRenderOptions,
    ) -> ipp_protocol::host::profiling::ProfileGpuCapability {
        self.presentation.profile_start(capture, host, options)
    }

    #[cfg(all(
        feature = "instrumentation",
        feature = "render",
        target_arch = "wasm32"
    ))]
    fn render_profile_stop(
        &mut self,
        capture: u64,
    ) -> ipp_protocol::host::profiling::ProfileGpuCapture {
        self.presentation.profile_stop(capture)
    }

    #[cfg(all(
        feature = "instrumentation",
        feature = "render",
        target_arch = "wasm32"
    ))]
    fn render_profile_cancel(&mut self, capture: u64) {
        self.presentation.profile_cancel(capture);
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn record_frame(&mut self, host: &mut HostRuntime, frame: &ipp_core::HostFrameReport) {
        self.presentation.record_frame(host, frame);
    }

    fn initialize(
        host: &mut HostRuntime,
        _schedulers: &ipp_host_session::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        for scheme in ["http", "https"] {
            host.register_stream_resource_provider(scheme)
                .map_err(|error| error.to_string())?;
        }
        host.register_stream_resource_provider("ipp")
            .map_err(|error| error.to_string())?;
        Ok(Self {
            input: Default::default(),
            pending: VecDeque::new(),
            outbox: VecDeque::new(),
            #[cfg(all(feature = "render", target_arch = "wasm32"))]
            presentation: render::RenderSurfaceService::new(host),
        })
    }

    fn gui_input(
        &mut self,
    ) -> Option<&mut ipp_host_session::services::gui_input::GuiHostInputService> {
        Some(&mut self.input)
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn render_viewport(&self) -> Option<(u32, u32)> {
        self.presentation.viewport()
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn prepare_presentation(
        &mut self,
        host: &mut HostRuntime,
        selected: Option<(ipp_core::OutputRef, ipp_core::WorldPublicationId)>,
    ) -> Result<(), ipp_host_session::HostPresentationFailure> {
        self.presentation.prepare(host, selected)
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn presentation_surface(
        &self,
    ) -> Result<
        ipp_protocol::host::presentation::PresentationSurface,
        ipp_protocol::host::presentation::PresentationError,
    > {
        self.presentation.surface()
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn configure_presentation(
        &mut self,
        viewport: ipp_core::WorldViewport,
    ) -> Result<(), ipp_protocol::host::presentation::PresentationError> {
        self.presentation.configure(viewport)
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn present(
        &mut self,
        host: &HostRuntime,
        output: ipp_core::OutputRef,
        publication: ipp_core::WorldPublicationId,
        viewport: ipp_core::WorldViewport,
        presentation_time: f64,
        completion: ipp_host_session::PresentationCompletion<'_>,
    ) -> Result<ipp_host_session::PresentationDrawSummary, ipp_host_session::HostPresentationFailure>
    {
        self.presentation.present(
            host,
            output,
            publication,
            viewport,
            presentation_time,
            completion,
        )
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn progress_assets(&mut self, host: &mut HostRuntime) {
        self.presentation.progress_resources(host);
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn progress_resources(&mut self, host: &mut HostRuntime) {
        self.presentation.progress_resources(host);
    }

    fn service_resources(&mut self, host: &mut HostRuntime) -> Result<(), String> {
        for id in host.take_resource_cancellations() {
            self.pending.retain(|request| request.id != id);
            self.queue_resource(0, id, "")?;
        }
        self.pending.extend(host.take_resource_requests());
        let mut generated = 0;
        for _ in 0..self.pending.len() {
            let request = self.pending.pop_front().expect("pending request");
            if request.source.starts_with("ipp://") {
                if generated == 8 {
                    self.pending.push_back(request);
                    continue;
                }
                generated += 1;
                let result = ipp_host_session::builtin_resource(&request);
                ipp_host_session::deliver_resource(host, request.id, result);
            } else {
                self.queue_resource(
                    if request.recovery {
                        2
                    } else {
                        1
                    },
                    request.id,
                    &request.source,
                )?;
            }
        }
        Ok(())
    }

    fn take_resource_request(&mut self) -> Option<Vec<u8>> {
        self.outbox.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipp_core::{
        AssetResourceStatus, Batch, Command, ComponentValue, EntityRef, FieldValue, FieldWrite,
        components::MeshInstance,
    };
    use std::mem::offset_of;

    #[test]
    fn browser_host_exposes_http_but_not_file_provider_requests() {
        let mut host = HostRuntime::new();
        let mut scheduler = ipp_host_session::services::task_scheduler::TaskSchedulerService::new();
        host.set_asset_load_scheduler(std::rc::Rc::new(scheduler.schedulers().host()));
        let mut platform =
            WasmHostServices::initialize(&mut host, &scheduler.schedulers()).unwrap();
        let id = host
            .create_world(
                Default::default(),
                &[
                    ipp_core::systems::animation::AnimationSystem::ID,
                    ipp_core::systems::asset_dependencies::AssetDependencySystem::ID,
                    ipp_core::systems::hierarchy::HierarchySystem::ID,
                    ipp_core::systems::look_at::LookAtSystem::ID,
                    ipp_core::systems::hierarchy::FinalPropagationSystem::ID,
                    ipp_core::systems::geometry::GeometrySystem::ID,
                    ipp_core::systems::render::RenderSystem::ID,
                ],
            )
            .unwrap();
        let mut world = host.world_mut(id).unwrap();
        let http = "https://example.test/mesh.ippm";
        let file = "file:///tmp/mesh.ippm";
        world
            .enqueue(Batch {
                id: 1,
                operations: [http, file]
                    .into_iter()
                    .enumerate()
                    .flat_map(|(index, source)| {
                        let alias = index as u32;
                        [
                            Command::Create {
                                alias,
                                metadata: Default::default(),
                                adopt: false,
                            },
                            Command::InsertComponent {
                                entity: EntityRef::Alias(alias),
                                component: ComponentValue::MESH_INSTANCE,
                                fields: vec![FieldWrite {
                                    offset: offset_of!(MeshInstance, source) as u32,
                                    value: FieldValue::String(source.into()),
                                }],
                                adopt: false,
                            },
                        ]
                    })
                    .collect(),
            })
            .unwrap();

        world.step(0.0).unwrap();
        drop(world);
        host.progress_assets();
        scheduler.poll_ready();
        platform.service_resources(&mut host).unwrap();
        host.progress_assets();
        let world = host.world_mut(id).unwrap();
        let request = platform.take_resource_request().unwrap();
        assert_eq!(request[0], 1);
        assert_eq!(&request[13..], http.as_bytes());
        assert!(platform.take_resource_request().is_none());
        assert!(matches!(
            &world
                .resource_snapshots()
                .into_iter()
                .find(|resource| *resource.source == *file)
                .unwrap()
                .status,
            AssetResourceStatus::Failed(error) if error.contains("file")
        ));
    }
}
