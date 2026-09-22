use crate::{CameraMotion, ComponentValue, EntityId, ErrorReason, PreparedCamera};

pub(in crate::world) enum CameraCommand {
    Activate(EntityId),
    Navigate(CameraMotion),
}

impl super::CameraSystem {
    pub(in crate::world) fn activate(
        &mut self,
        world: &crate::world::WorldSimulationState,
        entity: EntityId,
    ) -> Result<(), ErrorReason> {
        if !world.state.allocator.contains(entity) {
            return Err(ErrorReason::InvalidEntity);
        }
        let index = entity.index() as usize;
        super::prepare(
            entity,
            world
                .components
                .camera(index)
                .ok_or(ErrorReason::MissingComponent)?,
            world
                .components
                .transform(index)
                .ok_or(ErrorReason::MissingComponent)?,
            1,
            1,
        )?;
        if self.state.active_camera != Some(entity) {
            self.state.active_camera = Some(entity);
            self.state.state_changes.push(super::CameraStatePatch {
                active_camera: Some(entity),
            });
            crate::diagnostic!(
                Debug,
                "[IPP core] camera.activate entity={}",
                entity.to_bits()
            );
        }
        Ok(())
    }

    pub(in crate::world) fn navigate_active(
        &mut self,
        context: &mut crate::systems::SystemRuntimeAccess<'_>,
        motion: CameraMotion,
    ) -> Result<(), ErrorReason> {
        let entity = self
            .state
            .active_camera
            .ok_or(ErrorReason::NoActiveCamera)?;
        let ComponentValue::Camera(camera) = context
            .world
            .state
            .producer_value(&context.world.components, entity, ComponentValue::CAMERA)
            .ok_or(ErrorReason::MissingComponent)?
        else {
            unreachable!("camera component")
        };
        let ComponentValue::Transform(transform) = context
            .world
            .state
            .producer_value(&context.world.components, entity, ComponentValue::TRANSFORM)
            .ok_or(ErrorReason::MissingComponent)?
        else {
            unreachable!("transform component")
        };
        let (next_camera, next_transform) = super::navigate(entity, camera, transform, motion)?;
        let mut commands = Vec::new();
        for (previous, next) in [
            (
                ComponentValue::Camera(camera),
                ComponentValue::Camera(next_camera),
            ),
            (
                ComponentValue::Transform(transform),
                ComponentValue::Transform(next_transform),
            ),
        ] {
            for ((_, previous), (offset, value)) in previous.fields().into_iter().zip(next.fields())
            {
                if value == previous {
                    continue;
                }
                let crate::components::schema::FieldValue::F32(value) = value else {
                    unreachable!("navigation writes numeric fields")
                };
                commands.push(crate::Command::SetField {
                    entity: crate::EntityRef::Handle(entity),
                    component: next.type_id(),
                    field: crate::FieldWrite {
                        offset,
                        value: crate::FieldValue::F32(value),
                    },
                });
            }
        }
        context.apply_authored_commands(Some(self), &commands)?;
        if !commands.is_empty() {
            crate::diagnostic!(
                Debug,
                "[IPP core] camera.navigate entity={} motion={motion:?}",
                entity.to_bits()
            );
        }
        Ok(())
    }
}

pub(in crate::world) struct CameraReadAccess<'a> {
    world: &'a crate::world::WorldSimulationState,
    state: &'a super::CameraSystemState,
}

impl<'a> CameraReadAccess<'a> {
    pub(in crate::world) fn new(
        world: &'a crate::world::WorldSimulationState,
        state: &'a super::CameraSystemState,
    ) -> Self {
        Self {
            world,
            state,
        }
    }

    pub(in crate::world) fn render_viewport(&self) -> Option<(u32, u32)> {
        self.state.render_viewport
    }

    /// Observe the explicit selection; a new world has none.
    pub fn active_camera(&self) -> Option<EntityId> {
        self.state.active_camera
    }

    /// Borrow the selected camera's final effective component without a snapshot.
    pub fn active_camera_component(&self) -> Option<&'a super::Camera> {
        let entity = self.active_camera()?;
        if !self.world.state.allocator.contains(entity) {
            return None;
        }
        self.world.components.camera(entity.index() as usize)
    }

    /// Prepare the selected effective camera for rendering and matching queries.
    pub fn prepare_camera(&self, width: u32, height: u32) -> Result<PreparedCamera, ErrorReason> {
        let entity = self.active_camera().ok_or(ErrorReason::NoActiveCamera)?;
        let index = entity.index() as usize;
        self.world
            .components
            .transform(index)
            .ok_or(ErrorReason::MissingComponent)?;
        super::prepare_affine(
            entity,
            self.world
                .components
                .camera(index)
                .ok_or(ErrorReason::MissingComponent)?,
            &crate::systems::hierarchy::evaluated_affine(self.world, entity)?,
            width,
            height,
        )
    }
}

impl crate::WorldContext<'_> {
    /// Queue an explicit camera selection in the same ordered stream as batches.
    pub fn enqueue_camera_activate(&mut self, entity: EntityId) -> Result<(), ErrorReason> {
        self.enqueue_system_command(super::CameraSystem::ID, 0, CameraCommand::Activate(entity))
    }

    /// Queue navigation alongside scene batches and explicit selection.
    pub fn enqueue_camera_navigate(&mut self, motion: CameraMotion) -> Result<(), ErrorReason> {
        self.enqueue_system_command(super::CameraSystem::ID, 0, CameraCommand::Navigate(motion))
    }

    /// Observe the explicit selection; a new world has none.
    pub fn active_camera(&self) -> Option<EntityId> {
        self.system::<super::CameraSystem>(super::CameraSystem::ID)
            .expect("compiled camera system")
            .read(self.world)
            .active_camera()
    }

    /// Borrow the selected camera's final effective component without a snapshot.
    pub fn active_camera_component(&self) -> Option<&super::Camera> {
        self.system::<super::CameraSystem>(super::CameraSystem::ID)
            .expect("compiled camera system")
            .read(self.world)
            .active_camera_component()
    }

    /// Prepare the selected effective camera for rendering and matching queries.
    pub fn prepare_camera(&self, width: u32, height: u32) -> Result<PreparedCamera, ErrorReason> {
        self.system::<super::CameraSystem>(super::CameraSystem::ID)
            .expect("compiled camera system")
            .read(self.world)
            .prepare_camera(width, height)
    }
}

impl crate::WorldContext<'_> {
    /// Read the Host surface dimensions recorded for camera queries and rendering.
    pub fn render_viewport(&self) -> Option<(u32, u32)> {
        self.system::<super::CameraSystem>(super::CameraSystem::ID)
            .expect("compiled camera system")
            .read(self.world)
            .render_viewport()
    }

    /// Record the Host surface dimensions used to validate subsequent camera queries.
    pub fn set_render_viewport(&mut self, viewport: Option<(u32, u32)>) {
        self.with_system::<super::CameraSystem, _>(super::CameraSystem::ID, |system, _| {
            system.state.render_viewport = viewport;
        });
    }
}
