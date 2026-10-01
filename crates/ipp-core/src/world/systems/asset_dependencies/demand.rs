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

    #[cfg(test)]
    pub(in crate::world) fn authored_demand(&self) -> BTreeSet<AssetDemandSelection> {
        self.source_users.keys().cloned().collect()
    }
}

fn component_sources(
    world: &WorldSimulationState,
    authored: &WorldEntityState,
    entity: EntityId,
    component: u16,
) -> BTreeSet<AssetDemandSelection> {
    let mut demand = BTreeSet::new();
    let Some(state) = authored
        .entities
        .get(&entity)
        .and_then(|record| record.components.get(&component))
    else {
        return demand;
    };
    match &state.staged {
        Some(value) => value.resource_demand(&mut demand),
        None => world
            .components
            .resource_demand(component, entity.index() as usize, &mut demand),
    }
    demand
}

impl AssetDependencySystem {
    pub(super) fn update_authored_demand(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Result<(), ErrorReason> {
        let mut error = None;
        for &(entity, component) in &context.staged.operation_components {
            let mut demand = component_sources(
                context.world_data,
                &context.staged.entities_state,
                entity,
                component,
            );
            demand.retain(|selection| {
                let source =
                    AssetManagementService::scoped_selection(context.world_data.id, selection)
                        .descriptor();
                match context.assets.validate_reference(&source) {
                    Ok(()) => true,
                    Err(_) => {
                        error.get_or_insert(ErrorReason::InvalidAsset);
                        false
                    }
                }
            });
            self.state
                .replace_component_sources((entity, component), demand);
        }
        error.map_or(Ok(()), Err)
    }

    pub(super) fn restore_authored_demand(
        &mut self,
        runtime: &mut SystemRuntimeAccess<'_>,
    ) -> Result<(), String> {
        let mut sources = Vec::new();
        let mut demand = BTreeSet::new();
        for (&entity, record) in &runtime.world.state.entities {
            for &component in record.components.keys() {
                let component_demand =
                    component_sources(runtime.world, &runtime.world.state, entity, component);
                demand.extend(component_demand.iter().cloned());
                sources.push(((entity, component), component_demand));
            }
        }
        runtime
            .asset_acquisition
            .validate_users(runtime.world.id, demand)?;
        self.state.component_sources.clear();
        self.state.source_users.clear();
        for (key, demand) in sources {
            self.state.replace_component_sources(key, demand);
        }
        Ok(())
    }
}
