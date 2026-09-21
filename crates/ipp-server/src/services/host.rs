//! Native adapter for the shared host session policy.

use std::collections::VecDeque;

use ipp_core::HostRuntime;
use ipp_host_session::HostServices;

pub(crate) struct NativeHostServices {
    pending: VecDeque<ipp_core::AssetAcquisitionRequest>,
}

impl HostServices for NativeHostServices {
    const NAME: &'static str = "server";

    fn initialize(_host: &mut HostRuntime) -> Result<Self, String> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos() as u64;
        let namespace = timestamp ^ (u64::from(std::process::id()) << 32);
        _host.set_identity_namespace(namespace.max(1))?;
        #[cfg(feature = "builtin-assets")]
        _host
            .register_stream_resource_provider("ipp")
            .map_err(|error| error.to_string())?;
        Ok(Self {
            pending: VecDeque::new(),
        })
    }

    fn service_resources(&mut self, host: &mut HostRuntime) -> Result<(), String> {
        for id in host.take_resource_cancellations() {
            self.pending.retain(|request| request.id != id);
        }
        self.pending.extend(host.take_resource_requests());
        for _ in 0..8 {
            let Some(request) = self.pending.pop_front() else {
                break;
            };
            #[cfg(feature = "builtin-assets")]
            let result = ipp_host_session::builtin_resource(&request);
            #[cfg(not(feature = "builtin-assets"))]
            let result = Err("resource provider is unavailable in this native host build".into());
            ipp_host_session::deliver_resource(host, request.id, result);
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "builtin-assets"))]
mod tests {
    use super::*;
    use ipp_core::{
        AssetResourceStatus, Batch, Command, ComponentValue, EntityRef, FieldValue, FieldWrite,
        components::MeshInstance,
    };
    use std::mem::offset_of;

    #[test]
    fn native_host_installs_only_its_builtin_provider() {
        let mut host = HostRuntime::new();
        let mut platform = NativeHostServices::initialize(&mut host).unwrap();
        let id = host.create_world(Default::default()).unwrap();
        let mut world = host.world_mut(id).unwrap();
        let builtin = "ipp://mesh/cube?width=1&height=1&length=1";
        let unsupported = "https://example.test/mesh.ippm";
        world
            .enqueue(Batch {
                id: 1,
                operations: [builtin, unsupported]
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
        let mut world = host.world_mut(id).unwrap();
        world.step(0.0).unwrap();
        let resources = world.resource_snapshots();
        assert_eq!(
            resources
                .iter()
                .find(|resource| resource.source == builtin)
                .unwrap()
                .status,
            AssetResourceStatus::Loaded
        );
        assert!(matches!(
            &resources
                .iter()
                .find(|resource| resource.source == unsupported)
                .unwrap()
                .status,
            AssetResourceStatus::Failed(error) if error.contains("https")
        ));
    }
}
