//! Reborrow current local storage without aliasing the mutable runtime access.

use super::*;
use crate::systems::{SystemCommandContext, SystemWorldView};

#[cfg(test)]
#[path = "host_ingress_tests.rs"]
mod tests;

impl World {
    pub(crate) fn next_evaluation_tick(&self) -> Option<u64> {
        self.data.tick.checked_add(1)
    }

    pub(crate) fn ingress_world_view(&self) -> SystemWorldView<'_> {
        SystemWorldView {
            world: &self.data,
            authored: &self.data.state,
        }
    }
}

impl SystemWorldView<'_> {
    /// Exact runtime lifetime, independent of durable metadata.
    pub fn world_ref(&self) -> crate::WorldRef {
        crate::WorldRef {
            id: self.world.id,
            incarnation: self.world.identity,
        }
    }

    /// Current local fault, not an ancestor's presentation outcome.
    pub fn fault(&self) -> Option<ErrorReason> {
        self.world.fault
    }

    /// Current structural validity, without following or evaluating the tree.
    pub fn entity_link_valid(&self, entity: EntityId) -> bool {
        self.authored.allocator.contains(entity) && !self.authored.links.invalid.contains(&entity)
    }

    /// Whether the live entity has the component at this cut. A removed component
    /// is absent even while its old storage stays occupied for synchronous
    /// invalidation callbacks.
    pub fn component_is_active(&self, entity: EntityId, component: u16) -> bool {
        self.authored
            .entities
            .get(&entity)
            .and_then(|record| record.input(component))
            .is_some()
    }

    pub(crate) fn output_matches(
        &self,
        output: crate::OutputRef,
        outputs: crate::world::composition::WorldOutputs,
    ) -> bool {
        outputs.matches(self.world_ref(), self.authored, output)
    }
}

impl SystemCommandContext<'_> {
    /// Read current authority at this command's boundary. Missing foreign declarations
    /// or borrows remain unavailable; validation belongs to the receiving System.
    pub fn host_ingress(&self) -> Option<crate::HostIngressView<'_>> {
        Some(crate::HostIngressView {
            local: SystemWorldView {
                world: self.world.world,
                authored: &self.world.world.state,
            },
            local_outputs: self.local_outputs,
            declared: &self.declared_worlds,
            foreign: self.world.reference_worlds,
            topology: self.world.topology,
            publications: self.publications?,
            assets: self.world.asset_acquisition,
        })
    }
}
