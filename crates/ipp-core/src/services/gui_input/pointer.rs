use super::service::GuiRegistry;
use super::{GuiInputCommand, GuiInputContext, GuiInputError, GuiInputSessionId};
use crate::WorldAttachmentToken;
use crate::systems::gui::local::GuiEntityTarget;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// Exact service-issued pointer activation identity, not an admission capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GuiPointerLeaseId {
    session: GuiInputSessionId,
    serial: u64,
}

impl GuiPointerLeaseId {
    /// Read-only delivery correlation identity; it cannot construct an admission lease.
    pub fn identity(self) -> (GuiInputSessionId, u64) {
        (self.session, self.serial)
    }
}

pub(super) type GuiPointerKey = (GuiInputSessionId, u64, u64);

pub(super) struct GuiPointerSlot {
    key: GuiPointerKey,
    current: RefCell<Weak<GuiPointerIdentity>>,
    generation: Cell<u64>,
    registry: Weak<RefCell<GuiRegistry>>,
}

impl Drop for GuiPointerSlot {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.borrow_mut().pointer_slots.remove(&self.key);
        }
    }
}

struct GuiPointerIdentity {
    id: GuiPointerLeaseId,
    context: GuiInputContext,
    target: GuiEntityTarget,
    path: Box<[WorldAttachmentToken]>,
    slot: Rc<GuiPointerSlot>,
    active: Cell<bool>,
    revoked: Cell<bool>,
    registry: Weak<RefCell<GuiRegistry>>,
}

impl Drop for GuiPointerIdentity {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            let mut registry = registry.borrow_mut();
            registry.pointer_leases -= 1;
            registry.path_nodes -= self.path.len();
        }
    }
}

/// Shared identity/cancellation only; all hover/press/capture flags remain GUI-owned.
/// A newly reserved candidate is inert until an admitted local commit activates it.
#[derive(Clone)]
pub struct GuiPointerLease(Rc<GuiPointerIdentity>);

impl GuiPointerLease {
    pub(super) fn reserve(
        input: &GuiInputCommand,
        pointer: u64,
        serial: u64,
        registry: &Rc<RefCell<GuiRegistry>>,
    ) -> Result<Self, GuiInputError> {
        let proof = input.routed_proof();
        let (session, context) = proof.context.identity();
        let key = (session, context, pointer);
        let mut budget = registry.borrow_mut();
        if budget.pointer_leases >= budget.limits.pointer_leases
            || proof.path.len() > budget.limits.path_nodes.saturating_sub(budget.path_nodes)
        {
            return Err(GuiInputError::Capacity);
        }
        let slot = budget
            .pointer_slots
            .get(&key)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let slot = Rc::new(GuiPointerSlot {
                    key,
                    current: RefCell::new(Weak::new()),
                    generation: Cell::new(0),
                    registry: Rc::downgrade(registry),
                });
                budget.pointer_slots.insert(key, Rc::downgrade(&slot));
                slot
            });
        budget.pointer_leases += 1;
        budget.path_nodes += proof.path.len();
        Ok(Self(Rc::new(GuiPointerIdentity {
            id: GuiPointerLeaseId {
                session,
                serial,
            },
            context: proof.context.clone(),
            target: proof.target,
            path: proof.path.clone(),
            slot,
            active: Cell::new(false),
            revoked: Cell::new(false),
            registry: Rc::downgrade(registry),
        })))
    }

    /// Exact activation generation. Cloning the handle never clones interaction state.
    pub fn id(&self) -> GuiPointerLeaseId {
        self.0.id
    }

    /// Pointer number scoped by this lease's exact session and root-context generation.
    pub fn pointer(&self) -> u64 {
        self.0.slot.key.2
    }

    /// False for candidates, cancelled generations and revoked session/context lifetimes.
    pub fn is_live(&self) -> bool {
        self.0.active.get() && self.usable()
    }

    pub(super) fn usable(&self) -> bool {
        !self.0.revoked.get()
            && self.0.id.serial >= self.0.slot.generation.get()
            && self.0.context.0.live.get()
            && self.0.context.0.session.live.get()
    }

    pub(crate) fn validate(&self, input: &GuiInputCommand) -> Result<(), GuiInputError> {
        let proof = input.routed_proof();
        if !self.usable() {
            return Err(GuiInputError::Cancelled);
        }
        if self.0.context.identity() != proof.context.identity()
            || self.0.target != proof.target
            || self.0.path != proof.path
        {
            return Err(GuiInputError::StalePath);
        }
        Ok(())
    }

    pub(crate) fn activate(&self) {
        self.0.slot.generation.set(self.0.id.serial);
        let mut current = self.0.slot.current.borrow_mut();
        if let Some(previous) = current.upgrade()
            && !Rc::ptr_eq(&previous, &self.0)
        {
            previous.revoked.set(true);
        }
        *current = Rc::downgrade(&self.0);
        self.0.active.set(true);
    }

    pub(crate) fn revoke(&self) {
        self.0.revoked.set(true);
    }

    pub(super) fn session(&self) -> GuiInputSessionId {
        self.0.id.session
    }
}

#[cfg(test)]
#[path = "pointer_tests.rs"]
mod tests;
