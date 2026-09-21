//! Validated factory order and exclusively owned world instances.

use std::{any::Any, collections::BTreeMap, fmt, sync::Arc};

pub use super::contexts::{
    SystemCommandContext, SystemCommitContext, SystemInitContext, SystemLifecycleContext,
    SystemTeardownContext, SystemUpdateContext,
};

pub use super::contexts::SystemAssetContext;

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
    /// Substitute sparse producer originals without exposing this System's private storage.
    fn restore_component_input(
        &self,
        _entity: crate::EntityId,
        _incarnation: u64,
        _value: &mut crate::ComponentValue,
    ) {
    }

    /// Exclude subsystem-owned transient entities from durable capture.
    fn include_entity_in_snapshot(&self, _entity: crate::EntityId) -> bool {
        true
    }

    /// Capture owned subsystem state after generic producer identity selection.
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

    /// Whether this System holds routed input that must drain through a
    /// later mutation boundary before newer ingress may overtake it.
    /// Command-chunk admission treats a positive answer as transient
    /// backpressure, never as rejection.
    fn has_deferred_input(&self) -> bool {
        false
    }

    /// Applied entity/component observation, after synchronous invalidation.
    fn lifecycle(
        &mut self,
        _context: &SystemLifecycleContext<'_>,
        _observation: &super::lifecycle_publisher::LifecycleObservation,
    ) {
    }

    /// Reserve storage without changing live state or imposing count ceilings.
    fn reserve_capacity(
        &mut self,
        _hints: &crate::WorldSystemCapacityHints,
    ) -> Result<(), crate::ErrorReason> {
        Ok(())
    }

    /// Complete fallible service checks before ingress or evaluated restoration mutates state.
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

    /// Restore sparse inputs before ordered ingress; no simulation time advances here.
    fn prepare_mutation(&mut self, _context: &mut SystemUpdateContext<'_, '_>) {}

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

pub(in crate::world) struct SystemFactoryRegistration {
    pub factory: Arc<dyn SystemFactory>,
    pub id: SystemId,
    pub predecessors: Vec<usize>,
}

/// Host-owned reusable factories in deterministic dependency order.
pub struct SystemFactories {
    pub(in crate::world) ordered: Vec<SystemFactoryRegistration>,
}

impl SystemFactories {
    /// Validate the complete graph without constructing a single system.
    pub fn new(factories: Vec<Arc<dyn SystemFactory>>) -> Result<Self, SystemScheduleError> {
        let ids: Vec<_> = factories.iter().map(|factory| factory.id()).collect();
        let mut indices = BTreeMap::new();
        for (index, &id) in ids.iter().enumerate() {
            if indices.insert(id, index).is_some() {
                return Err(SystemScheduleError::Duplicate(id));
            }
        }

        let mut predecessors = vec![Vec::new(); factories.len()];
        for (index, factory) in factories.iter().enumerate() {
            for &dependency in factory.dependencies() {
                let (id, required) = match dependency {
                    SystemDependency::Required(id) => (id, true),
                    SystemDependency::After(id) => (id, false),
                };
                if id == ids[index] {
                    return Err(SystemScheduleError::SelfDependency(id));
                }
                if let Some(&predecessor) = indices.get(&id) {
                    if !predecessors[index].contains(&predecessor) {
                        predecessors[index].push(predecessor);
                    }
                } else if required {
                    return Err(SystemScheduleError::MissingRequired {
                        system: ids[index],
                        required: id,
                    });
                }
            }
        }

        let mut order = Vec::with_capacity(factories.len());
        let mut emitted = vec![false; factories.len()];
        while order.len() < factories.len() {
            let Some(next) = (0..factories.len())
                .find(|&index| !emitted[index] && predecessors[index].iter().all(|&p| emitted[p]))
            else {
                return Err(SystemScheduleError::Cycle(
                    ids.iter()
                        .enumerate()
                        .filter_map(|(index, &id)| (!emitted[index]).then_some(id))
                        .collect(),
                ));
            };
            emitted[next] = true;
            order.push(next);
        }

        let mut positions = vec![0; order.len()];
        for (position, &index) in order.iter().enumerate() {
            positions[index] = position;
        }
        Ok(Self {
            ordered: order
                .into_iter()
                .map(|index| SystemFactoryRegistration {
                    factory: Arc::clone(&factories[index]),
                    id: ids[index],
                    predecessors: predecessors[index].iter().map(|&p| positions[p]).collect(),
                })
                .collect(),
        })
    }

    /// Resolved order; unconstrained ties preserve registration order.
    pub fn ids(&self) -> impl ExactSizeIterator<Item = SystemId> + '_ {
        self.ordered.iter().map(|registration| registration.id)
    }

    /// Select registered factories for one World and validate the selected graph.
    pub fn select(&self, selected: &[SystemId]) -> Result<Self, SystemScheduleError> {
        let factories = selected
            .iter()
            .map(|&id| {
                self.ordered
                    .iter()
                    .find(|registration| registration.id == id)
                    .map(|registration| Arc::clone(&registration.factory))
                    .ok_or(SystemScheduleError::Unknown(id))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(factories)
    }
}

/// Owned entry. There is no clonable instance or state allocation handle.
pub(in crate::world) struct SystemInstance {
    pub id: SystemId,
    pub system: Box<dyn System>,
}

/// World-owned instances in their construction and update order.
pub struct SystemSchedule {
    pub(in crate::world) instances: Vec<SystemInstance>,
}

impl SystemSchedule {
    /// The immutable order used for every update of this World.
    pub fn ids(&self) -> impl ExactSizeIterator<Item = SystemId> + '_ {
        self.instances.iter().map(|instance| instance.id)
    }
}

/// Correlated outcome of an ordered system command, independent of event subscriptions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemCommandOutcome {
    /// Originating World attachment/session.
    pub session: u64,
    /// Caller correlation, with zero reserved for commands without replies.
    pub request_id: u64,
    /// Successful commands before completion or the first failed command.
    pub applied: usize,
    /// Actual applied-command result; failures do not imply rollback.
    pub result: Result<(), crate::ErrorReason>,
}
