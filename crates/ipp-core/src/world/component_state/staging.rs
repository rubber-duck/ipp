//! Staging of affected inputs before command application.
//!
//! Hydration, dirty tracking and effective-value preparation. Ownership lives
//! in [`super::super`]; this module only hosts the staging phase.
//!
//! An affected component is hydrated once per batch and later operations write
//! its staged producer in place. Components whose effective preparation is a
//! plain copy make that copy once, when the batch commits; components with
//! activation resources still prepare after every operation so a failure stops
//! the batch at the operation that caused it.

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
        // Hydration materializes the retained value without changing it.
        if hydrated {
            self.mark_component(entity, component);
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
    /// Record that this operation may change the component's effective value in
    /// any way; its observation compares the whole value.
    pub(in crate::world) fn touch_component(&mut self, id: EntityId, component: u16) {
        self.entities_state
            .operation_untracked
            .insert((id, component));
        self.mark_component(id, component);
    }

    /// Record an in-place producer write whose complete effect is `write`, and
    /// whether it changed the component's value.
    pub(in crate::world) fn touch_component_write(
        &mut self,
        id: EntityId,
        component: u16,
        write: super::observations::ComponentStagedWrite,
        changed: bool,
    ) {
        self.entities_state
            .operation_writes
            .push(((id, component), write, changed));
        self.mark_component(id, component);
    }

    fn mark_component(&mut self, id: EntityId, component: u16) {
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
        for key in std::mem::take(&mut self.entities_state.dirty) {
            if let Some(previous) = self.entities_state.prepared.remove(&key) {
                self.entities_state.prepared_bytes -= previous.activation_bytes();
            }
            let Some(input) = self
                .entities_state
                .entities
                .get(&key.0)
                .and_then(|record| record.layers.get(&key.1))
                .and_then(|layer| layer.inputs.input_value())
            else {
                self.entities_state.deferred_preparation.remove(&key);
                continue;
            };
            if input.defers_preparation() {
                // Readers until commit use the staged input, which has the same
                // content as its copied effective value.
                #[cfg(debug_assertions)]
                if let Err(reason) = input.validate_lifecycle() {
                    self.deactivate_input(key);
                    result = result.and(Err(reason));
                    continue;
                }
                self.entities_state.deferred_preparation.insert(key);
            } else {
                self.entities_state.deferred_preparation.remove(&key);
                result = result.and(self.prepare_component(key));
            }
        }
        result
    }

    /// Make the effective copies deferred by earlier operations, once per commit.
    pub(in crate::world) fn prepare_deferred_components(&mut self) -> Result<(), ErrorReason> {
        let mut result = Ok(());
        for key in std::mem::take(&mut self.entities_state.deferred_preparation) {
            result = result.and(self.prepare_component(key));
        }
        result
    }

    fn prepare_component(&mut self, key: (EntityId, u16)) -> Result<(), ErrorReason> {
        let remaining = self
            .entities_state
            .activation_budget
            .saturating_sub(self.entities_state.prepared_bytes);
        let Some(input) = self
            .entities_state
            .entities
            .get(&key.0)
            .and_then(|record| record.layers.get(&key.1))
            .and_then(|layer| layer.inputs.input_value())
        else {
            return Ok(());
        };
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
                self.entities_state.prepared.insert(key, value);
                Ok(())
            }
            Err(reason) => {
                // Failed activation has no rollback value. Drop the effective
                // incarnation; a later producer edit may activate it again.
                self.deactivate_input(key);
                Err(reason)
            }
        }
    }

    fn deactivate_input(&mut self, key: (EntityId, u16)) {
        if let Some(layer) = self
            .entities_state
            .entities
            .get_mut(&key.0)
            .and_then(|record| record.layers.get_mut(&key.1))
        {
            layer.inputs.deactivate();
        }
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
