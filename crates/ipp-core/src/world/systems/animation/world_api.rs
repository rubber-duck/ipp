//! Typed animation operations dispatch into the selected animation system.

use super::*;
use crate::WorldContext;

#[derive(Clone)]
pub(in crate::world) enum AnimationCommand {
    Controller {
        request_id: u64,
        command: AnimationControllerCommand,
    },
    Playback {
        id: AnimationControllerId,
        control: AnimationPlaybackControl,
    },
}

impl WorldContext<'_> {
    fn with_animation<R>(
        &mut self,
        operation: impl FnOnce(&mut AnimationAccess<'_, '_>) -> Result<R, ErrorReason>,
    ) -> Result<R, ErrorReason> {
        if !self
            .world
            .manifest
            .supports_operation(crate::systems::WorldOperation::Animation)
        {
            return Err(ErrorReason::UnsupportedDependency);
        }
        self.with_system::<AnimationSystem, _>(AnimationSystem::ID, |system, context| {
            operation(&mut AnimationAccess {
                system,
                context,
            })
        })
        .ok_or(ErrorReason::UnsupportedDependency)?
    }

    fn animation_system(&self) -> Option<&AnimationSystem> {
        self.system::<AnimationSystem>(AnimationSystem::ID)
    }

    /// Queue a correlated controller command in world mutation order.
    pub fn enqueue_animation_controller(
        &mut self,
        request_id: u64,
        command: AnimationControllerCommand,
    ) -> Result<(), ErrorReason> {
        let description = match &command {
            AnimationControllerCommand::Create(description)
            | AnimationControllerCommand::Update {
                description,
                ..
            } => Some(description),
            AnimationControllerCommand::Transition {
                transition,
                ..
            } => Some(&transition.description),
            _ => None,
        };
        if description.is_some_and(|description| {
            super::update::description_bytes(description) > self.world.limits.max_batch_bytes
        }) {
            return Err(ErrorReason::Capacity);
        }
        if let Some(description) = description {
            for driver in &description.drivers {
                super::binding::validate_target_support(&self.world.manifest, &driver.property)?;
            }
        }
        self.enqueue_system_command(
            AnimationSystem::ID,
            0,
            AnimationCommand::Controller {
                request_id,
                command,
            },
        )
    }

    /// Queue a controller clock operation.
    pub fn enqueue_playback(
        &mut self,
        id: AnimationControllerId,
        control: AnimationPlaybackControl,
    ) -> Result<(), ErrorReason> {
        super::update::validate_control(control)?;
        self.enqueue_system_command(
            AnimationSystem::ID,
            0,
            AnimationCommand::Playback {
                id,
                control,
            },
        )
    }

    /// Create a stopped controller directly at this exclusive mutation boundary.
    pub fn create_animation_controller(
        &mut self,
        description: AnimationControllerDescription,
    ) -> Result<AnimationControllerId, ErrorReason> {
        self.with_animation(|animation| animation.create_animation_controller(description))
    }

    /// Replace controller descriptions and parameters atomically.
    pub fn update_animation_controller(
        &mut self,
        id: AnimationControllerId,
        description: AnimationControllerDescription,
    ) -> Result<(), ErrorReason> {
        self.with_animation(|animation| animation.update_animation_controller(id, description))
    }

    /// Crossfade to a replacement controller description.
    pub fn transition_animation_controller(
        &mut self,
        id: AnimationControllerId,
        transition: AnimationControllerTransition,
    ) -> Result<(), ErrorReason> {
        self.with_animation(|animation| animation.transition_animation_controller(id, transition))
    }

    /// Restore a controller's targets and delete it.
    pub fn remove_animation_controller(
        &mut self,
        id: AnimationControllerId,
    ) -> Result<(), ErrorReason> {
        self.with_animation(|animation| animation.remove_animation_controller(id))
    }

    /// Apply a clock control directly at this exclusive mutation boundary.
    pub fn control_animation_controller(
        &mut self,
        id: AnimationControllerId,
        control: AnimationPlaybackControl,
    ) -> Result<(), ErrorReason> {
        self.with_animation(|animation| animation.control_playback(id, control))
    }

    /// Inspect every controller in identity order.
    pub fn animation_controller_page(
        &self,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<AnimationControllerSnapshot> {
        self.animation_system()
            .map(|system| system.ordinary_controller_page(after, target, limit))
            .unwrap_or_default()
    }

    /// Inspect all controllers in identity order.
    pub fn animation_controllers(&self) -> Vec<AnimationControllerSnapshot> {
        self.animation_system()
            .map(AnimationSystem::ordinary_controllers)
            .unwrap_or_default()
    }

    /// Inspect one controller's descriptions and clock.
    pub fn animation_controller(
        &self,
        id: AnimationControllerId,
    ) -> Option<AnimationControllerSnapshot> {
        self.animation_system()?.ordinary_controller(id)
    }

    /// Snapshot controller identities and frozen clocks.
    pub fn animation_persistent_state(&self) -> AnimationPersistentState {
        self.animation_system()
            .map(AnimationSystem::persistent_state)
            .unwrap_or_default()
    }

    /// Rebuild persistent controller descriptions and bindings in this World.
    pub fn restore_animation_controllers(
        &mut self,
        persistent: AnimationPersistentState,
    ) -> Result<(), ErrorReason> {
        self.with_animation(|animation| animation.restore_animation_controllers(persistent))
    }
}
