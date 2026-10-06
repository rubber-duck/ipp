//! System composition: factory graph validation, World-owned instances, scoped
//! callback contexts, typed update parameters, the authoring manifest and
//! persistence hooks.

mod catalog;
mod contexts;
mod manifest;
mod parameters;
mod persistence;
mod schedule;
mod system_trait;

pub use catalog::compiled_system_factories;
pub(in crate::world) use catalog::validate_authoring_instances;

pub use contexts::{
    OperationImpact, SystemAssetContext, SystemCommandContext, SystemCommitContext,
    SystemDependencies, SystemDependencyBinding, SystemEcsAccess, SystemInitContext,
    SystemLifecycleContext, SystemNumericContext, SystemOperationContext,
    SystemOperationPreparationContext, SystemRuntimeAccess, SystemTeardownContext,
    SystemUpdateContext, SystemWorldView,
};

pub use manifest::{SystemCapabilities, SystemCapability, WorldManifest, WorldOperation};

pub use parameters::{
    MutableSystemParameter, SystemBindings, SystemBoundUpdate, SystemParameter,
    SystemParameterInputs, SystemUpdateParameter, compact_dependencies,
    validate_parameter_services,
};

pub use persistence::{SystemLoadContext, SystemPersistentState, SystemSaveContext};

pub(in crate::world) use schedule::SystemInstance;
pub use schedule::{SystemCommandOutcome, SystemFactories, SystemSchedule};

pub use system_trait::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitError, SystemScheduleError,
};
