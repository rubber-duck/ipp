//! Staging of affected inputs before command application.
//!
//! Hydration, dirty tracking and effective-value preparation. Ownership lives
//! in [`super::super`]; this module only hosts the staging phase.

use super::super::*;

impl WorldMutationState {
    /// Hydrate only the affected value needed for lifecycle activation.
    pub(in crate::world) fn stage_component(
        &mut self,
        components: &registry::ComponentStorage,
        entity: EntityId,
        component: u16,
    ) {
        let hydrated = if let Some(layer) = self
            .entities
            .get_mut(&entity)
            .and_then(|record| record.layers.get_mut(&component))
            && layer.inputs.input_value().is_none()
        {
            layer
                .inputs
                .stage(components.get(component, entity.index() as usize));
            true
        } else {
            false
        };
        if hydrated {
            self.touch_component(entity, component);
        }
    }

    pub(in crate::world) fn stage_command_inputs(
        &mut self,
        components: &registry::ComponentStorage,
        command: &Command,
        aliases: &BTreeMap<u32, EntityId>,
    ) -> Result<(), ErrorReason> {
        match command {
            Command::InsertComponent {
                entity,
                component,
                ..
            }
            | Command::SetField {
                entity,
                component,
                ..
            }
            | Command::SetDynamicProperty {
                entity,
                component,
                ..
            }
            | Command::RemoveDynamicProperty {
                entity,
                component,
                ..
            }
            | Command::RemoveComponent {
                entity,
                component,
            } => {
                let entity = self.resolve(*entity, aliases)?;
                self.stage_component(components, entity, *component);
            }
            Command::InsertComponentValue {
                entity,
                value,
            } => {
                let entity = self.resolve(*entity, aliases)?;
                self.stage_component(components, entity, value.type_id());
            }
            _ => {}
        }
        Ok(())
    }
}

impl WorldMutationState {
    pub(in crate::world) fn touch_component(&mut self, id: EntityId, component: u16) {
        self.entities_state.dirty.insert((id, component));
        self.entities_state
            .operation_components
            .insert((id, component));
        let incarnation = self
            .entities_state
            .entities
            .get(&id)
            .and_then(|record| record.input(component))
            .map(|input| input.incarnation);
        self.entities_state
            .changed
            .insert_if_absent((id, component), incarnation);
    }

    pub(in crate::world) fn prepare_components(&mut self) -> Result<(), ErrorReason> {
        let mut result = Ok(());
        for (id, component) in std::mem::take(&mut self.entities_state.dirty) {
            if let Some(previous) = self.entities_state.prepared.remove(&(id, component)) {
                self.entities_state.prepared_bytes -= previous.activation_bytes();
            }
            let remaining = self
                .entities_state
                .activation_budget
                .saturating_sub(self.entities_state.prepared_bytes);
            if let Some(layer) = self
                .entities_state
                .entities
                .get_mut(&id)
                .and_then(|record| record.layers.get_mut(&component))
                && let Some(input) = layer.inputs.input_value()
            {
                #[cfg(debug_assertions)]
                let prepared = input
                    .validate_lifecycle()
                    .and_then(|()| input.prepare_effective(remaining));
                #[cfg(not(debug_assertions))]
                let prepared = input.prepare_effective(remaining);
                match prepared {
                    Ok(value) => {
                        let allocation = value.activation_bytes();
                        debug_assert!(allocation <= remaining);
                        self.entities_state.prepared_bytes += allocation;
                        self.entities_state.prepared.insert((id, component), value);
                    }
                    Err(reason) => {
                        // Failed activation has no rollback value. Drop the effective
                        // incarnation; a later producer edit may activate it again.
                        layer.inputs.deactivate();
                        result = result.and(Err(reason));
                    }
                }
            }
        }
        result
    }

    pub(in crate::world) fn prepare_changes(
        &mut self,
        _components: &registry::ComponentStorage,
        limits: WorldLimits,
    ) -> Result<(), ErrorReason> {
        self.entities_state.activation_budget = limits.max_staging_bytes;

        self.prepare_components()
    }
}
