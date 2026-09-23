//! Public compatibility adapters dispatch into the selected animation system.

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
    #[cfg(feature = "gui")]
    Internal(Vec<AnimationInternalCommand>),
}

/// Renderer-owned controller lifecycle applied through AnimationSystem at the
/// next ordinary mutation boundary. These commands intentionally produce no
/// client-correlated outcomes.
#[cfg(feature = "gui")]
#[derive(Clone)]
pub(in crate::world) enum AnimationInternalCommand {
    EnsureSkinTransition {
        owner: GuiSkinAnimationOwner,
        request: u64,
        source: AnimationControllerDescription,
        source_time: f64,
        source_sample: GuiSkinAnimationSample,
        transition: AnimationControllerTransition,
        destination_sample: GuiSkinAnimationSample,
    },
    DeleteSkin(GuiSkinAnimationOwner),
}

/// Full GUI identity owned by one derived skin controller.
#[cfg(feature = "gui")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::world) struct GuiSkinAnimationOwner {
    pub entity: crate::EntityId,
    pub primitive: crate::systems::surface::GuiPrimitiveId,
}

/// Authored numeric appearance expected at one skin motion sample.
#[cfg(feature = "gui")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::world) struct GuiSkinAnimationSample {
    pub color: [f32; 4],
    pub opacity: f32,
    pub scale: [f32; 2],
    /// Present exactly when the motion also drives the `align_x` lane.
    pub align_x: Option<f32>,
}

impl WorldContext<'_> {
    fn with_animation<R>(
        &mut self,
        operation: impl FnOnce(&mut AnimationAccess<'_, '_>) -> R,
    ) -> R {
        self.with_system::<AnimationSystem, _>(AnimationSystem::ID, |system, context| {
            operation(&mut AnimationAccess {
                system,
                context,
            })
        })
        .expect("validated animation system")
    }

    fn animation_system(&self) -> &AnimationSystem {
        self.system::<AnimationSystem>(AnimationSystem::ID)
            .expect("validated animation system")
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
        self.with_animation(|animation| {
            animation.update_ordinary_animation_controller(id, description)
        })
    }

    /// Crossfade to a replacement controller description.
    pub fn transition_animation_controller(
        &mut self,
        id: AnimationControllerId,
        transition: AnimationControllerTransition,
    ) -> Result<(), ErrorReason> {
        self.with_animation(|animation| {
            animation.transition_ordinary_animation_controller(id, transition)
        })
    }

    /// Restore a controller's targets and delete it.
    pub fn remove_animation_controller(
        &mut self,
        id: AnimationControllerId,
    ) -> Result<(), ErrorReason> {
        self.with_animation(|animation| animation.remove_ordinary_animation_controller(id))
    }

    /// Apply a clock control directly at this exclusive mutation boundary.
    pub fn control_animation_controller(
        &mut self,
        id: AnimationControllerId,
        control: AnimationPlaybackControl,
    ) -> Result<(), ErrorReason> {
        self.with_animation(|animation| animation.control_ordinary_playback(id, control))
    }

    /// Inspect every controller in identity order.
    pub fn animation_controller_page(
        &self,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<AnimationControllerSnapshot> {
        self.animation_system()
            .ordinary_controller_page(after, target, limit)
    }

    /// Inspect all controllers in identity order.
    pub fn animation_controllers(&self) -> Vec<AnimationControllerSnapshot> {
        self.animation_system().ordinary_controllers()
    }

    /// Inspect one controller's descriptions and clock.
    pub fn animation_controller(
        &self,
        id: AnimationControllerId,
    ) -> Option<AnimationControllerSnapshot> {
        self.animation_system().ordinary_controller(id)
    }

    /// Snapshot controller identities and frozen clocks.
    pub fn animation_persistent_state(&self) -> AnimationPersistentState {
        self.system::<AnimationSystem>(AnimationSystem::ID)
            .expect("validated animation system")
            .persistent_state()
    }

    /// Rebuild persistent controller descriptions and bindings in this World.
    pub fn restore_animation_controllers(
        &mut self,
        persistent: AnimationPersistentState,
    ) -> Result<(), ErrorReason> {
        self.with_animation(|animation| animation.restore_animation_controllers(persistent))
    }
}
