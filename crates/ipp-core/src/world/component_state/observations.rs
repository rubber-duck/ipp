//! Lifecycle observations derived from staged component changes.
//!
//! Effect recording and per-operation component lifecycle classification.
//! Ownership lives in [`super::super`]; this module only hosts observation.
//!
//! Each operation compares a component's effective value with the value observed
//! after the previous operation. When an operation only wrote an unlayered
//! producer in place, each write already reported whether it changed the value;
//! the writes are logged and replayed onto the observed value only if a later
//! operation of the same batch needs a whole-value comparison.

use super::super::*;

/// One in-place producer write on a component whose staged producer is its
/// effective input. Replays reproduce it on an identical observed value.
pub(in crate::world) enum ComponentStagedWrite {
    Field(FieldWrite),
    SetProperty(String, crate::DynamicValue),
    RemoveProperty(String),
}

impl ComponentStagedWrite {
    fn replay(&self, value: &mut ComponentValue) {
        match self {
            Self::Field(write) => {
                let replayed = registry::replay_field(value, write);
                debug_assert!(replayed.is_ok(), "observed value follows staged writes");
            }
            Self::SetProperty(name, property) => {
                if let Some(properties) = value.dynamic_properties_mut() {
                    let _ = properties.set(name, property.clone());
                }
            }
            Self::RemoveProperty(name) => {
                if let Some(properties) = value.dynamic_properties_mut() {
                    properties.remove(name);
                }
            }
        }
    }
}

impl WorldMutationState {
    #[cfg(feature = "diagnostics")]
    pub(in crate::world) fn record_entity_effect(&mut self, event: &'static str, entity: EntityId) {
        if crate::diagnostics::enabled(crate::diagnostics::Level::Debug) {
            self.entities_state.entity_effects.push((event, entity));
        }
    }

    pub(in crate::world) fn record_component_observations(
        &mut self,
        components: &registry::ComponentStorage,
    ) {
        use systems::lifecycle_publisher::{ComponentLifecycleKind, LifecycleObservation};
        let operation_components = std::mem::take(&mut self.entities_state.operation_components);
        for &(entity, component) in &operation_components {
            let original_incarnation = self.entities_state.changed[&(entity, component)];
            if !self
                .entities_state
                .operation_untracked
                .contains(&(entity, component))
                && let Some((incarnation, changed)) =
                    self.observe_staged_writes((entity, component), original_incarnation)
            {
                if changed {
                    self.entities_state
                        .lifecycle_effects
                        .push(LifecycleObservation::Component {
                            entity,
                            component,
                            kind: ComponentLifecycleKind::Updated,
                            previous_incarnation: incarnation,
                            incarnation,
                        });
                }
                continue;
            }
            let mut previous = self
                .entities_state
                .observed_components
                .remove(&(entity, component))
                .unwrap_or_else(|| {
                    (
                        original_incarnation,
                        components.get(component, entity.index() as usize),
                    )
                });
            if let Some(writes) = self
                .entities_state
                .observed_writes
                .remove(&(entity, component))
                && let Some(value) = previous.1.as_mut()
            {
                for write in &writes {
                    write.replay(value);
                }
            }
            let incarnation = self
                .entities_state
                .entities
                .get(&entity)
                .and_then(|record| record.input(component))
                .map(|input| input.incarnation);
            let value = incarnation.and_then(|_| {
                self.entities_state
                    .prepared
                    .get(&(entity, component))
                    .cloned()
                    .or_else(|| {
                        self.entities_state
                            .input_value(components, entity, component)
                    })
            });
            let kind = match (previous.0, incarnation) {
                (None, Some(_)) => Some(ComponentLifecycleKind::Inserted),
                (Some(_), None) => Some(ComponentLifecycleKind::Removed),
                (Some(before), Some(after)) if before != after => {
                    Some(ComponentLifecycleKind::Replaced)
                }
                (Some(_), Some(_)) if previous.1 != value => Some(ComponentLifecycleKind::Updated),
                _ => None,
            };
            if let Some(kind) = kind {
                self.entities_state
                    .lifecycle_effects
                    .push(LifecycleObservation::Component {
                        entity,
                        component,
                        kind,
                        previous_incarnation: previous.0,
                        incarnation,
                    });
            }
            self.entities_state
                .observed_components
                .insert((entity, component), (incarnation, value));
        }
        self.entities_state.operation_components = operation_components;
    }

    /// Classify an operation that only wrote an unlayered producer in place from
    /// the change each write reported, and log the writes for a later whole-value
    /// comparison. `None` selects whole-value comparison.
    fn observe_staged_writes(
        &mut self,
        key: (EntityId, u16),
        original_incarnation: Option<u64>,
    ) -> Option<(Option<u64>, bool)> {
        let state = &mut self.entities_state;
        let layer = state.entities.get(&key.0)?.layers.get(&key.1)?;
        let incarnation = layer.input().map(|input| input.incarnation);
        let previous = state
            .observed_components
            .get(&key)
            .map_or(original_incarnation, |(incarnation, _)| *incarnation);

        if incarnation.is_none()
            || previous != incarnation
            || !layer.inputs.stages_producer_directly()
        {
            return None;
        }

        let mut changed = false;
        let log = state.observed_writes.entry(key).or_default();
        for (_, write, write_changed) in state
            .operation_writes
            .extract_if(.., |(write, _, _)| *write == key)
        {
            changed |= write_changed;
            log.push(write);
        }

        Some((incarnation, changed))
    }
}
