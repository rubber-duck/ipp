//! Lifecycle observations derived from staged component changes.
//!
//! Effect recording and per-operation component lifecycle classification.
//! Ownership lives in [`super::super`]; this module only hosts observation.

use super::super::*;

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
        for &(entity, component) in &self.entities_state.operation_components {
            let original_incarnation = self.entities_state.changed[&(entity, component)];
            let previous = self
                .entities_state
                .observed_components
                .get(&(entity, component))
                .cloned()
                .unwrap_or_else(|| {
                    (
                        original_incarnation,
                        components.get(component, entity.index() as usize),
                    )
                });
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
    }
}
