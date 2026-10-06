//! State a World owns: its simulation clock and ingress, entity records with
//! their component lifetimes, and the exclusive per-operation mutation state.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::components::registry;
use crate::world::component_state::observations::ComponentStagedWrite;
use crate::world::entities::{Allocator, EntityPersistentId, links::EntityLinkStore};
use crate::world::ingress::{Ingress, WorldLimits};
use crate::world::mutation::MutationMap;
use crate::world::removals::DeferredRemoval;
use crate::world::systems;
use crate::{
    Command, ComponentValue, EntityId, EntityMetadata, ErrorReason, WorldCapacityHints,
    WorldMetadata,
};

#[derive(Clone, Copy)]
pub(crate) struct ComponentStateInstance {
    pub(crate) incarnation: u64,
}

/// One present component: its incarnation and, while a batch touches it, the
/// batch's single transient staged copy. The copy moves to the commit's prepared
/// values once the batch commits, so retained storage stays the only store.
pub(crate) struct WorldComponentState {
    pub(in crate::world) instance: ComponentStateInstance,
    pub(in crate::world) staged: Option<Box<ComponentValue>>,
}

#[derive(Default)]
pub(crate) struct WorldEntityRecord {
    pub(in crate::world) persistent_id: EntityPersistentId,
    pub(in crate::world) metadata: EntityMetadata,
    pub(in crate::world) components: BTreeMap<u16, WorldComponentState>,
}

impl WorldEntityRecord {
    /// Incarnation of a present component.
    pub(in crate::world) fn input(&self, component: u16) -> Option<&ComponentStateInstance> {
        self.components.get(&component).map(|state| &state.instance)
    }
}

#[derive(Default)]
pub(crate) struct WorldEntityState {
    pub(in crate::world) links: EntityLinkStore,
    pub(crate) dirty: BTreeSet<(EntityId, u16)>,
    pub(in crate::world) operation_components: BTreeSet<(EntityId, u16)>,
    // Keys this operation touched through a path other than in-place staged
    // writes; their observation compares whole values.
    pub(in crate::world) operation_untracked: BTreeSet<(EntityId, u16)>,
    // In-place staged writes of this operation and whether each changed the value.
    pub(in crate::world) operation_writes: Vec<((EntityId, u16), ComponentStagedWrite, bool)>,
    pub(in crate::world) operation_created: BTreeSet<EntityId>,
    pub(in crate::world) operation_deleted: BTreeSet<EntityId>,
    // Whether this operation adopted an existing entity or component.
    pub(in crate::world) operation_adopted: bool,
    pub(in crate::world) lifecycle_effects: Vec<systems::lifecycle_publisher::LifecycleObservation>,
    pub(in crate::world) observed_components:
        BTreeMap<(EntityId, u16), (Option<u64>, Option<ComponentValue>)>,
    // In-place writes applied after a component's observed value (or retained
    // storage when none was taken), replayed only for a whole-value comparison.
    pub(in crate::world) observed_writes: BTreeMap<(EntityId, u16), Vec<ComponentStagedWrite>>,
    pub(in crate::world) changed: MutationMap<(EntityId, u16), Option<u64>>,
    pub(in crate::world) prepared: MutationMap<(EntityId, u16), ComponentValue>,
    // Staged copies moved into `prepared` once, at the next commit boundary.
    pub(in crate::world) deferred_preparation: BTreeSet<(EntityId, u16)>,
    // A sparse candidate for one existing-property commit. Cleared after publish;
    // the retained Vec is capacity only, not a second component value store.
    pub(in crate::world) evaluated_target: Option<(EntityId, u16)>,
    pub(in crate::world) evaluated_properties: Vec<(u32, crate::components::schema::FieldValue)>,
    pub(in crate::world) explicit_fields: BTreeSet<(EntityId, u16, u32)>,
    pub(in crate::world) allocator: Allocator,
    pub(in crate::world) retired_entities: Vec<EntityId>,
    // Applied effects are drained even when a later operation fails.
    pub(in crate::world) entity_effects: Vec<(&'static str, EntityId)>,
    pub(crate) entities: BTreeMap<EntityId, WorldEntityRecord>,
    pub(in crate::world) symbols: BTreeMap<String, EntityId>,
    pub(in crate::world) classes: BTreeMap<String, BTreeSet<EntityId>>,
    pub(in crate::world) next_incarnation: u64,
    pub(in crate::world) next_persistent_entity_id: u64,
}

/// Exclusive mutation state moved from ECS and participating systems, never cloned.
#[derive(Default)]
pub(crate) struct WorldMutationState {
    pub(in crate::world) entities_state: WorldEntityState,
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

/// ECS storage, ingress and simulation clock retained by a World.
pub struct WorldSimulationState {
    #[cfg(feature = "instrumentation")]
    pub(in crate::world) profile_context: usize,
    pub(in crate::world) updating: bool,
    pub(in crate::world) prepared_frame: bool,
    pub(in crate::world) accepting_removals: bool,
    pub(in crate::world) restoring: bool,
    pub(in crate::world) fault: Option<ErrorReason>,
    pub(in crate::world) deferred_removals: Vec<DeferredRemoval>,
    pub(in crate::world) deferred_removal_members: BTreeSet<DeferredRemoval>,
    pub(in crate::world) identity: usize,
    pub(crate) id: crate::WorldId,
    pub(crate) metadata: WorldMetadata,
    pub(in crate::world) limits: WorldLimits,
    pub(crate) capacity_hints: WorldCapacityHints,
    pub(crate) manifest: systems::WorldManifest,
    pub(in crate::world) state: WorldEntityState,
    pub(in crate::world) components: registry::ComponentStorage,
    pub(in crate::world) queue: VecDeque<Ingress>,
    pub(in crate::world) command_buffers: Vec<Vec<Command>>,
    pub(in crate::world) lifecycle_cleanup: Vec<(EntityId, ComponentValue)>,
    pub(in crate::world) tick: u64,
    pub(in crate::world) time: f64,
}
