//! Private graph reconstruction followed by a non-fallible publication boundary.

use crate::host::attachments::topology::HostTopology;
use crate::host::*;
use crate::services::world_serialization::*;
use crate::{EntityId, EntityPersistentId, components::schema::FieldValue};
use std::collections::BTreeSet;

type EntityMap = BTreeMap<EntityPersistentId, EntityId>;

impl HostRuntime {
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

    fn restore_world_graph(
        &mut self,
        graph: WorldGraphSnapshot,
        budgets: WorldLimits,
        limits: WorldPersistenceLimits,
    ) -> Result<WorldGraphLoadResult, WorldPersistenceError> {
        let mut candidates = BTreeMap::new();
        let mut topology = HostTopology::default();
        topology.identity = self.topology.identity;
        let result =
            self.prepare_world_graph(&graph, budgets, limits, &mut candidates, &mut topology);
        let result = result.and_then(|created| {
            self.topology
                .adopt_restored(topology)
                .map_err(|error| error.to_string())?;
            Ok(created)
        });

        match result {
            Ok(created) => {
                for (_, world) in candidates {
                    self.worlds.insert(world.runtime_ref().id(), world);
                }

                Ok(WorldGraphLoadResult {
                    root: created[&graph.root],
                    created,
                })
            }
            Err(error) => {
                for (_, mut world) in candidates {
                    let id = world.runtime_ref().id();
                    world.teardown(&mut self.assets, &mut self.io, &mut self.data);
                    self.assets.release_world(id);
                }

                Err(error.into())
            }
        }
    }

    fn prepare_world_graph(
        &mut self,
        graph: &WorldGraphSnapshot,
        budgets: WorldLimits,
        limits: WorldPersistenceLimits,
        candidates: &mut BTreeMap<WorldGraphNodeId, World>,
        topology: &mut HostTopology,
    ) -> Result<BTreeMap<WorldGraphNodeId, WorldRef>, String> {
        let mut created = BTreeMap::new();
        for node in &graph.nodes {
            let selected: Result<Vec<_>, _> = node
                .world
                .selected_systems
                .iter()
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
            self.next_world = id.0;
            let mut world = World::construct(
                crate::world::WorldConstructionIdentity {
                    id,
                    #[cfg(feature = "instrumentation")]
                    host: self.identity(),
                },
                budgets,
                node.world.capacity_hints.clone(),
                &factories,
                &mut self.assets,
                &mut self.io,
                &mut self.data,
            )
            .map_err(|error| {
                self.assets.release_world(id);
                error.to_string()
            })?;
            world.data.metadata = node.world.metadata.clone();
            let reference = world.runtime_ref();
            topology.worlds.insert(id, reference);
            created.insert(node.id, reference);
            candidates.insert(node.id, world);
        }

        let mut entities: BTreeMap<WorldGraphNodeId, EntityMap> = BTreeMap::new();
        let mut deferred = BTreeMap::new();
        for node in &graph.nodes {
            let world = candidates.get_mut(&node.id).expect("private World");
            let ids = world
                .context(&mut self.assets, &mut self.io, &mut self.data, topology)
                .restore_world_entities(&node.world)?;
            entities.insert(node.id, ids);
            deferred.insert(
                node.id,
                node.references
                    .iter()
                    .map(|reference| (reference.entity, reference.component))
                    .collect::<BTreeSet<_>>(),
            );
        }

        for node in &graph.nodes {
            candidates
                .get_mut(&node.id)
                .expect("private World")
                .context(&mut self.assets, &mut self.io, &mut self.data, topology)
                .restore_world_components(&node.world, &entities[&node.id], &deferred[&node.id])?;
        }

        let mut outputs = BTreeMap::new();
        for node in &graph.nodes {
            for reference in &node.references {
                if let WorldSerializedReferenceValue::Output(output) = &reference.value {
                    let world = candidates
                        .get_mut(&output.world)
                        .expect("output World")
                        .context(&mut self.assets, &mut self.io, &mut self.data, topology);
                    let selection = match output.entity {
                        Some(entity) => {
                            world.bind_output(entities[&output.world][&entity], output.kind)
                        }
                        None => world.bind_output_target(crate::OutputTarget::Canvas),
                    }
                    .map_err(|error| error.to_string())?;
                    outputs.insert((output.world, output.entity, output.kind), selection);
                }
            }
        }

        for node in &graph.nodes {
            let mut fields: BTreeMap<_, Vec<_>> = BTreeMap::new();
            for reference in &node.references {
                fields
                    .entry((reference.entity, reference.component))
                    .or_default()
                    .push(reference);
            }

            for entity in &node.world.entities {
                for component in &entity.components {
                    let Some(references) = fields.get(&(entity.persistent_id, component.type_id()))
                    else {
                        continue;
                    };
                    let mut value = component.clone();
                    for reference in references {
                        let resolved = match &reference.value {
                            WorldSerializedReferenceValue::World(target) => {
                                FieldValue::World(Some(created[target]))
                            }
                            WorldSerializedReferenceValue::Output(output) => FieldValue::Output(
                                Some(outputs[&(output.world, output.entity, output.kind)]),
                            ),
                        };
                        value
                            .set_field(reference.field, resolved)
                            .map_err(|error| format!("Invalid graph reference: {error:?}"))?;
                    }
                    candidates
                        .get_mut(&node.id)
                        .expect("private World")
                        .context(&mut self.assets, &mut self.io, &mut self.data, topology)
                        .restore_world_component(
                            entities[&node.id][&entity.persistent_id],
                            value,
                            &entities[&node.id],
                        )?;
                }
            }

            candidates
                .get_mut(&node.id)
                .expect("private World")
                .context(&mut self.assets, &mut self.io, &mut self.data, topology)
                .finish_restored_components()?;
        }

        for node in &graph.nodes {
            candidates
                .get_mut(&node.id)
                .expect("private World")
                .context(&mut self.assets, &mut self.io, &mut self.data, topology)
                .restore_world_systems(&node.world, &entities[&node.id], limits)?;
        }

        for (&(node, _, _), &output) in &outputs {
            if !candidates[&node].output_valid(output) {
                return Err("Restored output became unavailable during System restoration".into());
            }
        }

        Ok(created)
    }
}
