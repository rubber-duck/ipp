//! Browser adapters for the shared host session policy.

#[cfg(all(feature = "render", target_arch = "wasm32"))]
use super::render;

use std::collections::VecDeque;

use ipp_core::{HostRuntime, WorldContext};
use ipp_host_session::HostServices;

pub(crate) struct WasmHostServices {
    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    presentation_world: Option<ipp_core::WorldId>,
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

    fn initialize(_host: &mut HostRuntime) -> Result<Self, String> {
        for scheme in ["http", "https"] {
            _host
                .register_stream_resource_provider(scheme)
                .map_err(|error| error.to_string())?;
        }
        #[cfg(feature = "builtin-assets")]
        _host
            .register_stream_resource_provider("ipp")
            .map_err(|error| error.to_string())?;
        Ok(Self {
            #[cfg(all(feature = "render", target_arch = "wasm32"))]
            presentation_world: None,
            pending: VecDeque::new(),
            outbox: VecDeque::new(),
            #[cfg(all(feature = "render", target_arch = "wasm32"))]
            presentation: render::RenderSurfaceService::new(_host),
        })
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn render_viewport(&self) -> Option<(u32, u32)> {
        self.presentation.viewport()
    }

    fn attach_world(&mut self, _world: ipp_core::WorldId) -> Result<(), String> {
        #[cfg(all(feature = "render", target_arch = "wasm32"))]
        {
            self.presentation_world = Some(_world);
            self.presentation.reset_world();
        }
        Ok(())
    }

    fn detach_world(&mut self, _world: ipp_core::WorldId) {
        #[cfg(all(feature = "render", target_arch = "wasm32"))]
        if self.presentation_world == Some(_world) {
            self.presentation_world = None;
            self.presentation.forget_world(_world);
        }
    }

    fn present(
        &mut self,
        _world: &mut WorldContext<'_>,
    ) -> Result<(), ipp_host_session::HostPresentationFailure> {
        #[cfg(all(feature = "render", target_arch = "wasm32"))]
        if self.presentation_world == Some(_world.id()) {
            self.presentation.render(_world)?;
        }
        Ok(())
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    fn progress_assets(&mut self, host: &mut HostRuntime) {
        self.presentation.progress_assets(host);
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
                #[cfg(feature = "builtin-assets")]
                let result = ipp_host_session::builtin_resource(&request);
                #[cfg(not(feature = "builtin-assets"))]
                let result = Err("ipp resource provider is unavailable in this build".into());
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
        let mut platform = WasmHostServices::initialize(&mut host).unwrap();
        let id = host.create_world(Default::default()).unwrap();
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
                            },
                            Command::InsertComponent {
                                entity: EntityRef::Alias(alias),
                                component: ComponentValue::MESH_INSTANCE,
                                fields: vec![FieldWrite {
                                    offset: offset_of!(MeshInstance, source) as u32,
                                    value: FieldValue::String(source.into()),
                                }],
                            },
                        ]
                    })
                    .collect(),
            })
            .unwrap();

        world.step(0.0).unwrap();
        drop(world);
        host.progress_assets();
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
                .find(|resource| resource.source == file)
                .unwrap()
                .status,
            AssetResourceStatus::Failed(error) if error.contains("file")
        ));
    }
}
