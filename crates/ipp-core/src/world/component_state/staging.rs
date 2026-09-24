//! Staging of affected inputs before command application.
//!
//! Hydration, dirty tracking and effective-value preparation. Ownership lives
//! in [`super::super`]; this module only hosts the staging phase.
//!
//! An affected component is hydrated once per batch and later operations write
//! its staged producer in place. Preparation is a plain copy: each affected
//! component's effective value is copied into the commit's `prepared` map once,
//! when the batch commits, and installed into stable storage from there. Until
//! then readers use the staged input, which has the same content. Preparation
//! never fails and owns no activation resources; ingress validation is
//! field-local.

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

    /// Defer every component this operation affected to the commit copy.
    /// Preparation cannot fail; the parameters and result follow the operation
    /// pipeline in [`crate::world::mutation`].
    pub(in crate::world) fn prepare_changes(
        &mut self,
        _components: &registry::ComponentStorage,
        _limits: WorldLimits,
    ) -> Result<(), ErrorReason> {
        for key in std::mem::take(&mut self.entities_state.dirty) {
            self.entities_state.prepared.remove(&key);
            let active = self
                .entities_state
                .entities
                .get(&key.0)
                .and_then(|record| record.layers.get(&key.1))
                .is_some_and(|layer| layer.inputs.input_value().is_some());
            if active {
                self.entities_state.deferred_preparation.insert(key);
            } else {
                self.entities_state.deferred_preparation.remove(&key);
            }
        }
        Ok(())
    }

    /// Make the effective copies deferred by earlier operations, once per commit.
    pub(in crate::world) fn prepare_deferred_components(&mut self) {
        for key in std::mem::take(&mut self.entities_state.deferred_preparation) {
            if let Some(input) = self
                .entities_state
                .entities
                .get(&key.0)
                .and_then(|record| record.layers.get(&key.1))
                .and_then(|layer| layer.inputs.input_value())
            {
                let value = input.clone();

                // Test-only oracle: ingress validation must already have made
                // every committed value whole-valid.
                #[cfg(feature = "checked-invariants")]
                if let Err(reason) = value.validate_lifecycle() {
                    panic!(
                        "checked invariant: component {} of entity {} fails whole validation ({reason})",
                        key.1,
                        key.0.to_bits()
                    );
                }

                self.entities_state.prepared.insert(key, value);
            }
        }
    }
}
