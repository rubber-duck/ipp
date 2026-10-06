//! System identity, the System and factory traits, and their construction errors.

use super::{
    SystemAssetContext, SystemCommandContext, SystemCommitContext, SystemInitContext,
    SystemLifecycleContext, SystemTeardownContext, SystemUpdateContext,
};
use std::{any::Any, fmt};

/// Stable identity of a system or a separately scheduled evaluation pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SystemId(pub &'static str);

/// Required predecessors and conditional ordering share one validated graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemDependency {
    /// Require this system to be selected, initialized and updated first.
    Required(SystemId),
    /// Run after this system when selected; absence is valid.
    After(SystemId),
}

/// Reusable construction configuration owned by the Host. Instances are never shared.
pub trait SystemFactory: Send + Sync {
    /// Stable identity of the instances this factory constructs.
    fn id(&self) -> SystemId;

    /// Inspected before any initializer executes.
    fn dependencies(&self) -> &[SystemDependency] {
        &[]
    }

    /// Component and operation support contributed by this factory.
    fn capabilities(&self) -> super::SystemCapabilities {
        super::SystemCapabilities::default()
    }

    /// Default reservations owned and interpreted by this system's module.
    fn capacity_hints(&self) -> crate::WorldSystemCapacityHints {
        crate::WorldSystemCapacityHints::default()
    }

    /// Construct fresh state, using only resolved predecessors and borrowed services.
    /// On failure, the factory must release any unfinished acquisition of its own.
    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError>;
}

/// A World exclusively owns each instance and its ordinary mutable state.
pub trait System: Any {
    /// Component whose entities are outputs of `kind` supplied by this evaluator.
    fn output_component(&self, _kind: crate::OutputKind) -> Option<u16> {
        None
    }

    /// Whether this evaluator supplies the World-level output of `kind`, which
    /// names no entity and lives as long as the World.
    fn world_output(&self, _kind: crate::OutputKind) -> bool {
        false
    }

    /// Own final typed results before the Host assembles descendant outputs.
    fn publish_output(
        &self,
        _world: &crate::WorldContext<'_>,
        _output: &mut crate::host::WorldOutputBuilder<'_>,
    ) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Extract completed attachment inputs after local evaluation and before descendants run.
    fn completed_attachments(
        &self,
        _world: &crate::WorldContext<'_>,
        _attachments: &mut Vec<crate::PublishedWorldAttachment>,
    ) {
    }

    /// Claim derived placement after every local update and before descendant contexts form.
    /// Return Unavailable for managed but unready anchors; never request spatial fallback.
    fn attachment_placement(
        &self,
        _world: &crate::WorldContext<'_>,
        _anchor: crate::EntityId,
    ) -> crate::AttachmentPlacement {
        crate::AttachmentPlacement::Unmanaged
    }

    /// Capture owned subsystem state after generic entity identity selection.
    fn save_persistent_state(
        &self,
        _context: &mut super::SystemSaveContext<'_>,
    ) -> Result<Option<super::SystemPersistentState>, String> {
        Ok(None)
    }

    /// Restore a subsystem contribution into this unpublished World.
    fn load_persistent_state(
        &mut self,
        _context: &mut super::SystemLoadContext<'_, '_>,
        state: Option<&super::SystemPersistentState>,
    ) -> Result<(), String> {
        if state.is_some() {
            Err("System does not accept persistent state".into())
        } else {
            Ok(())
        }
    }

    /// Pure declaration from command-owned identities, not current mutable System state.
    /// Each queued command (including each group member) declares its own immutable
    /// foreign World reads. This neither validates lifetimes nor authorizes execution.
    fn command_world_references(
        &self,
        _command: &dyn Any,
        _visit: &mut dyn FnMut(crate::WorldRef),
    ) {
    }

    /// Read-only readiness of a standalone queued command, not a command group.
    ///
    /// A false result retains a standalone command at the ingress head, without
    /// stopping evaluation. Cancellation must make a parked command drainable.
    fn command_ready(&self, _command: &dyn Any) -> bool {
        true
    }

    /// Interpret an owned command at its ordered World boundary.
    fn command(
        &mut self,
        _context: &mut SystemCommandContext<'_>,
        _session: u64,
        _command: &dyn Any,
    ) -> Result<(), crate::ErrorReason> {
        Err(crate::ErrorReason::InvalidValue)
    }

    /// Detach owned observations for later transport encoding.
    fn drain_events(&mut self, _session: u64) -> Vec<Box<dyn Any>> {
        Vec::new()
    }

    /// Release only one session's retained state at the session fence.
    fn release_session(&mut self, _session: u64) {}

    /// Applied entity/component observation, after synchronous invalidation.
    fn lifecycle(
        &mut self,
        _context: &SystemLifecycleContext<'_>,
        _observation: &crate::systems::lifecycle_publisher::LifecycleObservation,
    ) {
    }

    /// Reserve storage without changing live state or imposing count ceilings.
    fn reserve_capacity(
        &mut self,
        _hints: &crate::WorldSystemCapacityHints,
    ) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Complete fallible service checks before ingress mutates state.
    fn prepare_frame(
        &mut self,
        _context: &mut SystemUpdateContext<'_, '_>,
    ) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Admit owned subsystem input after every fallible frame check succeeds.
    fn accept_ingress(&mut self, _context: &mut SystemUpdateContext<'_, '_>) {}

    /// Start subsystem-local provisional identifiers for one ordered batch.
    fn begin_batch(&mut self) {}

    /// Attach subsystem-owned batch outcomes after partial effects are committed.
    fn finish_batch(&mut self, _outcome: &mut crate::BatchOutcome) {}

    /// Describe owned releases without invalidating bindings or staging any values.
    fn operation_impact(
        &self,
        _context: &super::SystemOperationPreparationContext<'_>,
        _impact: &mut super::OperationImpact,
    ) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Reserve effects using the complete pure impact from core and all selected Systems.
    fn operation_effect_demand(
        &self,
        _context: &super::SystemOperationPreparationContext<'_>,
        _impact: &super::OperationImpact,
        _demand: &mut crate::OperationEffectDemand,
    ) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Observe and stage subsystem inputs before the operation changes authored state.
    fn before_operation(
        &mut self,
        _context: &mut super::SystemOperationContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Interpret a subsystem-owned variant; None leaves it for another handler.
    fn apply_operation(
        &mut self,
        _context: &mut super::SystemOperationContext<'_>,
    ) -> Option<Result<(), crate::ErrorReason>> {
        None
    }

    /// Resolve subsystem effects even when an operation retained partial changes.
    fn after_operation(
        &mut self,
        _context: &mut super::SystemOperationContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Validate this subsystem's changes without taking ownership of generic mutation.
    fn validate_commit(
        &self,
        _context: &SystemCommitContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Refresh subsystem state after committed storage changes.
    fn after_commit(&mut self, _context: &mut SystemCommitContext<'_>) {}

    /// Invalidate bindings synchronously before effective storage reuse.
    fn before_commit(&mut self, _context: &mut SystemCommitContext<'_>) {}

    /// Invalidate derived numeric results once per compiled write batch. This
    /// cannot replace storage or request structural cleanup; use lifecycle hooks
    /// for those operations. Numeric writes do not invoke commit validation.
    fn before_numeric_update(&mut self, _context: &mut super::SystemNumericContext<'_>) {}

    /// Another System is about to write absolute values into these fields
    /// (entity, component, field offset), replacing whatever they held. A System
    /// that keeps contributions to a field forgets them here and applies them in
    /// full after the write.
    fn before_absolute_writes(&mut self, _fields: &[(crate::EntityId, u16, u32)]) {}

    /// Invalidate resource-dependent storage before the Host releases a shared payload or identity.
    fn before_asset_release(
        &mut self,
        _context: &mut SystemAssetContext<'_>,
        _event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
    }

    /// Observe an applied resource transition independently of asynchronous client delivery.
    fn asset_lifecycle(
        &mut self,
        _context: &mut SystemAssetContext<'_>,
        _event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
    }

    /// Prepare internal component inputs after ordered mutation and before evaluation.
    fn prepare_evaluation(&mut self, _context: &mut SystemUpdateContext<'_, '_>) {}

    /// Complete subsystem observations after all evaluation passes have finished.
    fn finish_update(
        &mut self,
        _context: &mut SystemUpdateContext<'_, '_>,
        _report: &mut crate::WorldUpdateReport,
    ) {
    }

    /// Observe the final stored values of the evaluated `tick`. This last frame phase runs
    /// after every System's [`Self::finish_update`] and cannot change World state.
    fn observe_frame(&mut self, _world: super::SystemWorldView<'_>, _tick: u64) {}

    /// Release world-local service usage while predecessors and services still exist.
    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {}

    /// Execute once at the stored position. Time advancement belongs to the Host.
    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>);
}

/// Factory graph errors are detected before initialization has effects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemScheduleError {
    /// A selected identity appears more than once.
    Duplicate(SystemId),
    /// A requested factory is not registered on this Host.
    Unknown(SystemId),
    /// A factory names itself as a predecessor.
    SelfDependency(SystemId),
    /// A required predecessor or compiled authoring implementation is absent.
    MissingRequired {
        /// Dependent system or the compiled World authoring contract.
        system: SystemId,
        /// Required predecessor or built-in authoring system.
        required: SystemId,
    },
    /// Both kinds of ordering edges participate in cycle detection.
    Cycle(Vec<SystemId>),
    /// A factory advertised a component outside the compiled registry.
    InvalidComponentCapability {
        /// Advertising factory.
        system: SystemId,
        /// Unknown compiled component identity.
        component: u16,
    },
    /// A selected component requires a component with no selected evaluator.
    MissingComponentCapability {
        /// Admitted component that declares a requirement.
        component: u16,
        /// Required but unsupported component.
        required: u16,
    },
}

impl fmt::Display for SystemScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate(id) => write!(f, "duplicate system {}", id.0),
            Self::Unknown(id) => write!(f, "unknown system factory {}", id.0),
            Self::SelfDependency(id) => write!(f, "system {} depends on itself", id.0),
            Self::MissingRequired {
                system,
                required,
            } => write!(f, "system {} requires {}", system.0, required.0),
            Self::Cycle(ids) => write!(f, "systems blocked by a dependency cycle: {ids:?}"),
            Self::InvalidComponentCapability {
                system,
                component,
            } => {
                write!(
                    f,
                    "system {} advertises unknown component {component}",
                    system.0
                )
            }
            Self::MissingComponentCapability {
                component,
                required,
            } => {
                write!(
                    f,
                    "component {component} requires unsupported component {required}"
                )
            }
        }
    }
}

impl std::error::Error for SystemScheduleError {}

/// Initialization failures preserve the Host's live world collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemInitError {
    /// A factory attempted to bind an undeclared or absent predecessor.
    UnavailableDependency(SystemId),
    /// The initialized predecessor has a different concrete system type.
    DependencyType(SystemId),
    /// A factory did not provide the compiled built-in implementation for its ID.
    AuthoringSystemType(SystemId),
    /// Factory-specific construction or service failure.
    Message(String),
}

impl fmt::Display for SystemInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnavailableDependency(id) => {
                write!(f, "undeclared or absent system dependency {}", id.0)
            }
            Self::DependencyType(id) => write!(f, "incorrect dependency type for {}", id.0),
            Self::AuthoringSystemType(id) => {
                write!(f, "incorrect built-in implementation for {}", id.0)
            }
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SystemInitError {}
