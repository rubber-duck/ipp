//! Batch-local entity names, resolved in command order.
//!
//! A logical batch names entities it introduces so later commands of the same
//! batch can reference them before the client knows their identities. `Create`
//! names its new or adopted entity. The names are only ever defined by an
//! earlier successful command, so an unknown name is either a forward reference
//! or a reference whose defining command failed; both reject the referencing
//! command with [`ErrorReason::UnknownAlias`]. A resolved name yields the same
//! generational identity a concrete handle would carry, so liveness and every
//! other validation stay with the command's own mutation boundary.
//!
//! A symbolic reference is not a batch name: it resolves against the World's
//! live symbolic identifiers at the same boundary and rejects with
//! [`ErrorReason::MissingSymbolicId`] when no live entity carries it.

use crate::{EntityId, EntityRef, ErrorReason};
use std::collections::BTreeMap;

/// Entities named by earlier commands of one logical batch.
#[derive(Debug, Default)]
pub(crate) struct EntityAliases {
    created: BTreeMap<u32, EntityId>,
}

impl EntityAliases {
    /// Identity named by `reference`, without checking that it is still live.
    ///
    /// `symbols` is the World's index of live symbolic identifiers at this
    /// operation boundary.
    pub(crate) fn identity(
        &self,
        reference: &EntityRef,
        symbols: &BTreeMap<String, EntityId>,
    ) -> Result<EntityId, ErrorReason> {
        match reference {
            EntityRef::Handle(id) => Ok(*id),
            EntityRef::Alias(alias) => self
                .created
                .get(alias)
                .copied()
                .ok_or(ErrorReason::UnknownAlias),
            EntityRef::Symbol(symbol) => symbols
                .get(&**symbol)
                .copied()
                .ok_or(ErrorReason::MissingSymbolicId),
        }
    }

    /// Whether a creation alias is already defined in this batch.
    pub(crate) fn contains_created(&self, alias: u32) -> bool {
        self.created.contains_key(&alias)
    }

    /// Name a newly created entity; callers reject duplicates first.
    pub(crate) fn insert_created(&mut self, alias: u32, entity: EntityId) {
        self.created.insert(alias, entity);
    }
}
