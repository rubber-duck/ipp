mod serialization;

pub(crate) mod composition;
mod host_ingress;
mod metadata;

mod entity_link_evaluation;
mod entity_links;
pub use entity_links::{EntityLink, EntityOrder, EntityPlacement, EntityPlacementRef};

pub(crate) use metadata::validate_world_symbolic_id;
pub use metadata::{
    EntityPersistentId, WorldCreateOptions, WorldDescriptor, WorldMetadata, WorldPersistentId,
    WorldSelector,
};

mod capacity;

pub use capacity::{WorldCapacityHints, WorldCapacityHintsPatch, WorldSystemCapacityHints};

#[cfg(test)]
mod storage_tests;

#[cfg(test)]
mod selection_tests;

#[cfg(test)]
mod asset_barrier_tests;

mod mutation;
mod operation_effects;
pub use operation_effects::{OperationImpact, SystemOperationPreparationContext};
mod mutation_map;
mod removals;

mod access;
pub use access::WorldContext;
use access::{SystemInstanceAccess, WorldReadContext};

pub mod systems;

pub use systems::render::{DebugRenderItem, RenderDiagnostic, RenderItem};

use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[cfg(test)]
use crate::components::Scalar;
use crate::{
    Batch, BatchError, BatchOutcome, Command, ComponentValue, EntityId, EntityMetadata, EntityRef,
    ErrorReason, FieldValue, FieldWrite, components::registry, identity::Allocator,
};

/// Explicit upper bounds for ingress.
#[derive(Clone, Copy, Debug)]
pub struct WorldLimits {
    /// Maximum operations in one indivisible batch; defaults to no count quota.
    pub max_operations: usize,
    /// Maximum owned batch allocation estimate, including spare capacities.
    /// Defaults to no byte quota; allocation and representability still apply.
    pub max_batch_bytes: usize,
    /// Maximum queued batches, system commands and queries per frame.
    pub max_queued_batches: usize,
}

impl Default for WorldLimits {
    fn default() -> Self {
        Self {
            max_operations: usize::MAX,
            max_batch_bytes: usize::MAX,
            max_queued_batches: 64,
        }
    }
}

/// Owned, read-only observation of one live entity.
#[derive(Clone, Debug, PartialEq)]
pub struct EntitySnapshot {
    /// World-local identity.
    pub id: EntityId,
    /// Current normalized metadata.
    pub metadata: EntityMetadata,
    /// Ordered relationship: parent and sibling order.
    pub link: EntityLink,
    /// Stored components in registry order.
    pub components: Vec<ComponentValue>,
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
}

#[derive(Clone, Copy)]
pub(crate) struct ComponentStateInstance {
    pub(crate) incarnation: u64,
}

/// One present component: its incarnation and, while a batch touches it, the
/// batch's single transient staged copy. The copy moves to the commit's prepared
/// values once the batch commits, so retained storage stays the only store.
pub(super) struct WorldComponentState {
    instance: ComponentStateInstance,
    staged: Option<Box<ComponentValue>>,
}

#[derive(Default)]
pub(crate) struct WorldEntityRecord {
    persistent_id: EntityPersistentId,
    metadata: EntityMetadata,
    components: BTreeMap<u16, WorldComponentState>,
}

impl WorldEntityRecord {
    /// Incarnation of a present component.
    fn input(&self, component: u16) -> Option<&ComponentStateInstance> {
        self.components.get(&component).map(|state| &state.instance)
    }
}

#[derive(Default)]
pub(crate) struct WorldEntityState {
    pub(in crate::world) links: entity_links::EntityLinkStore,
    pub(crate) dirty: BTreeSet<(EntityId, u16)>,
    operation_components: BTreeSet<(EntityId, u16)>,
    // Keys this operation touched through a path other than in-place staged
    // writes; their observation compares whole values.
    operation_untracked: BTreeSet<(EntityId, u16)>,
    // In-place staged writes of this operation and whether each changed the value.
    operation_writes: Vec<(
        (EntityId, u16),
        component_state::observations::ComponentStagedWrite,
        bool,
    )>,
    operation_created: BTreeSet<EntityId>,
    operation_deleted: BTreeSet<EntityId>,
    // Whether this operation adopted an existing entity or component.
    operation_adopted: bool,
    lifecycle_effects: Vec<systems::lifecycle_publisher::LifecycleObservation>,
    observed_components: BTreeMap<(EntityId, u16), (Option<u64>, Option<ComponentValue>)>,
    // In-place writes applied after a component's observed value (or retained
    // storage when none was taken), replayed only for a whole-value comparison.
    observed_writes:
        BTreeMap<(EntityId, u16), Vec<component_state::observations::ComponentStagedWrite>>,
    changed: mutation_map::MutationMap<(EntityId, u16), Option<u64>>,
    prepared: mutation_map::MutationMap<(EntityId, u16), ComponentValue>,
    // Staged copies moved into `prepared` once, at the next commit boundary.
    deferred_preparation: BTreeSet<(EntityId, u16)>,
    // A sparse candidate for one existing-property commit. Cleared after publish;
    // the retained Vec is capacity only, not a second component value store.
    evaluated_target: Option<(EntityId, u16)>,
    evaluated_properties: Vec<(u32, crate::components::schema::FieldValue)>,
    explicit_fields: BTreeSet<(EntityId, u16, u32)>,
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
/// Occupied components stay in stable boxed pages, one value per field.
/// Systems own evaluation, dependency validation and lifecycle cleanup.
pub struct World {
    pub(crate) data: WorldSimulationState,
    schedule: systems::SystemSchedule,
}

/// ECS storage, ingress and simulation clock retained by a World.
pub struct WorldSimulationState {
    updating: bool,
    prepared_frame: bool,
    accepting_removals: bool,
    restoring: bool,
    fault: Option<ErrorReason>,
    deferred_removals: Vec<removals::DeferredRemoval>,
    deferred_removal_members: BTreeSet<removals::DeferredRemoval>,
    identity: usize,
    pub(crate) id: crate::WorldId,
    pub(crate) metadata: WorldMetadata,
    limits: WorldLimits,
    pub(crate) capacity_hints: WorldCapacityHints,
    pub(crate) manifest: systems::WorldManifest,
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
    Batch {
        batch: Batch,
        effect_sink: Option<Box<dyn crate::OperationEffectSink>>,
    },
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
    /// Invalid Canvas creation state, or Canvas state for a World that does not
    /// select the Canvas System.
    Canvas(ErrorReason),
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
            Self::Canvas(error) => write!(f, "World creation Canvas state: {error}"),
        }
    }
}

impl std::error::Error for WorldConstructionError {}

/// Largest command capacity a World keeps for reuse: one full batch page, so
/// hosts can decode any page into a recycled buffer. Larger assembled batches
/// are released instead of cached.
pub const RECYCLED_COMMAND_BUFFER_COMMANDS: usize = 1024;

/// Recycled buffers a World keeps, at most 128 KiB of command slots each. Hosts
/// that decode pages away from the World return applied batches without taking
/// buffers, so the pool must not grow with the number of applied batches.
const RECYCLED_COMMAND_BUFFERS: usize = 2;

impl WorldContext<'_> {
    /// Reuse capacity from a consumed batch. The buffer contains no live commands.
    /// Hosts may grow it for larger input; queued batches retain exclusive ownership.
    pub fn take_command_buffer(&mut self) -> Vec<Command> {
        self.world.command_buffers.pop().unwrap_or_default()
    }

    /// Return unused decode capacity without retaining command payloads or identities.
    pub fn recycle_command_buffer(&mut self, mut commands: Vec<Command>) {
        commands.clear();
        if (1..=RECYCLED_COMMAND_BUFFER_COMMANDS).contains(&commands.capacity())
            && self.world.command_buffers.len() < RECYCLED_COMMAND_BUFFERS
        {
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
mod entity_aliases;
mod entity_state;

use entity_aliases::EntityAliases;

mod runtime;
use crate::commands::metadata_bytes;

mod queries;

mod component_binding;

mod component_query;
