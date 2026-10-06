//! Output binding, root presentation selection and its explicit rebind lifetime.

use crate::ErrorReason;
use crate::host::{
    HostRuntime, OutputKind, OutputRef, WorldDerivedChunk, WorldId, WorldPublicationId, WorldRef,
    WorldViewport,
};

/// Host-fenced identity of one successful explicit root binding. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootBindingGeneration {
    pub(in crate::host) host: u64,
    pub(in crate::host) serial: u64,
}

impl RootBindingGeneration {
    /// Stable Host identity and bind serial for transport observation and exact comparison.
    /// Reading this pair does not authorize reconstructing or selecting a root binding.
    pub fn identity(self) -> (u64, u64) {
        (self.host, self.serial)
    }
}

/// The authoritative root selection. Publication refresh does not replace this record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RootOutputBinding {
    /// Exact explicitly selected producer lifetime.
    pub output: OutputRef,
    /// Current authored presentation dimensions and density.
    pub viewport: WorldViewport,
    /// Identity replaced by every successful explicit bind, including equal values.
    pub generation: RootBindingGeneration,
}

impl HostRuntime {
    /// Explicitly bind or rebind the current Camera output of an entity; a
    /// World's canvas names no entity and is selected with [`OutputRef::canvas`].
    pub fn bind_output(
        &self,
        world: WorldRef,
        entity: crate::EntityId,
        kind: OutputKind,
    ) -> Result<OutputRef, ErrorReason> {
        if self.world_ref(world.id) != Some(world) {
            return Err(ErrorReason::InvalidEntity);
        }
        self.worlds
            .get(&world.id)
            .ok_or(ErrorReason::InvalidEntity)?
            .bind_output(entity, kind)
    }

    /// Explicit selection never follows an authoring session or replacement incarnation.
    pub fn set_root_output(
        &mut self,
        output: OutputRef,
        viewport: WorldViewport,
    ) -> Result<(), ErrorReason> {
        viewport.validate()?;
        if !self
            .worlds
            .get(&output.world.id)
            .is_some_and(|world| world.output_valid(output))
        {
            return Err(ErrorReason::InvalidEntity);
        }
        if self.topology.incoming.contains_key(&output.world.id) {
            return Err(ErrorReason::InvalidValue);
        }
        let serial = self
            .topology
            .next_root_binding
            .checked_add(1)
            .ok_or(ErrorReason::Capacity)?;
        self.topology.roots.insert(
            output.world.id,
            RootOutputBinding {
                output,
                viewport,
                generation: RootBindingGeneration {
                    host: self.topology.identity,
                    serial,
                },
            },
        );
        self.topology.next_root_binding = serial;
        Ok(())
    }

    /// Observe the exact root binding independently of output readiness.
    pub fn root_output_binding(
        &self,
        world: WorldRef,
    ) -> Result<Option<RootOutputBinding>, ErrorReason> {
        if self.world_ref(world.id()) != Some(world) {
            return Err(ErrorReason::InvalidEntity);
        }
        Ok(self.topology.roots.get(&world.id()).copied())
    }

    /// Withdraw root presentation without destroying the World.
    pub fn clear_root_output(&mut self, world: WorldId) {
        self.topology.roots.remove(&world);
    }

    /// Observe an available explicitly selected root view.
    pub fn root_output(
        &self,
        world: WorldId,
    ) -> Option<(OutputRef, WorldViewport, WorldPublicationId)> {
        let binding = self.topology.roots.get(&world)?;
        let publication = self.latest_publication(world)?;
        self.output(publication, binding.output)?;
        Some((binding.output, binding.viewport, publication))
    }

    /// Read historical output while its producer is valid; this does not authorize presentation.
    /// Only root selection and completed attachment traversal establish presentation paths.
    pub fn output(
        &self,
        publication: WorldPublicationId,
        selection: OutputRef,
    ) -> Option<&WorldDerivedChunk> {
        self.worlds
            .get(&selection.world.id)
            .filter(|world| world.output_valid(selection))?;
        self.publication(publication)?.output(selection)
    }
}

#[cfg(test)]
#[path = "root_binding_tests.rs"]
mod tests;
