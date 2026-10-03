//! Mutation-scoped validation of untrusted ingress tokens against authoritative Worlds.

use super::*;
use crate::{Command, FieldValue, FieldWrite};
use std::collections::BTreeSet;

/// Untrusted transported World identity; never an authored component value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldReferenceToken {
    world: WorldId,
    incarnation: u64,
}

impl WorldReferenceToken {
    /// Decode syntax without asserting that the named lifetime exists.
    pub fn untrusted(world: u64, incarnation: u64) -> Self {
        Self {
            world: WorldId(world),
            incarnation,
        }
    }
}

/// Untrusted transported output identity; resolution never silently rebinds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputReferenceToken {
    world: WorldReferenceToken,
    target: OutputTarget,
}

impl OutputReferenceToken {
    /// Decode syntax without manufacturing a live output handle.
    pub fn untrusted(world: WorldReferenceToken, target: OutputTarget) -> Self {
        Self {
            world,
            target,
        }
    }
}

pub(crate) struct ReferenceWorlds<'a>(BTreeMap<WorldId, &'a World>);

#[derive(Default)]
pub(crate) struct WorldReferenceRequests {
    worlds: BTreeSet<WorldId>,
}

impl ReferenceWorlds<'_> {
    pub(crate) fn get(&self, world: WorldRef) -> Option<&World> {
        self.0
            .get(&world.id())
            .copied()
            .filter(|candidate| candidate.runtime_ref() == world)
    }
}

impl WorldReferenceRequests {
    pub(crate) fn include(&mut self, world: WorldRef) {
        self.worlds.insert(world.id());
    }
}

/// Field values a command carries, including a compare-and-set's expected value.
fn field_values(command: &Command) -> impl Iterator<Item = &FieldValue> {
    let (fields, expected): (&[FieldWrite], Option<&FieldValue>) = match command {
        Command::InsertComponent {
            fields,
            ..
        } => (fields, None),
        Command::SetField {
            field,
            ..
        } => (std::slice::from_ref(field), None),
        Command::SetFieldIf {
            field,
            expected,
            ..
        } => (std::slice::from_ref(field), Some(&**expected)),
        _ => (&[], None),
    };
    fields.iter().map(|field| &field.value).chain(expected)
}

pub(crate) fn referenced_worlds<'a>(
    commands: impl Iterator<Item = &'a Command>,
) -> WorldReferenceRequests {
    let mut requested = WorldReferenceRequests::default();
    for command in commands {
        for value in field_values(command) {
            match *value {
                FieldValue::UnresolvedWorld(token) => {
                    requested.worlds.insert(token.world);
                }
                FieldValue::UnresolvedOutput(token) => {
                    requested.worlds.insert(token.world.world);
                }
                _ => {}
            }
        }
    }
    requested
}

impl HostRuntime {
    /// Borrow a command admission context, validating foreign lifetimes only at application.
    /// The immutable foreign borrows end with this context, before evaluation or publication.
    pub fn world_mut_for_commands(
        &mut self,
        world: WorldId,
        commands: &[Command],
    ) -> Option<WorldContext<'_>> {
        self.reference_context(world, referenced_worlds(commands.iter()))
    }

    pub(crate) fn reference_context(
        &mut self,
        world: WorldId,
        mut referenced: WorldReferenceRequests,
    ) -> Option<WorldContext<'_>> {
        referenced.worlds.remove(&world);
        if referenced.worlds.is_empty() {
            let mut context = self.worlds.get_mut(&world)?.context(
                &mut self.assets,
                &mut self.io,
                &mut self.data,
                &mut self.topology,
            );
            context.publications = Some(&self.publications);
            return Some(context);
        }

        #[cfg(test)]
        {
            self.topology.foreign_world_scans += 1;
        }
        let mut target = None;
        let mut foreign = BTreeMap::new();
        for (id, value) in self.worlds.iter_mut() {
            if *id == world {
                target = Some(value);
            } else if referenced.worlds.contains(id) {
                foreign.insert(*id, &*value);
            }
        }
        let mut context = target?.context(
            &mut self.assets,
            &mut self.io,
            &mut self.data,
            &mut self.topology,
        );
        context.reference_worlds = Some(ReferenceWorlds(foreign));
        context.publications = Some(&self.publications);
        Some(context)
    }
}

impl WorldContext<'_> {
    fn resolve_world_token(&self, token: WorldReferenceToken) -> Result<WorldRef, ErrorReason> {
        let world = if token.world == self.id() {
            Some(self.world_ref())
        } else {
            self.reference_worlds
                .as_ref()
                .and_then(|worlds| worlds.0.get(&token.world))
                .map(|world| world.runtime_ref())
        };
        world
            .filter(|world| world.incarnation() == token.incarnation)
            .ok_or(ErrorReason::InvalidEntity)
    }

    fn resolve_output_token(&self, token: OutputReferenceToken) -> Result<OutputRef, ErrorReason> {
        let world = self.resolve_world_token(token.world)?;
        if world.id() == self.id() {
            self.bind_output_target(token.target)
        } else {
            self.reference_worlds
                .as_ref()
                .and_then(|worlds| worlds.0.get(&world.id()))
                .ok_or(ErrorReason::InvalidEntity)?
                .bind_output_target(token.target)
        }
    }

    pub(crate) fn resolve_operation_references(
        &self,
        command: &mut Command,
    ) -> Result<(), ErrorReason> {
        let (fields, expected) = match command {
            Command::InsertComponent {
                fields,
                ..
            } => (fields.as_mut_slice(), None),
            Command::SetField {
                field,
                ..
            } => (std::slice::from_mut(field), None),
            Command::SetFieldIf {
                field,
                expected,
                ..
            } => (std::slice::from_mut(field), Some(&mut **expected)),
            _ => return Ok(()),
        };
        for value in fields
            .iter_mut()
            .map(|field| &mut field.value)
            .chain(expected)
        {
            *value = match *value {
                FieldValue::UnresolvedWorld(token) => {
                    FieldValue::World(Some(self.resolve_world_token(token)?))
                }
                FieldValue::UnresolvedOutput(token) => {
                    FieldValue::Output(Some(self.resolve_output_token(token)?))
                }
                _ => continue,
            };
        }
        Ok(())
    }
}
