//! World-specific evaluation and lifecycle modules.

pub mod asset_dependencies;

pub mod animation;

pub mod constraints;

pub mod hierarchy;

pub mod look_at;

pub mod camera;

pub mod geometry;

pub mod render;

#[cfg(feature = "skeletal-animation")]
pub mod skeleton;

pub mod state_overlay;

#[cfg(feature = "surfaces")]
pub mod surface;

#[cfg(feature = "gui")]
pub mod gui;

#[cfg(feature = "skeletal-animation")]
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
pub(in crate::world) use composition::{
    validate_authoring_factories, validate_authoring_instances,
};

pub use composition::compiled_system_factories;

pub use contexts::SystemAssetContext;

pub mod lifecycle_publisher;
pub use contexts::{SystemCommandContext, SystemLifecycleContext, SystemRuntimeAccess};
pub use scheduler::SystemCommandOutcome;

mod persistence;
pub use persistence::{SystemLoadContext, SystemPersistentState, SystemSaveContext};

pub use contexts::{SystemDependencies, SystemEffectiveEntitySnapshot, SystemOperationContext};

mod bindings;
pub use bindings::*;
pub use ipp_schema_derive::system_update;

#[cfg(feature = "particles")]
pub mod particles;

mod entity_references;
