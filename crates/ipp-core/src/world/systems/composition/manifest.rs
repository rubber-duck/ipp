//! Authoring capabilities advertised by factories and the immutable World manifest.

use super::{SystemFactories, SystemId, SystemScheduleError};
use std::collections::BTreeSet;

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
    /// Surface and Surface cache declarations.
    Surface,
    /// Ordinary entity Canvas output and raw content.
    Canvas,
    /// GUI declarations and updates.
    Gui,
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
