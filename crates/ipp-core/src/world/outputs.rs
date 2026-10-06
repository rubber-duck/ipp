//! Scoped Host identity, output lifetime and current frame access.

use crate::world::systems;
use crate::world::{World, WorldContext, WorldEntityState};
use crate::{EntityId, ErrorReason};

/// Which outputs a World's selected Systems supply: its canvas, and the
/// component whose entities are Camera outputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorldOutputs {
    canvas: Result<(), ErrorReason>,
    camera: Result<u16, ErrorReason>,
}

impl WorldOutputs {
    pub(in crate::world) fn of<'a>(
        instances: impl Iterator<Item = &'a systems::SystemInstance> + Clone,
    ) -> Self {
        let canvas = instances
            .clone()
            .filter(|instance| instance.system.world_output(crate::OutputKind::Canvas))
            .map(|_| ());
        let camera = instances
            .filter_map(|instance| instance.system.output_component(crate::OutputKind::Camera));
        Self {
            canvas: single(canvas),
            camera: single(camera),
        }
    }

    /// Whether `output` names a current output of the World holding `state`.
    pub(crate) fn matches(
        self,
        world: crate::WorldRef,
        state: &WorldEntityState,
        output: crate::OutputRef,
    ) -> bool {
        self.bind(world, state, output.target) == Ok(output)
    }

    /// The current output a target selects; a Camera target must name the
    /// current Camera component lifetime.
    pub(crate) fn bind(
        self,
        world: crate::WorldRef,
        state: &WorldEntityState,
        target: crate::OutputTarget,
    ) -> Result<crate::OutputRef, ErrorReason> {
        match target {
            crate::OutputTarget::Canvas => {
                self.canvas?;
                Ok(crate::OutputRef::canvas(world))
            }
            crate::OutputTarget::Camera {
                entity,
                ..
            } => {
                let output = self.bind_camera(world, state, entity)?;
                if output.target != target {
                    return Err(ErrorReason::InvalidEntity);
                }
                Ok(output)
            }
        }
    }

    /// The current Camera output of `entity`.
    pub(crate) fn bind_camera(
        self,
        world: crate::WorldRef,
        state: &WorldEntityState,
        entity: EntityId,
    ) -> Result<crate::OutputRef, ErrorReason> {
        let component = self.camera?;
        let incarnation = state
            .entities
            .get(&entity)
            .and_then(|record| record.input(component))
            .ok_or(ErrorReason::MissingComponent)?
            .incarnation;
        Ok(crate::OutputRef {
            world,
            target: crate::OutputTarget::Camera {
                entity,
                incarnation,
            },
        })
    }
}

/// Exactly one supplier; none is unsupported and several are ambiguous.
fn single<T>(mut suppliers: impl Iterator<Item = T>) -> Result<T, ErrorReason> {
    let supplier = suppliers.next().ok_or(ErrorReason::UnsupportedDependency)?;
    if suppliers.next().is_some() {
        return Err(ErrorReason::InvalidValue);
    }
    Ok(supplier)
}

impl World {
    pub(crate) fn runtime_ref(&self) -> crate::WorldRef {
        crate::WorldRef {
            id: self.data.id,
            incarnation: self.data.identity,
        }
    }

    pub(crate) fn outputs(&self) -> WorldOutputs {
        WorldOutputs::of(self.schedule.instances.iter())
    }

    pub(crate) fn output_valid(&self, output: crate::OutputRef) -> bool {
        output.world == self.runtime_ref()
            && self
                .outputs()
                .matches(self.runtime_ref(), &self.data.state, output)
    }

    /// Bind the current output a target selects in this World.
    pub(crate) fn bind_output_target(
        &self,
        target: crate::OutputTarget,
    ) -> Result<crate::OutputRef, ErrorReason> {
        self.outputs()
            .bind(self.runtime_ref(), &self.data.state, target)
    }

    /// Bind the current Camera output of `entity`; the canvas names no entity.
    pub(crate) fn bind_output(
        &self,
        entity: EntityId,
        kind: crate::OutputKind,
    ) -> Result<crate::OutputRef, ErrorReason> {
        if kind != crate::OutputKind::Camera {
            return Err(ErrorReason::InvalidValue);
        }
        self.outputs()
            .bind_camera(self.runtime_ref(), &self.data.state, entity)
    }
}

impl WorldContext<'_> {
    pub(crate) fn attachment_placement(
        &self,
        anchor: EntityId,
    ) -> Result<crate::AttachmentPlacement, ErrorReason> {
        if self.world.state.links.invalid.contains(&anchor) {
            return Err(ErrorReason::UnsupportedDependency);
        }
        let mut selected = crate::AttachmentPlacement::Unmanaged;
        for instance in self
            .instances
            .before
            .iter()
            .chain(self.instances.after.iter())
        {
            let claim = instance.system.attachment_placement(self, anchor);
            let owner = match claim {
                crate::AttachmentPlacement::Unmanaged => continue,
                crate::AttachmentPlacement::Unavailable {
                    owner,
                }
                | crate::AttachmentPlacement::Ready {
                    owner,
                    ..
                } => owner,
            };
            if selected != crate::AttachmentPlacement::Unmanaged {
                return Err(ErrorReason::InvalidValue);
            }
            if !self
                .outputs()
                .matches(self.world_ref(), &self.world.state, owner)
            {
                return Err(ErrorReason::InvalidEntity);
            }
            if let crate::AttachmentPlacement::Ready {
                affine,
                ..
            } = claim
            {
                systems::geometry::GeometryShapeTransform::new(affine)?;
            }
            selected = claim;
        }
        Ok(selected)
    }

    pub(crate) fn completed_attachments(&self) -> Vec<crate::host::PublishedWorldAttachment> {
        let mut output = Vec::new();
        for instance in self
            .instances
            .before
            .iter()
            .chain(self.instances.after.iter())
        {
            instance.system.completed_attachments(self, &mut output);
        }
        output
    }

    pub(crate) fn publish_output(
        &self,
        output: &mut crate::host::WorldOutputBuilder<'_>,
    ) -> Result<(), ErrorReason> {
        for instance in self
            .instances
            .before
            .iter()
            .chain(self.instances.after.iter())
        {
            instance.system.publish_output(self, output)?;
        }
        Ok(())
    }

    pub(crate) fn outputs(&self) -> WorldOutputs {
        WorldOutputs::of(
            self.instances
                .before
                .iter()
                .chain(self.instances.after.iter()),
        )
    }

    /// Bind the current Camera output of `entity`; the canvas names no entity
    /// and is selected with [`crate::OutputRef::canvas`].
    pub fn bind_output(
        &self,
        entity: EntityId,
        kind: crate::OutputKind,
    ) -> Result<crate::OutputRef, ErrorReason> {
        if kind != crate::OutputKind::Camera {
            return Err(ErrorReason::InvalidValue);
        }
        self.outputs()
            .bind_camera(self.world_ref(), &self.world.state, entity)
    }

    /// Bind the current output a target selects in this World.
    pub(crate) fn bind_output_target(
        &self,
        target: crate::OutputTarget,
    ) -> Result<crate::OutputRef, ErrorReason> {
        self.outputs()
            .bind(self.world_ref(), &self.world.state, target)
    }

    /// Exact World lifetime for Host graph declarations.
    pub fn world_ref(&self) -> crate::WorldRef {
        crate::WorldRef {
            id: self.world.id,
            incarnation: self.world.identity,
        }
    }

    /// Scoped current Host inputs; absent outside a composed evaluation phase.
    pub fn frame_context(&self) -> Option<&crate::WorldFrameContext> {
        self.frame_context
    }

    /// The World canvas output, for the System that supplies it.
    pub(crate) fn canvas_output(&self) -> crate::OutputRef {
        crate::OutputRef::canvas(self.world_ref())
    }

    /// The Camera output of `entity`, for the System that supplies it.
    pub(crate) fn camera_output(
        &self,
        entity: EntityId,
        component: u16,
    ) -> Result<crate::OutputRef, ErrorReason> {
        let incarnation = self
            .world
            .state
            .entities
            .get(&entity)
            .and_then(|record| record.input(component))
            .ok_or(ErrorReason::MissingComponent)?
            .incarnation;
        Ok(crate::OutputRef {
            world: self.world_ref(),
            target: crate::OutputTarget::Camera {
                entity,
                incarnation,
            },
        })
    }
}

impl systems::SystemRuntimeAccess<'_> {
    /// Current parent-derived frame inputs, never a preceding completed scene.
    pub fn frame_context(&self) -> Option<&crate::WorldFrameContext> {
        self.frame_context
    }
}

#[cfg(test)]
#[path = "outputs_tests.rs"]
mod tests;
