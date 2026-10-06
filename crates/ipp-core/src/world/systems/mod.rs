//! World-specific evaluation and lifecycle modules.

mod composition;

pub(in crate::world) use composition::{SystemInstance, validate_authoring_instances};

pub use composition::{
    MutableSystemParameter, OperationImpact, System, SystemAssetContext, SystemBindings,
    SystemBoundUpdate, SystemCapabilities, SystemCapability, SystemCommandContext,
    SystemCommandOutcome, SystemCommitContext, SystemDependencies, SystemDependency,
    SystemDependencyBinding, SystemEcsAccess, SystemFactories, SystemFactory, SystemId,
    SystemInitContext, SystemInitError, SystemLifecycleContext, SystemLoadContext,
    SystemNumericContext, SystemOperationContext, SystemOperationPreparationContext,
    SystemParameter, SystemParameterInputs, SystemPersistentState, SystemRuntimeAccess,
    SystemSaveContext, SystemSchedule, SystemScheduleError, SystemTeardownContext,
    SystemUpdateContext, SystemUpdateParameter, SystemWorldView, WorldManifest, WorldOperation,
    compact_dependencies, compiled_system_factories, validate_parameter_services,
};

pub use ipp_schema_derive::system_update;

pub mod animation;

pub mod asset_dependencies;

pub mod camera;

pub mod canvas;

pub mod constraints;

pub mod data_bindings;

pub mod geometry;

pub mod gui;

pub mod hierarchy;

pub mod lifecycle_publisher;

pub mod look_at;

pub mod particles;

pub mod plot;

pub mod render;

pub mod skeleton;

pub mod skinning;

pub mod surface;

pub mod world_attachment;
