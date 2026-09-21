//! Batch-local aliases for scoped overlay attachments.

use crate::{EntityId, ErrorReason, StateOverlayAlias, StateOverlayHandleKind, StateOverlayRef};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct StateOverlayBatch {
    aliases: BTreeMap<u32, u64>,
    pub(in crate::world) created: Vec<StateOverlayAlias>,
}

impl StateOverlayBatch {
    pub(crate) fn resolve(&self, reference: StateOverlayRef) -> Result<u64, ErrorReason> {
        match reference {
            StateOverlayRef::Handle(id) => Ok(id),
            StateOverlayRef::Alias(alias) => self
                .aliases
                .get(&alias)
                .copied()
                .ok_or(ErrorReason::UnknownAlias),
        }
    }

    pub(in crate::world) fn vacant(&self, alias: u32) -> Result<(), ErrorReason> {
        if self.aliases.contains_key(&alias) {
            Err(ErrorReason::DuplicateAlias)
        } else {
            Ok(())
        }
    }

    pub(in crate::world) fn add(
        &mut self,
        alias: u32,
        id: u64,
        kind: StateOverlayHandleKind,
        entity: Option<EntityId>,
    ) {
        self.aliases.insert(alias, id);
        self.created.push(StateOverlayAlias {
            alias,
            id,
            kind,
            entity,
        });
    }
}
