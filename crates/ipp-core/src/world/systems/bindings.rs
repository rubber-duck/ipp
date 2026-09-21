//! Typed update parameters. Initialization owns resolution; callbacks only borrow.

use super::{
    SystemDependencies, SystemDependency, SystemId, SystemInitContext, SystemInitError,
    SystemUpdateContext,
};

/// Generic component access, independent of current-System and service borrows.
pub struct SystemEcsAccess<'a> {
    pub(in crate::world) world: &'a mut crate::world::WorldSimulationState,
}

impl SystemEcsAccess<'_> {
    /// Identity of the independently owned World.
    pub fn id(&self) -> crate::WorldId {
        self.world.id
    }

    /// Observe effective components under evaluation order, without authored base reconstruction.
    pub fn inspect_effective(
        &self,
        entity: crate::EntityId,
    ) -> Option<super::SystemEffectiveEntitySnapshot> {
        super::contexts::SystemWorldView {
            world: self.world,
            authored: &self.world.state,
        }
        .inspect_effective(entity)
    }
}

/// Disjoint Host-owned services. A parameter consumes its service slot for this callback.
/// This permits separate mutable services without aliasing or runtime borrow guards.
pub struct SystemParameterInputs<'a> {
    /// Already validated World-local dependency slots.
    pub dependencies: SystemDependencies<'a>,
    /// Generic Host I/O, available to one parameter in this callback.
    pub data_sources: Option<&'a mut crate::services::data_source::DataSourceManagementService>,
    /// Shared Host resource catalog, available to one parameter in this callback.
    pub assets: Option<&'a mut crate::services::asset_management::AssetManagementService>,
}

/// Compiler-selected source of a shared typed update parameter.
/// Implement this for each concrete System or Service, not overlapping blanket categories.
pub trait SystemParameter: Sized {
    /// Copyable resolution metadata retained between callbacks.
    type Binding: Copy;
    /// Schedule edge contributed by this parameter, if it reads another System.
    const DEPENDENCY: Option<SystemDependency>;
    /// Unique Host service slot, checked for duplicate parameters at construction.
    const SERVICE: Option<&'static str> = None;

    /// Validate and resolve this parameter once at World construction.
    fn bind(context: &SystemInitContext<'_>) -> Result<Self::Binding, SystemInitError>;

    /// Borrow directly from a validated slot for this callback only.
    fn borrow<'a>(binding: Self::Binding, inputs: &mut SystemParameterInputs<'a>) -> &'a Self;
}

/// Only Host services may supply mutable dependency parameters.
pub trait MutableSystemParameter: SystemParameter {
    /// Consume a distinct service slot into an exclusive callback borrow.
    fn borrow_mut<'a>(
        binding: Self::Binding,
        inputs: &mut SystemParameterInputs<'a>,
    ) -> &'a mut Self;
}

/// Reference-shape adapter used by generated code, including optional dependencies.
pub trait SystemUpdateParameter<'a>: Sized {
    /// Copyable resolution metadata retained between callbacks.
    type Binding: Copy;
    /// Schedule edge contributed by this parameter, if it reads another System.
    const DEPENDENCY: Option<SystemDependency>;
    /// Unique Host service slot, checked for duplicate parameters at construction.
    const SERVICE: Option<&'static str>;

    /// Validate and resolve this parameter once at World construction.
    fn bind(context: &SystemInitContext<'_>) -> Result<Self::Binding, SystemInitError>;

    /// Resolve this reference shape without allocation or state cloning.
    fn borrow(binding: Self::Binding, inputs: &mut SystemParameterInputs<'a>) -> Self;
}

impl<'a, T: SystemParameter + 'a> SystemUpdateParameter<'a> for &'a T {
    type Binding = T::Binding;
    /// Schedule edge contributed by this parameter, if it reads another System.
    const DEPENDENCY: Option<SystemDependency> = T::DEPENDENCY;
    /// Unique Host service slot, checked for duplicate parameters at construction.
    const SERVICE: Option<&'static str> = T::SERVICE;

    /// Validate and resolve this parameter once at World construction.
    fn bind(context: &SystemInitContext<'_>) -> Result<Self::Binding, SystemInitError> {
        T::bind(context)
    }

    /// Resolve this reference shape without allocation or state cloning.
    fn borrow(binding: Self::Binding, inputs: &mut SystemParameterInputs<'a>) -> Self {
        T::borrow(binding, inputs)
    }
}

impl<'a, T: MutableSystemParameter + 'a> SystemUpdateParameter<'a> for &'a mut T {
    type Binding = T::Binding;
    /// Schedule edge contributed by this parameter, if it reads another System.
    const DEPENDENCY: Option<SystemDependency> = T::DEPENDENCY;
    /// Unique Host service slot, checked for duplicate parameters at construction.
    const SERVICE: Option<&'static str> = T::SERVICE;

    /// Validate and resolve this parameter once at World construction.
    fn bind(context: &SystemInitContext<'_>) -> Result<Self::Binding, SystemInitError> {
        T::bind(context)
    }

    /// Resolve this reference shape without allocation or state cloning.
    fn borrow(binding: Self::Binding, inputs: &mut SystemParameterInputs<'a>) -> Self {
        T::borrow_mut(binding, inputs)
    }
}

impl<'a, T: SystemParameter + 'a> SystemUpdateParameter<'a> for Option<&'a T> {
    type Binding = Option<T::Binding>;
    /// Schedule edge contributed by this parameter, if it reads another System.
    const DEPENDENCY: Option<SystemDependency> = match T::DEPENDENCY {
        Some(SystemDependency::Required(id) | SystemDependency::After(id)) => {
            Some(SystemDependency::After(id))
        }
        None => None,
    };
    /// Unique Host service slot, checked for duplicate parameters at construction.
    const SERVICE: Option<&'static str> = T::SERVICE;

    /// Validate and resolve this parameter once at World construction.
    fn bind(context: &SystemInitContext<'_>) -> Result<Self::Binding, SystemInitError> {
        match T::bind(context) {
            Ok(binding) => Ok(Some(binding)),
            Err(SystemInitError::UnavailableDependency(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Resolve this reference shape without allocation or state cloning.
    fn borrow(binding: Self::Binding, inputs: &mut SystemParameterInputs<'a>) -> Self {
        binding.map(|binding| T::borrow(binding, inputs))
    }
}

/// Generated adapter retains the concrete System identity and its explicit callbacks.
pub trait SystemBoundUpdate: Sized {
    /// Generated collection of private scoped handles.
    type Bindings: Copy;

    /// Generated schedule metadata with explicit ordering-only edges.
    fn dependencies() -> &'static [SystemDependency];

    /// Validate and resolve this parameter once at World construction.
    fn bind(context: &SystemInitContext<'_>) -> Result<Self::Bindings, SystemInitError>;

    /// Object-safe adapter invocation after temporary borrows have been split.
    fn update_bound(&mut self, bindings: Self::Bindings, context: &mut SystemUpdateContext<'_, '_>);
}

/// Stored once per System. No dependency collection is built during updates.
pub struct SystemBindings<T: SystemBoundUpdate>(Option<T::Bindings>);

impl<T: SystemBoundUpdate> Default for SystemBindings<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T: SystemBoundUpdate> SystemBindings<T> {
    /// Initialize all declared parameters before publishing a World.
    pub fn resolve(context: &SystemInitContext<'_>) -> Result<Self, SystemInitError> {
        T::bind(context).map(|bindings| Self(Some(bindings)))
    }

    /// Copy only initialization metadata; no runtime state is cloned.
    pub fn get(&self) -> T::Bindings {
        self.0.expect("initialized System update bindings")
    }
}

/// Compact generated optional metadata at compile time, never during a frame.
#[doc(hidden)]
pub const fn compact_dependencies<const N: usize>(
    values: [Option<SystemDependency>; N],
) -> ([SystemDependency; N], usize) {
    let mut output = [SystemDependency::After(SystemId("")); N];
    let mut len = 0;
    let mut index = 0;
    while index < N {
        if let Some(value) = values[index] {
            output[len] = value;
            len += 1;
        }
        index += 1;
    }
    (output, len)
}

/// Reject duplicate service slots during construction before any update can run.
#[doc(hidden)]
pub fn validate_parameter_services(values: &[Option<&'static str>]) -> Result<(), SystemInitError> {
    for (index, service) in values.iter().enumerate() {
        if let Some(service) = service
            && values[..index].contains(&Some(*service))
        {
            return Err(SystemInitError::Message(format!(
                "duplicate update service parameter {service}"
            )));
        }
    }
    Ok(())
}

/// Register a concrete System as an immutable typed update parameter.
#[macro_export]
macro_rules! system_parameter {
    ($system:ty) => {
        impl $crate::systems::SystemParameter for $system {
            type Binding = $crate::systems::SystemDependencyBinding<Self>;
            /// Schedule edge contributed by this parameter, if it reads another System.
            const DEPENDENCY: Option<$crate::systems::SystemDependency> =
                Some($crate::systems::SystemDependency::Required(Self::ID));

            /// Validate and resolve this parameter once at World construction.
            fn bind(
                context: &$crate::systems::SystemInitContext<'_>,
            ) -> Result<Self::Binding, $crate::systems::SystemInitError> {
                context.dependency(Self::ID)
            }

            /// Borrow directly from a validated slot for this callback only.
            fn borrow<'a>(
                binding: Self::Binding,
                inputs: &mut $crate::systems::SystemParameterInputs<'a>,
            ) -> &'a Self {
                inputs
                    .dependencies
                    .get(binding)
                    .expect("validated System parameter binding")
            }
        }
    };
}

macro_rules! service_parameter {
    ($service:ty, $field:ident, $id:literal) => {
        impl SystemParameter for $service {
            type Binding = ();
            /// Schedule edge contributed by this parameter, if it reads another System.
            const DEPENDENCY: Option<SystemDependency> = None;
            /// Unique Host service slot, checked for duplicate parameters at construction.
            const SERVICE: Option<&'static str> = Some($id);

            fn bind(_context: &SystemInitContext<'_>) -> Result<(), SystemInitError> {
                Ok(())
            }

            fn borrow<'a>((): (), inputs: &mut SystemParameterInputs<'a>) -> &'a Self {
                inputs
                    .$field
                    .take()
                    .expect("unique initialized service parameter")
            }
        }

        impl MutableSystemParameter for $service {
            fn borrow_mut<'a>((): (), inputs: &mut SystemParameterInputs<'a>) -> &'a mut Self {
                inputs
                    .$field
                    .take()
                    .expect("unique initialized service parameter")
            }
        }
    };
}

service_parameter!(
    crate::services::data_source::DataSourceManagementService,
    data_sources,
    "ipp.data-source"
);
service_parameter!(
    crate::services::asset_management::AssetManagementService,
    assets,
    "ipp.asset-management"
);

/// Object-safe adapter inside the System's explicit lifecycle/persistence impl.
#[macro_export]
macro_rules! system_update {
    ($field:ident) => {
        fn update(&mut self, context: &mut $crate::systems::SystemUpdateContext<'_, '_>) {
            <Self as $crate::systems::SystemBoundUpdate>::update_bound(
                self,
                self.$field.get(),
                context,
            );
        }
    };
}

#[cfg(feature = "skeletal-animation")]
crate::system_parameter!(crate::systems::skeleton::SkeletonSystem);
#[cfg(feature = "skeletal-animation")]
crate::system_parameter!(crate::systems::skinning::SkinningSystem);
crate::system_parameter!(crate::systems::animation::AnimationSystem);
crate::system_parameter!(crate::systems::state_overlay::StateOverlaySystem);
crate::system_parameter!(crate::systems::camera::CameraSystem);
