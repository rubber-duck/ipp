//! Compiled factory registration and authoring capability validation.

use super::scheduler::SystemInstance;
use super::{SystemFactories, SystemFactory, SystemId, SystemInitError, SystemScheduleError};
use std::{any::Any, collections::BTreeSet, sync::Arc};

/// Test-only fixture components every World admits without selecting a System.
#[cfg(test)]
const TEST_COMPONENTS: &[u16] = &[crate::ComponentValue::ROWS_FIXTURE];

/// Operations that require a selected evaluator rather than only a compiled schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorldOperation {
    /// Core ordered entity relationships.
    EntityLinks,
    /// Property and structural animation.
    Animation,
    /// Skeleton joint pose animation.
    JointAnimation,
    /// Scalar constraints.
    Constraints,
    /// Terminal spatial aiming.
    LookAt,
    /// Bounds and picking geometry.
    Geometry,
    /// Prepared render inputs.
    Rendering,
    /// Camera selection and evaluation.
    Camera,
    #[cfg(feature = "surfaces")]
    /// Surface and Surface cache declarations.
    Surface,
    #[cfg(feature = "surfaces")]
    /// Ordinary entity Canvas output and raw content.
    Canvas,
    #[cfg(feature = "gui")]
    /// GUI declarations and updates.
    Gui,
    #[cfg(feature = "particles")]
    /// Particle producers and playback.
    Particles,
}

/// One advertised capability and the selected systems needed to make it usable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemCapability<T> {
    /// Compiled component or operation identity.
    pub value: T,
    /// Additional selected factories required to expose it.
    pub requires: Vec<SystemId>,
}

impl<T> SystemCapability<T> {
    /// Declare an unconditional factory capability.
    pub fn new(value: T) -> Self {
        Self {
            value,
            requires: Vec::new(),
        }
    }

    /// Declare a capability available only with these selected factories.
    pub fn requiring(value: T, requires: impl IntoIterator<Item = SystemId>) -> Self {
        Self {
            value,
            requires: requires.into_iter().collect(),
        }
    }
}

/// Authoring support supplied by one selected factory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemCapabilities {
    /// Compiled component IDs the factory can evaluate.
    pub components: Vec<SystemCapability<u16>>,
    /// Operations implemented by the factory.
    pub operations: Vec<SystemCapability<WorldOperation>>,
}

impl SystemCapabilities {
    /// Declare unconditional component and operation support for one factory.
    pub fn new(
        components: impl IntoIterator<Item = u16>,
        operations: impl IntoIterator<Item = WorldOperation>,
    ) -> Self {
        Self {
            components: components.into_iter().map(SystemCapability::new).collect(),
            operations: operations.into_iter().map(SystemCapability::new).collect(),
        }
    }
}

/// Resolved, immutable admission contract for one World.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldManifest {
    systems: Vec<SystemId>,
    components: BTreeSet<u16>,
    operations: BTreeSet<WorldOperation>,
}

impl WorldManifest {
    /// Resolved systems in construction and evaluation order.
    pub fn systems(&self) -> &[SystemId] {
        &self.systems
    }

    /// Stable identity of the selected, ordered composition.
    pub fn composition_id(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325_u64;
        for system in &self.systems {
            for &byte in system.0.as_bytes().iter().chain(std::iter::once(&0)) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
        }
        hash
    }

    /// Admitted component IDs in stable identity order.
    pub fn components(&self) -> impl ExactSizeIterator<Item = u16> + '_ {
        self.components.iter().copied()
    }

    /// Supported operations in stable identity order.
    pub fn operations(&self) -> impl ExactSizeIterator<Item = WorldOperation> + '_ {
        self.operations.iter().copied()
    }

    /// Whether this World can author the compiled component.
    pub fn supports_component(&self, component: u16) -> bool {
        self.components.contains(&component)
    }

    /// Whether this World selected the operation's evaluator.
    pub fn supports_operation(&self, operation: WorldOperation) -> bool {
        self.operations.contains(&operation)
    }

    pub(crate) fn admit_command(&self, command: &crate::Command) -> Result<(), crate::ErrorReason> {
        use crate::Command;
        let component = match command {
            Command::InsertComponent {
                component,
                ..
            }
            | Command::SetField {
                component,
                ..
            }
            | Command::SetFieldIf {
                component,
                ..
            }
            | Command::SetDynamicProperty {
                component,
                ..
            }
            | Command::RemoveDynamicProperty {
                component,
                ..
            }
            | Command::RemoveComponent {
                component,
                ..
            } => Some(*component),
            Command::InsertComponentValue {
                value,
                ..
            } => Some(crate::ComponentValue::type_id(value)),
            #[cfg(feature = "gui")]
            Command::GuiAction {
                target,
                ..
            } => Some(target.component),
            Command::DetachWorldAttachmentIf {
                ..
            }
            | Command::DetachWorldAttachmentReceipt {
                ..
            } => Some(crate::ComponentValue::WORLD_ATTACHMENT),
            _ => None,
        };
        if let Some(component) = component {
            if crate::ComponentValue::field_count(component).is_err() {
                return Err(crate::ErrorReason::UnknownComponent);
            }
            if !self.supports_component(component) {
                return Err(crate::ErrorReason::UnsupportedDependency);
            }
        }
        Ok(())
    }

    pub(in crate::world) fn resolve(
        factories: &SystemFactories,
    ) -> Result<Self, SystemScheduleError> {
        let mut manifest = Self {
            systems: factories.ids().collect(),
            ..Self::default()
        };
        manifest.operations.insert(WorldOperation::EntityLinks);
        #[cfg(test)]
        manifest.components.extend(TEST_COMPONENTS);
        for registration in &factories.ordered {
            let capabilities = registration.factory.capabilities();
            for capability in capabilities.components {
                if crate::ComponentValue::field_count(capability.value).is_err() {
                    return Err(SystemScheduleError::InvalidComponentCapability {
                        system: registration.id,
                        component: capability.value,
                    });
                }
                if capability
                    .requires
                    .iter()
                    .all(|required| manifest.systems.contains(required))
                {
                    manifest.components.insert(capability.value);
                }
            }
            for capability in capabilities.operations {
                if capability
                    .requires
                    .iter()
                    .all(|required| manifest.systems.contains(required))
                {
                    manifest.operations.insert(capability.value);
                }
            }
        }
        for &component in &manifest.components {
            for &required in crate::ComponentValue::required_components(component) {
                if !manifest.components.contains(&required) {
                    return Err(SystemScheduleError::MissingComponentCapability {
                        component,
                        required,
                    });
                }
            }
        }
        Ok(manifest)
    }
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
            #[cfg(feature = "surfaces")]
            super::canvas::CanvasSystem::ID => any.is::<super::canvas::CanvasSystem>(),
            #[cfg(feature = "gui")]
            super::gui::GuiSystem::ID => any.is::<super::gui::GuiSystem>(),
            #[cfg(feature = "gui")]
            super::gui::GuiLayoutSystem::ID => any.is::<super::gui::GuiLayoutSystem>(),
            super::render::RenderSystem::ID => any.is::<super::render::RenderSystem>(),
            #[cfg(feature = "skeletal-animation")]
            super::skeleton::SkeletonSystem::ID => any.is::<super::skeleton::SkeletonSystem>(),
            #[cfg(feature = "skeletal-animation")]
            super::skinning::SkinningSystem::ID => any.is::<super::skinning::SkinningSystem>(),
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
                    || any.is::<super::render::RenderSystem>();
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
                    builtin |= any.is::<super::canvas::CanvasSystem>();
                }
                #[cfg(feature = "gui")]
                {
                    builtin |= any.is::<super::gui::GuiSystem>()
                        || any.is::<super::gui::GuiLayoutSystem>();
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
        Arc::new(super::world_attachment::WorldAttachmentSystemFactory),
        Arc::new(super::lifecycle_publisher::LifecyclePublisherSystemFactory),
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
        #[cfg(feature = "surfaces")]
        Arc::new(super::canvas::CanvasSystemFactory),
        #[cfg(feature = "gui")]
        Arc::new(super::gui::GuiSystemFactory),
        #[cfg(feature = "gui")]
        Arc::new(super::gui::GuiLayoutSystemFactory),
        Arc::new(super::render::RenderSystemFactory),
    ]
}
