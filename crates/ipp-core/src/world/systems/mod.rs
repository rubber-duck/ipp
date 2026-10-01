//! World-specific evaluation and lifecycle modules.

pub mod asset_dependencies;
pub mod world_attachment;

pub mod animation;

pub mod constraints;

pub mod hierarchy;

pub mod look_at;

pub mod camera;

pub mod geometry;

pub mod render;

pub mod skeleton;

pub mod surface;

pub mod canvas;

pub mod gui;

pub mod skinning;

mod contexts;
pub mod scheduler;
pub use contexts::{SystemDependencyBinding, SystemNumericContext, SystemWorldView};
pub use scheduler::{
    System, SystemCommitContext, SystemDependency, SystemFactories, SystemFactory, SystemId,
    SystemInitContext, SystemInitError, SystemSchedule, SystemScheduleError, SystemTeardownContext,
    SystemUpdateContext,
};

mod composition;
pub(in crate::world) use composition::validate_authoring_instances;

pub use composition::compiled_system_factories;
pub use composition::{SystemCapabilities, SystemCapability, WorldManifest, WorldOperation};

pub use contexts::SystemAssetContext;

pub mod lifecycle_publisher;
pub use contexts::{SystemCommandContext, SystemLifecycleContext, SystemRuntimeAccess};
pub use scheduler::SystemCommandOutcome;

mod persistence;
pub use persistence::{SystemLoadContext, SystemPersistentState, SystemSaveContext};

pub use super::{OperationImpact, SystemOperationPreparationContext};
pub use contexts::{SystemDependencies, SystemOperationContext};

mod bindings;
pub use bindings::*;
pub use ipp_schema_derive::system_update;

pub mod particles;

mod entity_references;
