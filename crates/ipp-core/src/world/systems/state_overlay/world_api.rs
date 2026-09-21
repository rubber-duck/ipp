use super::*;

impl crate::WorldContext<'_> {
    /// Whether a session's acknowledged owner is still a live root scope.
    pub fn state_overlay_owner_is_live(&self, handle: u64) -> bool {
        matches!(
            self.system::<StateOverlaySystem>(StateOverlaySystem::ID)
                .expect("compiled overlay system")
                .state
                .registry
                .borrow(handle),
            Some(StateOverlayEntry::Owner(_))
        )
    }

    /// Observe the component type of a live overlay for typed transport field handling.
    pub fn state_overlay_component(&self, handle: u64) -> Option<u16> {
        let system = self
            .system::<StateOverlaySystem>(StateOverlaySystem::ID)
            .expect("compiled overlay system");
        match system.state.registry.borrow(handle)? {
            StateOverlayEntry::Component(overlay) => Some(overlay.component),
            _ => None,
        }
    }
}

impl crate::WorldContext<'_> {
    /// Release departing ownership at the current safe mutation boundary.
    ///
    /// A logical command stream retains cleanup until its gate ends so a peer
    /// session cannot interleave ownership mutations between streamed pages.
    pub fn release_state_overlay_owners(&mut self, owners: impl IntoIterator<Item = u64>) {
        let mut owners: std::collections::BTreeSet<_> = owners.into_iter().collect();
        if self.world.command_stream.is_some() {
            self.with_system::<StateOverlaySystem, _>(StateOverlaySystem::ID, |system, _| {
                system.state.deferred_owner_releases.append(&mut owners);
            });
            return;
        }
        if let Some(deferred) = self
            .with_system::<StateOverlaySystem, _>(StateOverlaySystem::ID, |system, _| {
                std::mem::take(&mut system.state.deferred_owner_releases)
            })
        {
            owners.extend(deferred);
        }
        if owners.is_empty() {
            return;
        }
        self.release_state_overlay_owners_now(owners);
    }

    fn release_state_overlay_owners_now(&mut self, owners: impl IntoIterator<Item = u64>) {
        let Some(system) = self.system::<StateOverlaySystem>(StateOverlaySystem::ID) else {
            return;
        };
        let (commands, affected) = system.cleanup_plan(owners);
        if commands.is_empty() {
            return;
        }
        self.stage_underlying_components(affected);
        self.apply_cleanup_commands(&commands);
    }
}

impl StateOverlaySystem {
    fn cleanup_plan(
        &self,
        owners: impl IntoIterator<Item = u64>,
    ) -> (
        Vec<crate::Command>,
        std::collections::BTreeSet<(crate::EntityId, u16)>,
    ) {
        let owners: Vec<_> = owners
            .into_iter()
            .filter(|owner| {
                matches!(
                    self.state.registry.borrow(*owner),
                    Some(StateOverlayEntry::Owner(_))
                )
            })
            .collect();
        let affected = self
            .state
            .registry
            .iter()
            .filter_map(|(_, entry)| match entry {
                StateOverlayEntry::Component(overlay) if owners.contains(&overlay.owner) => {
                    Some((overlay.entity, overlay.component))
                }
                _ => None,
            })
            .collect();
        let commands = owners
            .into_iter()
            .map(|owner| crate::Command::ReleaseStateOverlayOwner {
                owner: StateOverlayRef::Handle(owner),
            })
            .collect();
        (commands, affected)
    }
}
