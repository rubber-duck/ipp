//! Camera-output fixtures shared by Host unit tests.

use super::{HostRuntime, OutputKind, OutputRef};
use crate::{Batch, Command, ComponentValue, EntityRef, components::Camera, systems::*};

/// Camera outputs and the evaluators they require.
pub(super) const CAMERA_SYSTEMS: &[SystemId] = &[
    animation::AnimationSystem::ID,
    asset_dependencies::AssetDependencySystem::ID,
    hierarchy::HierarchySystem::ID,
    look_at::LookAtSystem::ID,
    hierarchy::FinalPropagationSystem::ID,
    geometry::GeometrySystem::ID,
    camera::CameraSystem::ID,
];

pub(super) fn camera(host: &mut HostRuntime) -> OutputRef {
    camera_in(host, CAMERA_SYSTEMS)
}

pub(super) fn camera_in(host: &mut HostRuntime, systems: &[SystemId]) -> OutputRef {
    let world = host.create_world(Default::default(), systems).unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::Camera(Camera::default()),
                ),
            ],
        })
        .unwrap();

    let entity = host
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()[0]
        .1;
    let world = host.world_ref(world).unwrap();
    host.bind_output(world, entity, OutputKind::Camera).unwrap()
}
