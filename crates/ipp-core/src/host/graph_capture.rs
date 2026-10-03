//! One exclusive applied-state graph cut, never a publication or asset snapshot.

use super::*;
use crate::services::world_serialization::*;
use std::collections::BTreeSet;

impl HostRuntime {
    /// Detach an owned serializable graph without acquiring assets or selecting presentation.
    pub fn capture_world_graph(
        &mut self,
        root: WorldId,
        limits: WorldPersistenceLimits,
    ) -> Result<WorldGraphSnapshot, WorldPersistenceError> {
        let root_ref = self
            .worlds
            .get(&root)
            .ok_or("World does not exist")?
            .runtime_ref();
        let mut pending = vec![root_ref];
        let mut visited = BTreeSet::new();
        let mut order = Vec::new();
        while let Some(reference) = pending.pop() {
            if !visited.insert(reference) {
                continue;
            }
            if visited.len() > limits.max_bytes / 512 {
                return Err("Snapshot graph byte budget exhausted".into());
            }
            let world = self
                .worlds
                .get_mut(&reference.id())
                .ok_or("Missing attached World")?;
            if world.runtime_ref() != reference {
                return Err("Stale attached World".into());
            }
            let children = world
                .context(
                    &mut self.assets,
                    &mut self.io,
                    &mut self.data,
                    &mut self.topology,
                )
                .snapshot_children(limits.max_bytes)?;
            order.push(reference);
            pending.extend(children.into_iter().rev());
        }

        let mut references = WorldCaptureReferences {
            worlds: BTreeMap::new(),
            outputs: BTreeMap::new(),
        };
        for (index, world) in order.iter().enumerate() {
            references.worlds.insert(
                *world,
                WorldGraphNodeId(u32::try_from(index).map_err(|_| "Too many graph Worlds")?),
            );
        }

        for reference in &order {
            let world = self
                .worlds
                .get_mut(&reference.id())
                .expect("captured World");
            for (output, entity) in world
                .context(
                    &mut self.assets,
                    &mut self.io,
                    &mut self.data,
                    &mut self.topology,
                )
                .snapshot_outputs(limits.max_bytes)?
            {
                if references.outputs.len() >= limits.max_bytes / 128 {
                    return Err("Snapshot output identity budget exhausted".into());
                }
                references.outputs.insert(
                    output,
                    WorldSerializedOutput {
                        world: references.worlds[reference],
                        kind: output.kind(),
                        entity,
                    },
                );
            }
        }

        let mut nodes = Vec::new();
        let mut bytes = 0;
        for reference in &order {
            let mut fields = Vec::new();
            let world = self
                .worlds
                .get_mut(&reference.id())
                .expect("captured World")
                .context(
                    &mut self.assets,
                    &mut self.io,
                    &mut self.data,
                    &mut self.topology,
                )
                .capture_world_with_references(&references, &mut fields, &mut bytes, limits)?;
            nodes.push(WorldGraphNode {
                id: references.worlds[reference],
                world,
                references: fields,
            });
        }

        let graph = WorldGraphSnapshot {
            root: references.worlds[&root_ref],
            nodes,
        };
        graph.validate_budget(limits)?;
        graph.validate_references()?;
        Ok(graph)
    }
}
