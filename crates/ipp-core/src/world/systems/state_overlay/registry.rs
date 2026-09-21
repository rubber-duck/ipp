//! Scoped declaration identities, ownership modes, and retained lifecycle records.

use crate::{EntityId, ErrorReason};

/// Maximum committed lifecycle losses in one frame, matching bounded host events.
pub const MAX_STATE_OVERLAY_DIAGNOSTICS: usize = 16_384;

/// A world-local state overlay handle or an earlier attachment in this batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateOverlayRef {
    /// Generational state overlay identity, distinct from entity identities.
    Handle(u64),
    /// Batch-local alias in the state overlay namespace.
    Alias(u32),
}

/// Entity lifetime policy, independent of component mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityOverlayMode {
    /// Create and own a previously absent symbolic entity.
    Owned,
    /// Observe an existing entity without retaining its lifetime.
    Bound,
}

/// Persistent component lifetime policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentOverlayMode {
    /// Follow producer incarnations and share a fallback when absent.
    Auto,
    /// Bind one existing effective incarnation.
    Bound,
    /// Create and own a previously absent base incarnation.
    Owned,
}

/// Kind of state overlay handle returned by a successful attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateOverlayHandleKind {
    /// Root producer scope.
    Owner,
    /// Entity lifetime association.
    EntityOverlayBinding,
    /// Retained component declaration.
    ComponentStateOverlay,
}

/// State overlay aliases in successful command order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateOverlayAlias {
    /// Batch-local state overlay alias.
    pub alias: u32,
    /// Generational state overlay identity.
    pub id: u64,
    /// Validated state overlay handle kind.
    pub kind: StateOverlayHandleKind,
    /// Associated entity, absent for owner scopes.
    pub entity: Option<EntityId>,
}

/// Cause of a previously acknowledged declaration losing its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateOverlayLifecycleReason {
    /// The generational entity ceased to exist.
    EntityDeleted,
    /// A different component incarnation replaced the target.
    ComponentReplaced,
    /// No effective component remains.
    ComponentRemoved,
}

/// Owner-scoped lifecycle loss from an applied change, including failed batches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateOverlayLifecycleDiagnostic {
    /// Producer owner scope.
    pub owner: u64,
    /// Invalidated entity binding or overlay.
    pub state_overlay: u64,
    /// Original entity identity.
    pub entity: EntityId,
    /// Component type for declarations, absent for entity bindings.
    pub component: Option<u16>,
    /// Observed lifetime transition.
    pub reason: StateOverlayLifecycleReason,
}

pub(crate) type StateOverlayFields = Vec<crate::FieldWrite>;

/// Root of one client's state overlay lifetime.
#[derive(Clone)]
pub(crate) struct StateOverlayOwner;

/// Associates an owner with an entity and its lifetime policy.
#[derive(Clone)]
pub(crate) struct EntityOverlayBinding {
    pub(crate) owner: u64,
    pub(crate) entity: EntityId,
    pub(crate) mode: EntityOverlayMode,
    pub(crate) active: bool,
}

/// Sparse component values and the incarnation/lifetime they follow.
#[derive(Clone)]
pub(crate) struct ComponentStateOverlay {
    pub(crate) owner: u64,
    pub(crate) binding: u64,
    pub(crate) entity: EntityId,
    pub(crate) mode: ComponentOverlayMode,
    pub(crate) incarnation: u64,
    pub(crate) order: u64,
    pub(crate) component: u16,
    pub(crate) fields: StateOverlayFields,
    pub(crate) active: bool,
}

// The generated registry bounds fields.
#[derive(Clone)]
pub(crate) enum StateOverlayEntry {
    Owner(StateOverlayOwner),
    EntityBinding(EntityOverlayBinding),
    Component(ComponentStateOverlay),
}

impl StateOverlayEntry {
    pub(crate) fn owner(&self) -> Option<u64> {
        match self {
            Self::Owner(_) => None,
            Self::EntityBinding(EntityOverlayBinding {
                owner,
                ..
            })
            | Self::Component(ComponentStateOverlay {
                owner,
                ..
            }) => Some(*owner),
        }
    }
}

struct StateOverlaySlot {
    generation: u32,
    value: Option<StateOverlayEntry>,
}

/// Released slots keep generation evidence so duplicate cleanup is harmless.
/// Exhausted generations retire permanently instead of aliasing ancient handles.
#[derive(Default)]
pub(crate) struct StateOverlayRegistry {
    slots: Vec<StateOverlaySlot>,
    free: Vec<u32>,
    next_order: u64,
}

impl StateOverlayRegistry {
    pub(crate) fn reserve(&mut self, slots: usize) -> Result<(), ErrorReason> {
        self.slots
            .try_reserve(slots.saturating_sub(self.slots.len()))
            .map_err(|_| ErrorReason::Capacity)?;
        self.free
            .try_reserve(slots.saturating_sub(self.free.len()))
            .map_err(|_| ErrorReason::Capacity)
    }

    pub(crate) fn borrow(&self, handle: u64) -> Option<&StateOverlayEntry> {
        let slot = self.slots.get(handle as u32 as usize)?;
        (slot.generation == (handle >> 32) as u32)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    pub(crate) fn insert(&mut self, value: StateOverlayEntry) -> Result<u64, ErrorReason> {
        let index = if let Some(index) = self.free.pop() {
            index
        } else {
            let index = u32::try_from(self.slots.len()).map_err(|_| ErrorReason::Capacity)?;
            self.slots.push(StateOverlaySlot {
                generation: 1,
                value: None,
            });
            index
        };

        let slot = &mut self.slots[index as usize];
        slot.value = Some(value);
        Ok((u64::from(slot.generation) << 32) | u64::from(index))
    }

    pub(crate) fn order(&mut self) -> Result<u64, ErrorReason> {
        self.next_order = self
            .next_order
            .checked_add(1)
            .ok_or(ErrorReason::Capacity)?;
        Ok(self.next_order)
    }

    /// None denotes an issued but released handle, never a live replacement.
    pub(crate) fn get(&self, handle: u64) -> Result<Option<StateOverlayEntry>, ErrorReason> {
        let generation = (handle >> 32) as u32;
        let slot = self
            .slots
            .get(handle as u32 as usize)
            .ok_or(ErrorReason::InvalidStateOverlay)?;

        if generation == 0 || generation > slot.generation {
            return Err(ErrorReason::InvalidStateOverlay);
        }
        if generation < slot.generation {
            return Ok(None);
        }

        // A retired maximum-generation slot also denotes a released handle.
        if slot.value.is_none() && generation != u32::MAX {
            return Err(ErrorReason::InvalidStateOverlay);
        }

        Ok(slot.value.as_ref().cloned())
    }

    pub(crate) fn set(&mut self, handle: u64, value: StateOverlayEntry) {
        debug_assert!(self.borrow(handle).is_some());
        let slot = &mut self.slots[handle as u32 as usize];
        slot.value = Some(value);
    }

    pub(crate) fn release(&mut self, handle: u64) {
        debug_assert!(self.borrow(handle).is_some());
        let index = handle as u32;

        let slot = &mut self.slots[index as usize];
        slot.value = None;
        if let Some(generation) = slot.generation.checked_add(1) {
            slot.generation = generation;
            self.free.push(index);
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u64, &StateOverlayEntry)> + '_ {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.value
                .as_ref()
                .map(|value| ((u64::from(slot.generation) << 32) | index as u64, value))
        })
    }
}
