//! System selections and single-session helpers shared by the crate's unit-test suites.

use crate::{Host, HostServices, WorldSessionContext};

/// Camera outputs and the evaluators they require, for session tests.
pub(crate) const TEST_CAMERA_SYSTEMS: &[ipp_core::systems::SystemId] = &[
    ipp_core::systems::animation::AnimationSystem::ID,
    ipp_core::systems::asset_dependencies::AssetDependencySystem::ID,
    ipp_core::systems::hierarchy::HierarchySystem::ID,
    ipp_core::systems::look_at::LookAtSystem::ID,
    ipp_core::systems::hierarchy::FinalPropagationSystem::ID,
    ipp_core::systems::geometry::GeometrySystem::ID,
    ipp_core::systems::camera::CameraSystem::ID,
];

/// Rendered meshes and materials with the evaluators they require, for session tests.
pub(crate) const TEST_RENDER_SYSTEMS: &[ipp_core::systems::SystemId] = &[
    ipp_core::systems::animation::AnimationSystem::ID,
    ipp_core::systems::asset_dependencies::AssetDependencySystem::ID,
    ipp_core::systems::hierarchy::HierarchySystem::ID,
    ipp_core::systems::look_at::LookAtSystem::ID,
    ipp_core::systems::hierarchy::FinalPropagationSystem::ID,
    ipp_core::systems::geometry::GeometrySystem::ID,
    ipp_core::systems::render::RenderSystem::ID,
];

impl<P: HostServices> Host<P> {
    pub(crate) fn test_session(&mut self) -> WorldSessionContext<'_, P> {
        assert_eq!(self.sessions.len(), 1);
        let id = *self.sessions.keys().next().unwrap();
        self.session_mut(id).unwrap()
    }

    pub(crate) fn test_limits(&mut self, limits: ipp_core::WorldLimits) {
        assert_eq!(self.test_session().world().tick(), 0);
        assert!(self.test_session().world().entities().is_empty());
        let id = *self.sessions.keys().next().unwrap();
        let world = self.session_world(id).unwrap();
        let selected = self
            .runtime
            .world_manifest(world)
            .unwrap()
            .systems()
            .to_vec();
        self.close_session(id);
        self.open_session_with_limits(id, limits, &selected)
            .unwrap();
        self.test_session()
            .receive(&ipp_protocol::contract::HELLO)
            .unwrap();
        self.test_session().take_response().unwrap();
    }
}
