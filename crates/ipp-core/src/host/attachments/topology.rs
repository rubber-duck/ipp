//! Derived live graph and retiring incoming reservations, never World borrows.

use crate::host::{RootOutputBinding, WorldAttachment, WorldAttachmentToken, WorldRef};
use crate::{EntityId, ErrorReason, WorldId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AttachmentAnchor {
    pub world: WorldId,
    pub entity: EntityId,
}

#[derive(Default)]
pub(crate) struct HostTopology {
    pub identity: u64,
    pub worlds: BTreeMap<WorldId, WorldRef>,
    pub incoming: BTreeMap<WorldId, AttachmentAnchor>,
    pub desired: BTreeMap<AttachmentAnchor, WorldRef>,
    pub retiring: BTreeMap<AttachmentAnchor, BTreeSet<WorldId>>,
    pub tokens: BTreeMap<AttachmentAnchor, WorldAttachmentToken>,
    retiring_tokens: BTreeMap<(AttachmentAnchor, WorldId), WorldAttachmentToken>,
    published_tokens: BTreeMap<(u64, u64), WorldAttachmentToken>,
    next_token: u64,
    pub roots: BTreeMap<WorldId, RootOutputBinding>,
    pub next_root_binding: u64,
    pub revision: u64,
    #[cfg(test)]
    pub foreign_world_scans: usize,
    order_revision: Option<u64>,
    order: Vec<WorldId>,
}

impl HostTopology {
    pub(in crate::host) fn adopt_restored(&mut self, mut graph: Self) -> Result<(), ErrorReason> {
        let count = u64::try_from(graph.tokens.len()).map_err(|_| ErrorReason::Capacity)?;
        self.next_token
            .checked_add(count)
            .ok_or(ErrorReason::Capacity)?;
        if graph.worlds.keys().any(|id| self.worlds.contains_key(id)) || !graph.roots.is_empty() {
            return Err(ErrorReason::InvalidValue);
        }
        graph.retire_detaches(&BTreeSet::new());
        self.worlds.extend(graph.worlds);
        self.incoming.extend(graph.incoming);
        self.desired.extend(graph.desired);
        for (anchor, token) in graph.tokens {
            self.next_token += 1;
            self.tokens.insert(
                anchor,
                WorldAttachmentToken::new(
                    self.identity,
                    self.next_token,
                    token.parent(),
                    token.anchor(),
                    token.incarnation(),
                    token.child(),
                ),
            );
            token.retire();
        }
        self.revision += 1;
        Ok(())
    }

    pub fn preflight_token(&self) -> Result<(), ErrorReason> {
        self.next_token
            .checked_add(1)
            .ok_or(ErrorReason::Capacity)
            .map(|_| ())
    }

    pub fn write_token(
        &mut self,
        anchor: AttachmentAnchor,
        incarnation: u64,
        child: Option<WorldRef>,
    ) -> WorldAttachmentToken {
        self.next_token = self
            .next_token
            .checked_add(1)
            .expect("preflighted attachment revision");
        let token = WorldAttachmentToken::new(
            self.identity,
            self.next_token,
            self.worlds[&anchor.world],
            anchor.entity,
            incarnation,
            child,
        );
        let previous = self.tokens.insert(anchor, token.clone());
        self.set(anchor, child, previous.as_ref());
        if let Some(previous) = previous {
            self.retire_unreferenced_token(&previous);
        }
        token
    }

    pub fn detach_token(&mut self, anchor: AttachmentAnchor) -> Option<WorldAttachmentToken> {
        let token = self.tokens.remove(&anchor);
        self.set(anchor, None, token.as_ref());
        if let Some(token) = &token {
            self.retire_unreferenced_token(token);
        }
        token
    }

    pub fn validate_token(
        &self,
        parent: WorldRef,
        token: &WorldAttachmentToken,
    ) -> Result<(), ErrorReason> {
        if token.identity().0 != self.identity || token.parent() != parent {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }

    fn retire_unreferenced_token(&self, token: &WorldAttachmentToken) {
        let anchor = token.location();
        let reserved = token
            .child()
            .is_some_and(|child| self.retiring_tokens.get(&(anchor, child.id)) == Some(token));
        if self.tokens.get(&anchor) != Some(token)
            && !reserved
            && !self.published_tokens.contains_key(&token.identity())
        {
            token.retire();
        }
    }

    pub fn retain_published_tokens(
        &mut self,
        published: BTreeMap<(u64, u64), WorldAttachmentToken>,
    ) {
        let previous = std::mem::replace(&mut self.published_tokens, published);
        for (identity, token) in previous {
            if !self.published_tokens.contains_key(&identity) {
                self.retire_unreferenced_token(&token);
            }
        }
    }

    pub fn validate(
        &self,
        anchor: AttachmentAnchor,
        value: &WorldAttachment,
    ) -> Result<(), ErrorReason> {
        let Some(child) = value.child() else {
            return Ok(());
        };
        if self.worlds.get(&child.id) != Some(&child) {
            return Err(ErrorReason::InvalidEntity);
        }
        if child.id == anchor.world || self.roots.contains_key(&child.id) {
            return Err(ErrorReason::InvalidValue);
        }
        if self
            .incoming
            .get(&child.id)
            .is_some_and(|previous| *previous != anchor)
            || self
                .retiring
                .get(&anchor)
                .is_some_and(|children| children.contains(&child.id))
        {
            return Err(ErrorReason::InvalidValue);
        }
        let mut parent = anchor.world;
        let mut visited = BTreeSet::new();
        while let Some(incoming) = self.incoming.get(&parent) {
            if !visited.insert(parent) || incoming.world == child.id {
                return Err(ErrorReason::InvalidValue);
            }
            parent = incoming.world;
        }
        Ok(())
    }

    fn set(
        &mut self,
        anchor: AttachmentAnchor,
        child: Option<WorldRef>,
        previous_token: Option<&WorldAttachmentToken>,
    ) {
        if self.desired.get(&anchor).copied() == child {
            return;
        }
        if let Some(previous) = self.desired.remove(&anchor) {
            self.retiring.entry(anchor).or_default().insert(previous.id);
            if let Some(token) = previous_token {
                self.retiring_tokens
                    .insert((anchor, previous.id), token.clone());
            }
        }
        if let Some(child) = child {
            self.desired.insert(anchor, child);
            self.incoming.insert(child.id, anchor);
        }
        self.revision += 1;
    }

    pub fn retire_detaches(&mut self, published: &BTreeSet<(AttachmentAnchor, WorldId)>) {
        let mut changed = false;
        let mut released = Vec::new();
        self.retiring.retain(|anchor, children| {
            children.retain(|child| {
                if published.contains(&(*anchor, *child)) {
                    return true;
                }
                if self.incoming.get(child) == Some(anchor) {
                    self.incoming.remove(child);
                }
                if let Some(token) = self.retiring_tokens.remove(&(*anchor, *child)) {
                    released.push(token);
                }
                changed = true;
                false
            });
            !children.is_empty()
        });
        if changed {
            self.revision += 1;
        }
        for token in released {
            self.retire_unreferenced_token(&token);
        }
    }

    pub fn destroy(&mut self, world: WorldId) {
        for token in self
            .tokens
            .values()
            .chain(self.retiring_tokens.values())
            .chain(self.published_tokens.values())
        {
            if token.parent().id() == world
                || token.child().is_some_and(|child| child.id() == world)
            {
                token.retire();
            }
        }
        self.tokens.retain(|anchor, _| anchor.world != world);
        self.retiring_tokens
            .retain(|(anchor, child), _| anchor.world != world && *child != world);
        self.published_tokens.retain(|_, token| {
            token.parent().id() != world && token.child().is_none_or(|child| child.id() != world)
        });
        self.worlds.remove(&world);
        self.roots.remove(&world);
        self.desired
            .retain(|anchor, child| anchor.world != world && child.id != world);
        self.incoming
            .retain(|child, anchor| *child != world && anchor.world != world);
        self.retiring.retain(|anchor, children| {
            children.remove(&world);
            anchor.world != world && !children.is_empty()
        });
        self.revision += 1;
    }

    /// Scheduling ancestry follows incoming reservations, including not-yet-retired detaches.
    pub fn incoming_parent(&self, world: WorldId) -> Option<WorldId> {
        self.incoming.get(&world).map(|anchor| anchor.world)
    }

    fn incoming_children(&self) -> BTreeMap<WorldId, Vec<WorldId>> {
        let mut children: BTreeMap<WorldId, Vec<WorldId>> = BTreeMap::new();
        for (&child, anchor) in &self.incoming {
            children.entry(anchor.world).or_default().push(child);
        }
        children
    }

    pub fn order(&mut self) -> &[WorldId] {
        if self.order_revision == Some(self.revision) {
            return &self.order;
        }
        let children = self.incoming_children();
        let mut pending: Vec<_> = self
            .worlds
            .keys()
            .rev()
            .filter(|world| self.incoming_parent(**world).is_none())
            .copied()
            .collect();
        let mut order = Vec::with_capacity(self.worlds.len());
        while let Some(world) = pending.pop() {
            order.push(world);
            if let Some(children) = children.get(&world) {
                pending.extend(children.iter().rev());
            }
        }
        self.order = order;
        self.order_revision = Some(self.revision);
        &self.order
    }
}
