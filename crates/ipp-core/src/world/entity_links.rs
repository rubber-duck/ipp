use std::collections::{BTreeMap, BTreeSet};

use crate::{EntityId, EntityPersistentId, EntityRef, ErrorReason};

#[cfg(test)]
thread_local! {
    pub(super) static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One ordered structural edit. References resolve in the submitting World.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntityPlacementRef {
    /// Same-World parent, or a World-space root.
    pub parent: Option<EntityRef>,
    /// Insert before this sibling; absence appends once.
    pub before: Option<EntityRef>,
}

/// Resolved runtime identities supplied at a discrete structural transition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntityPlacement {
    /// Same-World parent, or a World-space root.
    pub parent: Option<EntityId>,
    /// Insert before this sibling; absence appends once.
    pub before: Option<EntityId>,
}

/// Fixed-width sibling order, with durable entity identity breaking ties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityOrder(u128);

impl EntityOrder {
    /// Current persisted order label. Rebalancing may change labels, not ordering.
    pub fn value(&self) -> u128 {
        self.0
    }

    /// Validate a persisted fixed-width order label.
    pub fn from_value(value: u128) -> Result<Self, ErrorReason> {
        if value == 0 || value == u128::MAX {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(Self(value))
    }

    fn between(before: Option<&Self>, after: Option<&Self>) -> Option<Self> {
        let lower = before.map_or(0, |order| order.0);
        let upper = after.map_or(u128::MAX, |order| order.0);
        let gap = upper.checked_sub(lower)?;
        if gap <= 1 {
            return None;
        }
        let increment = if after.is_none() {
            (1u128 << 64).min(gap / 2)
        } else {
            gap / 2
        };
        Some(Self(lower + increment))
    }
}

/// The parent/order value of an entity, held once in the World's link store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityLink {
    /// Parent in the same World; absence selects World space.
    pub parent: Option<EntityId>,
    /// Current sibling label, interpreted with durable identity for ties.
    pub order: EntityOrder,
}

struct EntityLinkRecord {
    link: EntityLink,
    persistent_id: EntityPersistentId,
    live: bool,
    transform: Option<Box<super::systems::hierarchy::ObjectTransformRuntime>>,
}

pub(super) type OrderedEntity = (EntityOrder, EntityPersistentId, EntityId);

/// One link per entity and the sibling index derived from those links.
#[derive(Default)]
pub(super) struct EntityLinkStore {
    records: BTreeMap<EntityId, Box<EntityLinkRecord>>,
    children: BTreeMap<Option<EntityId>, BTreeSet<OrderedEntity>>,
    pub(super) changed: BTreeSet<EntityId>,
    pub(super) operation_changed: BTreeSet<EntityId>,
    pub(super) affected: BTreeSet<EntityId>,
    pub(super) invalid: BTreeSet<EntityId>,
    #[cfg(test)]
    relabels: usize,
    #[cfg(test)]
    relabeled_values: usize,
    #[cfg(test)]
    invalidation_visits: usize,
}

impl EntityLinkStore {
    fn insert_child(&mut self, parent: Option<EntityId>, key: OrderedEntity) {
        self.children.entry(parent).or_default().insert(key);
    }

    fn remove_child(&mut self, parent: Option<EntityId>, key: OrderedEntity) {
        let children = self.children.get_mut(&parent).expect("indexed link");
        children.remove(&key);
        if children.is_empty() {
            self.children.remove(&parent);
        }
    }

    /// The entity's current link, absent for missing or retired entities.
    pub(super) fn effective(&self, entity: EntityId) -> Option<&EntityLink> {
        self.records
            .get(&entity)
            .filter(|record| record.live)
            .map(|record| &record.link)
    }

    pub(super) fn children(&self, parent: Option<EntityId>) -> impl Iterator<Item = EntityId> + '_ {
        self.children
            .get(&parent)
            .into_iter()
            .flatten()
            .map(|(_, _, entity)| *entity)
    }

    pub(super) fn next_sibling(&self, entity: EntityId) -> Option<EntityId> {
        use std::ops::Bound::{Excluded, Unbounded};

        let record = self.records.get(&entity).filter(|record| record.live)?;
        let key = (record.link.order, record.persistent_id, entity);
        self.children
            .get(&record.link.parent)?
            .range((Excluded(key), Unbounded))
            .inspect(|_| {
                #[cfg(test)]
                VISITS.set(VISITS.get() + 1);
            })
            .next()
            .map(|(_, _, sibling)| *sibling)
    }

    pub(super) fn parent(&self, entity: EntityId) -> Option<EntityId> {
        self.effective(entity).and_then(|link| link.parent)
    }

    pub(super) fn insert(&mut self, entity: EntityId, persistent_id: EntityPersistentId) {
        let mut order = self.tail(None, None);
        if EntityOrder::between(order.as_ref(), None).is_none() {
            self.relabel(None, order, None);
            order = self.tail(None, None);
        }
        let link = EntityLink {
            parent: None,
            order: EntityOrder::between(order.as_ref(), None).expect("available sibling label"),
        };
        self.insert_child(None, (link.order, persistent_id, entity));
        self.records.insert(
            entity,
            Box::new(EntityLinkRecord {
                link,
                persistent_id,
                live: true,
                transform: None,
            }),
        );
        self.mark(entity);
    }

    /// Order of the last child of `parent` other than `exclude`.
    fn tail(&self, parent: Option<EntityId>, exclude: Option<EntityId>) -> Option<EntityOrder> {
        self.children
            .get(&parent)
            .into_iter()
            .flat_map(|children| children.iter().rev())
            .find(|(_, _, sibling)| Some(*sibling) != exclude)
            .map(|(order, _, _)| *order)
    }

    fn neighbors(
        &self,
        entity: EntityId,
        parent: Option<EntityId>,
        before: Option<EntityId>,
    ) -> Result<(Option<EntityOrder>, Option<EntityOrder>), ErrorReason> {
        let Some(before) = before else {
            return Ok((self.tail(parent, Some(entity)), None));
        };
        let upper = self
            .effective(before)
            .filter(|link| link.parent == parent)
            .ok_or(ErrorReason::InvalidEntity)?
            .order;
        let target_key = (upper, self.records[&before].persistent_id, before);
        let lower = self
            .children
            .get(&parent)
            .into_iter()
            .flat_map(|children| children.range(..target_key).rev())
            .find(|(_, _, sibling)| *sibling != entity)
            .map(|(order, _, _)| *order);
        Ok((lower, Some(upper)))
    }

    fn relabel(
        &mut self,
        parent: Option<EntityId>,
        lower: Option<EntityOrder>,
        upper: Option<EntityOrder>,
    ) {
        #[cfg(test)]
        {
            self.relabels += 1;
        }
        let (start, stride, values) = (40..=128)
            .step_by(8)
            .find_map(|bits| {
                let width = 1u128.checked_shl(bits).unwrap_or(u128::MAX);
                let start = if bits == 128 {
                    0
                } else {
                    lower.map_or(0, |order| order.0).saturating_sub(width / 2)
                };
                let end = start.saturating_add(width);
                if upper.is_some_and(|order| order.0 > end) {
                    return None;
                }
                let first = (
                    EntityOrder(start),
                    EntityPersistentId(0),
                    EntityId::from_bits(0),
                );
                let last = (
                    EntityOrder(end),
                    EntityPersistentId(u64::MAX),
                    EntityId::from_bits(u64::MAX),
                );
                let values: Vec<_> = self
                    .children
                    .get(&parent)
                    .into_iter()
                    .flat_map(|children| children.range(first..=last))
                    .copied()
                    .collect();
                let stride = (end - start) / (values.len() as u128 + 2);
                (stride >= 1 << 32 || bits == 128).then_some((start, stride, values))
            })
            .expect("fixed-width ordering capacity");
        #[cfg(test)]
        {
            self.relabeled_values += values.len();
        }
        for (index, (_, _, entity)) in values.into_iter().enumerate() {
            self.replace(
                entity,
                EntityLink {
                    parent,
                    order: EntityOrder(start + stride * (index as u128 + 1)),
                },
            );
        }
    }

    pub(super) fn resolve(
        &mut self,
        entity: EntityId,
        parent: Option<EntityId>,
        before: Option<EntityId>,
    ) -> Result<EntityLink, ErrorReason> {
        self.effective(entity).ok_or(ErrorReason::InvalidEntity)?;
        if let Some(parent) = parent {
            self.effective(parent).ok_or(ErrorReason::InvalidEntity)?;
        }
        if before == Some(entity) {
            return Err(ErrorReason::InvalidValue);
        }
        let (mut lower, mut upper) = self.neighbors(entity, parent, before)?;
        if EntityOrder::between(lower.as_ref(), upper.as_ref()).is_none() {
            self.relabel(parent, lower, upper);
            (lower, upper) = self.neighbors(entity, parent, before)?;
        }
        Ok(EntityLink {
            parent,
            order: EntityOrder::between(lower.as_ref(), upper.as_ref())
                .ok_or(ErrorReason::Capacity)?,
        })
    }

    /// Write the entity's link after validating that it names live entities.
    pub(super) fn set(&mut self, entity: EntityId, value: EntityLink) -> Result<(), ErrorReason> {
        self.validate_value(entity, &value)?;
        self.replace(entity, value);
        Ok(())
    }

    fn validate_value(&self, entity: EntityId, value: &EntityLink) -> Result<(), ErrorReason> {
        self.effective(entity).ok_or(ErrorReason::InvalidEntity)?;
        if let Some(parent) = value.parent {
            self.effective(parent).ok_or(ErrorReason::InvalidEntity)?;
        }
        if value.order.0 == 0 || value.order.0 == u128::MAX {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }

    fn replace(&mut self, entity: EntityId, value: EntityLink) {
        let record = self.records.get(&entity).expect("live structural target");
        if record.link == value {
            return;
        }
        let persistent = record.persistent_id;
        self.remove_child(record.link.parent, (record.link.order, persistent, entity));
        self.insert_child(value.parent, (value.order, persistent, entity));
        self.records.get_mut(&entity).unwrap().link = value;
        self.mark(entity);
    }

    fn mark(&mut self, entity: EntityId) {
        self.changed.insert(entity);
        self.operation_changed.insert(entity);
    }

    /// Retire a deleted entity; its children become roots keeping their labels.
    pub(super) fn retire(&mut self, entity: EntityId) {
        let children: Vec<_> = self.children(Some(entity)).collect();
        for child in children {
            #[cfg(test)]
            {
                self.invalidation_visits += 1;
            }
            let mut link = self.records[&child].link.clone();
            link.parent = None;
            self.replace(child, link);
        }
        let record = self.records.get_mut(&entity).expect("live entity link");
        record.live = false;
        let parent = record.link.parent;
        let key = (record.link.order, record.persistent_id, entity);
        self.remove_child(parent, key);
        self.invalid.remove(&entity);
        self.mark(entity);
    }

    pub(super) fn release_retired(&mut self, entity: EntityId) {
        let record = self.records.remove(&entity).expect("retired entity link");
        assert!(!record.live);
    }

    pub(super) fn restore_identity(&mut self, entity: EntityId, persistent: EntityPersistentId) {
        let record = self.records.get(&entity).expect("restored entity");
        let parent = record.link.parent;
        let previous = (record.link.order, record.persistent_id, entity);
        let key = (record.link.order, persistent, entity);
        self.remove_child(parent, previous);
        self.records.get_mut(&entity).unwrap().persistent_id = persistent;
        self.insert_child(parent, key);
    }

    pub(super) fn subtree(&self, root: EntityId) -> Vec<EntityId> {
        let mut pending = vec![root];
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        while let Some(entity) = pending.pop() {
            if self.effective(entity).is_some() && seen.insert(entity) {
                result.push(entity);
                pending.extend(self.children(Some(entity)));
            }
        }
        result.reverse();
        result
    }

    pub(super) fn reconcile(&mut self) -> Result<(), ErrorReason> {
        self.affected.clear();
        let mut pending = Vec::new();
        for &entity in &self.operation_changed {
            if self.parent(entity).is_none() && !self.invalid.contains(&entity) {
                self.affected.insert(entity);
            } else {
                pending.push(entity);
            }
        }
        while let Some(entity) = pending.pop() {
            if self.affected.insert(entity) {
                pending.extend(self.children(Some(entity)));
            }
        }
        let mut done = BTreeSet::new();
        for &entity in &self.affected {
            if self.effective(entity).is_none() {
                continue;
            }
            let mut path = Vec::new();
            let mut visiting = BTreeSet::new();
            let mut current = entity;
            let invalid = loop {
                #[cfg(test)]
                VISITS.set(VISITS.get() + 1);
                if done.contains(&current) || !self.affected.contains(&current) {
                    break self.invalid.contains(&current);
                }
                if !visiting.insert(current) {
                    break true;
                }
                path.push(current);
                match self.parent(current) {
                    Some(parent) => current = parent,
                    None => break false,
                }
            };
            for entity in path {
                done.insert(entity);
                if invalid {
                    self.invalid.insert(entity);
                } else {
                    self.invalid.remove(&entity);
                }
            }
        }
        if self
            .affected
            .iter()
            .any(|entity| self.invalid.contains(entity))
        {
            Err(ErrorReason::UnsupportedDependency)
        } else {
            Ok(())
        }
    }

    pub(super) fn prepare_transform(&mut self, entity: EntityId) {
        self.records
            .get_mut(&entity)
            .expect("live entity")
            .transform
            .get_or_insert_with(Default::default);
    }

    pub(super) fn transform(
        &self,
        entity: EntityId,
    ) -> Option<&super::systems::hierarchy::ObjectTransformRuntime> {
        self.records.get(&entity)?.transform.as_deref()
    }

    pub(super) fn transform_mut(
        &mut self,
        entity: EntityId,
    ) -> Option<&mut super::systems::hierarchy::ObjectTransformRuntime> {
        self.records.get_mut(&entity)?.transform.as_deref_mut()
    }
}

impl super::WorldContext<'_> {
    /// Borrow the entity's relationship at this World boundary.
    pub fn entity_link(&self, entity: EntityId) -> Option<&EntityLink> {
        self.world.state.links.effective(entity)
    }

    /// Iterate the shared derived sibling index in sibling order.
    pub fn entity_children(&self, parent: Option<EntityId>) -> impl Iterator<Item = EntityId> + '_ {
        self.world.state.links.children(parent)
    }

    /// Return the next sibling, or `None` for missing, retired or last entities.
    pub fn entity_next_sibling(&self, entity: EntityId) -> Option<EntityId> {
        self.world.state.links.next_sibling(entity)
    }
}

impl super::WorldMutationState {
    pub(super) fn resolve_placement(
        &mut self,
        entity: EntityId,
        placement: &EntityPlacementRef,
        aliases: &super::EntityAliases,
    ) -> Result<EntityLink, ErrorReason> {
        let parent = placement
            .parent
            .as_ref()
            .map(|parent| self.resolve(parent, aliases))
            .transpose()?;
        let before = placement
            .before
            .as_ref()
            .map(|before| self.resolve(before, aliases))
            .transpose()?;
        self.links.resolve(entity, parent, before)
    }
}

#[cfg(test)]
#[path = "entity_links_tests.rs"]
mod tests;
