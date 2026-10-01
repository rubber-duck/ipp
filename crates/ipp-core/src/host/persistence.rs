//! Host-only snapshot lifecycle; no candidate World is published before graph validation.

use crate::services::world_serialization::{
    WorldGraphLoadResult, WorldGraphSnapshot, WorldLoadOptions, WorldPersistenceError,
    WorldPersistenceLimits,
};
use crate::{HostRuntime, WorldId, WorldLimits};

impl HostRuntime {
    /// Capture the complete authored descendant graph at one applied-state boundary.
    pub fn save_world(
        &mut self,
        id: WorldId,
        contract: u64,
        limits: WorldPersistenceLimits,
    ) -> Result<Vec<u8>, WorldPersistenceError> {
        self.capture_world_graph(id, limits)?
            .encode(contract, limits)
            .map_err(Into::into)
    }

    /// Restore fresh independently living Worlds. Failure publishes none of the graph.
    /// The result acknowledges every created World; dropping it has no lifecycle effect.
    pub fn load_world(
        &mut self,
        bytes: &[u8],
        contract: u64,
        options: WorldLoadOptions,
        budgets: WorldLimits,
        limits: WorldPersistenceLimits,
    ) -> Result<WorldGraphLoadResult, WorldPersistenceError> {
        let mut graph = WorldGraphSnapshot::decode(bytes, contract, limits)?;
        graph.rename(&options)?;
        for node in &mut graph.nodes {
            self.validate_new_world_symbol(&node.world.metadata.symbolic_id, None)?;
            if node.id == graph.root {
                node.world.capacity_hints = options
                    .capacity_hints
                    .apply(&node.world.capacity_hints)
                    .resolve(&node.world.capacity_hints)
                    .map_err(|error| error.to_string())?;
            }
        }
        self.restore_world_graph(graph, budgets, limits)
    }
}
