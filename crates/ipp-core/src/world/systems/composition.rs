//! Compiled factory registration and authoring capability validation.

use super::scheduler::SystemInstance;
use super::{SystemFactories, SystemFactory, SystemId, SystemInitError, SystemScheduleError};
use std::{any::Any, sync::Arc};

pub(in crate::world) fn validate_authoring_factories(
    factories: &SystemFactories,
) -> Result<(), SystemScheduleError> {
    let required = [
        super::lifecycle_publisher::LifecyclePublisherSystem::ID,
        super::animation::AnimationSystem::ID,
        super::asset_dependencies::AssetDependencySystem::ID,
        super::camera::CameraSystem::ID,
        super::constraints::ConstraintSystem::ID,
        super::geometry::GeometrySystem::ID,
        super::hierarchy::HierarchySystem::ID,
        super::look_at::LookAtSystem::ID,
        super::hierarchy::FinalPropagationSystem::ID,
        super::render::RenderSystem::ID,
        #[cfg(feature = "particles")]
        super::particles::ParticleSystem::ID,
        #[cfg(feature = "surfaces")]
        super::surface::SurfaceSystem::ID,
        #[cfg(feature = "gui")]
        super::gui::GuiSystem::ID,
        #[cfg(feature = "gui")]
        super::gui::GuiLayoutSystem::ID,
        #[cfg(feature = "gui")]
        super::gui::GuiInputSystem::ID,
        #[cfg(feature = "skeletal-animation")]
        super::skeleton::SkeletonSystem::ID,
        #[cfg(feature = "skeletal-animation")]
        super::skinning::SkinningSystem::ID,
        super::state_overlay::StateOverlaySystem::ID,
    ];
    for required in required {
        if !factories.ids().any(|id| id == required) {
            return Err(SystemScheduleError::MissingRequired {
                system: SystemId("ipp.world"),
                required,
            });
        }
    }
    Ok(())
}

pub(in crate::world) fn validate_authoring_instances(
    instances: &[SystemInstance],
) -> Result<(), SystemInitError> {
    for instance in instances {
        let any: &dyn Any = instance.system.as_ref();
        let valid = match instance.id {
            super::lifecycle_publisher::LifecyclePublisherSystem::ID => {
                any.is::<super::lifecycle_publisher::LifecyclePublisherSystem>()
            }
            super::animation::AnimationSystem::ID => any.is::<super::animation::AnimationSystem>(),
            super::asset_dependencies::AssetDependencySystem::ID => {
                any.is::<super::asset_dependencies::AssetDependencySystem>()
            }
            super::camera::CameraSystem::ID => any.is::<super::camera::CameraSystem>(),
            super::constraints::ConstraintSystem::ID => {
                any.is::<super::constraints::ConstraintSystem>()
            }
            super::hierarchy::HierarchySystem::ID => any.is::<super::hierarchy::HierarchySystem>(),
            super::look_at::LookAtSystem::ID => any.is::<super::look_at::LookAtSystem>(),
            super::hierarchy::FinalPropagationSystem::ID => {
                any.is::<super::hierarchy::FinalPropagationSystem>()
            }
            super::geometry::GeometrySystem::ID => any.is::<super::geometry::GeometrySystem>(),
            #[cfg(feature = "particles")]
            super::particles::ParticleSystem::ID => any.is::<super::particles::ParticleSystem>(),
            #[cfg(feature = "surfaces")]
            super::surface::SurfaceSystem::ID => any.is::<super::surface::SurfaceSystem>(),
            #[cfg(feature = "gui")]
            super::gui::GuiSystem::ID => any.is::<super::gui::GuiSystem>(),
            #[cfg(feature = "gui")]
            super::gui::GuiLayoutSystem::ID => any.is::<super::gui::GuiLayoutSystem>(),
            #[cfg(feature = "gui")]
            super::gui::GuiInputSystem::ID => any.is::<super::gui::GuiInputSystem>(),
            super::render::RenderSystem::ID => any.is::<super::render::RenderSystem>(),
            #[cfg(feature = "skeletal-animation")]
            super::skeleton::SkeletonSystem::ID => any.is::<super::skeleton::SkeletonSystem>(),
            #[cfg(feature = "skeletal-animation")]
            super::skinning::SkinningSystem::ID => any.is::<super::skinning::SkinningSystem>(),
            super::state_overlay::StateOverlaySystem::ID => {
                any.is::<super::state_overlay::StateOverlaySystem>()
            }
            _ => {
                #[allow(unused_mut)]
                let mut builtin = any.is::<super::lifecycle_publisher::LifecyclePublisherSystem>()
                    || any.is::<super::animation::AnimationSystem>()
                    || any.is::<super::asset_dependencies::AssetDependencySystem>()
                    || any.is::<super::camera::CameraSystem>()
                    || any.is::<super::constraints::ConstraintSystem>()
                    || any.is::<super::hierarchy::HierarchySystem>()
                    || any.is::<super::look_at::LookAtSystem>()
                    || any.is::<super::hierarchy::FinalPropagationSystem>()
                    || any.is::<super::geometry::GeometrySystem>()
                    || any.is::<super::render::RenderSystem>()
                    || any.is::<super::state_overlay::StateOverlaySystem>();
                #[cfg(feature = "skeletal-animation")]
                {
                    builtin |= any.is::<super::skeleton::SkeletonSystem>()
                        || any.is::<super::skinning::SkinningSystem>();
                }
                #[cfg(feature = "particles")]
                {
                    builtin |= any.is::<super::particles::ParticleSystem>();
                }
                #[cfg(feature = "surfaces")]
                {
                    builtin |= any.is::<super::surface::SurfaceSystem>();
                }
                #[cfg(feature = "gui")]
                {
                    builtin |= any.is::<super::gui::GuiSystem>()
                        || any.is::<super::gui::GuiLayoutSystem>()
                        || any.is::<super::gui::GuiInputSystem>();
                }
                !builtin
            }
        };
        if !valid {
            return Err(SystemInitError::AuthoringSystemType(instance.id));
        }
    }
    Ok(())
}

/// Factories for every authoring capability advertised by this build.
pub fn compiled_system_factories() -> Vec<Arc<dyn SystemFactory>> {
    vec![
        Arc::new(super::lifecycle_publisher::LifecyclePublisherSystemFactory),
        Arc::new(super::state_overlay::StateOverlaySystemFactory),
        Arc::new(super::animation::AnimationSystemFactory),
        Arc::new(super::constraints::ConstraintSystemFactory),
        Arc::new(super::asset_dependencies::AssetDependencySystemFactory),
        #[cfg(feature = "skeletal-animation")]
        Arc::new(super::skeleton::SkeletonSystemFactory),
        #[cfg(feature = "skeletal-animation")]
        Arc::new(super::skinning::SkinningSystemFactory),
        Arc::new(super::hierarchy::HierarchySystemFactory),
        Arc::new(super::look_at::LookAtSystemFactory),
        Arc::new(super::hierarchy::FinalPropagationSystemFactory),
        Arc::new(super::geometry::GeometrySystemFactory),
        Arc::new(super::camera::CameraSystemFactory),
        #[cfg(feature = "particles")]
        Arc::new(super::particles::ParticleSystemFactory),
        #[cfg(feature = "surfaces")]
        Arc::new(super::surface::SurfaceSystemFactory),
        #[cfg(feature = "gui")]
        Arc::new(super::gui::GuiSystemFactory),
        #[cfg(feature = "gui")]
        Arc::new(super::gui::GuiLayoutSystemFactory),
        #[cfg(feature = "gui")]
        Arc::new(super::gui::GuiInputSystemFactory),
        Arc::new(super::render::RenderSystemFactory),
    ]
}

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;
    use crate::systems::gui::{GuiInputSystem, GuiLayoutSystem, GuiSystem};

    #[test]
    fn authoring_requires_every_gui_system() {
        let cases: &[(SystemId, &[SystemId])] = &[
            (
                GuiSystem::ID,
                &[GuiSystem::ID, GuiLayoutSystem::ID, GuiInputSystem::ID],
            ),
            (
                GuiLayoutSystem::ID,
                &[GuiLayoutSystem::ID, GuiInputSystem::ID],
            ),
            (GuiInputSystem::ID, &[GuiInputSystem::ID]),
        ];

        for &(missing, removed) in cases {
            let factories = compiled_system_factories()
                .into_iter()
                .filter(|factory| !removed.contains(&factory.id()))
                .collect();
            let factories = SystemFactories::new(factories).expect("remaining graph is valid");
            assert_eq!(
                validate_authoring_factories(&factories),
                Err(SystemScheduleError::MissingRequired {
                    system: SystemId("ipp.world"),
                    required: missing,
                })
            );
        }
    }
}
