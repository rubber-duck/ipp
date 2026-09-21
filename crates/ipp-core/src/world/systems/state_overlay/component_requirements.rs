//! Required defaults share Auto fallback ownership and ordinary commit invalidation.

use super::{StateOverlayEntry, StateOverlayMutationAccess};
use crate::{ComponentOverlayMode, ComponentValue, components::registry::ComponentStorage};
use std::collections::BTreeSet;

impl StateOverlayMutationAccess<'_> {
    pub(super) fn resolve_requirements(&mut self, components: &ComponentStorage) {
        let entities: BTreeSet<_> = self
            .staged
            .dirty
            .iter()
            .map(|&(entity, _)| entity)
            .collect();
        for entity in entities {
            let Some(record) = self.staged.entities.get(&entity) else {
                continue;
            };
            let mut required = BTreeSet::new();
            let mut pending = Vec::new();
            for (&component, layer) in &record.layers {
                let authored = layer.inputs.base.is_some();
                let automatic = layer.inputs.overlay_handles.iter().any(|&handle| {
                    matches!(self.state_overlays.registry.borrow(handle),
                        Some(StateOverlayEntry::Component(value))
                            if value.active && value.mode == ComponentOverlayMode::Auto)
                });
                if authored || automatic {
                    pending.extend_from_slice(ComponentValue::required_components(component));
                }
            }
            while let Some(component) = pending.pop() {
                if required.insert(component) {
                    pending.extend_from_slice(ComponentValue::required_components(component));
                }
            }
            let mut changes: Vec<_> = record
                .layers
                .iter()
                .filter_map(|(&component, layer)| {
                    let needed = required.remove(&component);
                    (needed != layer.inputs.required).then_some((component, needed))
                })
                .collect();
            changes.extend(required.into_iter().map(|component| (component, true)));
            for (component, needed) in changes {
                self.staged.stage_component(components, entity, component);
                self.staged.touch_component(entity, component);
                self.staged
                    .entities
                    .get_mut(&entity)
                    .expect("live dependent")
                    .layers
                    .entry(component)
                    .or_default()
                    .inputs
                    .required = needed;
            }
        }
    }
}
