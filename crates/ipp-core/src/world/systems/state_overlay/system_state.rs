use super::*;

/// Authored declaration identities and resolution diagnostics owned by StateOverlaySystem.
#[derive(Default)]
pub struct StateOverlaySystemState {
    pub(in crate::world) registry: StateOverlayRegistry,
    pub(in crate::world) diagnostics: Vec<StateOverlayLifecycleDiagnostic>,
    pub(in crate::world) deferred_owner_releases: std::collections::BTreeSet<u64>,
}

impl StateOverlaySystemState {
    /// Whether one live component declaration carries `offset`. GUI
    /// ownership guards use this to tell property-only overlays
    /// (dimensions etc.) from restorable raw-content contributions.
    /// None denotes an unknown handle; callers fail closed.
    #[cfg(feature = "gui")]
    pub(in crate::world) fn component_overlay_declares(
        &self,
        handle: u64,
        entity: crate::EntityId,
        component: u16,
        offset: u32,
    ) -> Option<bool> {
        match self.registry.borrow(handle)? {
            StateOverlayEntry::Component(overlay)
                if overlay.entity == entity && overlay.component == component =>
            {
                Some(overlay.fields.iter().any(|field| field.offset == offset))
            }
            _ => Some(false),
        }
    }

    pub(in crate::world) fn owns_entity(&self, entity: crate::EntityId) -> bool {
        self.registry.iter().any(|(_, entry)| {
            matches!(entry, StateOverlayEntry::EntityBinding(binding)
                if binding.entity == entity
                    && binding.active
                    && binding.mode == EntityOverlayMode::Owned)
        })
    }
}
