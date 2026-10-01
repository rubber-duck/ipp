use super::service::{GuiContextState, GuiRegistry, GuiSessionState};
use super::{
    GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputError, GuiRequestKey, GuiRoutedInputProof,
};
use crate::systems::gui::local::{GuiEntityTarget, GuiLocalEffect, GuiLocalEffectSource};
use crate::{HostIngressView, WorldRef};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

pub(super) struct GuiPending {
    pub key: GuiRequestKey,
    session: Rc<GuiSessionState>,
    pub context: Option<Rc<GuiContextState>>,
    permit: RefCell<Option<Box<dyn GuiDeliveryPermit>>>,
    finished: Cell<bool>,
    deferred: RefCell<Option<GuiDeliveryTerminal>>,
    registry: Weak<RefCell<GuiRegistry>>,
    nodes: usize,
    payload_bytes: Cell<usize>,
}

impl GuiPending {
    pub fn new(
        key: GuiRequestKey,
        session: Rc<GuiSessionState>,
        context: Option<Rc<GuiContextState>>,
        permit: Box<dyn GuiDeliveryPermit>,
        registry: Weak<RefCell<GuiRegistry>>,
        nodes: usize,
    ) -> Rc<Self> {
        Rc::new(Self {
            key,
            session,
            context,
            permit: RefCell::new(Some(permit)),
            finished: Cell::new(false),
            deferred: RefCell::new(None),
            registry,
            nodes,
            payload_bytes: Cell::new(0),
        })
    }

    fn deliver(&self, permit: Box<dyn GuiDeliveryPermit>, terminal: GuiDeliveryTerminal) {
        if self.session.live.get() {
            permit.settle(terminal);
        }
    }

    pub fn finish(&self, terminal: GuiDeliveryTerminal) {
        if self.finished.replace(true) {
            return;
        }
        if let Some(registry) = self.registry.upgrade() {
            let mut registry = registry.borrow_mut();
            registry.pending.remove(&self.key);
        }
        let permit = self.permit.borrow_mut().take();
        if let Some(permit) = permit {
            self.deliver(permit, terminal);
        } else {
            *self.deferred.borrow_mut() = Some(terminal);
        }
    }

    fn prepare(&self, effect: Option<&GuiLocalEffect>) -> Result<(), GuiInputError> {
        self.prepare_with(|permit| permit.prepare(effect))
    }

    fn prepare_with(
        &self,
        prepare: impl FnOnce(&mut dyn GuiDeliveryPermit) -> Result<(), super::GuiDeliveryError>,
    ) -> Result<(), GuiInputError> {
        if self.finished.get()
            || !self.session.live.get()
            || self
                .context
                .as_ref()
                .is_some_and(|context| !context.live.get())
        {
            self.finish(GuiDeliveryTerminal::Cancelled);
            return Err(GuiInputError::Cancelled);
        }
        let Some(mut permit) = self.permit.borrow_mut().take() else {
            return Err(GuiInputError::Cancelled);
        };
        let result = prepare(permit.as_mut());
        let deferred = self.deferred.borrow_mut().take();
        if let Some(terminal) = deferred {
            self.deliver(permit, terminal);
            return Err(GuiInputError::Cancelled);
        }
        *self.permit.borrow_mut() = Some(permit);
        if !self.session.live.get()
            || self
                .context
                .as_ref()
                .is_some_and(|context| !context.live.get())
        {
            self.finish(GuiDeliveryTerminal::Cancelled);
            return Err(GuiInputError::Cancelled);
        }
        if let Err(error) = result {
            let error = GuiInputError::Delivery(error);
            self.finish(GuiDeliveryTerminal::Rejected(error));
            return Err(error);
        }
        Ok(())
    }
}

pub(super) struct GuiCommandTicket(Rc<GuiPending>);

impl GuiCommandTicket {
    pub fn new(pending: Rc<GuiPending>) -> Self {
        Self(pending)
    }
}

impl Drop for GuiCommandTicket {
    fn drop(&mut self) {
        self.0.finish(GuiDeliveryTerminal::Cancelled);
        if let Some(registry) = self.0.registry.upgrade() {
            let mut registry = registry.borrow_mut();
            registry.retained -= 1;
            registry.path_nodes -= self.0.nodes;
            registry.payload_bytes -= self.0.payload_bytes.get();
        }
    }
}

/// Opaque non-Clone command-owned ticket and authority. Embed in the owning GUI command.
/// This object does not contain control values and does not itself execute a GUI action.
pub struct GuiInputCommand {
    ticket: GuiCommandTicket,
    target: GuiEntityTarget,
    proof: GuiRoutedInputProof,
    entered: Cell<bool>,
    prepared: Cell<bool>,
}

impl GuiInputCommand {
    /// Prepare one native terminal and optional application effect without a second queue.
    pub(crate) fn prepare_native(
        &self,
        state: crate::systems::gui::local::GuiNativeTextState,
        effect: Option<GuiLocalEffect>,
    ) -> Result<GuiPreparedNative<'_>, GuiInputError> {
        if !self.entered.get()
            || self.prepared.replace(true)
            || self.target != state.fence.target
            || effect.as_ref().is_some_and(|effect| {
                effect.target != self.target || effect.source != self.effect_source()
            })
        {
            self.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        self.ticket
            .0
            .prepare_with(|permit| permit.prepare_native(&state))?;
        Ok(GuiPreparedNative {
            command: self,
            state: Some(state),
            effect,
        })
    }

    pub(super) fn pointer_reservation_ready(&self) -> Result<(), GuiInputError> {
        if self.ticket.0.finished.get() || self.entered.get() {
            Err(GuiInputError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.ticket.0.finished.get()
            || !self.ticket.0.session.live.get()
            || self
                .ticket
                .0
                .context
                .as_ref()
                .is_some_and(|context| !context.live.get())
    }

    /// Exact revocation handle for GUI-owned logical focus, unrelated to World session numbers.
    pub fn session_lifetime(&self) -> super::GuiInputSession {
        super::GuiInputSession(self.ticket.0.session.clone())
    }

    pub(crate) fn reserve_payload(&self, bytes: usize) -> Result<(), GuiInputError> {
        let result = (|| {
            if self.ticket.0.finished.get() || self.ticket.0.payload_bytes.get() != 0 {
                return Err(GuiInputError::Cancelled);
            }
            let registry = self
                .ticket
                .0
                .registry
                .upgrade()
                .ok_or(GuiInputError::Cancelled)?;
            let mut budget = registry.borrow_mut();
            if bytes
                > budget
                    .limits
                    .payload_bytes
                    .saturating_sub(budget.payload_bytes)
            {
                return Err(GuiInputError::Capacity);
            }
            budget.payload_bytes += bytes;
            self.ticket.0.payload_bytes.set(bytes);
            Ok(())
        })();
        if let Err(error) = result {
            self.reject(error);
        }
        result
    }

    pub(super) fn new(
        ticket: GuiCommandTicket,
        target: GuiEntityTarget,
        proof: GuiRoutedInputProof,
    ) -> Self {
        Self {
            ticket,
            target,
            proof,
            entered: Cell::new(false),
            prepared: Cell::new(false),
        }
    }

    /// Exact adapter correlation scope.
    pub fn key(&self) -> GuiRequestKey {
        self.ticket.0.key
    }

    /// The target that the receiving GUI System must validate locally.
    pub fn target(&self) -> GuiEntityTarget {
        self.target
    }

    /// Routed source: the Host context and completed path.
    pub fn routed_proof(&self) -> &GuiRoutedInputProof {
        &self.proof
    }

    /// Immutable reservation provenance; callers cannot relabel an effect at commit.
    pub fn effect_source(&self) -> GuiLocalEffectSource {
        self.proof.effect_source()
    }

    /// Pure declarations for System::command_world_references, including all remote parents.
    pub fn world_references(&self, visit: &mut dyn FnMut(WorldRef)) {
        visit(self.target.world);
        self.proof.visit(visit);
    }

    /// Check delivery FIRST, then current authoritative context/path and scheduling state.
    /// The receiver must call this before even focus revalidation and before any local mutation.
    /// Local target incarnation, role and eligibility checks remain the GUI System's responsibility.
    pub fn validate(&self, view: &HostIngressView<'_>) -> Result<(), GuiInputError> {
        self.ticket.0.prepare(None)?;
        if let Err(error) = self.proof.validate_target(view, self.target) {
            self.reject(error);
            return Err(error);
        }
        self.entered.set(true);
        Ok(())
    }

    /// Reserve exact immutable effect bytes after pure local validation, before mutation.
    /// The prepared object commits through a callback-free local mutation closure.
    pub fn prepare_effect(
        &self,
        effect: GuiLocalEffect,
    ) -> Result<GuiPreparedEffect<'_>, GuiInputError> {
        if !self.entered.get()
            || self.prepared.replace(true)
            || effect.target != self.target
            || effect.source != self.effect_source()
        {
            self.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        self.ticket.0.prepare(Some(&effect))?;
        Ok(GuiPreparedEffect {
            command: self,
            outcome: Some(GuiPreparedOutcome::Effect(effect)),
        })
    }

    /// Prepare a value operation that settles as [`GuiDeliveryTerminal::Written`]
    /// without a momentary effect; `changed` reports whether it writes a field.
    pub(crate) fn prepare_write(
        &self,
        changed: bool,
    ) -> Result<GuiPreparedEffect<'_>, GuiInputError> {
        if !self.entered.get() || self.prepared.replace(true) {
            self.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        self.ticket.0.prepare(None)?;
        Ok(GuiPreparedEffect {
            command: self,
            outcome: Some(GuiPreparedOutcome::Written {
                changed,
            }),
        })
    }

    /// Reject without an effect tick. Repeated rejection or later Drop is inert.
    pub fn reject(&self, reason: GuiInputError) {
        self.ticket.0.finish(GuiDeliveryTerminal::Rejected(reason));
    }
}

pub(crate) struct GuiPreparedNative<'a> {
    command: &'a GuiInputCommand,
    state: Option<crate::systems::gui::local::GuiNativeTextState>,
    effect: Option<GuiLocalEffect>,
}

impl GuiPreparedNative<'_> {
    pub(crate) fn commit(
        mut self,
        mutate: impl FnOnce(&crate::systems::gui::local::GuiNativeTextState),
        publish: impl FnOnce(&GuiLocalEffect),
    ) -> Result<(), GuiInputError> {
        let pending = &self.command.ticket.0;
        if pending.finished.get()
            || !pending.session.live.get()
            || pending
                .context
                .as_ref()
                .is_some_and(|context| !context.live.get())
        {
            return Err(GuiInputError::Cancelled);
        }
        let state = self.state.take().expect("prepared native state");
        mutate(&state);
        if let Some(effect) = &self.effect {
            publish(effect);
        }
        pending.finish(GuiDeliveryTerminal::NativeApplied(state));
        Ok(())
    }
}

impl Drop for GuiPreparedNative<'_> {
    fn drop(&mut self) {
        if self.state.is_some() {
            self.command.ticket.0.finish(GuiDeliveryTerminal::Cancelled);
        }
    }
}

enum GuiPreparedOutcome {
    Effect(GuiLocalEffect),
    Written {
        changed: bool,
    },
}

/// Exact prepared effect or value write retained only until immediate local
/// commit/settlement. Dropping without commit cancels; it is never a
/// persistent control-value mirror.
pub struct GuiPreparedEffect<'a> {
    command: &'a GuiInputCommand,
    outcome: Option<GuiPreparedOutcome>,
}

impl GuiPreparedEffect<'_> {
    /// Read the exact candidate effect the local GUI System is about to commit, if any.
    pub fn effect(&self) -> Option<&GuiLocalEffect> {
        match self.outcome.as_ref()? {
            GuiPreparedOutcome::Effect(effect) => Some(effect),
            GuiPreparedOutcome::Written {
                ..
            } => None,
        }
    }

    /// Recheck cancellation before local mutation, then deliver exactly this candidate.
    /// The infallible closure must only commit prevalidated local state: no external callbacks,
    /// new admission, reentrant service operations or additional World evaluation.
    pub fn commit(self, mutate: impl FnOnce()) -> Result<(), GuiInputError> {
        self.commit_with_publication(mutate, |_| {})
    }

    pub(crate) fn commit_with_publication(
        mut self,
        mutate: impl FnOnce(),
        publish: impl FnOnce(&GuiLocalEffect),
    ) -> Result<(), GuiInputError> {
        let pending = &self.command.ticket.0;
        if pending.finished.get()
            || !pending.session.live.get()
            || pending
                .context
                .as_ref()
                .is_some_and(|context| !context.live.get())
        {
            return Err(GuiInputError::Cancelled);
        }
        let outcome = self.outcome.take().expect("unconsumed prepared GUI effect");
        mutate();
        let terminal = match outcome {
            GuiPreparedOutcome::Effect(effect) => {
                publish(&effect);
                GuiDeliveryTerminal::Applied(effect)
            }
            GuiPreparedOutcome::Written {
                changed,
            } => GuiDeliveryTerminal::Written {
                target: self.command.target,
                source: self.command.effect_source(),
                changed,
            },
        };
        self.command.ticket.0.finish(terminal);
        Ok(())
    }
}

impl Drop for GuiPreparedEffect<'_> {
    fn drop(&mut self) {
        if self.outcome.is_some() {
            self.command.ticket.0.finish(GuiDeliveryTerminal::Cancelled);
        }
    }
}
