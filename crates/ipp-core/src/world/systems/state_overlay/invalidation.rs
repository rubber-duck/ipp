use super::StateOverlayEntry;
use super::registry::{ComponentStateOverlay, EntityOverlayBinding};
use crate::{
    ComponentOverlayMode, EntityId, StateOverlayLifecycleDiagnostic, StateOverlayLifecycleReason,
};

impl super::StateOverlayMutationAccess<'_> {
    pub(in crate::world) fn invalidate_overlay_bindings(&mut self, target: EntityId) {
        let resources: Vec<_> = self
            .state_overlays
            .registry
            .iter()
            .filter(|(_, resource)| match resource {
                StateOverlayEntry::EntityBinding(EntityOverlayBinding {
                    entity,
                    ..
                })
                | StateOverlayEntry::Component(ComponentStateOverlay {
                    entity,
                    ..
                }) => *entity == target,
                _ => false,
            })
            .map(|(id, resource)| (id, resource.clone()))
            .collect();
        for (id, mut resource) in resources {
            let (owner, component) = match &mut resource {
                StateOverlayEntry::EntityBinding(EntityOverlayBinding {
                    owner,
                    entity,
                    active,
                    ..
                }) if *entity == target && *active => {
                    *active = false;
                    (*owner, None)
                }
                StateOverlayEntry::Component(ComponentStateOverlay {
                    owner,
                    entity,
                    active,
                    component,
                    fields,
                    ..
                }) if *entity == target && *active => {
                    *active = false;
                    fields.clear();
                    (*owner, Some(*component))
                }
                _ => continue,
            };
            self.state_overlays.registry.set(id, resource);
            self.state_overlays
                .diagnostics
                .push(StateOverlayLifecycleDiagnostic {
                    owner,
                    state_overlay: id,
                    entity: target,
                    component,
                    reason: StateOverlayLifecycleReason::EntityDeleted,
                });
        }
    }
}

impl super::StateOverlayMutationAccess<'_> {
    pub(in crate::world) fn invalidate_component_state_overlays(
        &mut self,
        entity: EntityId,
        component: u16,
    ) {
        let handles: Vec<_> = self
            .state_overlays
            .registry
            .iter()
            .filter_map(|(id, entry)| match entry {
                StateOverlayEntry::Component(overlay)
                    if overlay.entity == entity
                        && overlay.component == component
                        && overlay.active
                        && overlay.mode != ComponentOverlayMode::Auto =>
                {
                    Some(id)
                }
                _ => None,
            })
            .collect();
        for handle in handles {
            let mut entry = self
                .state_overlays
                .registry
                .borrow(handle)
                .expect("live overlay")
                .clone();
            if let StateOverlayEntry::Component(overlay) = &mut entry {
                overlay.active = false;
                overlay.fields.clear();
                self.state_overlays
                    .diagnostics
                    .push(StateOverlayLifecycleDiagnostic {
                        owner: overlay.owner,
                        state_overlay: handle,
                        entity,
                        component: Some(component),
                        reason: StateOverlayLifecycleReason::ComponentRemoved,
                    });
            }
            self.state_overlays.registry.set(handle, entry);
        }
    }
}
