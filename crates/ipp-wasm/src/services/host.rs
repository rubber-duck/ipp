//! Browser adapters for the shared host session policy.

#[cfg(all(feature = "render", target_arch = "wasm32"))]
use super::render;

use std::collections::VecDeque;

use ipp_core::{HostRuntime, WorldContext};
use ipp_host_session::HostServices;

/// The single World a browser Host presents through its graphics context.
///
/// Presenting another World ends the previous World's presentation. This Host never
/// draws a World it does not present, so the previous World's renderer caches are
/// released rather than holding retained batches and glyph atlas demand until unload.
#[cfg(any(test, all(feature = "render", target_arch = "wasm32")))]
#[derive(Default)]
struct WasmPresentationTarget {
    world: Option<ipp_core::WorldId>,
}

#[cfg(any(test, all(feature = "render", target_arch = "wasm32")))]
impl WasmPresentationTarget {
    /// Present `world`, returning a different previously presented World to forget.
    fn attach(&mut self, world: ipp_core::WorldId) -> Option<ipp_core::WorldId> {
        self.world
            .replace(world)
            .filter(|previous| *previous != world)
    }

    /// Stop presenting `world`; returns whether it was the presented World.
    fn detach(&mut self, world: ipp_core::WorldId) -> bool {
        let presented = self.presents(world);
        if presented {
            self.world = None;
        }
        presented
    }

    fn presents(&self, world: ipp_core::WorldId) -> bool {
        self.world == Some(world)
    }
}

pub(crate) struct WasmHostServices {
    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    presentation_world: WasmPresentationTarget,
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
            presentation_world: WasmPresentationTarget::default(),
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
            if let Some(previous) = self.presentation_world.attach(_world) {
                self.presentation.forget_world(previous);
            }
            self.presentation.reset_world();
        }
        Ok(())
    }

    fn detach_world(&mut self, _world: ipp_core::WorldId) {
        #[cfg(all(feature = "render", target_arch = "wasm32"))]
        if self.presentation_world.detach(_world) {
            self.presentation.forget_world(_world);
        }
    }

    fn present(
        &mut self,
        _world: &mut WorldContext<'_>,
    ) -> Result<(), ipp_host_session::HostPresentationFailure> {
        #[cfg(all(feature = "render", target_arch = "wasm32"))]
        if self.presentation_world.presents(_world.id()) {
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

    #[test]
    fn presenting_another_world_forgets_only_the_previous_presentation() {
        let [first, second] = [ipp_core::WorldId(1), ipp_core::WorldId(2)];
        let mut target = WasmPresentationTarget::default();

        assert_eq!(target.attach(first), None);
        assert_eq!(
            target.attach(first),
            None,
            "another session on the presented World keeps its caches"
        );
        assert_eq!(target.attach(second), Some(first));
        assert!(target.presents(second) && !target.presents(first));
        assert!(
            !target.detach(first),
            "the replaced World was already forgotten when it lost presentation"
        );

        assert_eq!(
            target.attach(first),
            Some(second),
            "re-attaching presents the first World again from released caches"
        );
        assert!(target.detach(first));
        assert!(!target.presents(first));
        assert!(!target.detach(first));
        assert_eq!(target.attach(second), None);
    }
}
