//! Detached graph identities and typed references, independent of runtime handle lifetimes.

use super::{WorldLoadOptions, WorldPersistenceLimits, WorldSnapshot};
use crate::{EntityPersistentId, OutputKind, WorldMetadata, WorldRef};
use std::collections::{BTreeMap, BTreeSet};

/// Identity scoped to one complete snapshot, never a durable World identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldGraphNodeId(pub u32);

/// An output whose runtime lifetime is reconstructed on private load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldSerializedOutput {
    /// Target instance within this graph, independent of durable metadata.
    pub world: WorldGraphNodeId,
    /// Explicit output kind, never inferred from available components.
    pub kind: OutputKind,
    /// Durable Camera entity identity within the target instance; absent for
    /// the World's canvas.
    pub entity: Option<EntityPersistentId>,
}

/// Non-null graph references are stored here, not in forged runtime handles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldSerializedReferenceValue {
    /// An ordinary typed World field.
    World(WorldGraphNodeId),
    /// An ordinary typed output field.
    Output(WorldSerializedOutput),
}

/// One ordinary component field. The corresponding component value contains a null placeholder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldSerializedReference {
    /// Durable identity of the component's containing entity.
    pub entity: EntityPersistentId,
    /// Compiled component type.
    pub component: u16,
    /// Target-correct schema field offset.
    pub field: u32,
    /// Owned durable graph reference.
    pub value: WorldSerializedReferenceValue,
}

/// One independently living World in a captured graph.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldGraphNode {
    /// Instance identity local to this container.
    pub id: WorldGraphNodeId,
    /// Local authored state and System contributions.
    pub world: WorldSnapshot,
    /// Sparse replacements for non-null composition fields in the captured components.
    pub references: Vec<WorldSerializedReference>,
}

/// Complete authored graph cut. Root presentation, receipts and resource bytes are absent.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldGraphSnapshot {
    /// Originating saved World, without its Host presentation binding.
    pub root: WorldGraphNodeId,
    /// Complete serializable descendant set, with explicit identities independent of order.
    pub nodes: Vec<WorldGraphNode>,
}

/// Discoverable identity and name for one node before choosing rename overrides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldGraphNodeDescriptor {
    /// Identity used by load name replacements.
    pub id: WorldGraphNodeId,
    /// Saved name and durable metadata, neither of which grants runtime identity.
    pub metadata: WorldMetadata,
}

/// Bounded inspection result, not a reservation or a promise that loading will succeed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldGraphDescriptor {
    /// Saved graph root.
    pub root: WorldGraphNodeId,
    /// Every saved World, including copies with equal durable metadata.
    pub nodes: Vec<WorldGraphNodeDescriptor>,
}

/// Acknowledges every newly published World. Dropping this result destroys nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldGraphLoadResult {
    /// Fresh root lifetime, with no automatically selected presentation.
    pub root: WorldRef,
    /// Complete created scope for caller journaling and explicit exact-lifetime cleanup.
    pub created: BTreeMap<WorldGraphNodeId, WorldRef>,
}

/// Graph capture, encoding or loading failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldPersistenceError {
    /// Invalid input, unsupported state, collision or capacity failure; no graph was published.
    Invalid(String),
}

impl std::fmt::Display for WorldPersistenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(error) => formatter.write_str(error),
        }
    }
}

impl std::error::Error for WorldPersistenceError {}

impl From<String> for WorldPersistenceError {
    fn from(error: String) -> Self {
        Self::Invalid(error)
    }
}

impl From<&str> for WorldPersistenceError {
    fn from(error: &str) -> Self {
        Self::Invalid(error.into())
    }
}

/// Validate the complete bounded container without constructing Worlds or accessing assets.
pub fn inspect_world_graph(
    bytes: &[u8],
    contract: u64,
    limits: WorldPersistenceLimits,
) -> Result<WorldGraphDescriptor, WorldPersistenceError> {
    let graph = WorldGraphSnapshot::decode(bytes, contract, limits)?;
    Ok(WorldGraphDescriptor {
        root: graph.root,
        nodes: graph
            .nodes
            .into_iter()
            .map(|node| WorldGraphNodeDescriptor {
                id: node.id,
                metadata: node.world.metadata,
            })
            .collect(),
    })
}

impl WorldGraphSnapshot {
    pub(crate) fn rename(&mut self, options: &WorldLoadOptions) -> Result<(), String> {
        let ids: BTreeSet<_> = self.nodes.iter().map(|node| node.id).collect();
        if options.world_names.keys().any(|id| !ids.contains(id)) {
            return Err("Name replacement references an unknown graph node".into());
        }
        if let (Some(root), Some(mapped)) =
            (&options.symbolic_id, options.world_names.get(&self.root))
            && root != mapped
        {
            return Err("Conflicting root name replacements".into());
        }
        let mut names = BTreeSet::new();
        for node in &mut self.nodes {
            if let Some(name) = options.world_names.get(&node.id).or_else(|| {
                (node.id == self.root)
                    .then_some(options.symbolic_id.as_ref())
                    .flatten()
            }) {
                node.world.metadata.symbolic_id = name.clone();
            }
            crate::world::validate_world_symbolic_id(&node.world.metadata.symbolic_id)?;
            if !names.insert(node.world.metadata.symbolic_id.as_str()) {
                return Err("Duplicate graph World name".into());
            }
        }
        Ok(())
    }
}
