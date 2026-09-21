//! Host-only snapshot lifecycle; candidates remain private until fully validated.

use crate::services::world_serialization::{
    WorldLoadOptions, WorldPersistenceLimits, WorldSnapshot,
};
use crate::{HostRuntime, World, WorldConstructionError, WorldId, WorldLimits};

impl HostRuntime {
    /// Capture and encode one World at a mutation boundary without reading any assets.
    /// The returned bytes remain valid after subsequent mutation or World destruction.
    pub fn save_world(
        &mut self,
        id: WorldId,
        contract: u64,
        limits: WorldPersistenceLimits,
    ) -> Result<Vec<u8>, String> {
        let world = self.world_mut(id).ok_or("World does not exist")?;
        world.capture_world(limits)?.encode(contract, limits)
    }

    /// Deserialize into a new World and publish only after all typed data and references
    /// validate. Existing Worlds, names, sessions and asset consumers survive failure.
    pub fn load_world(
        &mut self,
        bytes: &[u8],
        contract: u64,
        options: WorldLoadOptions,
        budgets: WorldLimits,
        limits: WorldPersistenceLimits,
    ) -> Result<WorldId, String> {
        let mut snapshot = WorldSnapshot::decode(bytes, contract, limits)?;
        if let Some(symbol) = options.symbolic_id {
            snapshot.metadata.symbolic_id = symbol;
        }
        self.validate_new_world_symbol(&snapshot.metadata.symbolic_id, None)?;
        snapshot.capacity_hints = options
            .capacity_hints
            .apply(&snapshot.capacity_hints)
            .resolve(&snapshot.capacity_hints)
            .map_err(|error| error.to_string())?;
        let selected: Result<Vec<_>, _> = snapshot
            .capacity_hints
            .systems
            .keys()
            .map(|name| {
                self.system_factories
                    .ids()
                    .find(|id| id.0 == name)
                    .ok_or_else(|| format!("World requires unavailable system {name}"))
            })
            .collect();
        let factories = self
            .system_factories
            .select(&selected?)
            .map_err(|error| error.to_string())?;
        let id = self.next_world_id().map_err(|error| error.to_string())?;
        let result = World::construct(
            id,
            budgets,
            snapshot.capacity_hints.clone(),
            &factories,
            &mut self.assets,
            &mut self.data_sources,
        );
        let mut world = result.map_err(|error| {
            self.assets.release_world(id);
            error.to_string()
        })?;
        world.data.metadata = snapshot.metadata.clone();
        let restored = world
            .context(&mut self.assets, &mut self.data_sources)
            .restore_world_snapshot(&snapshot, limits);
        if let Err(error) = restored {
            world.teardown(&mut self.assets, &mut self.data_sources);
            self.assets.release_world(id);
            return Err(error);
        }
        self.publish_world(id, Ok::<_, WorldConstructionError>(world))
            .map_err(|error| error.to_string())
    }
}
