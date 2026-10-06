//! The native `HostServices` implementation: identity namespace and built-in
//! resource provider.

use std::collections::VecDeque;

use ipp_core::HostRuntime;
use ipp_host_session::HostServices;

/// Identity namespace and built-in resource provider of the native Host.
pub struct NativeHostServices {
    pending: VecDeque<ipp_core::AssetAcquisitionRequest>,
    input: ipp_host_session::services::gui_input::GuiHostInputService,
}

impl HostServices for NativeHostServices {
    const NAME: &'static str = "server";

    fn initialize(
        host: &mut HostRuntime,
        _schedulers: &ipp_host_session::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos() as u64;
        let namespace = timestamp ^ (u64::from(std::process::id()) << 32);
        host.set_identity_namespace(namespace.max(1))?;
        host.register_stream_resource_provider("ipp")
            .map_err(|error| error.to_string())?;
        Ok(Self {
            pending: VecDeque::new(),
            input: Default::default(),
        })
    }

    fn gui_input(
        &mut self,
    ) -> Option<&mut ipp_host_session::services::gui_input::GuiHostInputService> {
        Some(&mut self.input)
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
            let result = ipp_host_session::builtin_resource(&request);
            ipp_host_session::deliver_resource(host, request.id, result);
        }
        Ok(())
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
    fn native_host_installs_only_its_builtin_provider() {
        let mut host = HostRuntime::new();
        let mut scheduler = ipp_host_session::services::task_scheduler::TaskSchedulerService::new();
        host.set_asset_load_scheduler(std::rc::Rc::new(scheduler.schedulers().host()));
        let mut platform =
            NativeHostServices::initialize(&mut host, &scheduler.schedulers()).unwrap();
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
        scheduler.poll_ready();
        host.progress_assets();
        let mut world = host.world_mut(id).unwrap();
        world.step(0.0).unwrap();
        let resources = world.resource_snapshots();
        assert_eq!(
            resources
                .iter()
                .find(|resource| *resource.source == *builtin)
                .unwrap()
                .status,
            AssetResourceStatus::Loaded
        );
        assert!(matches!(
            &resources
                .iter()
                .find(|resource| *resource.source == *unsupported)
                .unwrap()
                .status,
            AssetResourceStatus::Failed(error) if error.contains("https")
        ));
    }
}
