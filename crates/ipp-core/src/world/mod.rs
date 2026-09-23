mod serialization;

mod metadata;

pub(crate) use metadata::validate_world_symbolic_id;
pub use metadata::{
    EntityPersistentId, WorldCreateOptions, WorldDescriptor, WorldMetadata, WorldPersistentId,
    WorldSelector,
};

mod capacity;

pub use capacity::{WorldCapacityHints, WorldCapacityHintsPatch, WorldSystemCapacityHints};

#[cfg(test)]
mod storage_tests;

mod mutation;
mod mutation_map;
mod removals;

mod access;
pub use access::WorldContext;
use access::{SystemInstanceAccess, WorldReadContext};

pub mod systems;

pub use systems::render::{DebugRenderItem, RenderDiagnostic, RenderItem};

use systems::state_overlay;

use crate::StateOverlayLifecycleDiagnostic;

use state_overlay::ComponentStateOverlayInputs as WorldComponentInputs;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[cfg(test)]
use crate::components::Scalar;
use crate::{
    Batch, BatchError, BatchOutcome, Command, ComponentValue, EntityId, EntityMetadata, EntityRef,
    ErrorReason, FieldValue, FieldWrite, components::registry, identity::Allocator,
};

/// Explicit upper bounds for ingress and transient component activation.
#[derive(Clone, Copy, Debug)]
pub struct WorldLimits {
    /// Maximum operations in one indivisible batch; defaults to no count quota.
    pub max_operations: usize,
    /// Maximum owned batch allocation estimate, including spare capacities.
    /// Defaults to no byte quota; allocation and representability still apply.
    pub max_batch_bytes: usize,
    /// Maximum queued batches, system commands and queries per frame.
    pub max_queued_batches: usize,
    /// Maximum transient component activation bytes during an update.
    pub max_staging_bytes: usize,
}

impl Default for WorldLimits {
    fn default() -> Self {
        Self {
            max_operations: usize::MAX,
            max_batch_bytes: usize::MAX,
            max_queued_batches: 64,
            max_staging_bytes: 16 << 20,
        }
    }
}

/// Owned, read-only observation of the actual world state.
#[derive(Clone, Debug, PartialEq)]
pub struct EntitySnapshot {
    /// World-local identity.
    pub id: EntityId,
    /// Current normalized metadata.
    pub metadata: EntityMetadata,
    /// Retained producer components in registry order.
    pub base: Vec<ComponentValue>,
    /// Evaluated components in registry order.
    pub effective: Vec<ComponentValue>,
}

/// Stage 8 publication after mutation and evaluation complete.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldUpdateReport {
    /// Ordered playback transitions committed during this frame.
    pub playback_events: Vec<crate::systems::animation::AnimationPlaybackEvent>,
    /// Correlated outcomes for ordered controller mutations.
    pub animation_controller_outcomes: Vec<crate::systems::animation::AnimationControllerOutcome>,
    /// Monotonic completed-frame number.
    pub tick: u64,
    /// Accumulated host-supplied simulation time.
    pub time: f64,
    /// Correlated ordered system-command results.
    pub system_command_outcomes: Vec<systems::SystemCommandOutcome>,
    /// One outcome per queued batch, in submission order.
    pub outcomes: Vec<BatchOutcome>,
    /// Changed camera-system settings in committed transition order.
    pub camera_state_changes: Vec<crate::CameraStateChange>,
    /// Changed global render settings in committed transition order.
    pub render_state_changes: Vec<crate::RenderStateChange>,
    /// Read-only geometry results observing the final camera and effective frame.
    pub geometry_picks: Vec<crate::GeometryPickOutcome>,
    /// Stateless plane projections observing the final effective camera.
    pub camera_projections: Vec<crate::CameraProjectOutcome>,
    /// Mesh completions processed before ordinary batches, in enqueue order.
    pub assets: Vec<crate::services::asset_management::AssetUploadOutcome>,
    /// Terminal provider outcomes committed this frame, in completion order.
    pub resource_changes: Vec<crate::AssetResourceSnapshot>,
    /// Committed lifecycle losses, in batch and deterministic resource order.
    pub diagnostics: Vec<StateOverlayLifecycleDiagnostic>,
    /// Ordered GUI input effects committed at this frame, with source/effect ticks.
    /// Routed against one immutable per-tick snapshot; applied at the next
    /// mutation boundary before animation with liveness revalidation.
    #[cfg(feature = "gui")]
    pub gui_input_effects: Vec<crate::systems::gui::GuiInputEffect>,
    /// GUI inputs cancelled between routing and application (removal,
    /// hide/disable, capture loss, platform blur, session replacement).
    /// Carries source/effect ticks; never mixed with effects.
    #[cfg(feature = "gui")]
    pub gui_input_cancellations: Vec<crate::systems::gui::GuiInputCancellation>,
    /// GUI action conflicts (revision mismatch, admission failure, touch
    /// arbitration). Reported separately from effects and cancellations.
    #[cfg(feature = "gui")]
    pub gui_input_conflicts: Vec<crate::systems::gui::GuiInputConflict>,
    /// Unhandled GUI inputs observable for scene controls. Each source input
    /// appears here at most once and never alongside an effect (no duplicate
    /// dispatch).
    #[cfg(feature = "gui")]
    pub gui_unhandled_inputs: Vec<crate::systems::gui::GuiUnhandledInput>,
    /// Authoritative focused-editable state changes for native platform bridges.
    #[cfg(feature = "gui")]
    pub gui_text_focus_updates: Vec<crate::systems::gui::GuiTextFocusUpdate>,
}

#[derive(Clone, Copy)]
pub(crate) struct ComponentStateInstance<T> {
    pub(crate) base: T,
    pub(crate) incarnation: u64,
}

#[derive(Default)]
pub(super) struct WorldComponentState {
    inputs: WorldComponentInputs,
}

impl WorldComponentState {
    fn input(&self) -> Option<&ComponentStateInstance<()>> {
        self.inputs.input()
    }
}

#[derive(Default)]
pub(crate) struct WorldEntityRecord {
    persistent_id: EntityPersistentId,
    metadata: EntityMetadata,
    layers: BTreeMap<u16, WorldComponentState>,
}

impl WorldEntityRecord {
    fn input(&self, component: u16) -> Option<&ComponentStateInstance<()>> {
        self.layers.get(&component)?.input()
    }
}

#[derive(Default)]
pub(crate) struct WorldEntityState {
    pub(crate) dirty: BTreeSet<(EntityId, u16)>,
    operation_components: BTreeSet<(EntityId, u16)>,
    // Keys this operation touched through a path other than in-place producer
    // writes; their observation compares whole values.
    operation_untracked: BTreeSet<(EntityId, u16)>,
    // In-place producer writes of this operation and whether each changed the value.
    operation_writes: Vec<(
        (EntityId, u16),
        component_state::observations::ComponentStagedWrite,
        bool,
    )>,
    operation_created: BTreeSet<EntityId>,
    operation_deleted: BTreeSet<EntityId>,
    lifecycle_effects: Vec<systems::lifecycle_publisher::LifecycleObservation>,
    observed_components: BTreeMap<(EntityId, u16), (Option<u64>, Option<ComponentValue>)>,
    // In-place writes applied after a component's observed value (or retained
    // storage when none was taken), replayed only for a whole-value comparison.
    observed_writes:
        BTreeMap<(EntityId, u16), Vec<component_state::observations::ComponentStagedWrite>>,
    changed: mutation_map::MutationMap<(EntityId, u16), Option<u64>>,
    prepared: mutation_map::MutationMap<(EntityId, u16), ComponentValue>,
    // Staged inputs whose effective copy is made once at the next commit boundary.
    deferred_preparation: BTreeSet<(EntityId, u16)>,
    // A sparse candidate for one existing-property commit. Cleared after publish;
    // the retained Vec is capacity only, not a second component value store.
    evaluated_target: Option<(EntityId, u16)>,
    evaluated_properties: Vec<(u32, crate::components::schema::FieldValue)>,
    #[cfg(feature = "surfaces")]
    deferred_mutations: Vec<DeferredComponentMutation>,
    explicit_fields: BTreeSet<(EntityId, u16, u32)>,
    activation_budget: usize,
    prepared_bytes: usize,
    allocator: Allocator,
    retired_entities: Vec<EntityId>,
    // Applied effects are drained even when a later operation fails.
    #[cfg(feature = "diagnostics")]
    entity_effects: Vec<(&'static str, EntityId)>,
    pub(crate) entities: BTreeMap<EntityId, WorldEntityRecord>,
    symbols: BTreeMap<String, EntityId>,
    classes: BTreeMap<String, BTreeSet<EntityId>>,
    next_incarnation: u64,
    next_persistent_entity_id: u64,
}

#[cfg(feature = "surfaces")]
type DeferredComponentMutationOperation =
    Box<dyn FnOnce(&mut registry::ComponentStorage) -> Result<(), ErrorReason>>;

#[cfg(feature = "surfaces")]
pub(crate) struct DeferredComponentMutation {
    entity: EntityId,
    component: u16,
    apply: DeferredComponentMutationOperation,
}

/// Exclusive mutation state moved from ECS and participating systems, never cloned.
#[derive(Default)]
pub(crate) struct WorldMutationState {
    entities_state: WorldEntityState,
}

impl std::ops::Deref for WorldMutationState {
    type Target = WorldEntityState;

    fn deref(&self) -> &WorldEntityState {
        &self.entities_state
    }
}

impl std::ops::DerefMut for WorldMutationState {
    fn deref_mut(&mut self) -> &mut WorldEntityState {
        &mut self.entities_state
    }
}

/// Single mutation owner of a headless ECS.
///
/// Batches mutate metadata and affected values without snapshots or rollback.
/// Occupied effective components stay in stable boxed pages.
/// Standard System composition includes StateOverlay declarations and scalar constraints.
/// Systems own evaluation, dependency validation and lifecycle cleanup.
pub struct World {
    pub(crate) data: WorldSimulationState,
    schedule: systems::SystemSchedule,
}

/// ECS storage, ingress and simulation clock retained by a World.
pub struct WorldSimulationState {
    updating: bool,
    prepared_frame: bool,
    mutation_prepared: bool,
    /// Restore precedes ingress that queue inspection cannot see: subsystem
    /// input admitted at Accept or a Host command stream. Restoration then
    /// returns every retained evaluated output before inputs are staged.
    admitting_ingress: bool,
    command_stream: Option<BTreeMap<u32, EntityId>>,
    accepting_removals: bool,
    forced_cleanup: bool,
    restoring: bool,
    fault: Option<ErrorReason>,
    deferred_removals: Vec<removals::DeferredRemoval>,
    deferred_removal_members: BTreeSet<removals::DeferredRemoval>,
    identity: usize,
    pub(crate) id: crate::WorldId,
    pub(crate) metadata: WorldMetadata,
    limits: WorldLimits,
    pub(crate) capacity_hints: WorldCapacityHints,
    state: WorldEntityState,
    components: registry::ComponentStorage,
    queue: VecDeque<Ingress>,
    command_buffers: Vec<Vec<Command>>,
    lifecycle_cleanup: Vec<(EntityId, ComponentValue)>,
    tick: u64,
    time: f64,
}

enum Ingress {
    System {
        system: systems::SystemId,
        session: u64,
        request_id: u64,
        command: Box<dyn std::any::Any>,
    },
    SystemBatch {
        system: systems::SystemId,
        session: u64,
        request_id: u64,
        commands: Vec<Box<dyn std::any::Any>>,
    },
    Batch(Batch),
}

/// A world is unpublished until its limits and complete selected graph validate.
#[derive(Debug)]
pub enum WorldConstructionError {
    /// Invalid or conflicting Host-visible World metadata.
    Metadata(String),
    /// Invalid ingress bounds, activation allowance or storage reservation.
    Limits(ErrorReason),
    /// Missing state/implementation or an invalid system dependency graph.
    Systems(systems::SystemScheduleError),
    /// A factory failed after graph validation. Earlier instances have been shut down.
    Initialization {
        /// Factory whose initialization failed.
        system: systems::SystemId,
        /// Concrete dependency or factory failure.
        error: systems::SystemInitError,
    },
}

impl std::fmt::Display for WorldConstructionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Metadata(error) => f.write_str(error),
            Self::Limits(error) => error.fmt(f),
            Self::Systems(error) => error.fmt(f),
            Self::Initialization {
                system,
                error,
            } => write!(f, "initializing {}: {}", system.0, error),
        }
    }
}

impl std::error::Error for WorldConstructionError {}

impl WorldContext<'_> {
    /// Reuse capacity from a consumed batch. The buffer contains no live commands.
    /// Hosts may grow it for larger input; queued batches retain exclusive ownership.
    /// Retained capacity is capped at 256 commands; larger native batches are not cached.
    pub fn take_command_buffer(&mut self) -> Vec<Command> {
        self.world
            .command_buffers
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(256))
    }

    /// Return unused decode capacity without retaining command payloads or identities.
    pub fn recycle_command_buffer(&mut self, mut commands: Vec<Command>) {
        commands.clear();
        if (1..=256).contains(&commands.capacity()) {
            self.world.command_buffers.push(commands);
        }
    }
}

#[cfg(all(test, feature = "diagnostics"))]
mod diagnostic_invariants {
    use super::*;
    use crate::diagnostics::{Level, configure};

    #[test]
    fn filtered_effect_recording_allocates_no_journal_storage() {
        fn sink(_level: Level, _line: std::fmt::Arguments<'_>) {}

        let mut state = WorldMutationState::default();
        for level in [Level::Off, Level::Error, Level::Warn, Level::Info] {
            configure(level, Some(sink));
            state.record_entity_effect("entity.create", EntityId::from_bits(1));
            state.record_entity_effect("entity.delete", EntityId::from_bits(1));
            assert!(state.entity_effects.is_empty());
            assert_eq!(state.entity_effects.capacity(), 0);
        }
        configure(Level::Debug, Some(sink));
        state.record_entity_effect("entity.create", EntityId::from_bits(1));
        assert_eq!(state.entity_effects.len(), 1);
        configure(Level::Off, None);
    }
}

mod component_state;
mod entity_state;

mod runtime;
use runtime::metadata_bytes;

mod queries;

mod component_binding;

mod component_query;
