//! Compiled built-in factory registration and authoring implementation checks.

use super::schedule::SystemInstance;
use super::{SystemFactory, SystemInitError};
use crate::systems::{
    animation, asset_dependencies, camera, canvas, constraints, data_bindings, geometry, gui,
    hierarchy, lifecycle_publisher, look_at, particles, plot, render, skeleton, skinning, surface,
    world_attachment,
};
use std::{any::Any, sync::Arc};

pub(in crate::world) fn validate_authoring_instances(
    instances: &[SystemInstance],
) -> Result<(), SystemInitError> {
    for instance in instances {
        let any: &dyn Any = instance.system.as_ref();
        let valid = match instance.id {
            lifecycle_publisher::LifecyclePublisherSystem::ID => {
                any.is::<lifecycle_publisher::LifecyclePublisherSystem>()
            }
            animation::AnimationSystem::ID => any.is::<animation::AnimationSystem>(),
            asset_dependencies::AssetDependencySystem::ID => {
                any.is::<asset_dependencies::AssetDependencySystem>()
            }
            camera::CameraSystem::ID => any.is::<camera::CameraSystem>(),
            constraints::ConstraintSystem::ID => any.is::<constraints::ConstraintSystem>(),
            hierarchy::HierarchySystem::ID => any.is::<hierarchy::HierarchySystem>(),
            look_at::LookAtSystem::ID => any.is::<look_at::LookAtSystem>(),
            hierarchy::FinalPropagationSystem::ID => any.is::<hierarchy::FinalPropagationSystem>(),
            geometry::GeometrySystem::ID => any.is::<geometry::GeometrySystem>(),
            plot::PlotSystem::ID => any.is::<plot::PlotSystem>(),
            data_bindings::DataBindingSystem::ID => any.is::<data_bindings::DataBindingSystem>(),
            particles::ParticleSystem::ID => any.is::<particles::ParticleSystem>(),
            surface::SurfaceSystem::ID => any.is::<surface::SurfaceSystem>(),
            canvas::CanvasSystem::ID => any.is::<canvas::CanvasSystem>(),
            gui::GuiSystem::ID => any.is::<gui::GuiSystem>(),
            gui::GuiLayoutSystem::ID => any.is::<gui::GuiLayoutSystem>(),
            render::RenderSystem::ID => any.is::<render::RenderSystem>(),
            skeleton::SkeletonSystem::ID => any.is::<skeleton::SkeletonSystem>(),
            skinning::SkinningSystem::ID => any.is::<skinning::SkinningSystem>(),
            _ => {
                let builtin = any.is::<lifecycle_publisher::LifecyclePublisherSystem>()
                    || any.is::<animation::AnimationSystem>()
                    || any.is::<asset_dependencies::AssetDependencySystem>()
                    || any.is::<camera::CameraSystem>()
                    || any.is::<constraints::ConstraintSystem>()
                    || any.is::<hierarchy::HierarchySystem>()
                    || any.is::<look_at::LookAtSystem>()
                    || any.is::<hierarchy::FinalPropagationSystem>()
                    || any.is::<geometry::GeometrySystem>()
                    || any.is::<render::RenderSystem>()
                    || any.is::<skeleton::SkeletonSystem>()
                    || any.is::<skinning::SkinningSystem>()
                    || any.is::<data_bindings::DataBindingSystem>()
                    || any.is::<plot::PlotSystem>()
                    || any.is::<particles::ParticleSystem>()
                    || any.is::<surface::SurfaceSystem>()
                    || any.is::<canvas::CanvasSystem>()
                    || any.is::<gui::GuiSystem>()
                    || any.is::<gui::GuiLayoutSystem>();
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
        Arc::new(world_attachment::WorldAttachmentSystemFactory),
        Arc::new(lifecycle_publisher::LifecyclePublisherSystemFactory),
        Arc::new(animation::AnimationSystemFactory),
        Arc::new(constraints::ConstraintSystemFactory),
        Arc::new(asset_dependencies::AssetDependencySystemFactory),
        Arc::new(skeleton::SkeletonSystemFactory),
        Arc::new(skinning::SkinningSystemFactory),
        Arc::new(hierarchy::HierarchySystemFactory),
        Arc::new(look_at::LookAtSystemFactory),
        Arc::new(hierarchy::FinalPropagationSystemFactory),
        Arc::new(geometry::GeometrySystemFactory),
        Arc::new(camera::CameraSystemFactory),
        Arc::new(data_bindings::DataBindingSystemFactory),
        Arc::new(plot::PlotSystemFactory),
        Arc::new(particles::ParticleSystemFactory),
        Arc::new(surface::SurfaceSystemFactory),
        Arc::new(canvas::CanvasSystemFactory),
        Arc::new(gui::GuiSystemFactory),
        Arc::new(gui::GuiLayoutSystemFactory),
        Arc::new(render::RenderSystemFactory),
    ]
}
