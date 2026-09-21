//! Authoring references stay indexed by the owning System across partial mutations.

use super::*;

impl AssetDependencySystemState {
    fn replace_component_sources(
        &mut self,
        key: (EntityId, u16),
        demand: BTreeSet<AssetDemandSelection>,
    ) {
        if self.component_sources.get(&key) == Some(&demand) {
            return;
        }
        if let Some(previous) = self.component_sources.remove(&key) {
            for source in previous {
                self.changed_sources.insert(source.clone());
                let count = self.source_users.get_mut(&source).expect("indexed source");
                *count -= 1;
                if *count == 0 {
                    self.source_users.remove(&source);
                }
            }
        }
        if !demand.is_empty() {
            for source in &demand {
                self.changed_sources.insert(source.clone());
                *self.source_users.entry(source.clone()).or_default() += 1;
            }
            self.component_sources.insert(key, demand);
        }
    }

    pub(in crate::world) fn authored_demand(&self) -> BTreeSet<AssetDemandSelection> {
        self.source_users.keys().cloned().collect()
    }
}

fn component_sources(
    world: &WorldSimulationState,
    authored: &WorldEntityState,
    entity: EntityId,
    component: u16,
    overlay: Option<&crate::systems::state_overlay::StateOverlaySystem>,
) -> BTreeSet<AssetDemandSelection> {
    let mut demand = BTreeSet::new();
    let Some(layer) = authored
        .entities
        .get(&entity)
        .and_then(|record| record.layers.get(&component))
    else {
        return demand;
    };
    for input in layer.inputs.retained_inputs() {
        input.resource_demand(&mut demand);
    }
    if layer.inputs.input_value().is_none() && layer.input().is_some() {
        world
            .components
            .resource_demand(component, entity.index() as usize, &mut demand);
    }
    if let Some(overlay) = overlay {
        overlay.component_resource_demand(layer, &mut demand);
    }
    demand
}

impl AssetDependencySystem {
    pub(super) fn update_authored_demand(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Result<(), ErrorReason> {
        let overlay = context.dependency(self.bindings.get().overlay);
        let mut changed_demand = BTreeSet::new();
        for &(entity, component) in &context.staged.operation_components {
            let demand = component_sources(
                context.world_data,
                &context.staged.entities_state,
                entity,
                component,
                overlay,
            );
            changed_demand.extend(demand.iter().cloned());
            self.state
                .replace_component_sources((entity, component), demand);
        }
        context
            .assets
            .validate_users(context.world_data.id, changed_demand)
            .map_err(|_| ErrorReason::Capacity)?;
        Ok(())
    }

    pub(super) fn restore_authored_demand(
        &mut self,
        runtime: &mut SystemRuntimeAccess<'_>,
    ) -> Result<(), String> {
        self.state.component_sources.clear();
        self.state.source_users.clear();
        for (&entity, record) in &runtime.world.state.entities {
            for &component in record.layers.keys() {
                let demand =
                    component_sources(runtime.world, &runtime.world.state, entity, component, None);
                self.state
                    .replace_component_sources((entity, component), demand);
            }
        }
        let demand = self.state.authored_demand();
        runtime
            .asset_acquisition
            .validate_users(runtime.world.id, demand)
    }
}
